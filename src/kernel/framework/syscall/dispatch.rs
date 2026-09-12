//! 系统调用分发实现
//!
//! 从 `mod.rs` 拆分而来, 包含主分发函数和所有 `sys_*` 处理函数。

#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::idt::InterruptFrame;
use core::sync::atomic::Ordering;

use super::raw;
use super::types::{
    Errno, QX_ACCEPT, QX_BIND, QX_BPF, QX_CET, QX_CGROUP_ATTACH, QX_CGROUP_CREATE,
    QX_CGROUP_DESTROY, QX_CGROUP_GET_STAT, QX_CGROUP_SET_LIMIT, QX_CONNECT, QX_EXECVE,
    QX_FTRACE_DISABLE, QX_FTRACE_ENABLE, QX_FTRACE_READ, QX_FTRACE_STAT, QX_FW_DETACH, QX_FW_GET,
    QX_FW_GET_INFO, QX_FW_LOAD, QX_GETPEERNAME, QX_GETSOCKNAME, QX_GETSOCKOPT, QX_IO_URING_ENTER,
    QX_IO_URING_REGISTER, QX_IO_URING_SETUP, QX_IO_URING_SUBMIT, QX_KEXEC, QX_KGDB_ENTER,
    QX_LISTEN, QX_NF_ADD_RULE, QX_NF_DEL_RULE, QX_PM, QX_PRCTL, QX_RECVFROM, QX_RECVMSG,
    QX_ROUTE_ADD, QX_ROUTE_DEL, QX_ROUTE_QUERY, QX_RT_SIGRETURN, QX_SECCOMP, QX_SECURE_BOOT,
    QX_SENDFILE, QX_SENDMSG, QX_SENDTO, QX_SETNS, QX_SETRLIMIT, QX_SETSOCKOPT, QX_SHUTDOWN,
    QX_SOCKET, QX_SPLICE, QX_TCGETPGRP, QX_TCSETPGRP, QX_TGKILL, QX_TICKLESS, QX_TIMESYNC, QX_TPM,
    QX_UEFI, QX_UNSHARE, SYS_CREDO_HOTPLUG_STATUS, SYS_FB_MMAP, SYS_FB_OPEN, SYS_FB_RELEASE,
    SYS_mremap, SYS_read, SYS_write,
};
// SYS_CREDO_DISK_INSTALL 仅 x86_64 (非 kernel_test) 或 kernel_test 模式使用, aarch64 生产构建不引用
#[cfg(any(feature = "kernel_test", target_arch = "x86_64"))]
use super::types::SYS_CREDO_DISK_INSTALL;

/// fb_mmap 目标虚拟地址上界 — 集中定义于 `framework::constants::limits`
/// (与用户指针校验边界语义不同, 见该常量注释).
use crate::kernel::framework::constants::limits::FB_MMAP_ADDR_MAX;

/// 用户态寄存器值 → 文件描述符 (i32) 严格转换
///
/// 用户态可在任意 64 位寄存器值上调用 syscall, 直接 `a0 as i32` 会导致
/// 大于 i32::MAX 的合法 fd 被截断为负数, 或 0xFFFFFFFF..FFFF 被截断为 -1
/// (误处理为 EBADF 而非 EINVAL). 此处用 try_from 严格校验, 失败时返回 -EINVAL.
///
/// SAFETY: 调用方需确保传入 fd (a0) 来自用户态寄存器, 内核不应信任其值域.
#[inline]
fn try_fd(a0: u64) -> Option<i32> {
    i32::try_from(a0).ok()
}

/// 用户态寄存器值 → 标志 (i32) 严格转换 (mode/flags 等)
#[inline]
fn try_flags(a: u64) -> Option<i32> {
    i32::try_from(a).ok()
}

#[cfg(target_arch = "x86_64")]
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// 调用者处于内核上下文. `ptr` 是已校验的用户态字符串指针.
pub unsafe extern "C" fn syscall_dispatch_from_frame(frame: *mut InterruptFrame) {
    // ═══ 诊断: syscall dispatch 入口 (仅调试构建, 生产不包含) ═══
    #[cfg(all(target_arch = "x86_64", feature = "debug_syscall"))]
    unsafe {
        core::arch::asm!(
            "push rax",
            "push rdx",
            "mov dx, 0x3F8",
            "mov al, 0x4A", // 'J' - dispatch entered
            "out dx, al",
            "pop rdx",
            "pop rax",
            options(nomem, preserves_flags),
        );
    }
    // ═══ 诊断结束 ═══

    // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
    unsafe {
        if frame.is_null() {
            return;
        }
        let f = &mut *frame;
        let syscall_num = f.rax;

        // B05-55 根治: 每次 syscall 进入, 把用户寄存器保存到当前进程 p.context.
        // fork/clone 复制 p.context 时即得真实用户状态 (否则 init 等直接进入
        // 用户态的进程 context 全零, 子进程 iretq 用全零帧 → 未进用户态).
        // 需在 rt_sigreturn 处理前保存 (sigreturn 之后 frame 被恢复为 signal 帧,
        // 保存的是恢复后的用户寄存器, 同样正确).
        {
            let cur = crate::kernel::framework::proc::SCHEDULER.current().unwrap_or(0);
            if cur != 0 {
                crate::kernel::framework::proc::proc_save_user_regs(cur, f);
            }
        }

        // rt_sigreturn 特殊处理: 需要直接修改 frame, 不走正常 dispatch
        // Linux x86_64 编号 15 / aarch64 编号 139
        #[cfg(target_arch = "x86_64")]
        let is_rt_sigreturn = syscall_num == crate::kernel::services::syscall::types::SYS_rt_sigreturn;
        #[cfg(target_arch = "aarch64")]
        let is_rt_sigreturn = syscall_num == 139;

        if is_rt_sigreturn {
            let sigframe_ptr = (f.rsp + 8) as *const crate::kernel::framework::proc::SignalFrame;
            if !sigframe_ptr.is_null() {
                let sigframe = core::ptr::read_unaligned(sigframe_ptr);
                f.r15 = sigframe.r15;
                f.r14 = sigframe.r14;
                f.r13 = sigframe.r13;
                f.r12 = sigframe.r12;
                f.r11 = sigframe.r11;
                f.r10 = sigframe.r10;
                f.r9 = sigframe.r9;
                f.r8 = sigframe.r8;
                f.rdi = sigframe.rdi;
                f.rsi = sigframe.rsi;
                f.rbp = sigframe.rbp;
                f.rdx = sigframe.rdx;
                f.rcx = sigframe.rcx;
                f.rbx = sigframe.rbx;
                f.rax = sigframe.rax;
                f.rip = sigframe.rip;
                f.cs = sigframe.cs;
                f.rflags = sigframe.rflags;
                f.rsp = sigframe.rsp;
                f.ss = sigframe.ss;
            }
            return;
        }

        let a0 = f.rdi;
        let a1 = f.rsi;
        let a2 = f.rdx;
        let a3 = f.r10;
        let a4 = f.r8;
        let a5 = f.r9;
        let result = syscall_dispatch(syscall_num, a0, a1, a2, a3, a4, a5);
        f.rax = result as u64;

        // 返回用户态前检查待投递信号
        // SAFETY: frame 有效, 当前在当前 CPU 的 syscall 上下文
        crate::kernel::framework::proc::do_signal_deliver(frame);
    }
}

