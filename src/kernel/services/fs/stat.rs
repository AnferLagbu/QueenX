#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 文件状态系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
//! - 委托 `framework/fs/vfs::api` 完成
//!
//! ## POSIX 语义
//!
//! - [`stat_syscall`] 跟随符号链接
//! - [`lstat_syscall`] 不跟随符号链接
//! - [`fstat_syscall`] 按 FD 查询

use crate::framework::credo;
use crate::framework::fs::VfsStat;
use crate::framework::fs::api as fw;
use crate::framework::syscall::Errno;
use crate::framework::syscall::raw;

const VFS_STAT_SIZE: u64 = core::mem::size_of::<VfsStat>() as u64;

// ============================================================================
// stat
// ============================================================================

/// stat(path, `st_buf`) — 跟随符号链接查询文件元数据
///
/// # Errors
/// 当路径或缓冲区指针为空/越界时返回 `EFAULT`; 当底层 stat 失败时返回 `EIO`.
pub fn stat_syscall(path_ptr: u64, st_buf_ptr: u64) -> Result<usize, Errno> {
    if path_ptr == 0 || st_buf_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(st_buf_ptr, VFS_STAT_SIZE) {
        return Err(Errno::EFAULT);
    }
    let pwm = current_pwm()?;
    // framework safe API: 返回 VfsStat (无 raw pointer 跨边界)
    let stat = fw::vfs_stat_safe(path_ptr as *const u8, pwm).ok_or(Errno::EIO)?;
    // framework safe API: 写结构体到 user buf, 内部已 check_user_buf
    if !raw::write_struct_to_user::<VfsStat>(st_buf_ptr, &stat) {
        return Err(Errno::EFAULT);
    }
    Ok(0)
}

// ============================================================================
// lstat
// ============================================================================

/// lstat(path, `st_buf`) — 不跟随符号链接查询文件元数据
///
/// Framekernel 简化: `vfs_stat` 不跟随 symlink, 行为同 lstat。
///
/// # Errors
/// 错误条件与 [`stat_syscall`] 相同, 参见其 `# Errors` 段.
pub fn lstat_syscall(path_ptr: u64, st_buf_ptr: u64) -> Result<usize, Errno> {
    stat_syscall(path_ptr, st_buf_ptr)
}

// ============================================================================
// fstat
// ============================================================================

/// fstat(fd, `st_buf`) — 按 FD 查询文件元数据
///
/// # Errors
/// 当 `fd` 为负数时返回 `EBADF`; 当缓冲区指针为空/越界时返回 `EFAULT`;
/// 当底层 fstat 失败时返回 `EIO`.
pub fn fstat_syscall(fd: i32, st_buf_ptr: u64) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if st_buf_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(st_buf_ptr, VFS_STAT_SIZE) {
        return Err(Errno::EFAULT);
    }
    let pwm = current_pwm()?;
    let stat = fw::vfs_fstat_safe(fd as u32, pwm).ok_or(Errno::EIO)?;
    if !raw::write_struct_to_user::<VfsStat>(st_buf_ptr, &stat) {
        return Err(Errno::EFAULT);
    }
    Ok(0)
}

// ============================================================================
// 内部辅助
// ============================================================================

#[expect(
    clippy::unnecessary_wraps,
    reason = "保留 Option/Result<()> 包装便于 API 兼容性 (调用方可能 match 或 .unwrap); 移除包装需同步修改调用点, 风险大"
)]
/// 取当前进程凭证,无会话时直接返回 EACCES (历史硬编码 `TEST_PWM` 路径已弃用)。
fn current_pwm() -> Result<u64, Errno> {
    Ok(credo::api::pwm_get_current())
}
