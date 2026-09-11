#![deny(unsafe_code)]
//! 系统调用分发策略 — services 层
//!
//! T5-1: 将 syscall 号 → 处理函数映射表 (分发策略) 从 framework 提取到 services.
//! framework 仅保留入口汇编 + unsafe 边界, services 拥有完整的分发映射.
//!
//! ## 架构
//!
//! ```text
//! 用户态 → framework 入口 (syscall/sysret 汇编)
//!        → framework::syscall_dispatch_from_frame (unsafe 边界)
//!        → services::syscall::dispatch::ServicesSyscallDispatch::dispatch (策略)
//!        → framework 回退 (未迁移的 syscall)
//! ```
//!
//! ## 迁移状态
//!
//! - 已迁移: 文件 I/O, 文件系统, 内存管理, 进程, 信号, 网络, 凭证, 同步, 定时器,
//!   事件轮询, eventfd/signalfd/timerfd, Credo 私有 syscall, 存储设备, inotify,
//!   内存建议与锁定, 进程创建/等待, 系统信息, CPU 亲和性, 进程优先级
//! - 待迁移: read/write, execve, firmware, ftrace/kgdb, seccomp/prctl,
//!   路由/Netfilter, `io_uring`, namespace, cgroup, NUMA, eBPF, PM, TPM,
//!   CET, tickless, timesync, kexec, UEFI, 帧缓冲, sendfile/splice 等
//!
//! 评估日期: 2026-06-19

use crate::kernel::framework::syscall::dispatch_trait::{
    SyscallDispatch, register_syscall_dispatch,
};
use crate::kernel::framework::syscall::Errno;

// ============================================================================
// 辅助函数
// ============================================================================

/// 将 services 层 Result 转为 i64 返回码
#[inline]
fn as_ret(r: Result<usize, Errno>) -> i64 {
    match r {
        Ok(v) => v as i64,
        Err(e) => e.as_ret(),
    }
}

// ============================================================================
// services 层系统调用分发策略
// ============================================================================

/// services 层系统调用分发策略
///
/// L-01: 已从 framework 迁移的 syscall 分支在此分发.
/// 返回 -ENOSYS (-38) 表示未处理, framework 回退处理.
pub struct ServicesSyscallDispatch;

impl SyscallDispatch for ServicesSyscallDispatch {
    fn dispatch(&self, num: u64, args: [u64; 6]) -> i64 {
        // M4: 按子系统拆分巨型 match，提高可读性和可维护性
        // 尝试各子系统分发函数，返回第一个匹配的结果
        if let Some(ret) = dispatch_fs(num, args) {
            return ret;
        }
        if let Some(ret) = dispatch_proc(num, args) {
            return ret;
        }
        if let Some(ret) = dispatch_net(num, args) {
            return ret;
        }
        if let Some(ret) = dispatch_mm(num, args) {
            return ret;
        }
        if let Some(ret) = dispatch_sync(num, args) {
            return ret;
        }
        if let Some(ret) = dispatch_credo(num, args) {
            return ret;
        }
        if let Some(ret) = dispatch_other(num, args) {
            return ret;
        }

        // 未匹配的 syscall — 返回 -ENOSYS 让 framework 回退处理
        crate::kernel::services::syscall::types::ENOSYS_RET
    }
}

// ============================================================================
// 子系统分发函数
// ============================================================================