macro_rules! dispatch {
    ($num:expr_2021, $name:expr_2021) => {{
        let ret = $num;
        // SAFETY: klog_write 是 C-ABI 日志函数，$name 是 Rust 静态字符串
        // (字节切片)，传给 C 时按指针 + 长度传递。
        unsafe {
            crate::kernel::framework::klog::klog_write(
                0,
                7,
                core::ptr::null(),
                core::ptr::null(),
                0,
                $name.as_ptr() as *const u8,
            );
        }
        ret
    }};
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// 从中断上下文 (int 0x80) 调用. 所有寄存器值来自被打断的用户上下文.
pub unsafe extern "C" fn syscall_dispatch(
    num: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> i64 {
    // TD-10: 进入内核态, tick 期间 sys_time 累加.
    crate::kernel::framework::proc::proc_set_in_kern(1);
    let result = syscall_dispatch_impl(num, a0, a1, a2, a3, a4, a5);
    // 出口恢复用户态, tick 期间 user_time 累加.
    crate::kernel::framework::proc::proc_set_in_kern(0);
    result
}

#[expect(
    clippy::too_many_lines,
    reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "syscall handler 中 u64 → u32/i32 转换: 严格校验在 try_fd/try_flags helper 内; 剩余 cast 是 sys_* 函数内数据转换, 已知安全"
)]
#[expect(
    clippy::cast_possible_wrap,
    reason = "syscall handler 中 usize/u64 互转: 内核/用户态地址均为 usize 表示, 位宽不变"
)]
fn syscall_dispatch_impl(num: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    // 直接 Linux ABI: syscall 编号直接使用 Linux 标准编号, 无需翻译

    // C7: Seccomp 过滤检查 (在 dispatch 之前)
    let args = [a0, a1, a2, a3, a4, a5];
    if let Some(ret) = crate::kernel::framework::proc::seccomp_check(num, &args) {
        return ret;
    }

    // L-01: 优先委托 services 层策略分发
    let svc_ret = super::dispatch_trait::current_syscall_dispatch().dispatch(num, args);
    if svc_ret != crate::kernel::services::syscall::types::ENOSYS_RET {
        return svc_ret;
    }

    // framework 回退: 处理尚未迁移到 services 的 syscall
    match num {
        // ==================== 文件 I/O ====================
        SYS_read => {
            // 严格校验 fd (用户态可传任意 u64), 失败返回 -EINVAL (Errno::EINVAL as i64)
            try_fd(a0).map_or_else(
                || -(Errno::EINVAL as i64),
                |fd| dispatch!(sys_read(fd, a1 as *mut u8, a2), b"read\0"),
            )
        }
        SYS_write => try_fd(a0).map_or_else(
            || -(Errno::EINVAL as i64),
            |fd| dispatch!(sys_write(fd, a1 as *const u8, a2), b"write\0"),
        ),

        // ==================== 内存管理 ====================
        SYS_mremap => {
            use crate::kernel::framework::mm::vma_get_current_mm;
            vma_get_current_mm().map_or(-1, |mm| {
                // flags (a3) 是 i32 (Linux mremap flags); 严格校验
                try_flags(a3).map_or_else(
                    || -(Errno::EINVAL as i64),
                    |flags| {
                        dispatch!(
                            match crate::kernel::services::mm::mremap::mremap_syscall(
                                mm, a0, a1, a2, flags,
                            ) {
                                Ok(addr) => addr as i64,
                                Err(e) => e.as_ret(),
                            },
                            b"mremap\0"
                        )
                    },
                )
            })
        }

        // ==================== 信号 ====================
        QX_RT_SIGRETURN => dispatch!(sys_rt_sigreturn(), b"rt_sigreturn\0"),

        // ==================== 设备固件加载 ====================
        QX_FW_LOAD => dispatch!(
            crate::kernel::framework::syscall::firmware::sys_fw_load(a0, a1, a2, a3),
            b"fw_load\0"
        ),
        QX_FW_GET => dispatch!(
            crate::kernel::framework::syscall::firmware::sys_fw_get(a0, a1, a2, a3),
            b"fw_get\0"
        ),
        QX_FW_GET_INFO => dispatch!(
            crate::kernel::framework::syscall::firmware::sys_fw_get_info(a0, a1),
            b"fw_get_info\0"
        ),
        QX_FW_DETACH => dispatch!(
            crate::kernel::framework::syscall::firmware::sys_fw_detach(a0),
            b"fw_detach\0"
        ),

        // ==================== 调试 / 跟踪 ====================
        QX_FTRACE_ENABLE => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_ftrace_enable(),
            b"ftrace_enable\0"
        ),
        QX_FTRACE_DISABLE => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_ftrace_disable(),
            b"ftrace_disable\0"
        ),
        QX_FTRACE_READ => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_ftrace_read(a0),
            b"ftrace_read\0"
        ),
        QX_FTRACE_STAT => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_ftrace_stat(a0),
            b"ftrace_stat\0"
        ),
        QX_KGDB_ENTER => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_kgdb_enter(),
            b"kgdb_enter\0"
        ),

        // ==================== C7: Seccomp / prctl ====================
        QX_SECCOMP => dispatch!(
            crate::kernel::framework::proc::sys_seccomp(a0 as u32, a1 as u32, a2),
            b"seccomp\0"
        ),
        QX_PRCTL => dispatch!(
            crate::kernel::framework::proc::sys_prctl_prctl(a0 as i64, a1, a2, a3, a4),
            b"prctl\0"
        ),

        // ==================== C5: 路由表 ====================
        QX_ROUTE_ADD => dispatch!(
            crate::kernel::framework::net::route::sys_route_add(a0, a1, a2),
            b"route_add\0"
        ),
        QX_ROUTE_DEL => dispatch!(
            crate::kernel::framework::net::route::sys_route_del(a0, a1, a2),
            b"route_del\0"
        ),
        QX_ROUTE_QUERY => dispatch!(
            crate::kernel::framework::net::route::sys_route_query(a0),
            b"route_query\0"
        ),

        // ==================== C5: Netfilter ====================
        QX_NF_ADD_RULE => dispatch!(
            crate::kernel::framework::net::netfilter::sys_nf_add_rule(a0, a1, a2, a3, a4, a5),
            b"nf_add_rule\0"
        ),
        QX_NF_DEL_RULE => dispatch!(
            crate::kernel::framework::net::netfilter::sys_nf_del_rule(a0, a1),
            b"nf_del_rule\0"
        ),

        // ==================== C4: io_uring ====================
        QX_IO_URING_SETUP => dispatch!(
            crate::kernel::framework::io::iouring::sys_io_uring_setup(a0),
            b"io_uring_setup\0"
        ),
        QX_IO_URING_ENTER => dispatch!(
            crate::kernel::framework::io::iouring::sys_io_uring_enter(a0, a1, a2),
            b"io_uring_enter\0"
        ),
        QX_IO_URING_REGISTER => dispatch!(
            crate::kernel::framework::io::iouring::sys_io_uring_register(a0, a1, a2, a3),
            b"io_uring_register\0"
        ),
        QX_IO_URING_SUBMIT => dispatch!(
            crate::kernel::framework::io::iouring::sys_io_uring_submit_sqe(a0, a1, a2, a3, a4, a5),
            b"io_uring_submit\0"
        ),

        // ==================== D1: Namespace ====================
        QX_UNSHARE => dispatch!(
            crate::kernel::framework::proc::sys_unshare(a0),
            b"unshare\0"
        ),
        QX_SETNS => dispatch!(
            crate::kernel::framework::proc::sys_setns(a0, a1),
            b"setns\0"
        ),

        // ==================== D2: cgroup ====================
        QX_CGROUP_CREATE => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_create(a0, a1, a2),
            b"cgroup_create\0"
        ),
        QX_CGROUP_DESTROY => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_destroy(a0),
            b"cgroup_destroy\0"
        ),
        QX_CGROUP_ATTACH => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_attach(a0, a1),
            b"cgroup_attach\0"
        ),
        QX_CGROUP_SET_LIMIT => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_set_limit(a0, a1, a2),
            b"cgroup_set_limit\0"
        ),
        QX_CGROUP_GET_STAT => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_get_stat(a0, a1),
            b"cgroup_get_stat\0"
        ),

        // ==================== D4: eBPF ====================
        QX_BPF => dispatch!(
            crate::kernel::framework::debug::sys_bpf(a0, a1, a2),
            b"bpf\0"
        ),

        // ==================== D5: 电源管理 ====================
        QX_PM => dispatch!(
            crate::kernel::framework::driver::sys_pm(a0, a1, a2),
            b"pm\0"
        ),

        // ==================== D6: 安全启动 + TPM ====================
        QX_SECURE_BOOT => dispatch!(
            crate::kernel::framework::credo::sys_secure_boot(a0, a1, a2, a3),
            b"secure_boot\0"
        ),
        QX_TPM => dispatch!(
            crate::kernel::framework::credo::sys_tpm(a0, a1, a2, a3),
            b"tpm\0"
        ),

        // ==================== D7: Shadow Stack (CET) ====================
        QX_CET => dispatch!(
            crate::kernel::framework::arch::shadow_stack::sys_cet(a0, a1, a2),
            b"cet\0"
        ),

        // ==================== D8: 无 tick 模式 (NO_HZ) ====================
        QX_TICKLESS => dispatch!(
            crate::kernel::framework::timer::sys_tickless(a0, a1, a2),
            b"tickless\0"
        ),

        // ==================== D9: NTP/PTP 时钟同步 ====================
        QX_TIMESYNC => dispatch!(
            crate::kernel::framework::timer::sys_timesync(a0, a1, a2),
            b"timesync\0"
        ),

        // ==================== D10: kexec ====================
        QX_KEXEC => dispatch!(
            crate::kernel::framework::driver::sys_kexec(a0, a1, a2, a3),
            b"kexec\0"
        ),

        // ==================== D11: UEFI ====================
        QX_UEFI => dispatch!(
            crate::kernel::framework::driver::sys_uefi(a0, a1, a2),
            b"uefi\0"
        ),

        // ==================== 进程 ====================
        QX_TCGETPGRP => dispatch!(
            crate::kernel::framework::proc::session::sys_tcgetpgrp(a0 as i32),
            b"tcgetpgrp\0"
        ),
        QX_TCSETPGRP => dispatch!(
            crate::kernel::framework::proc::session::sys_tcsetpgrp(a0 as i32, a1 as i32),
            b"tcsetpgrp\0"
        ),

        // ==================== 网络 (services 代理) ====================
        #[cfg(feature = "net")]
        QX_GETSOCKNAME => dispatch!(sys_getsockname(a0 as i32, a1, a2), b"getsockname\0"),
        #[cfg(feature = "net")]
        QX_GETPEERNAME => dispatch!(sys_getpeername(a0 as i32, a1, a2), b"getpeername\0"),
        #[cfg(not(feature = "net"))]
        QX_SOCKET | QX_CONNECT | QX_ACCEPT | QX_SENDTO | QX_RECVFROM | QX_SHUTDOWN | QX_BIND
        | QX_LISTEN | QX_SENDMSG | QX_RECVMSG | QX_SETSOCKOPT | QX_GETSOCKOPT | QX_GETSOCKNAME
        | QX_GETPEERNAME => {
            dispatch!(Errno::ENOSYS.as_ret(), b"net_nosys\0")
        }

        // ==================== 进程创建 ====================
        QX_EXECVE => dispatch!(
            crate::kernel::services::proc::execve::ExecveResult::from_ret(sys_execve(
                a0 as *const u8,
                a1 as *const *const u8,
                a2 as *const *const u8
            ))
            .as_ret(),
            b"execve\0"
        ),

        // ==================== 时间 ====================
        QX_SETRLIMIT => dispatch!(
            crate::kernel::framework::proc::sys_setrlimit(a0 as i32, a1),
            b"setrlimit\0"
        ),
        QX_TGKILL => dispatch!(sys_tgkill(a0 as i32, a1 as i32, a2 as i32), b"tgkill\0"),

        // ==================== sendfile / splice ====================
        QX_SENDFILE => dispatch!(
            crate::kernel::framework::syscall::sendfile::sys_sendfile(
                a0 as i32,
                a1 as i32,
                a2,
                a3 as usize
            ),
            b"sendfile\0"
        ),
        QX_SPLICE => dispatch!(
            crate::kernel::framework::syscall::sendfile::sys_splice(
                a0 as i32,
                a1,
                a2 as i32,
                a3,
                a4 as usize,
                a5 as u32
            ),
            b"splice\0"
        ),

        // ==================== Credo 私有 syscall ====================
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_DISK_INSTALL => dispatch!(sys_boot_install(a0 as u32), b"credo_diskinst\0"),
        #[cfg(feature = "kernel_test")]
        SYS_CREDO_DISK_INSTALL => dispatch!(Errno::ENOSYS.as_ret(), b"credo_disk_nosys\0"),

        SYS_CREDO_HOTPLUG_STATUS => dispatch!(
            sys_hotplug_status(a0 as *mut u8, a1 as u32),
            b"credo_hotplug_status\0"
        ),

        // ==================== 帧缓冲设备 ====================
        SYS_FB_OPEN => dispatch!(sys_fb_open(a0, a1), b"fb_open\0"),
        SYS_FB_MMAP => dispatch!(sys_fb_mmap(a0, a1, a2), b"fb_mmap\0"),
        SYS_FB_RELEASE => dispatch!(sys_fb_release(a0), b"fb_release\0"),

        // 未匹配的 syscall 编号
        _ => Errno::ENOSYS.as_ret(),
    }
}

