//! 系统调用 API 层
//!
//! `QueenX` 原生 syscall (QX_*) + Linux 兼容 (SYS_*) + Credo 私有 syscall 的统一分发入口,
//! 用户态→内核态的唯一合法路径。
//!
//! ## 编号空间
//! - 0-299   : Linux 兼容编号 (SYS_*), 直接使用 Linux 标准编号
//! - 400-499 : Credo 私有 syscall
//! - 500+    : `QueenX` 原生编号 (QX_*)
//!
//! ## 调用方契约
//! - `boot::isr.asm` —— 中断/异常入口 (int 0x80 / syscall 指令)
//! - `idt::handlers` —— ISR 存根调用 `syscall_dispatch_from_frame`
//! - `proc::exec::load_elf` —— execve 时验证用户指针
//! - `credo::api` —— 能力检查路径复用 `validate_user_ptr`
//! - `chitin::user_driver` —— 用户态驱动透传
//!
//! ## 内部接口
//! - `types.rs` —— Errno, syscall 编号常量
//! - `mmap.rs` —— mmap/munmap/mprotect 实现
//! - `mod.rs` —— `syscall_dispatch()` 核心分发器 (所有 sys_* 实现)
//!
//! ## 安全约束
//! - 所有公开函数均通过 `validate_user_ptr` / `validate_user_buf` 检查用户指针
//! - 用户指针必须在 [1, 0x7FFFFFFFE000) 范围内
//! - `syscall_dispatch` / `syscall_dispatch_from_frame` 必须在中断上下文调用
//! - `syscall_register` 仅在启动阶段单线程调用
//!
//! ## 性能特征
//! - 分发路径: O(1) match 分支, 编译器优化为跳转表
//! - 指针验证: 两次比较, ≤ 5ns
//! - 覆盖 70+ POSIX syscall + 40+ Credo 私有 syscall

pub use super::types::Errno;

// ============================================================================
// QueenX 原生 syscall 编号 (QX_*)
// ============================================================================

pub use super::types::{
    QX_ACCEPT, QX_ACCESS, QX_ALARM, QX_BIND, QX_BPF, QX_BRK, QX_CET, QX_CGROUP_ATTACH,
    QX_CGROUP_CREATE, QX_CGROUP_DESTROY, QX_CGROUP_GET_STAT, QX_CGROUP_SET_LIMIT, QX_CHDIR,
    QX_CHMOD, QX_CHOWN, QX_CLOCK_GETRES, QX_CLOCK_GETTIME, QX_CLONE, QX_CLOSE, QX_CONNECT,
    QX_CREAT, QX_DUP, QX_DUP2, QX_EPOLL_CREATE, QX_EPOLL_CTL, QX_EPOLL_WAIT, QX_EVENTFD,
    QX_EVENTFD2, QX_EXECVE, QX_EXIT, QX_EXIT_GROUP, QX_FB_MMAP, QX_FB_OPEN, QX_FB_RELEASE,
    QX_FCHMOD, QX_FCHOWN, QX_FCNTL, QX_FLOCK, QX_FORK, QX_FSTAT, QX_FSYNC, QX_FTRACE_DISABLE,
    QX_FTRACE_ENABLE, QX_FTRACE_READ, QX_FTRACE_STAT, QX_FTRUNCATE, QX_FUTEX, QX_FW_DETACH,
    QX_FW_GET, QX_FW_GET_INFO, QX_FW_LOAD, QX_GET_CANARY, QX_GET_MEMPOLICY, QX_GETCPU, QX_GETCWD,
    QX_GETDENTS, QX_GETEGID, QX_GETEUID, QX_GETGID, QX_GETITIMER, QX_GETPEERNAME, QX_GETPGID,
    QX_GETPID, QX_GETPPID, QX_GETPRIORITY, QX_GETRANDOM, QX_GETRLIMIT, QX_GETRUSAGE, QX_GETSID,
    QX_GETSOCKNAME, QX_GETSOCKOPT, QX_GETTID, QX_GETTIMEOFDAY, QX_GETUID, QX_INOTIFY_ADD_WATCH,
    QX_INOTIFY_INIT1, QX_INOTIFY_RM_WATCH, QX_IO_URING_ENTER, QX_IO_URING_REGISTER,
    QX_IO_URING_SETUP, QX_IO_URING_SUBMIT, QX_IOCTL, QX_KEXEC, QX_KGDB_ENTER, QX_KILL, QX_LINK,
    QX_LISTEN, QX_LSEEK, QX_LSTAT, QX_MIGRATE_PAGES, QX_MKDIR, QX_MMAP, QX_MOUNT, QX_MPROTECT,
    QX_MREMAP, QX_MUNMAP, QX_NANOSLEEP, QX_NF_ADD_RULE, QX_NF_DEL_RULE, QX_NICE, QX_OPEN, QX_PIPE,
    QX_PM, QX_POLL, QX_PRCTL, QX_READ, QX_READLINK, QX_RECVFROM, QX_RECVMSG, QX_RENAME, QX_RMDIR,
    QX_ROUTE_ADD, QX_ROUTE_DEL, QX_ROUTE_QUERY, QX_RT_SIGACTION, QX_RT_SIGPROCMASK,
    QX_RT_SIGRETURN, QX_SCHED_GETAFFINITY, QX_SCHED_SETAFFINITY, QX_SCHED_YIELD, QX_SECCOMP,
    QX_SECURE_BOOT, QX_SELECT, QX_SENDMSG, QX_SENDTO, QX_SET_MEMPOLICY, QX_SETEGID, QX_SETEUID,
    QX_SETGID, QX_SETITIMER, QX_SETNS, QX_SETPGID, QX_SETPRIORITY, QX_SETREUID, QX_SETSID,
    QX_SETSOCKOPT, QX_SETUID, QX_SHUTDOWN, QX_SIGNALFD, QX_SIGNALFD4, QX_SOCKET, QX_STAT,
    QX_SYMLINK, QX_SYNC, QX_SYSINFO, QX_TGKILL, QX_TICKLESS, QX_TIME, QX_TIMER_CREATE,
    QX_TIMER_DELETE, QX_TIMER_GETOVERRUN, QX_TIMER_GETTIME, QX_TIMER_SETTIME, QX_TIMERFD_CREATE,
    QX_TIMERFD_GETTIME, QX_TIMERFD_SETTIME, QX_TIMES, QX_TIMESYNC, QX_TPM, QX_TRUNCATE, QX_UEFI,
    QX_UMASK, QX_UMOUNT2, QX_UNAME, QX_UNLINK, QX_UNSHARE, QX_WAIT4, QX_WRITE,
};

