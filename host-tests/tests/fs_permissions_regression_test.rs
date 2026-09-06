//! fs: B06-02/03/07 权限与句柄修复回归测试
//!
//! 验收:
//!   - chown_syscall: UID/GID 未注册返回 EINVAL, 不再回退 root (B06-02)
//!   - open_by_handle_at_syscall: 无 CAP_SYS_ADMIN (SYSTEM 域 0x01) 返回 EPERM (B06-03)
//!   - poll_syscall: fd 上限用 VFS_MAX_FDS (32) 而非 256, 防越界索引 32 长数组 (B06-07)
//!
//! ## B08-20 迁移 (2026-09-06)
//! 原镜像三个 syscall 的纯判定逻辑已改引内核真实 API:
//! - chown_syscall 的 UID 判定 → `identity::get_table().find_by_uid` (真实身份表)
//! - open_by_handle_at_syscall 的 CAP_SYS_ADMIN 判定 → `framework::credo::pwm_has_capability`
//! - poll_syscall 的 fd 上限 → 内核常量 `framework::fs::VFS_MAX_FDS`
//!
//! ## 因内核 host 不可测已移除 (syscall 完整路径)
//! 三个 syscall 完整函数 (chown_syscall / open_by_handle_at_syscall / poll_syscall) 依赖
//! 进程凭证上下文 (pwm_get_current / session::get_current_pwm) 与 VFS 全局状态
//! (VFS_MANAGER.fd_table / vfs_chown_ext), 且 open_by_handle_at / poll 走
//! copy_from_user / read_struct_from_user (SMAP stac/clac 指令, host 不可用),
//! host 上不可直接调用. 完整路径回归保留在 QEMU 集成测试.
//!
//! 追踪: B06-02 / B06-03 / B06-07
//! SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

use queenx::kernel::framework::credo::identity;
use queenx::kernel::framework::credo::pwm_has_capability;
use queenx::kernel::framework::fs::VFS_MAX_FDS;
use queenx::kernel::services::credo::capability::CAP_DOMAIN_SYSTEM;
use queenx::kernel::services::credo::types::{CapBits, CapDomain};

/// 注册并缓存测试身份 (creator=0 → 最高特权级, uid=0).
fn test_pwm() -> u64 {
    static PWM: OnceLock<u64> = OnceLock::new();
    *PWM.get_or_init(|| {
        identity::get_table()
            .create("fp-pw", "fs-permissions", 0)
            .expect("注册测试身份失败")
    })
}

/// 独立测试身份, 供"授予 CAP_SYS_ADMIN"用例修改能力而不污染 `test_pwm`.
fn admin_pwm() -> u64 {
    static PWM: OnceLock<u64> = OnceLock::new();
    *PWM.get_or_init(|| {
        identity::get_table()
            .create("fp-admin", "fs-permissions-admin", 0)
            .expect("注册测试身份失败")
    })
}

// ============================================================================
// B06-02: chown_syscall UID/GID 判定 (改引内核 identity::find_by_uid)
// ============================================================================

/// 内核 [services/fs/file_ops.rs::chown_syscall] 的 uid→pwm 判定 (B06-02 修复后):
///
/// 原实现 `tbl.find_by_uid(uid).map_or(0, ...)` 在 uid 未注册时回退 owner_pwm=0 (root),
/// 存在提权漏洞; 修复后未注册 uid/gid 返回 `EINVAL` (errno=22), 不再默认 root.
/// 本测试直接验证内核 `identity::get_table().find_by_uid` 真实行为:
/// 返回 None = 未注册 → chown_syscall 返回 EINVAL; Some = 已注册 → 取 owner_pwm.
#[test]
fn chown_registered_uid_returns_pwm() {
    // 已注册 uid (creator=0 身份 uid=0) 正常映射到对应 pwm, 不回退 0
    let pwm = test_pwm();
    let tbl = identity::get_table();
    let found = tbl.find_by_uid(0);
    // B08-20: 并行测试共享全局 identity 表, 多个测试文件均可能注册 uid=0 身份,
    // find_by_uid(0) 返回的是"第一个 uid=0 条目"而非必然是 test_pwm().
    // 断言语义: uid=0 已被注册 (Some) 且其 pwm 非 0 (不回退 root 语义), 不锁定具体 pwm.
    assert!(found.is_some(), "已注册身份应可通过 find_by_uid(0) 找到");
    let found_pwm = found.unwrap().get_pwm().0;
    assert_ne!(found_pwm, 0, "uid=0 映射的 pwm 必须非 0 (不回退 root)");
    assert!(found_pwm == pwm || identity::find(pwm).is_some(),
        "test_pwm 身份应已注册 (无论 find_by_uid 命中哪个 uid=0 条目)");
}