#[expect(
    clippy::match_same_arms,
    reason = "match_same_arms: match arm 重复是为可读性/调试断点; 当前优先 expect"
)]
/// 文件系统相关系统调用
fn dispatch_fs(num: u64, args: [u64; 6]) -> Option<i64> {
    use crate::kernel::services::syscall::types::{
        QX_GETXATTR, QX_LISTXATTR, QX_REMOVEXATTR, QX_SETXATTR, QX_SNAPSHOT_CLONE,
        QX_SNAPSHOT_CREATE, QX_SNAPSHOT_DESTROY, QX_SNAPSHOT_ROLLBACK, SYS_access, SYS_alarm,
        SYS_chdir, SYS_chmod, SYS_chown, SYS_clock_gettime, SYS_close, SYS_copy_file_range,
        SYS_creat, SYS_dup, SYS_dup2, SYS_dup3, SYS_faccessat, SYS_fchmod, SYS_fchmodat,
        SYS_fchown, SYS_fcntl, SYS_flock, SYS_fstat, SYS_fsync, SYS_ftruncate, SYS_getcwd,
        SYS_getdents, SYS_getitimer, SYS_inotify_add_watch, SYS_inotify_init1,
        SYS_inotify_rm_watch, SYS_ioctl, SYS_link, SYS_linkat, SYS_lseek, SYS_lstat, SYS_mkdir,
        SYS_mount, SYS_name_to_handle_at, SYS_newfstatat, SYS_open, SYS_open_by_handle_at,
        SYS_openat, SYS_pipe, SYS_pipe2, SYS_poll, SYS_readlink, SYS_readlinkat, SYS_rename,
        SYS_renameat, SYS_rmdir, SYS_select, SYS_setitimer, SYS_stat, SYS_symlink, SYS_symlinkat,
        SYS_sync, SYS_time, SYS_times, SYS_truncate, SYS_umask, SYS_umount2, SYS_unlink,
        SYS_unlinkat,
    };
    let [a0, a1, a2, a3, a4, a5] = args;

    Some(match num {
        // 文件 I/O
        SYS_open => as_ret(crate::kernel::services::fs::open::open_syscall(
            a0, a1 as i32, a2 as i32,
        )),
        SYS_close => as_ret(crate::kernel::services::fs::open::close_syscall(a0 as i32)),
        SYS_stat => as_ret(crate::kernel::services::fs::stat::stat_syscall(a0, a1)),
        SYS_fstat => as_ret(crate::kernel::services::fs::stat::fstat_syscall(
            a0 as i32, a1,
        )),
        SYS_lstat => as_ret(crate::kernel::services::fs::stat::lstat_syscall(a0, a1)),
        SYS_creat => as_ret(crate::kernel::services::fs::open::creat_syscall(
            a0, a2 as i32,
        )),

        // 文件系统操作
        SYS_mkdir => as_ret(crate::kernel::services::fs::mode::mkdir_syscall(
            a0, a1 as i32,
        )),
        SYS_rmdir => as_ret(crate::kernel::services::fs::mode::rmdir_syscall(a0)),
        SYS_chmod => as_ret(crate::kernel::services::fs::mode::chmod_syscall(
            a0, a1 as u32,
        )),
        SYS_fchmod => as_ret(crate::kernel::services::fs::mode::fchmod_syscall(
            a0 as i32, a1 as u32,
        )),
        SYS_umask => as_ret(crate::kernel::services::fs::mode::umask_syscall(a0 as u32)),
        SYS_access => as_ret(crate::kernel::services::fs::access::access_syscall(
            a0, a1 as i32,
        )),
        SYS_unlink => as_ret(crate::kernel::services::fs::access::unlink_syscall(a0)),
        SYS_rename => as_ret(crate::kernel::services::fs::misc::rename_syscall(a0, a1)),
        SYS_symlink => as_ret(crate::kernel::services::fs::link::symlink_syscall(a0, a1)),
        SYS_readlink => as_ret(crate::kernel::services::fs::link::readlink_syscall(
            a0, a1, a2,
        )),
        SYS_link => as_ret(crate::kernel::services::fs::link::link_syscall(a0, a1)),

        // *at() 系列
        SYS_openat => as_ret(crate::kernel::services::fs::open::open_syscall(
            a1, a2 as i32, a3 as i32,
        )),
        SYS_newfstatat => as_ret(crate::kernel::services::fs::stat::fstat_syscall(
            a1 as i32, a2,
        )),
        SYS_unlinkat => as_ret(crate::kernel::services::fs::access::unlink_syscall(a1)),
        SYS_renameat => as_ret(crate::kernel::services::fs::misc::rename_syscall(a1, a3)),
        SYS_linkat => as_ret(crate::kernel::services::fs::link::link_syscall(a1, a3)),
        SYS_symlinkat => as_ret(crate::kernel::services::fs::link::symlink_syscall(a0, a2)),
        SYS_readlinkat => as_ret(crate::kernel::services::fs::link::readlink_syscall(
            a1, a2, a3,
        )),
        SYS_fchmodat => as_ret(crate::kernel::services::fs::mode::chmod_syscall(
            a1, a2 as u32,
        )),
        SYS_faccessat => as_ret(crate::kernel::services::fs::access::faccessat_syscall(
            a0 as i32, a1, a2 as i32, a3 as i32,
        )),
        SYS_fchown => as_ret(crate::kernel::services::fs::misc::fchown_syscall(
            a0 as i32, a1, a2,
        )),

        // 同步与挂载
        SYS_sync => as_ret(crate::kernel::services::fs::misc::sync_syscall()),
        SYS_fsync => as_ret(crate::kernel::services::fs::misc::fsync_syscall(a0 as i32)),
        SYS_mount => as_ret(crate::kernel::services::fs::mount::mount_syscall(
            a0, a1, a2,
        )),
        SYS_umount2 => as_ret(crate::kernel::services::fs::mount::umount2_syscall(
            a0, a1 as i32,
        )),

        // 路径
        SYS_getcwd => as_ret(crate::kernel::services::fs::path::getcwd_syscall(a0, a1)),
        SYS_chdir => as_ret(crate::kernel::services::fs::path::chdir_syscall(a0)),

        // 文件描述符操作
        SYS_pipe => as_ret(crate::kernel::services::fs::io::pipe_syscall(a0)),
        SYS_pipe2 => as_ret(crate::kernel::services::fs::io::pipe2_syscall(
            a0, a2 as i32,
        )),
        SYS_dup => as_ret(crate::kernel::services::fs::io::dup_syscall(a0 as i32)),
        SYS_dup2 => as_ret(crate::kernel::services::fs::io::dup2_syscall(
            a0 as i32, a1 as i32,
        )),
        SYS_dup3 => as_ret(crate::kernel::services::fs::io::dup3_syscall(
            a0 as i32, a1 as i32, a2 as i32,
        )),
        SYS_fcntl => as_ret(crate::kernel::services::fs::io::fcntl_syscall(
            a0 as i32, a1 as i32, a2,
        )),

        // 文件操作
        SYS_ioctl => crate::kernel::services::fs::file_ops::ioctl_syscall(a0 as i32, a1, a2),
        SYS_poll => crate::kernel::services::fs::file_ops::poll_syscall(a0, a1 as u32, a2 as i32),
        SYS_select => crate::kernel::services::fs::file_ops::poll_syscall(a0, a1 as u32, a2 as i32),
        SYS_chown => crate::kernel::services::fs::file_ops::chown_syscall(a0, a1 as u32, a2 as u32),
        SYS_truncate => crate::kernel::services::fs::file_ops::truncate_syscall(a0, a1 as i64),
        SYS_ftruncate => {
            crate::kernel::services::fs::file_ops::ftruncate_syscall(a0 as i32, a1 as i64)
        }
        SYS_flock => crate::kernel::services::fs::file_ops::flock_syscall(a0 as i32, a1 as i32),
        SYS_lseek => {
            crate::kernel::services::fs::dir_ops::lseek_syscall(a0 as i32, a1 as i64, a2 as i32)
        }
        SYS_getdents => crate::kernel::services::fs::dir_ops::getdents_syscall(a0 as i32, a1, a2),

        // inotify
        SYS_inotify_init1 => crate::kernel::services::fs::inotify::sys_inotify_init1(a0 as i32),
        SYS_inotify_add_watch => crate::kernel::services::fs::inotify::sys_inotify_add_watch(
            a0 as i64, a1 as u32, a2 as u32,
        ),
        SYS_inotify_rm_watch => {
            crate::kernel::services::fs::inotify::sys_inotify_rm_watch(a0 as i64, a1 as i32)
        }

        // 时间与统计
        SYS_clock_gettime => {
            crate::kernel::services::timer::clock::clock_gettime_syscall(a0 as i32, a1)
        }
        SYS_times => as_ret(crate::kernel::services::fs::misc::times_syscall(a0)),
        SYS_time => as_ret(crate::kernel::services::fs::misc::time_syscall(a0)),
        SYS_getitimer => as_ret(crate::kernel::services::fs::misc::getitimer_syscall(
            a0 as i32, a1,
        )),
        SYS_alarm => as_ret(crate::kernel::services::fs::misc::alarm_syscall(a0 as u32)),
        SYS_setitimer => as_ret(crate::kernel::services::fs::misc::setitimer_syscall(
            a0 as i32, a1, a2,
        )),

        // 高级文件操作
        SYS_copy_file_range => as_ret(crate::kernel::services::fs::io::copy_file_range_syscall(
            a0 as i32,
            a1,
            a2 as i32,
            a3,
            a4 as usize,
        )),
        SYS_name_to_handle_at => {
            // 显式错误透传: 具体 Errno 而非通用负值 (B05-41 返工)
            match crate::kernel::services::fs::file_handle::name_to_handle_at_syscall(
                a0 as i32, a1, a2 as i32, a3, a4 as u64, a5 as u32,
            ) {
                Ok(v) => v,
                Err(e) => e.as_ret(),
            }
        }
        SYS_open_by_handle_at => {
            // 显式错误透传: 具体 Errno 而非通用负值 (B05-41 返工)
            match crate::kernel::services::fs::file_handle::open_by_handle_at_syscall(
                a0 as i32, a1, a2 as i32, a3 as u32,
            ) {
                Ok(v) => v,
                Err(e) => e.as_ret(),
            }
        }

        // 扩展属性
        QX_SETXATTR => as_ret(crate::kernel::services::fs::xattr::setxattr_syscall(
            a0,
            a1,
            a2,
            a3 as usize,
            a5,
        )),
        QX_GETXATTR => as_ret(crate::kernel::services::fs::xattr::getxattr_syscall(
            a0,
            a1,
            a2,
            a3 as usize,
            a5,
        )),
        QX_LISTXATTR => as_ret(crate::kernel::services::fs::xattr::listxattr_syscall(
            a0,
            a1,
            a2 as usize,
            a4,
        )),
        QX_REMOVEXATTR => as_ret(crate::kernel::services::fs::xattr::removexattr_syscall(
            a0, a1, a4,
        )),

        // 快照
        QX_SNAPSHOT_CREATE => {
            as_ret(crate::kernel::services::fs::snapshot::snapshot_create_syscall(a0))
        }
        QX_SNAPSHOT_DESTROY => {
            as_ret(crate::kernel::services::fs::snapshot::snapshot_destroy_syscall(a0))
        }
        QX_SNAPSHOT_ROLLBACK => {
            as_ret(crate::kernel::services::fs::snapshot::snapshot_rollback_syscall(a0))
        }
        QX_SNAPSHOT_CLONE => {
            as_ret(crate::kernel::services::fs::snapshot::snapshot_clone_syscall(a0, a1))
        }

        _ => return None,
    })
}