// ============================================================================
// 文件 I/O — read / write
// ============================================================================

#[expect(
    clippy::cast_possible_truncation,
    reason = "fd as u32: fd 由 try_fd 严格校验, 在 i32 范围内; count as u32: 单次 read 不会超过 u32::MAX (Linux ABI)"
)]
fn sys_read(fd: i32, buf: *mut u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !raw::check_user_buf(buf as u64, count) {
        return Errno::EFAULT.as_ret();
    }
    if fd == 1 || fd == 2 {
        return Errno::EBADF.as_ret();
    }
    if fd == 0 {
        #[cfg(not(feature = "kernel_test"))]
        {
            #[cfg(target_arch = "x86_64")]
            {
                // 键盘 stdin: keyboard_has_data/get_char 由 framework input 机制提供。
                // 串口 stdin 随 DECISION-G §6.4 char 下沉移除 (framework 不再持有串口 FFI,
                // 控制台输入待 devfs 桥接入 services char 权威)。
                if let Some(c) = raw::read_keyboard_byte() {
                    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                    unsafe { raw::write_u8(buf, c) };
                    return 1;
                }
            }
        }
        return 0;
    }
    if crate::kernel::framework::syscall::eventfd::is_eventfd_fd(fd) {
        return crate::kernel::framework::syscall::eventfd::sys_eventfd_read(fd, buf as u64);
    }
    if crate::kernel::framework::syscall::signalfd::is_signalfd_fd(fd) {
        return crate::kernel::framework::syscall::signalfd::sys_signalfd_read(fd, buf as u64);
    }
    if crate::kernel::framework::syscall::timerfd::is_timerfd_fd(fd) {
        return crate::kernel::framework::syscall::timerfd::sys_timerfd_read(fd, buf as u64);
    }
    if crate::kernel::framework::fs::is_inotify_fd(fd) {
        return crate::kernel::framework::fs::sys_inotify_read(i64::from(fd), buf, count as usize);
    }
    i64::from(crate::kernel::framework::fs::vfs_read(
        fd as u32,
        buf,
        count as u32,
    ))
}

