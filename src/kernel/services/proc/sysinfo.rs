#![deny(unsafe_code)]
//! 系统信息策略 — getrusage / sysinfo / getrlimit / setrlimit / gethostname / sethostname / boot_check
//!
//! 从 framework/syscall/mod.rs 迁移的策略代码:
//! - getrusage_syscall: 资源使用统计
//! - sysinfo_syscall: 系统信息
//! - getrlimit_syscall: 资源限制查询
//! - setrlimit_syscall: 资源限制设置 (T2 批 1 自 framework 回退层迁移)
//! - gethostname_syscall: 获取主机名
//! - sethostname_syscall: 设置主机名
//! - boot_check_syscall: 启动检查
//!
//! ## 框内核边界
//! - 100% safe Rust
//! - 通过 framework::syscall::api 安全写入用户空间
//! - 无 unsafe, 无裸指针

use crate::framework::syscall::Errno;

/// getrusage(who, rusage) 策略
pub fn getrusage_syscall(who: i32, rusage_ptr: u64) -> i64 {
    let pid = crate::framework::proc::process_get_current_pid();
    i64::from(crate::framework::proc::proc_get_rusage(
        pid,
        who,
        rusage_ptr as *mut u8,
        144,
    ))
}

/// sysinfo(info) 策略
pub fn sysinfo_syscall(info_ptr: u64) -> i64 {
    if info_ptr == 0 {
        return Errno::EINVAL.as_ret();
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    struct SysInfo {
        uptime: i64,
        loads: [u64; 3],
        totalram: u64,
        freeram: u64,
        sharedram: u64,
        bufferram: u64,
        totalswap: u64,
        freeswap: u64,
        procs: u16,
        _pad: [u8; 6],
        totalhigh: u64,
        freehigh: u64,
        mem_unit: u32,
    }

    let ticks = crate::framework::syscall::api::get_ticks();
    let si = SysInfo {
        uptime: (ticks / 1000) as i64,
        loads: [0, 0, 0],
        totalram: 128 * 1024 * 1024,
        freeram: 97 * 1024 * 1024,
        sharedram: 0,
        bufferram: 0,
        totalswap: 0,
        freeswap: 0,
        procs: 1,
        _pad: [0u8; 6],
        totalhigh: 0,
        freehigh: 0,
        mem_unit: 1,
    };

    if !crate::framework::syscall::api::write_struct_to_user(info_ptr, &si) {
        return Errno::EFAULT.as_ret();
    }
    0
}

/// setrlimit(resource, rlim) 策略 — 设置资源限制
///
/// T2 批 1 (syscall-followup): 自 framework 回退层迁移. RlimitTable 机制
/// 字段仍归 framework (DECISION-J 第十九批判据), services 仅做策略:
/// 指针校验 + 读取 rlim_cur/rlim_max + 特权判定 + 委托表更新.
pub fn setrlimit_syscall(resource: i32, rlim_ptr: u64) -> i64 {
    use crate::framework::proc::rlimit::RLIMIT_NLIMITS;

    if rlim_ptr == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !(0..RLIMIT_NLIMITS as i32).contains(&resource) {
        return Errno::EINVAL.as_ret();
    }

    // 从用户空间读取 rlim_cur / rlim_max (16 字节, safe 包装先校验后读)
    let mut vals = [0u64; 2];
    if !crate::framework::syscall::api::read_struct_from_user(rlim_ptr, &mut vals) {
        return Errno::EFAULT.as_ret();
    }
    let (cur, max) = (vals[0], vals[1]);

    // 特权判定: pid=1 (init) 视为特权进程
    let pid = crate::framework::proc::process_get_current_pid();
    let is_privileged = pid == 1;

    match crate::framework::proc::process_with(pid, |proc| {
        let mut rlimit_table = proc.rlimit_table.lock();
        rlimit_table.set(resource as usize, cur, max, is_privileged)
    }) {
        Some(Ok(())) => 0,
        Some(Err(e)) => e.as_ret(),
        None => Errno::ESRCH.as_ret(),
    }
}

/// getrlimit(resource, rlim) 策略
pub fn getrlimit_syscall(_resource: i32, rlim_ptr: u64) -> i64 {
    if rlim_ptr == 0 {
        return Errno::EINVAL.as_ret();
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    struct Rlimit {
        rlim_cur: u64,
        rlim_max: u64,
    }

    let r = Rlimit {
        rlim_cur: u64::MAX,
        rlim_max: u64::MAX,
    };

    if !crate::framework::syscall::api::write_struct_to_user(rlim_ptr, &r) {
        return Errno::EFAULT.as_ret();
    }
    0
}

/// gethostname(buf, size) 策略
pub fn gethostname_syscall(buf_ptr: u64, size: u64) -> i64 {
    if buf_ptr == 0 || size == 0 {
        return Errno::EFAULT.as_ret();
    }
    if !crate::framework::syscall::api::validate_user_buf(buf_ptr, size) {
        return Errno::EFAULT.as_ret();
    }

    let hostname = b"localhost\0";
    let copy_len = hostname.len().min(size as usize);
    // 使用 write_struct_to_user 逐字节写入
    for (i, &byte) in hostname.iter().enumerate().take(copy_len) {
        if !crate::framework::syscall::api::write_struct_to_user(buf_ptr + i as u64, &byte)
        {
            return Errno::EFAULT.as_ret();
        }
    }
    0
}

/// sethostname(name, len) 策略
pub fn sethostname_syscall(name_ptr: u64, len: u64) -> i64 {
    if name_ptr == 0 || len == 0 || len > 63 {
        return Errno::EINVAL.as_ret();
    }
    let pwm = crate::framework::credo::pwm_get_current();
    if !crate::framework::credo::pwm_has_capability(pwm, 0, 9) {
        return Errno::EACCES.as_ret();
    }
    0
}

/// `boot_check(check_type)` 策略
pub fn boot_check_syscall(check_type: i32) -> i64 {
    match check_type {
        0 => i64::from(crate::framework::credo::pwm_any_identity_exists()),
        _ => -1,
    }
}

/// reboot(cmd) 策略
///
/// PWM 权限检查 + 委托 framework 执行重启机制
pub fn reboot_syscall(cmd: i32) -> i64 {
    let pwm = crate::framework::credo::pwm_get_current();
    if !crate::framework::credo::pwm_has_capability(pwm, 0, 0x01) {
        return Errno::EACCES.as_ret();
    }
    crate::framework::syscall::api::reboot_mechanism(cmd)
}
