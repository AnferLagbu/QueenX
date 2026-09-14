//! VFS 路径 / 目录 / 链接 / 元数据 / cwd 操作 — 从 `api.rs` 拆出的物理子模块
//!
//! 归属: 路径相关 `#[no_mangle] pub extern "C"` 函数 (mkdir/rmdir/unlink/
//! link/symlink/readlink/rename/chmod/chown/utimensat/stat/cwd) 及其 safe
//! 包装. `api.rs` 通过 `pub use path::*;` 保持对外符号名与调用路径不变
//! (`#[no_mangle]` 全局符号不受模块位置影响).

use super::api::{ptr_to_str, split_parent_name, with_cstr};
use super::types::VfsStat;
use super::vfs::VFS_MANAGER;
use crate::framework::userptr::{UserRefMut, UserWritePtr};

// ============================================================================
// VFS 核心接口 (内部)
// ============================================================================

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_unlink_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // 在删除前获取 inode 号, 用于删除后释放 POSIX 锁
    let ino_before = fs_opt.and_then(|fs| fs.fs_resolve_path(rel_path));

    let result = fs_opt.map_or(-1, |fs| match fs.fs_unlink(rel_path, pwm) {
        Ok(()) => 0,
        Err(_) => -1,
    });

    // 文件删除成功后, 释放该 inode 上的 POSIX 锁 + inotify 通知
    if result == 0 {
        if let Some(ino) = ino_before {
            crate::framework::fs::vfs::flock::posix_lock_release_inode(ino);
            let (parent_path, name) = split_parent_name(rel_path);
            let parent_ino = fs_opt.map_or(0, |fs| fs.fs_resolve_path(parent_path).unwrap_or(0));
            super::inotify::inotify_notify(parent_ino, super::inotify::IN_DELETE, name, false);
            super::inotify::inotify_notify(ino, super::inotify::IN_DELETE_SELF, "", false);
        }
    }

    result
}