#[expect(
    clippy::match_same_arms,
    reason = "match_same_arms: match arm 重复是为可读性/调试断点; 当前优先 expect"
)]
/// 进程相关系统调用
fn dispatch_proc(num: u64, args: [u64; 6]) -> Option<i64> {
    use crate::kernel::services::syscall::types::{
        SYS_clone, SYS_clone3, SYS_exit, SYS_exit_group, SYS_fork, SYS_getpgid, SYS_getpid,
        SYS_getppid, SYS_getpriority, SYS_getrlimit, SYS_getrusage, SYS_getsid, SYS_gettid,
        SYS_gettimeofday, SYS_kill, SYS_memfd_create, SYS_nanosleep, SYS_nice, SYS_pidfd_getfd,
        SYS_pidfd_open, SYS_pidfd_send_signal, SYS_reboot, SYS_rt_sigaction, SYS_rt_sigprocmask,
        SYS_sched_getaffinity, SYS_sched_setaffinity, SYS_sched_yield, SYS_sethostname, SYS_setpgid,
        SYS_setpriority, SYS_setsid, SYS_sysinfo, SYS_uname, SYS_wait4,
    };
    let [a0, a1, a2, a3, a4, _a5] = args;

    Some(match num {
        // 进程信息
        SYS_getpid => crate::kernel::services::proc::info::getpid_syscall() as i64,
        SYS_getppid => crate::kernel::services::proc::info::getppid_syscall() as i64,
        SYS_getpgid => as_ret(crate::kernel::services::proc::info::getpgid_syscall(
            a0 as i32,
        )),
        SYS_gettid => crate::kernel::services::proc::info::gettid_syscall() as i64,
        SYS_setsid => crate::kernel::services::proc::session::proc_setsid(),
        SYS_getsid => crate::kernel::services::proc::session::proc_getsid(a0 as i32),
        SYS_setpgid => crate::kernel::services::proc::session::proc_setpgid(a0 as i32, a1 as i32),

        // 信号
        SYS_rt_sigaction => as_ret(crate::kernel::services::proc::signal::rt_sigaction_syscall(
            a0 as i32, a1, a2,
        )),
        SYS_rt_sigprocmask => {
            as_ret(crate::kernel::services::proc::signal::rt_sigprocmask_syscall(a0 as i32, a1, a2))
        }
        SYS_kill => as_ret(crate::kernel::services::proc::signal::kill_syscall(
            a0 as i32, a1 as i32,
        )),

        // 进程优先级
        SYS_nice => crate::kernel::services::proc::priority::nice_syscall(a0 as i32),
        SYS_getpriority => {
            crate::kernel::services::proc::priority::getpriority_syscall(a0 as i32, a1 as u32)
        }
        SYS_setpriority => crate::kernel::services::proc::priority::setpriority_syscall(
            a0 as i32, a1 as u32, a2 as i32,
        ),

        // CPU 亲和性
        SYS_sched_setaffinity => {
            crate::kernel::services::proc::affinity::sched_setaffinity_syscall(
                a0 as i32, a1 as u32, a2,
            )
        }
        SYS_sched_getaffinity => {
            crate::kernel::services::proc::affinity::sched_getaffinity_syscall(
                a0 as i32, a1 as u32, a2,
            )
        }

        // 进程生命周期
        SYS_fork => crate::kernel::services::proc::lifecycle::fork_syscall(),
        SYS_exit => crate::kernel::services::proc::lifecycle::exit_syscall(a0 as i32),
        // SIMPLIFIED: exit_group 暂等同 exit (B05-43 返工登记); 影响面: 线程组未实现
        // 组级终止, 仅结束当前进程; 何时需扩展: 引入 tgid/线程组基础结构后遍历组内
        // 全部线程终止 (审查 DECISION-071 关联).
        SYS_exit_group => crate::kernel::services::proc::lifecycle::exit_syscall(a0 as i32),
        SYS_sched_yield => crate::kernel::services::proc::lifecycle::sched_yield_syscall(),

        // 系统信息
        SYS_getrusage => crate::kernel::services::proc::sysinfo::getrusage_syscall(a0 as i32, a1),
        SYS_sysinfo => crate::kernel::services::proc::sysinfo::sysinfo_syscall(a0),
        SYS_getrlimit => crate::kernel::services::proc::sysinfo::getrlimit_syscall(a0 as i32, a1),
        SYS_uname => as_ret(crate::kernel::services::proc::info::uname_syscall(a0)),
        SYS_gettimeofday => as_ret(crate::kernel::services::timer::clock::gettimeofday_syscall(
            a0,
        )),

        // 定时器
        SYS_nanosleep => as_ret(crate::kernel::services::timer::sleep::nanosleep_syscall(
            a0, a1,
        )),

        // 进程创建/等待
        SYS_clone => as_ret(crate::kernel::services::proc::clone::clone_syscall(
            a0, a1, a2, a3, a4,
        )),
        SYS_clone3 => {
            // clone3(2): 首个参数指向用户空间 `struct clone_args`.
            // SIMPLIFIED: 仅提取 flags/stack/parent_tid/child_tid/tls 五个字段委托
            // `clone_syscall`, 忽略 pidfd/set_tid/cgroup/exit_signal 等高级字段;
            // 影响面: 使用这些高级字段的调用方 (如线程库 clone3 路径) 语义不完整;
            // 何时需扩展: 完整实现 clone3 (独立 sys_clone3 机制, 支持全部字段) 后替换.
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct CloneArgs {
                flags: u64,
                pidfd: u64,
                child_tid: u64,
                parent_tid: u64,
                exit_signal: u64,
                stack: u64,
                stack_size: u64,
                tls: u64,
            }
            let mut args = CloneArgs {
                flags: 0,
                pidfd: 0,
                child_tid: 0,
                parent_tid: 0,
                exit_signal: 0,
                stack: 0,
                stack_size: 0,
                tls: 0,
            };
            if !crate::kernel::framework::syscall::api::read_struct_from_user(a0, &mut args) {
                return Some(Errno::EFAULT.as_ret());
            }
            as_ret(crate::kernel::services::proc::clone::clone_syscall(
                args.flags,
                args.stack,
                args.parent_tid,
                args.child_tid,
                args.tls,
            ))
        }
        SYS_wait4 => as_ret(crate::kernel::services::proc::wait4::wait4_syscall(
            a0 as i32, a1, a2 as i32,
        )),

        // 系统信息
        SYS_reboot => crate::kernel::services::proc::sysinfo::reboot_syscall(a0 as i32),
        SYS_sethostname => crate::kernel::services::proc::sysinfo::sethostname_syscall(a0, a1),

        // memfd
        SYS_memfd_create => as_ret(crate::kernel::services::proc::memfd::memfd_create_syscall(
            a0, a1 as u32,
        )),

        // pidfd
        SYS_pidfd_open => as_ret(crate::kernel::services::proc::pidfd::pidfd_open(
            a0 as u32, a1 as u32,
        )),
        SYS_pidfd_send_signal => as_ret(crate::kernel::services::proc::pidfd::pidfd_send_signal(
            a0 as u32, a1 as i32, a2, a3 as u32,
        )),
        SYS_pidfd_getfd => as_ret(crate::kernel::services::proc::pidfd::pidfd_getfd(
            a0 as u32, a1 as u32, a2 as u32,
        )),

        _ => return None,
    })
}

