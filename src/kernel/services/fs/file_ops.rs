#![deny(unsafe_code)]
//! 文件操作策略 — ioctl / clock_gettime / poll / chown / truncate / ftruncate / flock
//!
//! 从 framework/syscall/mod.rs 迁移的策略代码:
//! - ioctl_syscall: 设备 I/O 控制
//! - poll_syscall: 轮询
//! - chown_syscall: 文件属主修改
//! - truncate_syscall / ftruncate_syscall: 文件截断
//! - flock_syscall: BSD 风格文件锁
//!
//! 注: clock_gettime 已迁往 services::timer::clock (B05-26 时间归位).
//!
//! ## 框内核边界
//! - 100% safe Rust
//! - 通过 framework 公开 API 访问
//! - 无 unsafe, 无裸指针

use crate::framework::syscall::Errno;

const POLLIN: i16 = 1;
const POLLOUT: i16 = 4;

const TIOCGWINSZ: u64 = 0x5413;
const TCGETS: u64 = 0x5401;

#[expect(
    clippy::struct_field_names,
    reason = "struct_field_names: 字段名前缀相同是为可读性/调试; 当前优先 expect"
)]
/// ioctl(fd, request, arg) 策略
pub fn ioctl_syscall(_fd: i32, request: u64, arg: u64) -> i64 {
    if arg == 0 {
        return Errno::EINVAL.as_ret();
    }
    match request {
        TIOCGWINSZ => {
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct Winsize {
                ws_row: u16,
                ws_col: u16,
                ws_xpixel: u16,
                ws_ypixel: u16,
            }
            let ws = Winsize {
                ws_row: 25,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if !crate::framework::syscall::api::write_struct_to_user(arg, &ws) {
                return Errno::EFAULT.as_ret();
            }
            0
        }
        TCGETS => Errno::ENOSYS.as_ret(),
        _ => Errno::ENOTTY.as_ret(),
    }
}

/// poll(fds, nfds, timeout) 策略
pub fn poll_syscall(fds_ptr: u64, nfds: u32, _timeout: i32) -> i64 {
    if fds_ptr == 0 || nfds == 0 {
        return 0;
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    let mut ready: i32 = 0;
    let pfd_size = core::mem::size_of::<PollFd>() as u64;

    for i in 0..nfds as usize {
        let offset = i as u64 * pfd_size;
        let mut pfd = PollFd {
            fd: -1,
            events: 0,
            revents: 0,
        };

        if !crate::framework::syscall::api::read_struct_from_user(
            fds_ptr + offset,
            &mut pfd,
        ) {
            continue;
        }

        pfd.revents = 0;
        if pfd.fd < 0 {
            // 写回原位
            let _ = crate::framework::syscall::api::write_struct_to_user(
                fds_ptr + offset,
                &pfd,
            );
            continue;
        }
        if pfd.events & POLLIN != 0 {
            let fd_table = crate::framework::fs::VFS_MANAGER.fd_table.lock();
            // B06-07: fd 上限用 VFS_MAX_FDS (32) 而非硬编码 256, 防止越界索引 32 长数组
            if (pfd.fd as usize) < crate::framework::fs::VFS_MAX_FDS
                && fd_table[pfd.fd as usize].used
            {
                pfd.revents |= POLLIN;
                ready += 1;
            }
        }
        if pfd.events & POLLOUT != 0 {
            pfd.revents |= POLLOUT;
            ready += 1;
        }

        let _ =
            crate::framework::syscall::api::write_struct_to_user(fds_ptr + offset, &pfd);
    }
    i64::from(ready)
}

/// chown(path, uid, gid) 策略
pub fn chown_syscall(path_ptr: u64, uid: u32, gid: u32) -> i64 {
    if path_ptr == 0 || !crate::framework::syscall::api::validate_user_ptr(path_ptr) {
        return Errno::EFAULT.as_ret();
    }
    let path = path_ptr as *const u8;
    let tbl = crate::framework::credo::identity::get_table();
    // B06-02: UID/GID 未注册时返回 EINVAL, 不得默认 root (原 map_or(0, ...) 存在提权漏洞)
    let owner_pwm = match tbl.find_by_uid(uid) {
        Some(e) => e.get_pwm().0,
        None => return Errno::EINVAL.as_ret(),
    };
    let group_pwm = match tbl.find_by_uid(gid) {
        Some(e) => e.get_pwm().0,
        None => return Errno::EINVAL.as_ret(),
    };
    let pwm = crate::framework::credo::pwm_get_current();
    i64::from(crate::framework::fs::vfs_chown_ext(
        path, owner_pwm, group_pwm, pwm,
    ))
}

/// truncate(path, length) 策略
pub fn truncate_syscall(path_ptr: u64, length: i64) -> i64 {
    if path_ptr == 0
        || !crate::framework::syscall::api::validate_user_ptr(path_ptr)
        || length < 0
    {
        return Errno::EINVAL.as_ret();
    }
    let path = path_ptr as *const u8;
    let fd = crate::framework::fs::vfs_open(
        path,
        0o2,
        crate::framework::credo::pwm_get_current(),
    );
    if fd < 0 {
        return Errno::ENOENT.as_ret();
    }
    let result = crate::framework::fs::vfs_truncate_internal(fd as u32, length as u64);
    crate::framework::fs::vfs_close(fd as u32);
    if result < 0 { Errno::EIO.as_ret() } else { 0 }
}

/// ftruncate(fd, length) 策略
pub fn ftruncate_syscall(fd: i32, length: i64) -> i64 {
    if fd < 0 || length < 0 {
        return Errno::EINVAL.as_ret();
    }
    let result = crate::framework::fs::vfs_truncate_internal(fd as u32, length as u64);
    if result < 0 { Errno::EIO.as_ret() } else { 0 }
}

/// `AT_FDCWD` — 相对路径基于当前工作目录 (Linux ABI)
const AT_FDCWD: i32 = -100;

/// fchownat(dirfd, path, uid, gid, flags) 策略 (T1 G1 实装)
///
/// SIMPLIFIED: 仅支持 `dirfd == AT_FDCWD` (绝对路径语义, 复用 `chown_syscall`
/// 的 UID/GID→PWM 查表 + `vfs_chown_ext`); 非 AT_FDCWD 的目录 fd 相对路径
/// 需 VFS 目录 fd 解析机制, 暂不支持. `flags` (AT_SYMLINK_NOFOLLOW) 忽略
/// (vfs_chown_ext 不跟随 symlink).
pub fn fchownat_syscall(dirfd: i32, path_ptr: u64, uid: u32, gid: u32, _flags: i32) -> i64 {
    if dirfd != AT_FDCWD {
        return Errno::ENOTSUP.as_ret();
    }
    chown_syscall(path_ptr, uid, gid)
}

/// fallocate(fd, mode, offset, len) 策略 (T1 G1 实装)
///
/// SIMPLIFIED: 仅支持 `mode == 0` (分配并扩展文件大小到 `offset+len`,
/// Linux fallocate 默认语义); 其他 mode 标志 (KEEP_SIZE 等) 暂不实现.
/// 基于 `vfs_fstat_safe` + `vfs_truncate_internal` (仅扩展不缩小) 近似.
pub fn fallocate_syscall(fd: i32, mode: i32, offset: u64, len: u64) -> i64 {
    if mode != 0 {
        return Errno::ENOSYS.as_ret();
    }
    if fd < 0 {
        return Errno::EBADF.as_ret();
    }
    let Some(end) = offset.checked_add(len) else {
        return Errno::EFBIG.as_ret();
    };
    // 仅扩展不缩小: 目标大小超过当前 size 时才截断扩展
    let cur_size = crate::framework::fs::api::vfs_fstat_safe(
        fd as u32,
        crate::framework::credo::pwm_get_current(),
    )
    .map_or(0, |st| u64::from(st.size));
    if end > cur_size {
        let r = crate::framework::fs::vfs_truncate_internal(fd as u32, end);
        if r < 0 {
            return Errno::EIO.as_ret();
        }
    }
    0
}

#[expect(
    clippy::match_same_arms,
    reason = "match_same_arms: match arm 重复是为可读性/调试断点; 当前优先 expect"
)]
/// flock(fd, operation) 策略
pub fn flock_syscall(fd: i32, operation: i32) -> i64 {
    use crate::framework::fs::{FlockResult, sys_flock as do_flock};

    if fd < 0 {
        return Errno::EBADF.as_ret();
    }

    let ino = {
        let fd_table = crate::framework::fs::VFS_MANAGER.fd_table.lock();
        if (fd as usize) >= crate::framework::fs::VFS_MAX_FDS || !fd_table[fd as usize].used
        {
            return Errno::EBADF.as_ret();
        }
        fd_table[fd as usize].node_id
    };

    let pid = crate::framework::proc::process_get_current_pid();

    match do_flock(fd, operation, pid, ino) {
        FlockResult::Ok => 0,
        FlockResult::WouldBlock => Errno::EAGAIN.as_ret(),
        FlockResult::Invalid => Errno::EINVAL.as_ret(),
        FlockResult::NoSpace => Errno::ENOLCK.as_ret(),
        FlockResult::NotHeld => Errno::EINVAL.as_ret(),
    }
}