// ============================================================================
// link / symlink / readlink — 见 services/fs/link.rs, 在 ramfs/nestfs
// 真正实现 link/symlink 前, 暂时由 dispatch 直接返回 ENOSYS.
// 保留 framework API 的需求: 一旦 ramfs/nestfs 支持, services 不变, 仅
// 调整 framework 实现即可. 当前未保留 stub, 避免假实现.
// ============================================================================

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_mkdir_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let (parent_path, name) = split_parent_name(rel_path);
    if name.is_empty() {
        return -1;
    }

    // E6-4: trait object 分发
    fs_opt.map_or(-1, |fs| match fs.fs_mkdir(rel_path, pwm) {
        Ok(()) => {
            let parent_ino = fs.fs_resolve_path(parent_path).unwrap_or(0);
            super::inotify::inotify_notify(parent_ino, super::inotify::IN_CREATE, name, true);
            0
        }
        Err(_) => -1,
    })
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_rmdir_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发
    fs_opt.map_or(-1, |fs| match fs.fs_rmdir(rel_path, pwm) {
        Ok(()) => 0,
        Err(_) => -1,
    })
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
#[expect(
    clippy::no_effect_underscore_binding,
    reason = "no_effect_underscore_binding: let _ = expr 用于类型推导/副作用; 当前优先 expect"
)]
pub extern "C" fn vfs_stat_internal(path: *const u8, st: *mut VfsStat, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let _pwm = pwm;
    if st.is_null() {
        return -1;
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let mut st_ref = unsafe { UserRefMut::new(st) };

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发
    let result = fs_opt.map_or(-1, |fs| {
        fs.fs_stat(rel_path, pwm).map_or(-1, |stat| {
            *st_ref.as_mut() = stat;
            0
        })
    });

    if result == 0 {
        let tbl = crate::framework::credo::identity::get_table();
        let r = st_ref.as_mut();
        r.uid = tbl.uid_of(r.owner_pwm);
        r.gid = tbl.gid_of(r.group_pwm);
        if r.gid == 0xFFFF_FFFF {
            r.gid = r.uid;
        }
    }

    result
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn vfs_set_cwd_internal(path: *const u8) {
    let path = ptr_to_str(path);
    VFS_MANAGER.set_cwd(path);
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
// 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_get_cwd_internal(buf: *mut u8, size: u32) -> i32 {
    if buf.is_null() || size == 0 {
        return -1;
    }
    let cwd = VFS_MANAGER.get_cwd();
    let bytes = cwd.as_bytes();
    let len = bytes.len().min((size - 1) as usize);
    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    let mut user_buf = unsafe { UserWritePtr::new(buf as *mut u8, size as usize) };
    let slice = user_buf.as_mut_slice();
    slice[..len].copy_from_slice(&bytes[..len]);
    slice[len] = 0;
    len as i32
}

// ============================================================================
// 公共 VFS API (safe 包装 + no_mangle 转发)
// ============================================================================

/// Safe 包装: `vfs_mkdir` (接受 &str 路径)
pub fn vfs_mkdir_safe(path: &str, pwm: u64) -> i32 {
    with_cstr(path, |ptr| vfs_mkdir_internal(ptr, pwm))
}

/// Safe 包装: `vfs_unlink` (接受 &str 路径)
pub fn vfs_unlink_safe(path: &str, pwm: u64) -> i32 {
    with_cstr(path, |ptr| vfs_unlink_internal(ptr, pwm))
}

/// Safe 包装: `vfs_rmdir` (接受 &str 路径)
pub fn vfs_rmdir_safe(path: &str, pwm: u64) -> i32 {
    with_cstr(path, |ptr| vfs_rmdir_internal(ptr, pwm))
}

/// Safe 包装: `vfs_symlink` (接受 &str 路径)
pub fn vfs_symlink_safe(target: &str, linkpath: &str, pwm: u64) -> i32 {
    with_cstr(target, |t| with_cstr(linkpath, |l| vfs_symlink(t, l, pwm)))
}

/// Safe 包装: `vfs_link` (接受 &str 路径)
pub fn vfs_link_safe(oldpath: &str, newpath: &str, pwm: u64) -> i32 {
    with_cstr(oldpath, |o| with_cstr(newpath, |n| vfs_link(o, n, pwm)))
}

/// Safe 包装: `vfs_rename` (接受 &str 路径)
pub fn vfs_rename_safe(old: &str, new: &str, pwm: u64) -> i32 {
    with_cstr(old, |o| with_cstr(new, |n| vfs_rename(o, n, pwm)))
}

/// Safe 包装: `vfs_readlink` (接受 &str 路径)
pub fn vfs_readlink_safe(path: &str, buf: &mut [u8], pwm: u64) -> i32 {
    with_cstr(path, |ptr| {
        vfs_readlink(ptr, buf.as_mut_ptr(), buf.len() as u64, pwm)
    })
}

/// Safe 包装: `vfs_utimensat` (接受 &str 路径)
pub fn vfs_utimensat_safe(path: &str, atime: u64, mtime: u64, pwm: u64) -> i32 {
    with_cstr(path, |ptr| vfs_utimensat(ptr, atime, mtime, pwm))
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn vfs_stat(path: *const u8, st: *mut VfsStat, pwm: u64) -> i32 {
    vfs_stat_internal(path, st, pwm)
}

#[expect(
    clippy::borrow_as_ptr,
    reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
)]
/// Safe 包装: services 层用, 返回 `VfsStat` 而非 raw pointer.
///
/// 内部复用 `vfs_stat_internal`, 在 stack 上接收结果, 然后转为 Option 返回.
/// 服务层拿到 `Option<VfsStat>` 后可安全地用 `write_struct_to_user` 写回 user.
pub fn vfs_stat_safe(path: *const u8, pwm: u64) -> Option<VfsStat> {
    if path.is_null() {
        return None;
    }
    let mut st = VfsStat::default();
    let r = vfs_stat_internal(path, &mut st as *mut VfsStat, pwm);
    if r < 0 { None } else { Some(st) }
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn vfs_mkdir(path: *const u8, pwm: u64) -> i32 {
    vfs_mkdir_internal(path, pwm)
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_chmod(path: *const u8, mode: u16, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发
    fs_opt.map_or(-1, |fs| match fs.fs_chmod(rel_path, mode, pwm) {
        Ok(()) => 0,
        Err(_) => -1,
    })
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn vfs_chown(path: *const u8, owner_pwm: u64, pwm: u64) -> i32 {
    vfs_chown_ext(path, owner_pwm, 0, pwm)
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_chown_ext(path: *const u8, owner_pwm: u64, group_pwm: u64, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发
    fs_opt.map_or(-1, |fs| {
        match fs.fs_chown(rel_path, owner_pwm, group_pwm, pwm) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// 设置文件时间戳 (utimensat)
///
/// - `path`: 文件路径
/// - `atime`: 访问时间 (纳秒), `u64::MAX` 表示不修改
/// - `mtime`: 修改时间 (纳秒), `u64::MAX` 表示不修改
/// - `pwm`: 权限凭证
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_utimensat(path: *const u8, atime: u64, mtime: u64, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    fs_opt.map_or(-1, |fs| {
        match fs.fs_utimensat(rel_path, atime, mtime, pwm) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn vfs_unlink(path: *const u8, pwm: u64) -> i32 {
    vfs_unlink_internal(path, pwm)
}

/// link(oldpath, newpath) — 创建硬链接.
/// E6-5: 通过 trait object 分发
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_link(oldpath: *const u8, newpath: *const u8, pwm: u64) -> i32 {
    let old_path = ptr_to_str(oldpath);
    let new_path = ptr_to_str(newpath);
    if old_path.is_empty() || new_path.is_empty() {
        return -22;
    }
    let pwm_eff = pwm;

    let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(old_path) {
        Some(r) => r,
        None => return -2,
    };
    fs_opt.map_or(-1, |fs| match fs.fs_link(old_path, new_path, pwm_eff) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    })
}

/// symlink(target, linkpath) — 创建符号链接.
/// E6-5: 通过 trait object 分发
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_symlink(target: *const u8, linkpath: *const u8, pwm: u64) -> i32 {
    let tgt = ptr_to_str(target);
    let link_path = ptr_to_str(linkpath);
    if tgt.is_empty() || link_path.is_empty() || tgt.len() >= 128 {
        return -22;
    }
    let pwm_eff = pwm;

    let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(link_path) {
        Some(r) => r,
        None => return -2,
    };
    fs_opt.map_or(-1, |fs| match fs.fs_symlink(tgt, link_path, pwm_eff) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    })
}

/// readlink(path, buf, bufsiz) — 读取符号链接目标.
/// E6-5: 通过 trait object 分发
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
// 有意窄化: 用户内存代理, 指针/长度上下文保证
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_readlink(path: *const u8, buf: *mut u8, bufsiz: u64, pwm: u64) -> i32 {
    let _ = pwm;
    let p = ptr_to_str(path);
    if p.is_empty() {
        return -22;
    }
    if buf.is_null() || bufsiz == 0 {
        return -22;
    }

    let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(p) {
        Some(r) => r,
        None => return -2,
    };
    fs_opt.map_or(-1, |fs| {
        // SAFETY: buf 经调用方校验, bufsiz 字节可写.
        let slice = unsafe { core::slice::from_raw_parts_mut(buf, bufsiz as usize) };
        match fs.fs_readlink(p, slice) {
            Ok(n) => n as i32,
            Err(e) => e.as_i32(),
        }
    })
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_rename(old: *const u8, new: *const u8, pwm: u64) -> i32 {
    let old_path = ptr_to_str(old);
    let new_path = ptr_to_str(new);

    let (old_mount_idx, _old_fs_type, old_fs_opt) = match VFS_MANAGER.resolve_mount_fs(old_path) {
        Some(r) => r,
        None => return -1,
    };
    let (new_mount_idx, _new_fs_type, _) = match VFS_MANAGER.resolve_mount_fs(new_path) {
        Some(r) => r,
        None => return -1,
    };

    // rename 跨卷不支持 (简化)
    if old_mount_idx != new_mount_idx {
        return -22;
    }

    let old_rel = VFS_MANAGER.get_relative_path(old_path, old_mount_idx);
    let new_rel = VFS_MANAGER.get_relative_path(new_path, new_mount_idx);

    // E6-4: trait object 分发
    old_fs_opt.map_or(-1, |fs| match fs.fs_rename(old_rel, new_rel, pwm) {
        Ok(()) => 0,
        Err(_) => -1,
    })
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn vfs_rmdir(path: *const u8, pwm: u64) -> i32 {
    vfs_rmdir_internal(path, pwm)
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn vfs_get_cwd(buf: *mut u8, size: u32) -> i32 {
    vfs_get_cwd_internal(buf, size)
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn vfs_set_cwd(path: *const u8) {
    vfs_set_cwd_internal(path);
}

// ============================================================================
// 扩展属性 (xattr) — framework 层
// ============================================================================

/// 设置扩展属性
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_setxattr_internal(
    path: *const u8,
    name: *const u8,
    value: *const u8,
    size: u32,
    pwm: u64,
) -> i32 {
    let path = ptr_to_str(path);
    let name = ptr_to_str(name);
    let value = if !value.is_null() && size > 0 {
        // SAFETY: 调用方保证 value 指向有效的 size 字节缓冲区
        unsafe { core::slice::from_raw_parts(value, size as usize) }
    } else {
        &[]
    };

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -2, // ENOENT
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    fs_opt.map_or(-38, |fs| match fs.fs_setxattr(rel_path, name, value, pwm) {
        Ok(()) => 0,
        Err(_) => -1,
    })
}

/// 获取扩展属性
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
// 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_getxattr_internal(
    path: *const u8,
    name: *const u8,
    value: *mut u8,
    size: u32,
    pwm: u64,
) -> i32 {
    let path = ptr_to_str(path);
    let name = ptr_to_str(name);

    if value.is_null() || size == 0 {
        return -1;
    }

    // SAFETY: 调用方保证 value 指向有效的 size 字节缓冲区
    let buf = unsafe { core::slice::from_raw_parts_mut(value, size as usize) };

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -2, // ENOENT
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    fs_opt.map_or(-38, |fs| {
        fs.fs_getxattr(rel_path, name, buf, pwm)
            .map_or(-1, |len| len as i32)
    })
}

/// 列出扩展属性
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
// 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_listxattr_internal(
    path: *const u8,
    list: *mut u8,
    size: u32,
    pwm: u64,
) -> i32 {
    let path = ptr_to_str(path);

    if list.is_null() || size == 0 {
        return -1;
    }

    // SAFETY: 调用方保证 list 指向有效的 size 字节缓冲区
    let buf = unsafe { core::slice::from_raw_parts_mut(list, size as usize) };

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -2, // ENOENT
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    fs_opt.map_or(-38, |fs| {
        fs.fs_listxattr(rel_path, buf, pwm)
            .map_or(-1, |len| len as i32)
    })
}

/// 删除扩展属性
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub extern "C" fn vfs_removexattr_internal(path: *const u8, name: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let name = ptr_to_str(name);

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -2, // ENOENT
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    fs_opt.map_or(-38, |fs| match fs.fs_removexattr(rel_path, name, pwm) {
        Ok(()) => 0,
        Err(_) => -1,
    })
}

// ============================================================================
// 快照 (snapshot) — framework 层
// ============================================================================

/// 从原始指针获取快照名称字符串
///
/// # Safety
/// 调用方必须保证 `ptr` 指向有效的以 null 结尾的 UTF-8 字符串。
pub fn snapshot_get_name(ptr: u64) -> alloc::string::String {
    if ptr == 0 {
        return alloc::string::String::new();
    }
    let s = ptr_to_str(ptr as *const u8);
    alloc::string::String::from(s)
}