/// 从用户空间缓冲区复制数据到内核缓冲区.
///
/// framekernel 架构下内核页表不映射用户页面, 需要通过用户页表
/// 将用户虚拟地址转译为物理地址, 再通过 `KERNEL_BASE` 恒等映射访问.
///
/// 返回实际复制的字节数; 若任一转译失败则返回已复制字节数 (调用方按需处理).
fn copy_from_user_buf(user_buf: *const u8, kernel_buf: &mut [u8], user_cr3: u64) -> usize {
    if user_buf.is_null() || kernel_buf.is_empty() || user_cr3 == 0 {
        return 0;
    }
    let vmm = crate::kernel::framework::mm::get_vmm();
    let page_size = crate::kernel::framework::mm::PAGE_SIZE as u64;
    let kernel_base = crate::kernel::framework::mm::KERNEL_BASE as u64;
    let total = kernel_buf.len();
    let mut copied: usize = 0;
    while copied < total {
        let user_va = user_buf as u64 + copied as u64;
        let page_va = user_va & !(page_size - 1);
        let offset = user_va & (page_size - 1);
        let step = (page_size - offset).min((total - copied) as u64) as usize;
        let phys = match vmm
            .get_physical_in_pml4(user_cr3, crate::kernel::framework::mm::VirtAddr(page_va))
        {
            Some(p) => p.as_u64(),
            None => break,
        };
        let kernel_va = phys + kernel_base + offset;
        // SAFETY: kernel_va 由用户页表转译 + KERNEL_BASE 偏移, 在恒等映射范围内.
        unsafe {
            core::ptr::copy_nonoverlapping(
                kernel_va as *const u8,
                kernel_buf.as_mut_ptr().add(copied),
                step,
            );
        }
        copied += step;
    }
    copied
}