#[test]
fn chown_unregistered_uid_returns_einval_not_root() {
    // 未注册 uid 必须返回 EINVAL (find_by_uid None), 不得回退 0 (root) — B06-02 核心
    let tbl = identity::get_table();
    // 本测试进程仅注册 uid=0 的身份, 其余 uid 均未注册
    for uid in [42u32, 43, 1000, 2000] {
        assert!(tbl.find_by_uid(uid).is_none(), "uid {} 未注册应返回 None", uid);
    }
}

#[test]
fn chown_max_uid_sentinel_rejected() {
    // uid == u32::MAX (Linux "(uid_t)-1" 哨兵) 未注册 → None, 而非回退 root
    let tbl = identity::get_table();
    assert!(tbl.find_by_uid(u32::MAX).is_none());
}

// ============================================================================
// B06-03: open_by_handle_at_syscall CAP 检查 (改引内核 pwm_has_capability)
// ============================================================================

/// 内核 [services/fs/file_handle.rs::open_by_handle_at_syscall] 的权限检查 (B06-03):
///
/// 采用 SYSTEM 域 (domain=0) + CAP_SYS_ADMIN (0x01), 与 mount/umount2 先例一致;
/// 无能力返回 `EPERM` (errno=1). 本测试直接验证内核
/// `framework::credo::pwm_has_capability(pwm, SYSTEM, 0x01)` 判定.
#[test]
fn open_by_handle_without_cap_returns_eperm() {
    // 注册身份初始 SYSTEM caps = VIABLE_FLOOR[SYSTEM] = 0 → 无 CAP_SYS_ADMIN → EPERM
    let pwm = test_pwm();
    assert!(
        !pwm_has_capability(pwm, CAP_DOMAIN_SYSTEM, 0x01),
        "默认身份不应有 CAP_SYS_ADMIN (对应 open_by_handle_at_syscall → EPERM)"
    );
}

#[test]
fn open_by_handle_bootstrap_has_cap_allowed() {
    // 内核差异: pwm==0 是 bootstrap 全权身份, 拥有 CAP_SYS_ADMIN → 放行
    assert!(pwm_has_capability(0, CAP_DOMAIN_SYSTEM, 0x01));
}

#[test]
fn open_by_handle_grant_sys_admin_allows() {
    // 经内核 PwmEntry::fetch_or_caps 授予 SYSTEM 0x01 后 → 拥有 CAP_SYS_ADMIN → 放行
    let pwm = admin_pwm();
    assert!(!pwm_has_capability(pwm, CAP_DOMAIN_SYSTEM, 0x01));
    identity::find(pwm)
        .expect("测试身份应已注册")
        .fetch_or_caps(CapDomain::SYSTEM, CapBits(0x01));
    assert!(pwm_has_capability(pwm, CAP_DOMAIN_SYSTEM, 0x01));
}

// ============================================================================
// B06-07: poll_syscall fd 上限判定 (改引内核 VFS_MAX_FDS 常量)
// ============================================================================

/// 内核 [services/fs/file_ops.rs::poll_syscall] 的 fd 上限 (B06-07 修复后):
///
/// 原实现用硬编码 `< 256` 做上限后直接索引 32 长 fd_table, fd∈[32,255] 越界 panic;
/// 修复后上限改用 `VFS_MAX_FDS` (32), fd≥32 一律视为"不就绪"且不越界.
/// poll_syscall 完整路径依赖 read_struct_from_user (SMAP stac/clac) 与
/// VFS_MANAGER.fd_table, host 不可直接调用; 此处验证内核常量契约
/// `framework::fs::VFS_MAX_FDS == 32` (VFS_MANAGER.fd_table 即为 32 长数组,
/// 编译期保证 fd≥32 不越界).
#[test]
fn poll_vfs_max_fds_is_32() {
    assert_eq!(VFS_MAX_FDS, 32, "B06-07: fd 上限必须为 VFS_MAX_FDS=32, 而非 256");
}
