//! sendfile / splice — 零拷贝数据传输 services 层安全封装
//!
//! 100% safe Rust, 0 unsafe.
//! 封装 framework 层 sendfile/splice 接口, 提供类型安全的 API.

#![deny(unsafe_code)]

pub use crate::framework::syscall::{
    SPLICE_F_GIFT, SPLICE_F_MORE, SPLICE_F_MOVE, SPLICE_F_NONBLOCK,
};

/// sendfile 系统调用
pub fn sys_sendfile(out_fd: i32, in_fd: i32, offset_ptr: u64, count: usize) -> i64 {
    crate::framework::syscall::sys_sendfile(out_fd, in_fd, offset_ptr, count)
}

/// splice 系统调用
pub fn sys_splice(
    fd_in: i32,
    off_in: u64,
    fd_out: i32,
    off_out: u64,
    len: usize,
    flags: u32,
) -> i64 {
    crate::framework::syscall::sys_splice(fd_in, off_in, fd_out, off_out, len, flags)
}