/// 网络相关系统调用
fn dispatch_net(num: u64, args: [u64; 6]) -> Option<i64> {
    use crate::kernel::services::syscall::types::{
        SYS_accept, SYS_bind, SYS_connect, SYS_getpeername, SYS_getsockname, SYS_getsockopt,
        SYS_listen, SYS_recvfrom, SYS_recvmsg, SYS_sendmsg, SYS_sendto, SYS_setsockopt,
        SYS_shutdown, SYS_socket,
    };
    let [a0, a1, a2, a3, a4, a5] = args;

    Some(match num {
        SYS_socket => as_ret(crate::kernel::services::net::syscall::socket_syscall(
            a0 as i32, a1 as i32, a2 as i32,
        )),
        SYS_connect => as_ret(crate::kernel::services::net::syscall::connect_syscall(
            a0 as i32, a1, a2 as u32,
        )),
        SYS_accept => as_ret(crate::kernel::services::net::syscall::accept_syscall(
            a0 as i32, a1, a2,
        )),
        SYS_sendto => as_ret(crate::kernel::services::net::syscall::sendto_syscall(
            a0 as i32, a1, a2 as u32, a3 as i32, a4, a5 as u32,
        )),
        SYS_recvfrom => as_ret(crate::kernel::services::net::syscall::recvfrom_syscall(
            a0 as i32, a1, a2 as u32, a3 as i32, a4, a5,
        )),
        SYS_shutdown => as_ret(crate::kernel::services::net::syscall::shutdown_syscall(
            a0 as i32, a1 as i32,
        )),
        SYS_bind => as_ret(crate::kernel::services::net::syscall::bind_syscall(
            a0 as i32, a1, a2 as u32,
        )),
        SYS_listen => as_ret(crate::kernel::services::net::syscall::listen_syscall(
            a0 as i32, a1 as i32,
        )),
        SYS_sendmsg => as_ret(crate::kernel::services::net::syscall::sendmsg_syscall(
            a0 as i32, a1, a2 as i32,
        )),
        SYS_recvmsg => as_ret(crate::kernel::services::net::syscall::recvmsg_syscall(
            a0 as i32, a1, a2 as i32,
        )),
        SYS_setsockopt => as_ret(crate::kernel::services::net::syscall::setsockopt_syscall(
            a0 as i32, a1 as i32, a2 as i32, a3, a4 as u32,
        )),
        SYS_getsockopt => as_ret(crate::kernel::services::net::syscall::getsockopt_syscall(
            a0 as i32, a1 as i32, a2 as i32, a3, a4,
        )),
        SYS_getsockname => {
            as_ret(crate::kernel::services::net::syscall::getsockname_syscall(
                a0 as i32, a1, a2,
            ))
        }
        SYS_getpeername => {
            as_ret(crate::kernel::services::net::syscall::getpeername_syscall(
                a0 as i32, a1, a2,
            ))
        }

        _ => return None,
    })
}