fn sys_write(fd: i32, buf: *const u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !raw::check_user_buf(buf as u64, count) {
        return Errno::EFAULT.as_ret();
    }
    if fd == 1 || fd == 2 {
        let user_cr3 = crate::kernel::framework::mm::read_user_cr3_asm();
        let mut remaining = (count as usize).min(4096);
        let mut buf_off: usize = 0;
        while remaining > 0 {
            let chunk = remaining.min(256);
            let mut kernel_buf = [0u8; 256];
            let copied = copy_from_user_buf(
                // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
                unsafe { buf.add(buf_off) },
                &mut kernel_buf[..chunk],
                user_cr3,
            );
            if copied == 0 {
                return Errno::EFAULT.as_ret();
            }
            crate::kernel::framework::klog::serial_write_bytes(&kernel_buf[..copied]);
            buf_off += copied;
            remaining -= copied;
        }
        return count as i64;
    }
    if crate::kernel::framework::syscall::eventfd::is_eventfd_fd(fd) {
        if count < 8 {
            return Errno::EINVAL.as_ret();
        }
        let user_cr3 = crate::kernel::framework::mm::read_user_cr3_asm();
        let mut val_buf = [0u8; 8];
        if copy_from_user_buf(buf, &mut val_buf, user_cr3) < 8 {
            return Errno::EFAULT.as_ret();
        }
        let value = u64::from_ne_bytes(val_buf);
        return crate::kernel::framework::syscall::eventfd::sys_eventfd_write(fd, value);
    }
    // 文件写入: 分块拷贝用户数据到内核缓冲区, 再走 VFS
    let user_cr3 = crate::kernel::framework::mm::read_user_cr3_asm();
    let total = (count as usize).min(4096);
    let mut kernel_buf = [0u8; 256];
    let mut written: usize = 0;
    while written < total {
        let chunk = (total - written).min(kernel_buf.len());
        let copied = copy_from_user_buf(
            // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
            unsafe { buf.add(written) },
            &mut kernel_buf[..chunk],
            user_cr3,
        );
        if copied == 0 {
            break;
        }
        let n = crate::kernel::framework::fs::vfs_write_safe(fd as u32, &kernel_buf[..copied]);
        if n < 0 {
            return i64::from(n);
        }
        written += copied;
    }
    written as i64
}

// ============================================================================
// execve / 网络 / 时间
// ============================================================================

#[expect(
    clippy::similar_names,
    reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
)]
fn sys_execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    let _ = envp;
    let mut argc: u32 = 0;
    if !argv.is_null() {
        if !raw::check_user_ptr(argv as u64) {
            return Errno::EFAULT.as_ret();
        }
        let mut p = argv;
        loop {
            if !raw::check_user_ptr(p as u64) {
                return Errno::EFAULT.as_ret();
            }
            // SAFETY: p 是经过 check_user_ptr 验证的用户空间指针
            let entry = unsafe { core::ptr::read_volatile(p) };
            if entry.is_null() {
                break;
            }
            if !raw::check_user_ptr(entry as u64) {
                return Errno::EFAULT.as_ret();
            }
            argc += 1;
            // SAFETY: p 指向用户空间数组元素; 由 argc 计数 + NULL 终止保证不越界
            p = unsafe { p.add(1) };
        }
    }

    // SUID 处理
    let mut stat_buf = core::mem::MaybeUninit::<crate::kernel::framework::fs::VfsStat>::uninit();
    let current_pwm = crate::kernel::framework::credo::get_current_pwm();
    let stat_result =
        crate::kernel::framework::fs::vfs_stat_internal(path, stat_buf.as_mut_ptr(), current_pwm);
    if stat_result == 0 {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let st = unsafe { stat_buf.assume_init() };
        if (st.perm & 0o4000) != 0 && st.owner_pwm != 0 {
            crate::kernel::framework::credo::elevate_for_suid(st.owner_pwm);
        }
    }

    let result = crate::kernel::framework::proc::proc_exec_replace(path, argv, argc);
    if result < 0 {
        Errno::ENOENT.as_ret()
    } else {
        0
    }
}

