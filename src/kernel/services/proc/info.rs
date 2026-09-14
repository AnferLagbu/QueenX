#![deny(unsafe_code)]
//! 信息查询系统调用 — services 层安全代理
//!
//! ## 安全边界
//!
//! - services 层: 验证参数类型/范围,委托 framework 实现
//! - framework 层: 实际访问进程表/写入用户空间缓冲
//!
//! ## 范围
//!
//! - getpid / gettid / getppid: 进程/线程 ID 查询
//! - getpgid: 进程组 ID
//! - uname: 系统信息
//! - gettimeofday: 时钟

use crate::framework::syscall::Errno;

// ============================================================================
// 进程/线程 ID
// ============================================================================

/// getpid — 返回当前进程 PID (恒成功)
pub fn getpid_syscall() -> usize {
    let ret = crate::framework::syscall::info::sys_getpid();
    if ret < 0 { 0 } else { ret as usize }
}

/// gettid — 返回当前线程 TID (恒成功)
pub fn gettid_syscall() -> usize {
    let ret = crate::framework::syscall::info::sys_gettid();
    if ret < 0 { 0 } else { ret as usize }
}

/// getppid — 返回父进程 PID
pub fn getppid_syscall() -> usize {
    let ret = crate::framework::syscall::info::sys_getppid();
    if ret < 0 { 0 } else { ret as usize }
}

/// getpgid — 返回进程组 ID
///
/// pid 范围: 0 (当前进程) 或 > 0
///
/// # Errors
///
/// 当 `pid < 0` 时返回 `EINVAL`; 底层查询失败时返回对应的 `Errno`.
pub fn getpgid_syscall(pid: i32) -> Result<usize, Errno> {
    if pid < 0 {
        return Err(Errno::EINVAL);
    }
    let ret = crate::framework::syscall::info::sys_getpgid(pid);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

// ============================================================================
// uname
// ============================================================================

/// uname — 系统信息
///
/// `buf` 指向 struct utsname (6 × 65 字节字段)
///
/// # Errors
///
/// 当 `buf == 0` 时返回 `EFAULT`.
pub fn uname_syscall(buf: u64) -> Result<usize, Errno> {
    if buf == 0 {
        return Err(Errno::EFAULT);
    }
    let ret = crate::framework::syscall::info::sys_uname(buf);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

// ============================================================================
// gettimeofday -> 已迁往 services::timer::clock (B05-26 时间归位)
// ============================================================================