/// 内存管理相关系统调用
fn dispatch_mm(num: u64, args: [u64; 6]) -> Option<i64> {
    use crate::kernel::services::syscall::types::{
        SYS_brk, SYS_get_mempolicy, SYS_getcpu, SYS_madvise, SYS_migrate_pages, SYS_mincore,
        SYS_mlock, SYS_mlockall, SYS_mmap, SYS_mprotect, SYS_munlock, SYS_munlockall, SYS_munmap,
        SYS_set_mempolicy,
    };
    let [a0, a1, a2, a3, a4, a5] = args;

    Some(match num {
        // 基础内存管理
        SYS_mprotect => as_ret(crate::kernel::services::mm::mprotect::mprotect_syscall(
            a0, a1, a2 as i32,
        )),
        SYS_brk => as_ret(crate::kernel::services::mm::brk::brk_syscall(a0)),

        // mmap 系列
        SYS_mmap => crate::kernel::services::mm::mmap::mmap_syscall_entry(
            a0, a1, a2 as i32, a3 as i32, a4 as i32, a5,
        ),
        SYS_munmap => crate::kernel::services::mm::mmap::munmap_syscall_entry(a0, a1),

        // 内存建议与锁定
        SYS_madvise => crate::kernel::services::mm::madvise_mlock::sys_madvise(a0, a1, a2),
        SYS_mlock => crate::kernel::services::mm::madvise_mlock::sys_mlock(a0, a1),
        SYS_munlock => crate::kernel::services::mm::madvise_mlock::sys_munlock(a0, a1),
        SYS_mlockall => crate::kernel::services::mm::madvise_mlock::sys_mlockall(a0),
        SYS_munlockall => crate::kernel::services::mm::madvise_mlock::sys_munlockall(),
        SYS_mincore => crate::kernel::services::mm::madvise_mlock::sys_mincore(a0, a1, a2),

        // NUMA
        SYS_get_mempolicy => crate::kernel::services::mm::numa::sys_get_mempolicy(a0, a1),
        SYS_set_mempolicy => crate::kernel::services::mm::numa::sys_set_mempolicy(a0, a1),
        SYS_migrate_pages => crate::kernel::services::mm::numa::sys_migrate_pages(a0),
        SYS_getcpu => crate::kernel::services::mm::numa::sys_getcpu(),

        _ => return None,
    })
}

