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
use core::sync::atomic::Ordering;

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

    if ((flags & CLONE_VM) != 0 || (flags & CLONE_THREAD) != 0) && (flags & CLONE_SIGHAND) == 0 {
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

/// `set_robust_list(head, len)` 策略 — 登记当前进程的 robust futex 链表头
///
/// `len` 必须为 `sizeof(struct robust_list_head)` = 24.
/// 登记后由退出路径 (`framework::proc::robust::exit_cleanup`) 消费:
/// 进程退出时遍历链表, 对本进程持有的 futex 置 `FUTEX_OWNER_DIED` 并唤醒.
///
/// # Errors
///
/// - `len != 24` → `EINVAL`
/// - 当前进程不存在 → `ESRCH`
pub fn set_robust_list_syscall(head: u64, len: u64) -> Result<usize, Errno> {
    const ROBUST_LIST_HEAD_SIZE: u64 = 24;
    if len != ROBUST_LIST_HEAD_SIZE {
        return Err(Errno::EINVAL);
    }

    let pid = crate::framework::proc::api::process_get_current_pid();
    crate::framework::proc::api::process_with(pid, |p| {
        p.robust_head.store(head, Ordering::Release);
        p.robust_len
            .store(ROBUST_LIST_HEAD_SIZE as u32, Ordering::Release);
    })
    .ok_or(Errno::ESRCH)?;
    Ok(0)
}

/// `get_robust_list(pid, head_ptr, len_ptr)` 策略 — 查询目标进程的 robust list 登记
///
/// `pid == 0` 表示当前进程. `head_ptr`/`len_ptr` 为 NULL 时跳过对应写回
/// (Linux 允许).
///
// SIMPLIFIED: 不做跨进程读权限校验 (Linux 要求 PTRACE_MODE_READ, 当前无
// ptrace/uid 模型); 影响面: 任意进程可探测他进程是否登记 robust list
// (仅元数据, 非内存内容); 何时需扩展: 引入 ptrace 权限模型后补 EPERM 判定.
///
/// # Errors
///
/// - `pid < 0` (无负 pid 语义) 或目标进程不存在 → `ESRCH`
/// - 写回失败 → `EFAULT`
pub fn get_robust_list_syscall(pid: i32, head_ptr: u64, len_ptr: u64) -> Result<usize, Errno> {
    if pid < 0 {
        return Err(Errno::ESRCH);
    }

    let target = if pid == 0 {
        crate::framework::proc::api::process_get_current_pid()
    } else {
        pid as u32
    };
    let (head, len) = crate::framework::proc::api::process_with(target, |p| {
        (
            p.robust_head.load(Ordering::Acquire),
            p.robust_len.load(Ordering::Acquire),
        )
    })
    .ok_or(Errno::ESRCH)?;

    if head_ptr != 0 && !crate::framework::syscall::api::write_struct_to_user(head_ptr, &head) {
        return Err(Errno::EFAULT);
    }
    if len_ptr != 0 && !crate::framework::syscall::api::write_struct_to_user(len_ptr, &len) {
        return Err(Errno::EFAULT);
    }
    Ok(0)
}