// ============================================================================
// Linux 兼容编号 (SYS_*) — 直接使用 Linux 标准编号
// ============================================================================

pub const SYS_read: u64 = 0;
pub const SYS_write: u64 = 1;
pub const SYS_open: u64 = 2;
pub const SYS_close: u64 = 3;
pub const SYS_stat: u64 = 4;
pub const SYS_fstat: u64 = 5;
pub const SYS_lseek: u64 = 8;
pub const SYS_mmap: u64 = 9;
pub const SYS_munmap: u64 = 11;
pub const SYS_brk: u64 = 12;
pub const SYS_rt_sigaction: u64 = 13;
pub const SYS_rt_sigprocmask: u64 = 14;
pub const SYS_ioctl: u64 = 16;
pub const SYS_pipe: u64 = 22;
pub const SYS_dup: u64 = 32;
pub const SYS_dup2: u64 = 33;
pub const SYS_nanosleep: u64 = 35;
pub const SYS_getpid: u64 = 39;
pub const SYS_fork: u64 = 57;
pub const SYS_execve: u64 = 59;
pub const SYS_exit: u64 = 60;
pub const SYS_wait4: u64 = 61;
pub const SYS_kill: u64 = 62;
pub const SYS_getdents: u64 = 78;
pub const SYS_getcwd: u64 = 79;
pub const SYS_chdir: u64 = 80;
pub const SYS_rename: u64 = 82;
pub const SYS_mkdir: u64 = 83;
pub const SYS_rmdir: u64 = 84;
pub const SYS_unlink: u64 = 87;
pub const SYS_readlink: u64 = 89;
pub const SYS_chmod: u64 = 90;
pub const SYS_gettimeofday: u64 = 96;
pub const SYS_getuid: u64 = 102;
pub const SYS_getgid: u64 = 104;
pub const SYS_sync: u64 = 162;
pub const SYS_mount: u64 = 165;
pub const SYS_umount2: u64 = 166;
pub const SYS_sched_yield: u64 = 24;
pub const SYS_exit_group: u64 = 231;
pub const SYS_futex: u64 = 202;
pub const SYS_clock_gettime: u64 = 228;
pub const SYS_CREDO_BASE: u64 = 400;

/// syscall 编号空间上界 (与 `services::syscall::types::MAX_SYSCALLS` 语义一致).
pub const MAX_SYSCALLS: u64 = 900;

/// 验证用户态指针是否在合法范围内
pub fn validate_user_ptr(ptr: u64) -> bool {
    super::validate_user_ptr(ptr)
}