#[expect(
    clippy::match_same_arms,
    reason = "match_same_arms: match arm 重复是为可读性/调试断点; 当前优先 expect"
)]
/// 同步原语相关系统调用
fn dispatch_sync(num: u64, args: [u64; 6]) -> Option<i64> {
    use crate::kernel::services::syscall::types::{
        SYS_epoll_create, SYS_epoll_create1, SYS_epoll_ctl, SYS_epoll_wait, SYS_eventfd,
        SYS_eventfd2, SYS_futex, SYS_signalfd, SYS_signalfd4, SYS_timerfd_create,
        SYS_timerfd_gettime, SYS_timerfd_settime,
    };
    let [a0, a1, a2, a3, a4, _a5] = args;

    Some(match num {
        // futex
        SYS_futex => {
            match crate::kernel::services::sync::futex::futex_syscall(
                a0, a1 as i32, a2 as i32, a3, a4 as u32,
            ) {
                Ok(crate::kernel::services::sync::futex::FutexResult::Woken) => 0,
                Ok(crate::kernel::services::sync::futex::FutexResult::WokenCount(n)) => {
                    i64::from(n)
                }
                Ok(crate::kernel::services::sync::futex::FutexResult::Requeued {
                    woken, ..
                }) => i64::from(woken),
                Ok(crate::kernel::services::sync::futex::FutexResult::Pending) => 0,
                Err(e) => e.as_ret(),
            }
        }

        // epoll
        SYS_epoll_create => as_ret(crate::kernel::services::sync::epoll::epoll_create_syscall(
            a0 as i32,
        )),
        SYS_epoll_create1 => as_ret(crate::kernel::services::sync::epoll::epoll_create_syscall(
            a0 as i32,
        )),
        SYS_epoll_ctl => as_ret(crate::kernel::services::sync::epoll::epoll_ctl_syscall(
            a0 as i64, a1 as i32, a2 as i32, a3,
        )),
        SYS_epoll_wait => as_ret(crate::kernel::services::sync::epoll::epoll_wait_syscall(
            a0 as i64, a1, a2 as i32, a3 as i32,
        )),

        // eventfd
        SYS_eventfd => as_ret(crate::kernel::services::sync::eventfd::eventfd_syscall(
            a0, a1 as i32,
        )),
        SYS_eventfd2 => as_ret(crate::kernel::services::sync::eventfd::eventfd_syscall(
            a0, a1 as i32,
        )),

        // signalfd
        SYS_signalfd => as_ret(crate::kernel::services::sync::signalfd::signalfd_syscall(
            a0 as i32, a1, a2 as i32,
        )),
        SYS_signalfd4 => as_ret(crate::kernel::services::sync::signalfd::signalfd_syscall(
            a0 as i32, a1, a2 as i32,
        )),

        // timerfd
        SYS_timerfd_create => as_ret(
            crate::kernel::services::timer::timerfd::timerfd_create_syscall(a0 as i32, a1 as i32),
        ),
        SYS_timerfd_settime => as_ret(
            crate::kernel::services::timer::timerfd::timerfd_settime_syscall(
                a0 as i32, a1 as i32, a2, a3,
            ),
        ),
        SYS_timerfd_gettime => {
            as_ret(crate::kernel::services::timer::timerfd::timerfd_gettime_syscall(a0 as i32, a1))
        }

        _ => return None,
    })
}

