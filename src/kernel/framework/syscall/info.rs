//! 系统信息查询系统调用 (TCB)
//!
//! POSIX 标准的只读查询类系统调用:
//! - getpid / gettid / getppid / getpgid: 进程/线程 ID
//! - uname: 系统信息
//! - gettimeofday: 时钟

use crate::framework::proc::api;
use crate::framework::syscall::Errno;
use crate::framework::syscall::raw;

// ============================================================================
// 进程/线程 ID
// ============================================================================

/// getpid — 返回当前进程 PID
pub fn sys_getpid() -> i64 {
    i64::from(api::process_get_current_pid())
}

/// gettid — 返回当前线程 TID (线程与进程 ID 共享, 等同 getpid)
pub fn sys_gettid() -> i64 {
    i64::from(api::process_get_current_pid())
}

/// getppid — 返回父进程 PID
pub fn sys_getppid() -> i64 {
    let pid = api::process_get_current_pid();
    i64::from(api::proc_get_ppid(pid))
}

/// getpgid — 返回进程组 ID
///
/// 若 pid == 0, 返回当前进程的进程组.
pub fn sys_getpgid(pid: i32) -> i64 {
    crate::framework::proc::proc_getpgid(pid)
}

// ============================================================================
// uname — 系统信息
// ============================================================================

/// uname 系统调用
///
/// `buf` 指向 struct utsname (6 个 65 字节字符串字段).
pub fn sys_uname(buf: u64) -> i64 {
    if buf == 0 || !raw::check_user_buf(buf, 390) {
        return Errno::EFAULT.as_ret();
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    struct Utsname {
        sysname: [u8; 65],
        nodename: [u8; 65],
        release: [u8; 65],
        version: [u8; 65],
        machine: [u8; 65],
        domainname: [u8; 65],
    }

    let mut uts = Utsname {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };

    // 填入系统信息
    copy_str(&mut uts.sysname, b"QueenX");
    copy_str(&mut uts.nodename, b"queenx-node");
    copy_str(&mut uts.release, b"0.1.0");
    copy_str(&mut uts.version, b"QueenX 0.1.0 (queenx)");
    #[cfg(target_arch = "x86_64")]
    copy_str(&mut uts.machine, b"x86_64");
    #[cfg(target_arch = "aarch64")]
    copy_str(&mut uts.machine, b"aarch64");
    copy_str(&mut uts.domainname, b"(none)");

    // SAFETY: buf 由 check_user_buf 验证为可写, 大小 390 = sizeof(Utsname)
    unsafe {
        core::ptr::write_volatile(buf as *mut Utsname, uts);
    }
    0
}

/// 复制字符串到固定长度数组 (NUL 终止)
fn copy_str(dst: &mut [u8], src: &[u8]) {
    let len = src.len().min(dst.len() - 1);
    dst[..len].copy_from_slice(&src[..len]);
    dst[len] = 0;
}

// ============================================================================
// gettimeofday — 时钟
// ============================================================================

/// gettimeofday 系统调用
///
/// `tv` 指向 struct timeval { `tv_sec`, `tv_usec` }.
pub fn sys_gettimeofday(tv: u64) -> i64 {
    if tv == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !raw::check_user_buf(tv, 16) {
        return Errno::EFAULT.as_ret();
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    struct Timeval {
        tv_sec: i64,
        tv_usec: i64,
    }
    let ticks = raw::get_ticks();
    let t = Timeval {
        tv_sec: (ticks / 1000) as i64,
        tv_usec: ((ticks % 1000) * 1000) as i64,
    };

    // SAFETY: tv 由 check_user_buf 验证为可写, 大小 16 = sizeof(Timeval)
    unsafe {
        core::ptr::write_volatile(tv as *mut Timeval, t);
    }
    0
}
