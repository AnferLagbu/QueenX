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