#[cfg(feature = "net")]
fn sys_getsockname(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    crate::kernel::framework::net::syscall::getsockname_syscall(sockfd, addr, addrlen)
}

#[cfg(feature = "net")]
fn sys_getpeername(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    crate::kernel::framework::net::syscall::getpeername_syscall(sockfd, addr, addrlen)
}

#[expect(
    clippy::similar_names,
    reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
)]
pub(crate) fn sys_nanosleep(req: u64, rem: u64) -> i64 {
    if req == 0 || !raw::check_user_ptr(req) {
        return Errno::EINVAL.as_ret();
    }
    #[repr(C)]
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    // SAFETY: `const` 由调用方保证为有效指针; 只读访问
    let ts = unsafe { core::ptr::read_volatile(req as *const Timespec) };
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return Errno::EINVAL.as_ret();
    }

    let total_ns = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
    if total_ns == 0 {
        return 0;
    }

    if total_ns < 1_000_000 {
        let start = crate::kernel::framework::timer::hrtimer_clock_read();
        let target = start + total_ns;
        while crate::kernel::framework::timer::hrtimer_clock_read() < target {
            core::hint::spin_loop();
        }
    } else {
        let total_ms = total_ns / 1_000_000;
        let _ = crate::kernel::framework::timer::sleep::timer_sleep(total_ms);
    }

    let _ = rem;
    0
}

pub(crate) fn sys_kill(pid: i32, sig: i32) -> i64 {
    if !(0..=31).contains(&sig) {
        return Errno::EINVAL.as_ret();
    }
    match crate::kernel::framework::proc::do_signal_send_extended(pid, sig as u8) {
        Ok(_) => 0,
        Err(-1) => Errno::EINVAL.as_ret(),
        Err(-2) => Errno::ESRCH.as_ret(),
        Err(_) => Errno::EPERM.as_ret(),
    }
}

fn sys_tgkill(_tgid: i32, tid: i32, sig: i32) -> i64 {
    sys_kill(tid, sig)
}

// ============================================================================
// 信号框架
// ============================================================================

const SIG_BLOCK: i32 = 0;
const SIG_UNBLOCK: i32 = 1;
const SIG_SETMASK: i32 = 2;

pub(crate) fn sys_rt_sigaction(signum: i32, act: u64, oact: u64) -> i64 {
    if !(1..=31).contains(&signum) {
        return Errno::EINVAL.as_ret();
    }

    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

    if oact != 0 {
        if !raw::check_user_buf(oact, 8) {
            return Errno::EFAULT.as_ret();
        }
        let old = crate::kernel::framework::proc::get_sigaction(pid, signum as u8);
        match old {
            // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
            Some(v) => unsafe { raw::write_u64(oact as *mut u64, v) },
            None => return Errno::EINVAL.as_ret(),
        }
    }

    if act != 0 {
        if !raw::check_user_buf(act, 8) {
            return Errno::EFAULT.as_ret();
        }
        // SAFETY: `const` 由调用方保证为有效指针; 只读访问
        let new_action = unsafe { raw::read_u64(act as *const u64) };
        match crate::kernel::framework::proc::set_sigaction(pid, signum as u8, new_action) {
            Some(_) => {}
            None => return Errno::EINVAL.as_ret(),
        }
    }

    0
}

pub(crate) fn sys_rt_sigprocmask(how: i32, set: u64, oset: u64) -> i64 {
    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

    if oset != 0 {
        if !raw::check_user_buf(oset, 8) {
            return Errno::EFAULT.as_ret();
        }
        let old = crate::kernel::framework::proc::get_blocked_mask(pid);
        // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
        unsafe { raw::write_u64(oset as *mut u64, old) };
    }

    if set != 0 {
        if !raw::check_user_buf(set, 8) {
            return Errno::EFAULT.as_ret();
        }
        // SAFETY: `const` 由调用方保证为有效指针; 只读访问
        let new_set = unsafe { raw::read_u64(set as *const u64) };
        let old = crate::kernel::framework::proc::get_blocked_mask(pid);
        let updated = match how {
            SIG_BLOCK => old | new_set,
            SIG_UNBLOCK => old & !new_set,
            SIG_SETMASK => new_set,
            _ => return Errno::EINVAL.as_ret(),
        };
        let updated = updated & !((1u64 << 9) | (1u64 << 19));
        crate::kernel::framework::proc::set_blocked_mask(pid, updated);
    }

    0
}

fn sys_rt_sigreturn() -> i64 {
    if let Some(pid) =
        Some(crate::kernel::framework::proc::process_get_current_pid()).filter(|&p| p != 0)
    {
        crate::kernel::framework::proc::process_with_mut(pid, |proc| {
            let flags = proc.sigaltstack_flags.load(Ordering::Acquire);
            proc.sigaltstack_flags.store(
                flags & !crate::kernel::framework::proc::SS_ONSTACK,
                Ordering::Release,
            );
        });
    }
    0
}

