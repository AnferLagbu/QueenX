#![deny(unsafe_code)]
//! 路径/工作目录系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe,纯类型安全
//! - 委托 `framework/fs/vfs::api` 完成
//!
//! ## POSIX 语义
//!
//! - [`chdir_syscall`] 切换当前工作目录
//! - [`getcwd_syscall`] 取当前工作目录到用户缓冲

use crate::framework::fs::api as fw;
use crate::framework::syscall::Errno;
use crate::framework::syscall::raw;

// ============================================================================
// chdir
// ============================================================================

/// chdir(path) — 切换工作目录
///
/// # 参数
/// - `path_ptr`: 用户空间 NUL 终止 C 字符串地址
///
/// # Errors
/// 当 `path_ptr` 为空或不在用户可访问范围内时返回 `EFAULT`.
pub fn chdir_syscall(path_ptr: u64) -> Result<usize, Errno> {
    if path_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    // SAFETY: path_ptr 由 check_user_ptr 验证为可读
    let ptr = path_ptr as *const u8;
    fw::vfs_set_cwd(ptr);
    Ok(0)
}

// ============================================================================
// getcwd
// ============================================================================

/// getcwd(buf, size) — 取当前工作目录到用户缓冲
///
/// # 参数
/// - `buf_ptr`:  用户空间缓冲地址
/// - `size`:     缓冲长度
///
/// # Errors
/// 当 `buf_ptr` 为空或 `size` 为 0 时返回 `EINVAL`; 当缓冲区越界时返回 `EFAULT`;
/// 其余错误以对应的 `Errno` 返回.
pub fn getcwd_syscall(buf_ptr: u64, size: u64) -> Result<usize, Errno> {
    if buf_ptr == 0 || size == 0 {
        return Err(Errno::EINVAL);
    }
    if !raw::check_user_buf(buf_ptr, size) {
        return Err(Errno::EFAULT);
    }
    // SAFETY: buf_ptr 由 check_user_buf 验证为可写
    let buf = buf_ptr as *mut u8;
    let n = fw::vfs_get_cwd(buf, size as u32);
    if n < 0 {
        Err(Errno::from_ret(i64::from(n)))
    } else {
        Ok(n as usize)
    }
}