/// Credo 私有系统调用
fn dispatch_credo(num: u64, args: [u64; 6]) -> Option<i64> {
    use crate::kernel::services::syscall::types::{
        SYS_CREDO_BOOT_CHECK, SYS_CREDO_CHANGE_PASSWORD, SYS_CREDO_CHECK_CAP,
        SYS_CREDO_CREATE_FIRST, SYS_CREDO_CREATE_IDENTITY, SYS_CREDO_DELETE_IDENTITY,
        SYS_CREDO_DISK_FORMAT, SYS_CREDO_DISK_INFO, SYS_CREDO_DISK_LIST, SYS_CREDO_DISK_PARTITION,
        SYS_CREDO_FAT_FORMAT, SYS_CREDO_GET_CAPS, SYS_CREDO_GET_PWM, SYS_CREDO_GETHOSTNAME,
        SYS_CREDO_GRANT, SYS_CREDO_IDENTITY_INFO, SYS_CREDO_LOGIN, SYS_CREDO_LOGOUT,
        SYS_CREDO_PROC_CPUTIME, SYS_CREDO_PROC_LIST, SYS_CREDO_PROC_SETPRI, SYS_CREDO_PROC_SLEEP,
        SYS_CREDO_REBOOT, SYS_CREDO_REVOKE, SYS_CREDO_SET_PWM, SYS_CREDO_SETHOSTNAME,
        SYS_CREDO_VERIFY_PASSWORD, SYS_getegid, SYS_geteuid, SYS_getgid, SYS_getuid, SYS_setegid,
        SYS_seteuid, SYS_setgid, SYS_setregid, SYS_setreuid, SYS_setuid,
    };
    let [a0, a1, a2, a3, _a4, _a5] = args;

    Some(match num {
        // 凭证 - UID/GID
        SYS_getuid => as_ret(crate::kernel::services::credo::uid::getuid_syscall()),
        SYS_getgid => as_ret(crate::kernel::services::credo::uid::getgid_syscall()),
        SYS_setuid => as_ret(crate::kernel::services::credo::uid::setuid_syscall(
            a0 as u32,
        )),
        SYS_setgid => as_ret(crate::kernel::services::credo::uid::setgid_syscall(
            a0 as u32,
        )),
        SYS_geteuid => as_ret(crate::kernel::services::credo::uid::geteuid_syscall()),
        SYS_getegid => as_ret(crate::kernel::services::credo::uid::getegid_syscall()),
        SYS_seteuid => as_ret(crate::kernel::services::credo::uid::seteuid_syscall(
            a0 as u32,
        )),
        SYS_setegid => as_ret(crate::kernel::services::credo::uid::setegid_syscall(
            a0 as u32,
        )),
        SYS_setreuid => as_ret(crate::kernel::services::credo::uid::setreuid_syscall(
            a0 as u32, a1 as u32,
        )),
        SYS_setregid => as_ret(crate::kernel::services::credo::uid::setregid_syscall(
            a0 as u32, a1 as u32,
        )),

        // Credo 认证
        SYS_CREDO_LOGIN => crate::kernel::services::credo::auth::auth_login_syscall(a0, a1),
        SYS_CREDO_LOGOUT => crate::kernel::services::credo::auth::auth_logout_syscall(),
        SYS_CREDO_CREATE_IDENTITY => {
            crate::kernel::services::credo::auth::auth_create_syscall(a0, a1, a2 as u8)
        }
        SYS_CREDO_DELETE_IDENTITY => crate::kernel::services::credo::auth::auth_delete_syscall(a0),
        SYS_CREDO_IDENTITY_INFO => crate::kernel::services::credo::auth::auth_info_syscall(a0),
        SYS_CREDO_CHANGE_PASSWORD => {
            crate::kernel::services::credo::auth::auth_changepw_syscall(a0, a1)
        }
        SYS_CREDO_VERIFY_PASSWORD => crate::kernel::services::credo::auth::auth_verify_syscall(a0),
        SYS_CREDO_CREATE_FIRST => {
            crate::kernel::services::credo::auth::auth_create_first_syscall(a0)
        }
        SYS_CREDO_GRANT => {
            crate::kernel::services::credo::auth::auth_grant_syscall(a0, a1, a2 as u16, a3)
        }
        SYS_CREDO_REVOKE => {
            crate::kernel::services::credo::auth::auth_revoke_syscall(a0, a1, a2 as u16, a3)
        }
        SYS_CREDO_CHECK_CAP => {
            crate::kernel::services::credo::auth::auth_check_cap_syscall(a0, a1 as u16, a2)
        }
        SYS_CREDO_GET_CAPS => {
            crate::kernel::services::credo::auth::auth_get_caps_syscall(a0, a1 as u16)
        }
        SYS_CREDO_GET_PWM => crate::kernel::services::credo::auth::pwm_get_syscall(),
        SYS_CREDO_SET_PWM => crate::kernel::services::credo::auth::pwm_set_syscall(a0),

        // Credo 系统信息
        SYS_CREDO_GETHOSTNAME => {
            crate::kernel::services::proc::sysinfo::gethostname_syscall(a0, a1)
        }
        SYS_CREDO_SETHOSTNAME => {
            crate::kernel::services::proc::sysinfo::sethostname_syscall(a0, a1)
        }
        SYS_CREDO_BOOT_CHECK => {
            crate::kernel::services::proc::sysinfo::boot_check_syscall(a0 as i32)
        }
        SYS_CREDO_PROC_LIST => {
            crate::kernel::services::proc::proc_mgmt::proc_list_syscall(a0, a1 as u32)
        }
        SYS_CREDO_PROC_SETPRI => {
            crate::kernel::services::proc::proc_mgmt::proc_setpri_syscall(a0 as u32, a1 as u32)
        }
        SYS_CREDO_PROC_CPUTIME => {
            crate::kernel::services::proc::proc_mgmt::credo_proc_cputime_syscall(a0 as u32)
        }
        SYS_CREDO_PROC_SLEEP => {
            // 单位约定: 输入为毫秒 (Credo 策略), 底层 nanosleep 为纳秒.
            const MS_TO_NS: u64 = 1_000_000;
            let ns = a0 * MS_TO_NS;
            as_ret(crate::kernel::services::timer::sleep::nanosleep_syscall(
                ns, a1,
            ))
        }
        SYS_CREDO_REBOOT => crate::kernel::services::proc::sysinfo::reboot_syscall(a0 as i32),

        // 存储设备
        SYS_CREDO_DISK_LIST => as_ret(
            crate::kernel::services::credo::storage::disk::disk_list(a0, a1 as u32)
                .map(|n| n as usize),
        ),
        SYS_CREDO_DISK_INFO => {
            match crate::kernel::services::credo::storage::disk::disk_info(a0 as u32, a1) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            }
        }
        SYS_CREDO_DISK_FORMAT => {
            match crate::kernel::services::credo::storage::disk::disk_format(a0 as u32, a1) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            }
        }
        SYS_CREDO_DISK_PARTITION => {
            match crate::kernel::services::credo::storage::disk::disk_partition(a0 as u32, a1) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            }
        }
        SYS_CREDO_FAT_FORMAT => {
            match crate::kernel::services::credo::storage::disk::fat_format(a0 as u32) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            }
        }

        _ => return None,
    })
}

