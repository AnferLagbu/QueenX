//! wait4 — 进程等待子进程退出系统调用 (TCB)
//!
//! POSIX `pid_t wait4(pid_t pid, int *wstatus, int options, struct rusage *rusage)`.
//!
//! ## pid 参数
//!
//! - pid > 0: 等待特定 PID 的子进程
//! - pid == 0: 等待同进程组任意子进程
//! - pid == -1: 等待任意子进程 (POSIX `wait()`)
//! - pid < -1: 等待进程组 |pid| 内的任意子进程
//!
//! ## options
//!
//! - WNOHANG  = 0x1: 非阻塞, 无子进程退出立即返回 0
//! - WUNTRACED = 0x2: 报告已停止的子进程
//! - WCONTINUED = 0x8: 报告已恢复的子进程
//!
//! ## 阻塞语义
//!
//! 当前简化实现: 同步等待直至子进程变为 Zombie 或 Terminated.
//! 非阻塞模式 (WNOHANG) 通过 SCHEDULER.block + 调度器轮询实现.

use crate::framework::proc::ProcessState;
use crate::framework::proc::api;
use crate::framework::syscall::Errno;
use crate::framework::syscall::raw;

use core::sync::atomic::Ordering;

/// wait4 options
pub const WNOHANG: i32 = 0x1;
pub const WUNTRACED: i32 = 0x2;
pub const WCONTINUED: i32 = 0x8;

/// 等待收集结果 (wait4 与 services 层 waitid 共用)
#[derive(Debug)]
pub struct WaitInfo {
    /// 被收割 (或观察) 的子进程 PID
    pub pid: u32,
    /// 子进程退出码 (原始值)
    pub exit_code: u32,
}

/// 等待结果
#[derive(Debug)]
pub enum WaitOutcome {
    /// 收割成功 (keep_zombie 时不释放子进程 PCB)
    Reaped(WaitInfo),
    /// 有匹配子进程但尚未退出 (仅非阻塞模式返回)
    Running,
    /// 无匹配子进程
    NoChild,
}

/// 统一的子进程等待/收割机制 (sys_wait4 与 services 层 waitid 共用)
///
/// - `target_pid > 0`: 等待特定 PID 的子进程
/// - `target_pid == 0`: 等待同进程组任意子进程
/// - `target_pid == -1`: 等待任意子进程
/// - `target_pid < -1`: 等待进程组 |target_pid| 内的任意子进程
///
/// `non_blocking`: true 时子进程未退出立即返回 `Running`.
/// `keep_zombie`: true 时读取退出码后保留子进程 Zombie 状态 (WNOWAIT 语义).
pub fn wait_reap(target_pid: i32, non_blocking: bool, keep_zombie: bool) -> WaitOutcome {
    let current_pid = api::process_get_current_pid();
    if current_pid == 0 {
        return WaitOutcome::NoChild;
    }

    let Some(child_pid) = find_waitable_child(current_pid, target_pid) else {
        return WaitOutcome::NoChild;
    };

    let state = api::process_with(child_pid, super::super::proc::process::Process::get_state)
        .unwrap_or(ProcessState::Terminated);

    if state != ProcessState::Zombie {
        if non_blocking {
            return WaitOutcome::Running;
        }
        // 阻塞等待: 循环检查子进程状态, 直到变为 Zombie
        loop {
            let state =
                api::process_with(child_pid, super::super::proc::process::Process::get_state)
                    .unwrap_or(ProcessState::Terminated);
            if state == ProcessState::Zombie {
                return reap_zombie(child_pid, keep_zombie);
            }
            // 子进程未退出, 阻塞当前进程并调度到子进程
            crate::framework::proc::scheduler_yield();
        }
    }

    reap_zombie(child_pid, keep_zombie)
}

/// 收割 Zombie 子进程: 读取退出码, 按 `keep_zombie` 决定是否释放 PCB
fn reap_zombie(child_pid: u32, keep_zombie: bool) -> WaitOutcome {
    let exit_code =
        api::process_with(child_pid, |p| p.exit_code.load(Ordering::SeqCst)).unwrap_or(0);
    if !keep_zombie {
        api::process_remove_and_free(child_pid);
    }
    WaitOutcome::Reaped(WaitInfo {
        pid: child_pid,
        exit_code,
    })
}

/// wait4 系统调用实现
///
/// 返回子进程 PID, 或错误 (ECHILD/EINTR).
pub fn sys_wait4(pid: i32, wstatus_ptr: u64, options: i32) -> i64 {
    let current_pid = api::process_get_current_pid();
    if current_pid == 0 {
        return Errno::ECHILD.as_ret();
    }

    // 验证 wstatus 指针 (如非零)
    if wstatus_ptr != 0 && !raw::check_user_ptr(wstatus_ptr) {
        return Errno::EFAULT.as_ret();
    }

    // 验证 options 标志
    let valid_opts = WNOHANG | WUNTRACED | WCONTINUED;
    if options & !valid_opts != 0 {
        return Errno::EINVAL.as_ret();
    }

    let non_blocking = options & WNOHANG != 0;

    match wait_reap(pid, non_blocking, false) {
        WaitOutcome::Reaped(info) => {
            // 写入 wstatus (WIFEXITED | exit_code << 8)
            if wstatus_ptr != 0 {
                // SAFETY: wstatus_ptr 已通过 check_user_ptr 验证
                unsafe {
                    let status: i32 = (info.exit_code as i32) << 8;
                    core::ptr::write_volatile(wstatus_ptr as *mut i32, status);
                }
            }
            i64::from(info.pid)
        }
        // WNOHANG: 子进程尚未退出
        WaitOutcome::Running => 0,
        WaitOutcome::NoChild => {
            if non_blocking {
                0 // WNOHANG: 无可等待子进程
            } else {
                Errno::ECHILD.as_ret()
            }
        }
    }
}

/// 查找可等待的子进程
///
/// 根据 pid 参数匹配, 返回 PID 或 None.
fn find_waitable_child(parent_pid: u32, target_pid: i32) -> Option<u32> {
    let children = api::process_with(parent_pid, |p| p.children.lock().clone()).unwrap_or_default();

    for &child in &children {
        let child_pid = child.0;
        let state = api::process_with(child_pid, super::super::proc::process::Process::get_state)
            .unwrap_or(ProcessState::Terminated);

        // 只匹配未结束的子进程 (或 Zombie 用于收割)
        if state == ProcessState::Terminated {
            continue;
        }

        // pid 匹配规则
        if target_pid == -1 {
            // 任意子进程
            return Some(child_pid);
        } else if target_pid > 0 {
            // 特定 PID
            if child_pid == target_pid as u32 {
                return Some(child_pid);
            }
        } else if target_pid == 0 {
            // 同进程组 (简化: 总是匹配)
            return Some(child_pid);
        } else {
            // target_pid < -1: 进程组 ID = |target_pid|
            let want_pgid = target_pid.unsigned_abs();
            let pgid = api::process_with(child_pid, |p| p.pgid.load(Ordering::SeqCst))
                .unwrap_or(0);
            if pgid == want_pgid {
                return Some(child_pid);
            }
        }
    }
    None
}
