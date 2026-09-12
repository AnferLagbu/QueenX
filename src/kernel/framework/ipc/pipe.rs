//! 管道 (Pipe) FFI 边界 — T6-1 策略已迁移至 services/ipc/pipe.rs
//!
//! 本模块仅保留:
//! - `is_pipe_fd()` 公开接口 (供 sendfile/splice 使用)
//! - FFI 函数 (用户空间指针转换, 委托 services 策略)
//!
//! ## SAFETY
//!
//! - FFI 函数通过 `RacyCell::get_mut()` 安全访问全局 IPC_NAMESPACE.
//! - 用户空间指针通过 `UserReadPtr/WritePtr/RefMut` 安全访问.

use crate::kernel::framework::ipc::strategy::current_ipc_strategy;
use crate::kernel::framework::proc::process_get_current_pid;
use crate::kernel::framework::userptr::{UserReadPtr, UserRefMut, UserWritePtr};

/// 判断 fd 是否为 pipe fd (公开接口, 供 sendfile/splice 使用)
pub fn is_pipe_fd(fd: i32) -> bool {
    current_ipc_strategy().is_pipe_fd(fd)
}

/// POSIX `pipe(pipefd)` 内核实现。
///
/// # Safety
/// `pipefd` 必须是可写指针, 含至少 2 个 `i32` 空间 (用于返回 [`read_fd`, `write_fd`])。
/// 由 `sys_pipe` 分发, cred 校验已通过。
#[unsafe(no_mangle)]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
pub unsafe extern "C" fn ipc_pipe_create(pipefd: *mut i32) -> i32 {
    if pipefd.is_null() {
        return -1;
    }

    let ns = super::IPC_NAMESPACE.get_mut();
    let next_id = super::NEXT_IPC_ID.get_mut();
    let current_pid = process_get_current_pid();

    match current_ipc_strategy().pipe_create(ns, next_id, current_pid) {
        Ok((rfd, wfd)) => {
            // SAFETY: pipefd 已校验非空; 调用方保证其指向用户态内存中
            // 至少 2 个有效的 i32 值.
            let mut fds = unsafe { UserRefMut::<[i32; 2]>::new(pipefd as *mut [i32; 2]) };
            let arr = fds.as_mut();
            arr[0] = rfd;
            arr[1] = wfd;
            0
        }
        Err(_) => -1,
    }
}

/// POSIX `read(fd, buf, count)` 内核实现 (仅 pipe fd)。
///
/// # Safety
/// `buf` 必须是有效可写指针, 至少 `count` 字节, 内存必须在调用期间保持有效。
/// 由 `sys_read` 分发, cred 校验已通过。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipc_pipe_read(fd: i32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    let ns = super::IPC_NAMESPACE.get_mut();
    // SAFETY: buf 已校验非空; 调用方保证其指向用户态内存中
    // 至少 `count` 个有效字节.
    let mut user_buf = unsafe { UserWritePtr::new(buf, count as usize) };
    current_ipc_strategy()
        .pipe_read(ns, fd, user_buf.as_mut_slice(), count)
        .map_or(-1, |n| n as i32)
}

/// POSIX `write(fd, buf, count)` 内核实现 (仅 pipe fd)。
///
/// # Safety
/// `buf` 必须是有效可读指针, 至少 `count` 字节, 内存必须在调用期间保持有效。
/// 由 `sys_write` 分发, cred 校验已通过。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipc_pipe_write(fd: i32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    let ns = super::IPC_NAMESPACE.get_mut();
    // SAFETY: buf 已校验非空; 调用方保证其指向用户态内存中
    // 至少 `count` 个有效字节.
    let user_buf = unsafe { UserReadPtr::new(buf, count as usize) };
    current_ipc_strategy()
        .pipe_write(ns, fd, user_buf.as_slice(), count)
        .map_or(-1, |n| n as i32)
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn ipc_pipe_close(fd: i32) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    match current_ipc_strategy().pipe_close(ns, fd) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