pub(crate) fn sys_sigaltstack(ss: u64, old_ss: u64) -> i64 {
    use crate::kernel::framework::proc::{SS_DISABLE, SS_ONSTACK};

    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

    let result = crate::kernel::framework::proc::process_with_mut(pid, |proc| {
        if old_ss != 0 {
            if !raw::check_user_buf(old_ss, 24) {
                return Errno::EFAULT.as_ret();
            }
            let cur_addr = proc.sigaltstack_addr.load(Ordering::Acquire);
            let cur_size = proc.sigaltstack_size.load(Ordering::Acquire);
            let cur_flags = proc.sigaltstack_flags.load(Ordering::Acquire);
            // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
            unsafe {
                raw::write_u64(old_ss as *mut u64, cur_addr);
                raw::write_u64((old_ss + 8) as *mut u64, u64::from(cur_flags));
                raw::write_u64((old_ss + 16) as *mut u64, cur_size);
            }
        }

        if ss != 0 {
            if !raw::check_user_buf(ss, 24) {
                return Errno::EFAULT.as_ret();
            }
            // SAFETY: `const` 由调用方保证为有效指针; 只读访问
            let new_addr = unsafe { raw::read_u64(ss as *const u64) };
            let new_flags_in = unsafe { raw::read_u64((ss + 8) as *const u64) } as u32;
            let new_size = unsafe { raw::read_u64((ss + 16) as *const u64) };

            if (new_flags_in & SS_DISABLE) != 0 {
                proc.sigaltstack_addr.store(0, Ordering::Release);
                proc.sigaltstack_size.store(0, Ordering::Release);
                let cur = proc.sigaltstack_flags.load(Ordering::Acquire);
                proc.sigaltstack_flags
                    .store(cur | SS_DISABLE, Ordering::Release);
            } else {
                proc.sigaltstack_addr.store(new_addr, Ordering::Release);
                proc.sigaltstack_size.store(new_size, Ordering::Release);
                let cur = proc.sigaltstack_flags.load(Ordering::Acquire);
                proc.sigaltstack_flags
                    .store(cur & !(SS_ONSTACK | SS_DISABLE), Ordering::Release);
            }
        }
        0i64
    });

    result.unwrap_or_else(|| Errno::ESRCH.as_ret())
}

// ============================================================================
// 热插拔 / 帧缓冲
// ============================================================================

fn sys_hotplug_status(buf: *mut u8, buf_size: u32) -> i64 {
    if buf.is_null() || buf_size == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !raw::check_user_buf(buf as u64, u64::from(buf_size)) {
        return Errno::EFAULT.as_ret();
    }

    let status = crate::kernel::framework::driver::hotplug::HOTPLUG_MANAGER.status();

    let mut offset: u32 = 0;

    let header: [u8; 16] = [
        u8::from(status.enabled),
        0,
        0,
        0,
        (status.slot_count & 0xFF) as u8,
        ((status.slot_count >> 8) & 0xFF) as u8,
        ((status.slot_count >> 16) & 0xFF) as u8,
        ((status.slot_count >> 24) & 0xFF) as u8,
        (status.blk_device_count & 0xFF) as u8,
        ((status.blk_device_count >> 8) & 0xFF) as u8,
        ((status.blk_device_count >> 16) & 0xFF) as u8,
        ((status.blk_device_count >> 24) & 0xFF) as u8,
        0,
        0,
        0,
        0,
    ];
    if offset + 16 > buf_size {
        return i64::from(offset);
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::ptr::copy_nonoverlapping(header.as_ptr(), buf.add(offset as usize), 16);
    }
    offset += 16;

    let slot_size: u32 = 8;
    for slot in &status.slots {
        if offset + slot_size > buf_size {
            break;
        }
        let info: [u8; 8] = [
            slot.bus,
            slot.device,
            slot.function,
            slot.slot_number,
            u8::from(slot.presence),
            (u8::from(slot.surprise_capable) << 1) | u8::from(slot.hotplug_capable),
            0,
            0,
        ];
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::copy_nonoverlapping(
                info.as_ptr(),
                buf.add(offset as usize),
                slot_size as usize,
            );
        }
        offset += slot_size;
    }

    let dev_size: u32 = 16;
    for dev in &status.blk_devices {
        if offset + dev_size > buf_size {
            break;
        }
        let info: [u8; 16] = [
            dev.drive,
            u8::from(dev.present),
            u8::from(dev.removing),
            0,
            (dev.io_count & 0xFF) as u8,
            ((dev.io_count >> 8) & 0xFF) as u8,
            ((dev.io_count >> 16) & 0xFF) as u8,
            ((dev.io_count >> 24) & 0xFF) as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::copy_nonoverlapping(
                info.as_ptr(),
                buf.add(offset as usize),
                dev_size as usize,
            );
        }
        offset += dev_size;
    }

    i64::from(offset)
}

// ============================================================================
// 帧缓冲设备
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
struct FbInfo {
    phys_addr: u64,
    size: u64,
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u8,
    _pad: [u8; 3],
}

fn sys_fb_open(info_ptr: u64, _flags: u64) -> i64 {
    if info_ptr == 0 || !raw::check_user_ptr(info_ptr) {
        return Errno::EFAULT.as_ret();
    }

    let fb_addr =
        crate::kernel::framework::driver::FB_PHYS_ADDR.load(core::sync::atomic::Ordering::Acquire);
    if fb_addr == 0 {
        return Errno::ENODEV.as_ret();
    }

    let fb_size =
        crate::kernel::framework::driver::FB_PHYS_SIZE.load(core::sync::atomic::Ordering::Acquire);

    let (width, height, pitch, bpp) = match crate::kernel::framework::driver::get_framebuffer() {
        Some(guard) => {
            let fb = guard.as_ref().unwrap();
            (
                fb.width(),
                fb.height(),
                fb.pitch(),
                fb.format().bits_per_pixel() as u8,
            )
        }
        None => return Errno::ENODEV.as_ret(),
    };

    let info = FbInfo {
        phys_addr: fb_addr,
        size: fb_size,
        width,
        height,
        pitch,
        bpp,
        _pad: [0; 3],
    };

    let dst = info_ptr as *mut FbInfo;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { raw::write_struct(dst, &info) };
    0
}