/// 其他系统调用 (POSIX Timer, 熵源等)
fn dispatch_other(num: u64, args: [u64; 6]) -> Option<i64> {
    use crate::kernel::services::syscall::types::{
        QX_GET_CANARY, SYS_clock_getres, SYS_getrandom, SYS_timer_create, SYS_timer_delete,
        SYS_timer_getoverrun, SYS_timer_gettime, SYS_timer_settime,
    };
    let [a0, a1, a2, a3, _a4, _a5] = args;

    Some(match num {
        // POSIX Timer (从 framework 回退迁移, §6.1 下沉 services/syscall/posix_timer)
        SYS_timer_create => crate::kernel::services::syscall::posix_timer::sys_timer_create(a0, a1, a2),
        SYS_timer_settime => {
            crate::kernel::services::syscall::posix_timer::sys_timer_settime(a0, a1, a2, a3)
        }
        SYS_timer_gettime => crate::kernel::services::syscall::posix_timer::sys_timer_gettime(a0, a1),
        SYS_timer_delete => crate::kernel::services::syscall::posix_timer::sys_timer_delete(a0),
        SYS_timer_getoverrun => {
            crate::kernel::services::syscall::posix_timer::sys_timer_getoverrun(a0)
        }
        SYS_clock_getres => crate::kernel::services::syscall::posix_timer::sys_clock_getres(a0, a1),

        // 熵源 / Stack Canary (§6.1 下沉 services/syscall/canary)
        SYS_getrandom => crate::kernel::services::syscall::canary::sys_getrandom(a0, a1, a2),
        QX_GET_CANARY => crate::kernel::services::syscall::canary::sys_get_canary(a0, a1),

        _ => return None,
    })
}

// ============================================================================
// 注册
// ============================================================================

/// 注册 services 层分发策略到 framework
///
/// # Errors
///
/// 当分发策略已被注册时返回 `Err(())`.
pub fn register_services_dispatch() -> Result<(), ()> {
    static POLICY: ServicesSyscallDispatch = ServicesSyscallDispatch;
    let r = register_syscall_dispatch(&POLICY);
    crate::kernel::framework::klog::log_info(
        crate::kernel::framework::klog::LogCategory::Boot,
        format_args!(
            "[SYSCALL] register_services_dispatch result={}",
            if r.is_ok() { "OK" } else { "ERR" }
        ),
    );
    r.map_err(|_| ())
}
