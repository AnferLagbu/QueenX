#![deny(unsafe_code)]
//! wait4 — services 层安全代理
//!
//! 为 wait4 系统调用提供参数验证:
//! - pid 参数合法性 (POSIX pid 语义)
//! - options 标志组合
//! - wstatus 指针非零时由 framework 层 `check_user_ptr` 验证
//!
//! ## 安全边界
//!
//! - services 层验证 pid 范围/options 合法
//! - 进程表操作和阻塞委托给 framework 层 (TCB)

use crate::framework::syscall::Errno;

/// wait4 安全代理
///
/// 验证: pid 范围合法, options 仅含合法标志
///
/// # Errors
///
/// - `pid` 超出合法范围或 `options` 含非法标志 → `EINVAL`
/// - 底层 `sys_wait4` 返回负值时转换为对应的 `Errno`
pub fn wait4_syscall(pid: i32, wstatus_ptr: u64, options: i32) -> Result<usize, Errno> {
    // pid 范围: -PID_MAX_LIMIT .. PID_MAX_LIMIT
    // 简化: -32768..=32767
    const PID_MAX: i32 = 0x7FFF;
    const PID_MIN: i32 = -0x8000;
    if !(PID_MIN..=PID_MAX).contains(&pid) {
        return Err(Errno::EINVAL);
    }

    // options 标志验证
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    const WNOHANG: i32 = 0x1;
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    const WUNTRACED: i32 = 0x2;
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    const WCONTINUED: i32 = 0x8;
    let valid_opts = WNOHANG | WUNTRACED | WCONTINUED;
    if options & !valid_opts != 0 {
        return Err(Errno::EINVAL);
    }

    // wstatus 指针如果为 0, 允许 (调用方不需要状态)
    // 否则由 framework 内部 check_user_ptr 验证

    let ret = crate::framework::syscall::wait4::sys_wait4(pid, wstatus_ptr, options);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

// ============================================================================
// waitid — T1-G2 自 framework 回退层实装
// ============================================================================

/// `idtype_t`: 等待全部子进程
const P_ALL: i32 = 0;
/// `idtype_t`: 等待特定 PID
const P_PID: i32 = 1;
/// `idtype_t`: 等待特定进程组
const P_PGID: i32 = 2;

/// waitid options: 报告已退出子进程
const WEXITED: i32 = 0x0000_0004;
/// waitid options: 报告已停止子进程 (与 wait4 `WUNTRACED` 同位)
const WSTOPPED: i32 = 0x0000_0002;
/// waitid options: 报告已恢复子进程
const WCONTINUED: i32 = 0x0000_0008;
/// waitid options: 非阻塞
const WNOHANG: i32 = 0x0000_0001;
/// waitid options: 观察但不收割 (子进程保持 Zombie)
const WNOWAIT: i32 = 0x0100_0000;

/// `si_code`: 正常退出
const CLD_EXITED: i32 = 1;
/// SIGCHLD 信号编号 (x86_64)
const SIGCHLD: i32 = 17;

/// `struct siginfo_t` 的 SIGCHLD 子布局 (Linux x86_64 用户 ABI, 128 字节)
///
/// 写回字段: `si_signo/si_errno/si_code/si_pid/si_uid/si_status/si_utime/si_stime`.
#[repr(C)]
#[derive(Copy, Clone)]
struct SiginfoChld {
    /// 信号编号 (SIGCHLD)
    si_signo: i32,
    /// errno (恒 0)
    si_errno: i32,
    /// 来源码 (CLD_EXITED)
    si_code: i32,
    /// 对齐填充
    __pad0: i32,
    /// 子进程 PID
    si_pid: i32,
    /// 子进程 UID (无 uid 模型, 恒 0)
    si_uid: u32,
    /// 退出码
    si_status: i32,
    /// 用户态耗时 (未跟踪, 恒 0)
    si_utime: i32,
    /// 内核态耗时 (未跟踪, 恒 0)
    si_stime: i32,
    /// 剩余填充 (保持 128 字节 ABI 尺寸)
    __pad: [u8; 96],
}

/// `waitid(idtype, id, infop, options)` 策略 — 等待子进程状态变化
///
// SIMPLIFIED: 1) `si_code` 恒为 CLD_EXITED — 信号投递路径未记录致死信号
// (termsig), 无法区分 CLD_KILLED/CLD_DUMPED; 影响面: 被信号终止的子进程
// siginfo 归类为正常退出; 何时需扩展: 信号投递记录 termsig 至 Process 后映射.
// 2) `si_uid` 恒 0 — 无 uid 模型 (Credo PWM). 3) 拒绝 WSTOPPED/WCONTINUED
// (EINVAL) — 无进程 stop/continue 状态跟踪, 作业控制等待不可用.
///
/// # Errors
///
/// - `options` 含非法位, 或未含 `WEXITED/WSTOPPED/WCONTINUED` 之一 → `EINVAL`
/// - `options` 含 `WSTOPPED/WCONTINUED` (无状态跟踪) → `EINVAL`
/// - `idtype` 非法, 或 `P_PID/P_PGID` 下 `id == 0` 或超出 PID 空间 → `EINVAL`
/// - 无匹配子进程 → `ECHILD`
/// - siginfo 写回失败 → `EFAULT`
pub fn waitid_syscall(idtype: i32, id: u64, infop: u64, options: i32) -> Result<usize, Errno> {
    // PID 空间上界 (与 wait4 校验一致)
    const PID_MAX: u64 = 0x7FFF;
    const VALID_OPTS: i32 = WEXITED | WSTOPPED | WCONTINUED | WNOHANG | WNOWAIT;
    if options & !VALID_OPTS != 0 {
        return Err(Errno::EINVAL);
    }
    // 必须指定等待类别之一 (Linux 语义)
    if options & (WEXITED | WSTOPPED | WCONTINUED) == 0 {
        return Err(Errno::EINVAL);
    }
    // 无 stopped/continued 状态跟踪 (见函数级 SIMPLIFIED 注释)
    if options & (WSTOPPED | WCONTINUED) != 0 {
        return Err(Errno::EINVAL);
    }

    let target: i32 = match idtype {
        P_ALL => -1,
        P_PID => {
            if id == 0 || id > PID_MAX {
                return Err(Errno::EINVAL);
            }
            id as i32
        }
        P_PGID => {
            if id == 0 || id > PID_MAX {
                return Err(Errno::EINVAL);
            }
            // wait_reap 进程组语义: target < -1 → 匹配 pgid == |target|
            -(id as i32)
        }
        _ => return Err(Errno::EINVAL),
    };

    let non_blocking = options & WNOHANG != 0;
    let keep_zombie = options & WNOWAIT != 0;

    match crate::framework::syscall::wait4::wait_reap(target, non_blocking, keep_zombie) {
        crate::framework::syscall::wait4::WaitOutcome::Reaped(info) => {
            if infop != 0 {
                let si = SiginfoChld {
                    si_signo: SIGCHLD,
                    si_errno: 0,
                    si_code: CLD_EXITED,
                    __pad0: 0,
                    si_pid: info.pid as i32,
                    si_uid: 0,
                    si_status: info.exit_code as i32,
                    si_utime: 0,
                    si_stime: 0,
                    __pad: [0; 96],
                };
                if !crate::framework::syscall::api::write_struct_to_user(infop, &si) {
                    return Err(Errno::EFAULT);
                }
            }
            Ok(0)
        }
        // WNOHANG: 子进程尚未退出
        crate::framework::syscall::wait4::WaitOutcome::Running => Ok(0),
        crate::framework::syscall::wait4::WaitOutcome::NoChild => Err(Errno::ECHILD),
    }
}