/// 验证用户态缓冲区是否在合法范围内
pub fn validate_user_buf(ptr: u64, len: u64) -> bool {
    super::validate_user_buf(ptr, len)
}

/// 安全写入 u64 到用户空间指针 (先校验后写入)
pub fn write_u64_to_user(ptr: u64, val: u64) -> bool {
    super::raw::write_u64_to_user(ptr, val)
}

/// 安全从用户空间指针读取 u64 (先校验后读取)
pub fn read_u64_from_user(ptr: u64) -> Option<u64> {
    super::raw::read_u64_from_user(ptr)
}

/// 安全写入结构体到用户空间指针 (先校验后写入)
pub fn write_struct_to_user<T: Copy>(ptr: u64, src: &T) -> bool {
    super::raw::write_struct_to_user(ptr, src)
}

/// 安全从用户空间指针读取结构体 (先校验后读取)
pub fn read_struct_from_user<T: Copy>(ptr: u64, dst: &mut T) -> bool {
    super::raw::read_struct_from_user(ptr, dst)
}

/// 安全写入 rlimit (两个 u64) 到用户空间指针
pub fn write_rlimit_to_user(ptr: u64, cur: u64, max: u64) -> bool {
    super::raw::write_rlimit_to_user(ptr, cur, max)
}

/// 获取系统 ticks 计数 (ms 精度)
pub fn get_ticks() -> u64 {
    super::raw::get_ticks()
}

/// nanosleep 系统调用实现 (TCB: 操作 hrtimer + 调度器)
pub fn sys_nanosleep(req: u64, rem: u64) -> i64 {
    super::sys_nanosleep(req, rem)
}

/// kill 系统调用实现 (TCB: 操作进程信号位)
pub fn sys_kill(pid: i32, sig: i32) -> i64 {
    super::sys_kill(pid, sig)
}

/// `rt_sigaction` 系统调用实现 (TCB: 操作 sigaction 表)
pub fn sys_rt_sigaction(signum: i32, act: u64, oact: u64) -> i64 {
    super::sys_rt_sigaction(signum, act, oact)
}

/// `rt_sigprocmask` 系统调用实现 (TCB: 操作信号掩码)
pub fn sys_rt_sigprocmask(how: i32, set: u64, oset: u64) -> i64 {
    super::sys_rt_sigprocmask(how, set, oset)
}

/// P1-I-45: sigaltstack 系统调用实现 (TCB: 替代栈注册/查询)
pub fn sys_sigaltstack(ss: u64, old_ss: u64) -> i64 {
    super::sys_sigaltstack(ss, old_ss)
}

/// reboot 机制: cmd=0 停机, cmd=1 重启 (TCB: 操作 IDT/PSCI)
pub fn reboot_mechanism(cmd: i32) -> i64 {
    match cmd {
        0 => loop {},
        1 => match () {
            #[cfg(target_arch = "x86_64")]
            // SAFETY: reboot_via_idt 仅在 x86_64 停机/重启路径调用,
            // 此时无其他线程运行, IDT 操作安全
            () => unsafe { super::raw::reboot_via_idt() },
            #[cfg(target_arch = "aarch64")]
            // SAFETY: reboot_via_psci 仅在 aarch64 停机/重启路径调用,
            // PSCI SMC 调用由固件保证幂等
            () => unsafe { super::raw::reboot_via_psci() },
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            () => loop {},
        },
        _ => crate::kernel::framework::syscall::Errno::EINVAL.as_ret(),
    }
}

/// mmap 机制: 获取当前进程 mm 或分配裸页 (TCB: 操作页分配器)
pub fn mmap_get_mm_or_alloc(size: u64) -> Option<*mut u8> {
    if crate::kernel::framework::mm::vma_get_current_mm().is_some() {
        return None; // 有 mm, 走 VMA 路径
    }
    let pages = size.div_ceil(crate::kernel::framework::mm::PAGE_SIZE);
    // SAFETY: alloc_pages 在无 mm 时由 mmap 路径调用,
    // pages 由 size 向上取整计算, 不会溢出
    let ptr = unsafe { super::raw::alloc_pages(pages) };
    if ptr.is_null() { None } else { Some(ptr) }
}

/// munmap 机制: 无 mm 时释放裸页 (TCB: 操作页分配器)
pub fn munmap_free_pages(addr: u64, size: u64) {
    let pages = size.div_ceil(crate::kernel::framework::mm::PAGE_SIZE);
    // SAFETY: free_pages 在无 mm 时由 munmap 路径调用,
    // addr 来自之前的 alloc_pages 返回值, pages 计算与分配时一致
    unsafe { super::raw::free_pages(addr as *mut u8, pages) }
}
