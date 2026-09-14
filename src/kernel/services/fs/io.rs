#![deny(unsafe_code)]
//! IO 系统调用 — services 层安全代理
//!
//! ## 范围
//!
//! - pipe: 匿名管道创建
//! - dup/dup2: 文件描述符复制
//! - fcntl: 文件控制
//!
//! ## 安全边界
//!
//! - services 层: 验证参数类型/范围,委托 framework 实现
//! - framework 层: 实际访问 VFS / 创建内核对象

use crate::framework::syscall::Errno;

/// pipe 系统调用安全代理
///
/// `fds` 指向用户空间 i32`[2]` 数组 (8 字节)
///
/// # Errors
/// 当 `fds` 为空指针时返回 `EFAULT`; 其余错误由底层 `sys_pipe` 以对应 `Errno` 传播.
pub fn pipe_syscall(fds: u64) -> Result<usize, Errno> {
    if fds == 0 {
        return Err(Errno::EFAULT);
    }
    let ret = crate::framework::syscall::io::sys_pipe(fds);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

/// pipe2 系统调用安全代理 (支持 `flags`)
///
/// `fds` 指向用户空间 i32`[2]` 数组 (8 字节)
///
/// # Errors
/// 当 `fds` 为空指针时返回 `EFAULT`; 其余错误由底层 `sys_pipe2` 以对应 `Errno` 传播.
pub fn pipe2_syscall(fds: u64, flags: i32) -> Result<usize, Errno> {
    if fds == 0 {
        return Err(Errno::EFAULT);
    }
    let ret = crate::framework::syscall::io::sys_pipe2(fds, flags);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

/// dup 安全代理
///
/// # Errors
/// 当 `oldfd` 为负数时返回 `EBADF`; 其余错误由底层 `sys_dup` 以对应 `Errno` 传播.
pub fn dup_syscall(oldfd: i32) -> Result<usize, Errno> {
    if oldfd < 0 {
        return Err(Errno::EBADF);
    }
    let ret = crate::framework::syscall::io::sys_dup(oldfd);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

/// dup2 安全代理
///
/// # Errors
/// 当 `oldfd` 或 `newfd` 为负数时返回 `EBADF`; 其余错误由底层 `sys_dup2` 以对应 `Errno` 传播.
pub fn dup2_syscall(oldfd: i32, newfd: i32) -> Result<usize, Errno> {
    if oldfd < 0 || newfd < 0 {
        return Err(Errno::EBADF);
    }
    let ret = crate::framework::syscall::io::sys_dup2(oldfd, newfd);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

/// dup3 安全代理 (支持 `flags`)
///
/// # Errors
/// 当 `oldfd` 或 `newfd` 为负数时返回 `EBADF`; `oldfd == newfd` 时返回 `EINVAL`
/// (dup3 语义要求两 fd 不同); 其余错误由底层 `sys_dup3` 以对应 `Errno` 传播.
pub fn dup3_syscall(oldfd: i32, newfd: i32, flags: i32) -> Result<usize, Errno> {
    if oldfd < 0 || newfd < 0 {
        return Err(Errno::EBADF);
    }
    let ret = crate::framework::syscall::io::sys_dup3(oldfd, newfd, flags);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

/// fcntl 安全代理
///
/// # Errors
/// 当 `fd` 为负数时返回 `EBADF`; 其余错误由底层 `sys_fcntl` 以对应 `Errno` 传播.
pub fn fcntl_syscall(fd: i32, cmd: i32, arg: u64) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let ret = crate::framework::syscall::io::sys_fcntl(fd, cmd, arg);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}

/// `copy_file_range` — 在两个文件描述符之间复制数据
///
/// 简化实现: 使用 read/write 循环 (非零拷贝)
///
/// # Arguments
/// * `fd_in` - 源文件描述符
/// * `off_in` - 源偏移量指针
/// * `fd_out` - 目标文件描述符
/// * `off_out` - 目标偏移量指针
/// * `len` - 复制长度
///
/// # Returns
/// 成功返回复制的字节数，失败返回 Errno
///
/// # Errors
/// 当 `fd_in` 或 `fd_out` 为负数时返回 `EBADF`;
/// 当底层 read/write 失败且尚未复制任何字节时返回对应的 `Errno`.
pub fn copy_file_range_syscall(
    fd_in: i32,
    _off_in: u64,
    fd_out: i32,
    _off_out: u64,
    len: usize,
) -> Result<usize, Errno> {
    // 参数验证
    if fd_in < 0 || fd_out < 0 {
        return Err(Errno::EBADF);
    }
    if len == 0 {
        return Ok(0);
    }

    // 限制单次复制大小 (避免栈溢出)
    let chunk_size = len.min(4096);
    let mut buf = alloc::vec![0u8; chunk_size];
    let mut total_copied = 0usize;

    loop {
        let remaining = len - total_copied;
        if remaining == 0 {
            break;
        }

        let to_read = remaining.min(chunk_size);

        // 从源 fd 读取
        let read_ret = crate::framework::fs::api::vfs_read(
            fd_in as u32,
            buf.as_mut_ptr(),
            to_read as u32,
        );
        if read_ret < 0 {
            if total_copied > 0 {
                return Ok(total_copied);
            }
            return Err(Errno::from_ret(i64::from(read_ret)));
        }
        let bytes_read = read_ret as usize;
        if bytes_read == 0 {
            break; // EOF
        }

        // 写入目标 fd
        let write_ret = crate::framework::fs::api::vfs_write(
            fd_out as u32,
            buf.as_ptr(),
            bytes_read as u32,
        );
        if write_ret < 0 {
            if total_copied > 0 {
                return Ok(total_copied);
            }
            return Err(Errno::from_ret(i64::from(write_ret)));
        }

        total_copied += bytes_read as usize;

        // 如果写入的字节数少于读取的, 停止
        if (write_ret as usize) < bytes_read {
            break;
        }
    }

    Ok(total_copied)
}
