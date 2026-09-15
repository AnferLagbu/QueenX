#![deny(unsafe_code)]
//! execve 系统调用 — services 层策略
//!
//! ## 职责拆分 (T2 批 1, syscall-followup)
//!
//! - services (本文件): 指针校验策略 + argv 扫描 + SUID 提权判定 + 委托进程替换
//! - framework 机制: [`crate::framework::proc::proc_exec_replace`] (进程替换/
//!   页表切换), `framework::syscall::api::read_u64_from_user` (用户内存 safe 读取)
//!
//! 原 `framework::syscall::execve::ExecveResult` 包装类型随回退分支迁移删除
//! (其唯一消费者是 framework 回退分支), services 直接返回精确 Errno
//! (`EFAULT` / `ENOENT`), 语义与原 `from_ret` 映射一致.

use crate::framework::syscall::Errno;

/// execve(path, argv, envp) 系统调用策略
///
/// `path` / `argv` / `envp` 均为用户空间指针 (u64 形式); `envp` 当前忽略
/// (与原 framework 实现一致, 环境变量传递待进程基础设施扩展后实装).
///
/// # Errors
/// - `path` 为空或未通过用户指针校验 → `EFAULT`
/// - `argv` 数组遍历中任一指针 (槽位或条目) 非法 → `EFAULT`
/// - 进程替换失败 → `ENOENT` (与原 framework 实现语义一致)
#[expect(
    clippy::similar_names,
    reason = "argv/envp 是 POSIX execve ABI 标准参数名, 重命名会破坏与 Linux 语义的对应关系"
)]
pub fn execve_syscall(path: u64, argv: u64, envp: u64) -> Result<usize, Errno> {
    use crate::framework::syscall::api;
    use crate::framework::syscall::raw;

    if path == 0 || !raw::check_user_ptr(path) {
        return Err(Errno::EFAULT);
    }
    let _ = envp;

    // argv 扫描: 逐项读取用户空间指针数组, 统计 argc 直到 NULL 终止
    let mut argc: u32 = 0;
    if argv != 0 {
        if !raw::check_user_ptr(argv) {
            return Err(Errno::EFAULT);
        }
        let mut slot = argv;
        loop {
            if !raw::check_user_ptr(slot) {
                return Err(Errno::EFAULT);
            }
            let Some(entry) = api::read_u64_from_user(slot) else {
                return Err(Errno::EFAULT);
            };
            if entry == 0 {
                break;
            }
            if !raw::check_user_ptr(entry) {
                return Err(Errno::EFAULT);
            }
            argc += 1;
            slot += 8;
        }
    }

    // SUID 处理: 可执行文件带 setuid 位且属主非 root 时提权
    let mut st = crate::framework::fs::VfsStat::default();
    let current_pwm = crate::framework::credo::get_current_pwm();
    let stat_result =
        crate::framework::fs::api::vfs_stat_internal(path as *const u8, &raw mut st, current_pwm);
    if stat_result == 0 && (st.perm & 0o4000) != 0 && st.owner_pwm != 0 {
        crate::framework::credo::elevate_for_suid(st.owner_pwm);
    }

    // 进程替换 (framework 机制: ELF 加载 + 地址空间切换)
    let result =
        crate::framework::proc::proc_exec_replace(path as *const u8, argv as *const *const u8, argc);
    if result < 0 {
        Err(Errno::ENOENT)
    } else {
        Ok(0)
    }
}
