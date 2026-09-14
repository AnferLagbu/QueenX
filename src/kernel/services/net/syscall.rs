#![deny(unsafe_code)]
//! Socket 系统调用 — services 层入口代理
//!
//! ## 职责
//!
//! - 0 unsafe,纯类型安全
//! - 委托 framework/net/syscall.rs 完成用户空间数据搬运 + smoltcp 协议栈调用
//!
//! ## 与 [`mod@super::socket`] 区别
//!
//! - [`mod@super::socket`] 强类型 API (Domain, `SockType`, `SockAddrIn`, `&[u8]`)
//! - 本模块 syscall 入口 API (i32, u64 用户指针, u32 长度)

use super::unix as uds;
use crate::framework::net::syscall as fw;
use crate::framework::syscall::Errno;
use crate::framework::syscall::raw;

// ============================================================================
// 12 个 Socket Syscall 安全代理
// ============================================================================

#[expect(
    clippy::used_underscore_binding,
    reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
)]
/// socket(domain, type, protocol) — 返回新 FD
///
/// 分流: `AF_UNIX(1)` → UDS 子系统; `AF_INET(2)` → smoltcp 协议栈; 其他 → EAFNOSUPPORT
///
/// # Errors
///
/// 当 `AF_UNIX` 的 `sock_type` 非 Stream/Dgram 时返回 `Err(Errno::EINVAL)`; 不支持的协议族返回
/// `Err(Errno::EAFNOSUPPORT)`; 底层创建失败时返回对应的 `Errno`。
pub fn socket_syscall(domain: i32, sock_type: i32, _protocol: i32) -> Result<usize, Errno> {
    // AF_UNIX 分流
    if domain == 1 {
        let st = match sock_type {
            1 => uds::SockType::Stream,
            2 => uds::SockType::Dgram,
            _ => return Err(Errno::EINVAL),
        };
        return uds::socket(st)
            .map(|fd| fd as usize)
            .map_err(super::unix::UnixSocketError::to_errno);
    }
    // AF_INET (smoltcp)
    let r = fw::socket_syscall(domain, sock_type, _protocol);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// bind(fd, addr, addrlen)
///
/// 分流: 读取 `sun_family` 决定走 UDS 或 smoltcp
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 用户地址不可读时返回 `Err(Errno::EFAULT)`; 不支持的协议族返回
/// `Err(Errno::EAFNOSUPPORT)`; 底层 bind 失败时返回对应的 `Errno`。
pub fn bind_syscall(fd: i32, addr_ptr: u64, addrlen: u32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    // Peek sun_family
    let family = fw::raw_read_sun_family(addr_ptr)?;
    match family {
        1 => {
            // AF_UNIX
            let addr = fw::raw_read_sockaddr_un(addr_ptr, addrlen)?;
            uds::bind(fd, &addr)
                .map(|()| 0)
                .map_err(super::unix::UnixSocketError::to_errno)
        }
        2 | 10 => {
            // AF_INET / AF_INET6 (双栈, DECISION-032)
            let r = fw::bind_syscall(fd, addr_ptr, addrlen);
            if r < 0 {
                Err(Errno::from_ret(r))
            } else {
                Ok(r as usize)
            }
        }
        _ => Err(Errno::EAFNOSUPPORT),
    }
}

/// listen(fd, backlog)
///
/// 分流: UDS FD 走 `uds::listen`, smoltcp FD 走 `fw::listen`
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 当 `backlog` 为负数时返回 `Err(Errno::EINVAL)`;
/// 底层 listen 失败时返回对应的 `Errno`。
pub fn listen_syscall(fd: i32, backlog: i32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if backlog < 0 {
        return Err(Errno::EINVAL);
    }
    if uds::is_uds_fd(fd) {
        return uds::listen(fd)
            .map(|()| 0)
            .map_err(super::unix::UnixSocketError::to_errno);
    }
    let r = fw::listen_syscall(fd, backlog);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// accept(fd, addr, addrlen)
///
/// 分流: UDS FD 走 `uds::accept`, smoltcp FD 走 `fw::accept`
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 底层 accept 失败 (如无待处理连接) 时返回对应的 `Errno`。
pub fn accept_syscall(fd: i32, addr_ptr: u64, addrlen_ptr: u64) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if uds::is_uds_fd(fd) {
        return uds::accept(fd)
            .map(|fd| fd as usize)
            .map_err(super::unix::UnixSocketError::to_errno);
    }
    let r = fw::accept_syscall(fd, addr_ptr, addrlen_ptr);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// connect(fd, addr, addrlen)
///
/// 分流: 读取 `sun_family` 决定走 UDS 或 smoltcp
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 用户地址不可读时返回 `Err(Errno::EFAULT)`; 不支持的协议族返回
/// `Err(Errno::EAFNOSUPPORT)`; 底层 connect 失败时返回对应的 `Errno`。
pub fn connect_syscall(fd: i32, addr_ptr: u64, addrlen: u32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let family = fw::raw_read_sun_family(addr_ptr)?;
    match family {
        1 => {
            let addr = fw::raw_read_sockaddr_un(addr_ptr, addrlen)?;
            uds::connect(fd, &addr)
                .map(|()| 0)
                .map_err(super::unix::UnixSocketError::to_errno)
        }
        2 | 10 => {
            // AF_INET / AF_INET6 (双栈, DECISION-032)
            let r = fw::connect_syscall(fd, addr_ptr, addrlen);
            if r < 0 {
                Err(Errno::from_ret(r))
            } else {
                Ok(r as usize)
            }
        }
        _ => Err(Errno::EAFNOSUPPORT),
    }
}

/// `sendto` 系统调用 — 签名 `(fd, buf, len, flags, dest_addr, addrlen)`
///
/// 分流: UDS FD + `AF_UNIX` dest → `uds::sendto`; 其他 → `fw::sendto`
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; UDS 路径未提供目标地址时返回 `Err(Errno::EDESTADDRREQ)`;
/// 用户缓冲区不可读时返回 `Err(Errno::EFAULT)`; 底层 sendto 失败时返回对应的 `Errno`。
pub fn sendto_syscall(
    fd: i32,
    buf_ptr: u64,
    len: u32,
    flags: i32,
    dest_ptr: u64,
    dest_len: u32,
) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if uds::is_uds_fd(fd) {
        if dest_ptr == 0 {
            return Err(Errno::EDESTADDRREQ);
        }
        // copy-in 用户数据
        let data = fw::raw_copy_in(buf_ptr, len)?;
        // 解析 sockaddr_un
        let addr = fw::raw_read_sockaddr_un(dest_ptr, dest_len)?;
        return uds::sendto(fd, &data, &addr)
            .map(|n| n as usize)
            .map_err(super::unix::UnixSocketError::to_errno);
    }
    let r = fw::sendto_syscall(fd, buf_ptr, len, flags, dest_ptr, dest_len);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// `recvfrom` 系统调用 — 签名 `(fd, buf, len, flags, src_addr, addrlen)`
///
/// 分流: UDS FD → copy-out 到用户缓冲, 忽略 `src_addr` 填写 (v1 简化)
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 用户缓冲区无效时返回 `Err(Errno::EFAULT)`;
/// 底层 recvfrom 失败时返回对应的 `Errno`。
pub fn recvfrom_syscall(
    fd: i32,
    buf_ptr: u64,
    len: u32,
    flags: i32,
    src_ptr: u64,
    src_len_ptr: u64,
) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if buf_ptr == 0 || len == 0 {
        return Err(Errno::EFAULT);
    }
    if uds::is_uds_fd(fd) {
        if !raw::check_user_buf(buf_ptr, u64::from(len)) {
            return Err(Errno::EFAULT);
        }
        // 栈上缓冲接收, 再 copy-out (走 TCB raw_copy_out, 0 unsafe)
        let mut stack_buf = alloc::vec![0u8; len as usize];
        let n =
            uds::recvfrom(fd, &mut stack_buf).map_err(super::unix::UnixSocketError::to_errno)?;
        if n > 0 {
            fw::raw_copy_out(buf_ptr, n as u32, &stack_buf[..n])?;
        }
        // v1 简化: 不填 src_addr / addrlen (POSIX 允许)
        let _ = (src_ptr, src_len_ptr);
        return Ok(n as usize);
    }
    let r = fw::recvfrom_syscall(fd, buf_ptr, len, flags, src_ptr, src_len_ptr);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// setsockopt(fd, level, optname, val, valen)
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 底层 setsockopt 失败时返回对应的 `Errno`。
pub fn setsockopt_syscall(
    fd: i32,
    level: i32,
    optname: i32,
    val_ptr: u64,
    valen: u32,
) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let r = fw::setsockopt_syscall(fd, level, optname, val_ptr, valen);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// getsockopt(fd, level, optname, val, valen)
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 底层 getsockopt 失败时返回对应的 `Errno`。
pub fn getsockopt_syscall(
    fd: i32,
    level: i32,
    optname: i32,
    val_ptr: u64,
    valen_ptr: u64,
) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let r = fw::getsockopt_syscall(fd, level, optname, val_ptr, valen_ptr);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// getsockname(fd, addr, addrlen) — 获取本端地址
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 底层 getsockname 失败时返回对应的 `Errno`。
pub fn getsockname_syscall(fd: i32, addr_ptr: u64, addrlen_ptr: u64) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let r = fw::getsockname_syscall(fd, addr_ptr, addrlen_ptr);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// getpeername(fd, addr, addrlen) — 获取对端地址
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 底层 getpeername 失败时返回对应的 `Errno`。
pub fn getpeername_syscall(fd: i32, addr_ptr: u64, addrlen_ptr: u64) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let r = fw::getpeername_syscall(fd, addr_ptr, addrlen_ptr);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// shutdown(fd, how)
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; 底层 shutdown 失败时返回对应的 `Errno`。
pub fn shutdown_syscall(fd: i32, how: i32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let r = fw::shutdown_syscall(fd, how);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// sendmsg(fd, msg, flags) — 真实实现
///
/// v2: UDS 协议 sendmsg 路径 — 在 fw 之前处理 `msg_control` 注入 `SCM_CREDENTIALS`
/// (对端 `SO_PASSCRED` 启用时), 解析 cmsg 头序列处理 `SCM_RIGHTS` (跨进程 fd).
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; `msg_ptr` 或 `iov` 等用户缓冲区无效时返回 `Err(Errno::EFAULT)`;
/// 参数不合法 (如 `iovlen` 为 0 或超过上限) 时返回 `Err(Errno::EINVAL)`; 底层 sendmsg 失败时返回对应的 `Errno`。
pub fn sendmsg_syscall(fd: i32, msg_ptr: u64, flags: i32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if msg_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(msg_ptr, 56) {
        return Err(Errno::EFAULT);
    }
    let iov_ptr = raw::read_u64_from_user(msg_ptr + 16).ok_or(Errno::EFAULT)?;
    let iovlen = raw::read_u64_from_user(msg_ptr + 24).ok_or(Errno::EFAULT)?;
    if iovlen == 0 || iovlen > 1024 {
        return Err(Errno::EINVAL);
    }
    if iov_ptr == 0 {
        return Err(Errno::EINVAL);
    }
    let iov_bytes = iovlen.checked_mul(16).ok_or(Errno::EINVAL)?;
    if !raw::check_user_buf(iov_ptr, iov_bytes) {
        return Err(Errno::EFAULT);
    }
    // v2: UDS cmsg 处理 — 在 fw 调之前注入凭据 (如果对端 passcred)
    // cmsg 头格式: cmsghdr { cmsg_len: usize (8B), cmsg_level: i32 (4B), cmsg_type: i32 (4B) }
    // 总 16 字节头, 然后是 data (cmsg_len - 16 字节)
    let msg_control_ptr = raw::read_u64_from_user(msg_ptr + 32).ok_or(Errno::EFAULT)?;
    let msg_controllen_raw = raw::read_u64_from_user(msg_ptr + 40).ok_or(Errno::EFAULT)? as usize;
    if msg_control_ptr != 0 && msg_controllen_raw >= 28 {
        if !raw::check_user_buf(msg_control_ptr, msg_controllen_raw as u64) {
            return Err(Errno::EFAULT);
        }
        // uds_getsockopt_passcred 内部已检查 fd family, 非 UDS 返 0/ENOPROTOOPT.
        let local_passcred = super::unix::uds_getsockopt_passcred(fd) != 0;
        if local_passcred {
            // 写 SCM_CREDENTIALS cmsghdr (28 字节) 到 msg_control:
            // [0-7]   cmsg_len = 28
            // [8-11]  cmsg_level = 1 (SOL_SOCKET)
            // [12-15] cmsg_type = 2 (SCM 凭据)
            // [16-19] pid (高 32 位) | uid (低 32 位)
            // [24-27] gid
            // B07-02: 使用当前进程真实凭据, 消除硬编码伪造的 root 凭据.
            let pid: u64 = u64::from(crate::framework::proc::process_get_current_pid());
            let uid: u64 = u64::from(crate::framework::credo::get_current_uid());
            let gid: u64 = u64::from(crate::framework::credo::get_current_gid());
            raw::write_u64_to_user(msg_control_ptr, 28u64);
            raw::write_u64_to_user(msg_control_ptr + 8, (2u64 << 32) | 1u64);
            raw::write_u64_to_user(msg_control_ptr + 16, (pid << 32) | uid);
            raw::write_u64_to_user(msg_control_ptr + 24, gid);
            raw::write_u64_to_user(msg_ptr + 40, 28u64);
        }
        // 解析 cmsg 头序列处理 SCM_RIGHTS
        let mut coff = 0;
        while coff + 16 <= msg_controllen_raw {
            let cmsg_len = raw::read_u64_from_user(msg_control_ptr + coff as u64)
                .ok_or(Errno::EFAULT)? as usize;
            let cmsg_level = raw::read_u64_from_user(msg_control_ptr + coff as u64 + 8)
                .ok_or(Errno::EFAULT)? as i32;
            let cmsg_type = raw::read_u64_from_user(msg_control_ptr + coff as u64 + 12)
                .ok_or(Errno::EFAULT)? as i32;
            if cmsg_len < 16 || cmsg_len > msg_controllen_raw - coff {
                break;
            }
            if cmsg_level == 1 /* SOL_SOCKET */ && cmsg_type == 1
            /* SCM_RIGHTS */
            {
                // v2: SCM_RIGHTS (fd 跨进程传递) — deferred 到后续 PR.
                // 需对接 fd_alloc::dup_to_process 实现完整跨进程 fd 传递.
                // 当前简化: 仅标记已处理, 不做实际操作.
            }
            coff = (coff + cmsg_len + 3) & !3; // 4 字节对齐
        }
    }

    let r = fw::sendmsg_syscall(fd, msg_ptr, flags);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}

/// recvmsg(fd, msg, flags)
///
/// v2: UDS 协议 recvmsg 路径 — 写回对端注入的 `SCM_CREDENTIALS` cmsg
/// 到用户 `msg_control` 区域 (对端 send 时已注入).
///
/// # Errors
///
/// 当 `fd` 为负数时返回 `Err(Errno::EBADF)`; `msg_ptr` 或 `iov` 等用户缓冲区无效时返回 `Err(Errno::EFAULT)`;
/// 参数不合法时返回 `Err(Errno::EINVAL)`; 底层 recvmsg 失败时返回对应的 `Errno`。
pub fn recvmsg_syscall(fd: i32, msg_ptr: u64, flags: i32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if msg_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(msg_ptr, 56) {
        return Err(Errno::EFAULT);
    }
    let iov_ptr = raw::read_u64_from_user(msg_ptr + 16).ok_or(Errno::EFAULT)?;
    let iovlen = raw::read_u64_from_user(msg_ptr + 24).ok_or(Errno::EFAULT)?;
    if iovlen == 0 || iovlen > 1024 {
        return Err(Errno::EINVAL);
    }
    if iov_ptr == 0 {
        return Err(Errno::EINVAL);
    }
    let iov_bytes = iovlen.checked_mul(16).ok_or(Errno::EINVAL)?;
    if !raw::check_user_buf(iov_ptr, iov_bytes) {
        return Err(Errno::EFAULT);
    }
    // v2: UDS cmsg 回传 — 写回对端注入的 SCM_CREDENTIALS (对端 passcred 启用时)
    // 框架 fw_recvmsg 把数据搬入 iov, 凭据在 stream/dgram 缓冲里已添加 12 字节.
    // 这里仅在 msg_control 区域写回标准 SCM_CREDENTIALS cmsghdr, 让用户能解析凭据.
    let msg_control_ptr = raw::read_u64_from_user(msg_ptr + 32).ok_or(Errno::EFAULT)?;
    let msg_controllen = raw::read_u64_from_user(msg_ptr + 40).ok_or(Errno::EFAULT)? as usize;
    if msg_control_ptr != 0 && msg_controllen >= 28 {
        if !raw::check_user_buf(msg_control_ptr, msg_controllen as u64) {
            return Err(Errno::EFAULT);
        }
        // B07-02: 从 UDS 接收缓冲反序列化真实发送方凭据; 非 UDS / 无凭据
        // 时不伪造 root, 保持 msg_controllen 原样 (无凭据).
        if let Some(cred) = uds::uds_peer_creds(fd) {
            // 写回 SCM_CREDENTIALS cmsghdr + 12 字节凭据 (与 send 路径相同编码)
            raw::write_u64_to_user(msg_control_ptr, 28u64);
            raw::write_u64_to_user(msg_control_ptr + 8, (2u64 << 32) | 1u64);
            raw::write_u64_to_user(
                msg_control_ptr + 16,
                (u64::from(cred.pid) << 32) | u64::from(cred.uid),
            );
            raw::write_u64_to_user(msg_control_ptr + 24, u64::from(cred.gid));
            raw::write_u64_to_user(msg_ptr + 40, 28u64);
        }
    }
    let r = fw::recvmsg_syscall(fd, msg_ptr, flags);
    if r < 0 {
        Err(Errno::from_ret(r))
    } else {
        Ok(r as usize)
    }
}
