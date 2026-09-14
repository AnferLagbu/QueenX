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

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
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

    // 查找匹配的子进程
    let child_pid = if let Some(p) = find_waitable_child(current_pid, pid) {
        p
    } else {
        if non_blocking {
            return 0; // WNOHANG: 无可等待子进程
        }
        return Errno::ECHILD.as_ret(); // 无子进程
    };

    // 检查子进程是否已退出
    let state = api::process_with(child_pid, super::super::proc::process::Process::get_state)
        .unwrap_or(ProcessState::Terminated);

    if state == ProcessState::Zombie {
        // 子进程已退出, 收集状态
        let exit_code =
            api::process_with(child_pid, |p| p.exit_code.load(Ordering::SeqCst)).unwrap_or(0);

        // 写入 wstatus (WIFEXITED | exit_code << 8)
        if wstatus_ptr != 0 {
            // SAFETY: wstatus_ptr 由 check_user_ptr 验证
            unsafe {
                let status: i32 = (exit_code as i32) << 8;
                core::ptr::write_volatile(wstatus_ptr as *mut i32, status);
            }
        }

        // 释放子进程 PCB
        api::process_remove_and_free(child_pid);
        return i64::from(child_pid);
    }

    // 子进程仍在运行
    if non_blocking {
        return 0;
    }

    // 阻塞等待: 循环检查子进程状态, 直到变为 Zombie
    loop {
        let state = api::process_with(child_pid, super::super::proc::process::Process::get_state)
            .unwrap_or(ProcessState::Terminated);

        if state == ProcessState::Zombie {
            // 子进程已退出, 收集状态
            let exit_code =
                api::process_with(child_pid, |p| p.exit_code.load(Ordering::SeqCst)).unwrap_or(0);

            if wstatus_ptr != 0 {
                // SAFETY: wstatus_ptr 由 check_user_ptr 验证
                unsafe {
                    let status: i32 = (exit_code as i32) << 8;
                    core::ptr::write_volatile(wstatus_ptr as *mut i32, status);
                }
            }

            api::process_remove_and_free(child_pid);
            return i64::from(child_pid);
        }

        // 子进程未退出, 阻塞当前进程并调度到子进程
        crate::framework::proc::scheduler_yield();
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
            // 简化: 暂不实现进程组, 跳过
        }
    }
    None
}