fn sys_fb_mmap(target_vaddr: u64, size: u64, _prot: u64) -> i64 {
    if target_vaddr == 0 || target_vaddr & 0xFFF != 0 {
        return Errno::EINVAL.as_ret();
    }
    if target_vaddr > FB_MMAP_ADDR_MAX - size {
        return Errno::EINVAL.as_ret();
    }

    let fb_phys =
        crate::kernel::framework::driver::FB_PHYS_ADDR.load(core::sync::atomic::Ordering::Acquire);
    if fb_phys == 0 {
        return Errno::ENODEV.as_ret();
    }

    let fb_total =
        crate::kernel::framework::driver::FB_PHYS_SIZE.load(core::sync::atomic::Ordering::Acquire);
    if size > fb_total {
        return Errno::EINVAL.as_ret();
    }

    let cr3 = crate::kernel::framework::proc::user_proc::user_entry_cr3
        .load(core::sync::atomic::Ordering::SeqCst);
    if cr3 == 0 {
        return Errno::ENODEV.as_ret();
    }

    let vmm = crate::kernel::framework::mm::get_vmm();
    let flags = crate::kernel::framework::mm::PageFlags::PRESENT
        | crate::kernel::framework::mm::PageFlags::WRITABLE
        | crate::kernel::framework::mm::PageFlags::USER
        | crate::kernel::framework::mm::PageFlags::WRITE_THROUGH;

    let phys_page_aligned = fb_phys & !(crate::kernel::framework::mm::PAGE_SIZE - 1);
    let offset = fb_phys - phys_page_aligned;
    let pages = (size + offset).div_ceil(crate::kernel::framework::mm::PAGE_SIZE);

    for i in 0..pages {
        let pa = crate::kernel::framework::mm::PhysAddr(
            phys_page_aligned + i * crate::kernel::framework::mm::PAGE_SIZE,
        );
        let va = crate::kernel::framework::mm::VirtAddr(
            target_vaddr + i * crate::kernel::framework::mm::PAGE_SIZE,
        );
        vmm.map_page_in_table(cr3, va, pa, flags);
    }

    target_vaddr as i64
}

fn sys_fb_release(_vaddr: u64) -> i64 {
    0
}

// ============================================================================
// 引导安装 (仅 x86_64)
// ============================================================================

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn write_le32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset] = val as u8;
    buf[offset + 1] = (val >> 8) as u8;
    buf[offset + 2] = (val >> 16) as u8;
    buf[offset + 3] = (val >> 24) as u8;
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
const BOOT_PART_SECTORS: u32 = 16384;

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
fn sys_boot_install(disk_id: u32) -> i64 {
    let pwm = crate::kernel::framework::credo::pwm_get_current();
    if !crate::kernel::framework::credo::pwm_has_capability(pwm, 4, 0) {
        return Errno::EACCES.as_ret();
    }
    let stage1 = include_bytes!("../../../../build/stage1.bin");
    if !crate::kernel::framework::driver::hdd_is_present(disk_id as u8) {
        return Errno::ENOENT.as_ret();
    }
    let mut mbr = [0u8; 512];
    if crate::kernel::framework::driver::hdd_read_sector(disk_id as u8, 0, &mut mbr) < 0 {
        return Errno::EIO.as_ret();
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { core::ptr::copy_nonoverlapping(stage1.as_ptr(), mbr.as_mut_ptr(), 440) };
    let total_sectors = crate::kernel::framework::driver::hdd_total_sectors(disk_id as u8);
    let hvfs_start = BOOT_PART_SECTORS;
    let hvfs_sectors = if total_sectors > u64::from(hvfs_start) + 1 {
        total_sectors - u64::from(hvfs_start)
    } else {
        0xFFFFFFFFu64
    };
    write_le32(&mut mbr, 446, 0x00000800);
    write_le32(&mut mbr, 450, 0x06FEFFFF);
    write_le32(&mut mbr, 454, 64u32);
    write_le32(&mut mbr, 458, BOOT_PART_SECTORS - 64);
    write_le32(&mut mbr, 462, hvfs_start);
    write_le32(&mut mbr, 466, 0x83FEFFFF);
    write_le32(&mut mbr, 470, hvfs_start);
    let hvfs_len = if hvfs_sectors > 0xFFFFFFFF {
        0xFFFFFFFFu32
    } else {
        hvfs_sectors as u32
    };
    write_le32(&mut mbr, 474, hvfs_len);
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    if crate::kernel::framework::driver::hdd_write_sector(disk_id as u8, 0, &mbr) < 0 {
        return Errno::EIO.as_ret();
    }
    let kernel_ptr = raw::kernel_start_ptr();
    let kernel_len = {
        const HHDM_OFFSET: usize = 0xFFFF_8000_0000_0000;
        let phys_end = raw::kernel_end_phys(HHDM_OFFSET);
        phys_end - (kernel_ptr as usize)
    };
    let total_kernel_sectors = kernel_len.div_ceil(512) as u32;
    let max_sectors = 2047u32;
    let copy_sectors = if total_kernel_sectors > max_sectors {
        max_sectors
    } else {
        total_kernel_sectors
    };
    for s in 0..copy_sectors {
        let offset = s as usize * 512;
        let remaining = kernel_len.saturating_sub(offset);
        if remaining == 0 {
            break;
        }
        let n = if remaining < 512 { remaining } else { 512 };
        let mut buf = [0u8; 512];
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::copy_nonoverlapping(kernel_ptr.add(offset), buf.as_mut_ptr(), n);
        }
        if crate::kernel::framework::driver::hdd_write_sector(disk_id as u8, u64::from(1 + s), &buf)
            < 0
        {
            return Errno::EIO.as_ret();
        }
    }
    let mut cfg = [0u8; 512];
    cfg[0] = b'A';
    cfg[1] = b'N';
    cfg[2] = b'T';
    cfg[3] = b'X';
    write_le32(&mut cfg, 4, BOOT_PART_SECTORS);
    cfg[510] = 0x55;
    cfg[511] = 0xAA;
    crate::kernel::framework::driver::hdd_write_sector(disk_id as u8, 2046, &cfg);
    0
}
