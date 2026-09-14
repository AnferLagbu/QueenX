#![deny(unsafe_code)]
//! clone — services 层安全代理
//!
//! 为 clone 系统调用提供参数验证:
//! - flags 合法性检查
//! - `child_stack` 对齐检查
//! - `CLONE_VM` + `CLONE_THREAD` 必须同时设置 `CLONE_SIGHAND`
//!
//! ## 安全边界
//!
//! - services 层验证标量参数和标志组合
//! - 页表/进程操作委托给 framework 层 (TCB)

use crate::framework::syscall::Errno;

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
/// clone 安全代理
///
/// 验证: flags 合法, `CLONE_VM+CLONE_THREAD` 需要 `CLONE_SIGHAND`
///
/// # Errors
///
/// - `CLONE_VM`/`CLONE_THREAD` 未同时设置 `CLONE_SIGHAND`, 或 `child_stack`
///   非零但未按 16 字节对齐 → `EINVAL`
/// - 底层 clone 返回负值时转换为对应的 `Errno`
pub fn clone_syscall(
    flags: u64,
    child_stack: u64,
    parent_tidptr: u64,
    child_tidptr: u64,
    tls: u64,
) -> Result<usize, Errno> {
    // CLONE_VM + CLONE_THREAD 必须同时设置 CLONE_SIGHAND (POSIX 线程要求)
    // 注: Rust 中 `&` 优先级高于 `==`, 括号仅为明确语义 (B05-31 审计复核).
    const CLONE_VM: u64 = 0x00000100;
    const CLONE_THREAD: u64 = 0x00010000;
    const CLONE_SIGHAND: u64 = 0x00000800;

    if ((flags & CLONE_VM) != 0 || (flags & CLONE_THREAD) != 0)
        && (flags & CLONE_SIGHAND) == 0
    {
        return Err(Errno::EINVAL);
    }

    // child_stack 如果非零, 必须对齐到 16 字节 (x86_64 ABI)
    if child_stack != 0 && !child_stack.is_multiple_of(16) {
        return Err(Errno::EINVAL);
    }

    let ret = crate::framework::syscall::clone::sys_clone(
        flags,
        child_stack,
        parent_tidptr,
        child_tidptr,
        tls,
    );
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}
