//! 系统信息查询系统调用 (TCB)
//!
//! POSIX 标准的只读查询类系统调用:
//! - getpid / gettid / getppid / getpgid: 进程/线程 ID
//! - uname: 系统信息
//!
//! 时钟查询 (`gettimeofday` / `clock_gettime`) 属 services 策略:
//! 墙钟基准由 `framework::timer::time_sync` 机制持有, services 经
//! `framework::syscall::api::write_struct_to_user` 安全写入用户缓冲.

use alloc::string::String;

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
    // 主机名/域名取自当前进程 UTS namespace (单一权威: 与 sethostname/gethostname/
    // setdomainname 同源, 不再各自硬编码)
    let uts_ns = crate::framework::proc::namespace::uts_current();
    let nodename = uts_ns
        .as_ref()
        .map_or_else(default_nodename, |n| n.get_nodename());
    let domainname = uts_ns.as_ref().map_or_else(String::new, |n| n.get_domainname());

    copy_str(&mut uts.sysname, b"QueenX");
    copy_str(&mut uts.nodename, nodename.as_bytes());
    copy_str(&mut uts.release, b"0.1.0");
    copy_str(&mut uts.version, b"QueenX 0.1.0 (queenx)");
    #[cfg(target_arch = "x86_64")]
    copy_str(&mut uts.machine, b"x86_64");
    #[cfg(target_arch = "aarch64")]
    copy_str(&mut uts.machine, b"aarch64");
    copy_str(&mut uts.domainname, domainname.as_bytes());

    // SAFETY: buf 由 check_user_buf 验证为可写, 大小 390 = sizeof(Utsname)
    unsafe {
        core::ptr::write_volatile(buf as *mut Utsname, uts);
    }
    0
}

/// 无进程上下文时 `uname` 使用的主机名 (与 `UtsNamespace::new` 初值同源)
fn default_nodename() -> String {
    String::from_utf8_lossy(crate::framework::proc::namespace::UTS_DEFAULT_NODENAME).into_owned()
}

/// 复制字符串到固定长度数组 (NUL 终止)
fn copy_str(dst: &mut [u8], src: &[u8]) {
    let len = src.len().min(dst.len() - 1);
    dst[..len].copy_from_slice(&src[..len]);
    dst[len] = 0;
}
