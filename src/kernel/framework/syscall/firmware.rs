//! 设备固件加载 — 系统调用实现
//!
//! - `sys_fw_load(node_id, path_ptr, path_len, version)`  // 函数原型
//!   从用户态路径读取文件, 附着到 ChitinNode.firmware
//! - `sys_fw_get_info(node_id, info_ptr)` — 拷贝 `FirmwareInfo` 到用户态
//! - `sys_fw_get(node_id, buf_ptr, buf_len, offset)` — 按 offset 拷贝到用户态缓冲
//! - `sys_fw_detach(node_id)` — 移除固件
//!
//! ## 安全
//!
//! - 用户态指针经 `check_user_ptr` / `validate_user_buf` 校验后再拷贝
//! - 路径最长 4096 字节, 超过返回 -EINVAL
//! - 读取走 `services/fs::open` + `framework/io::vfs_read`

use crate::framework::chitin::{
    FW_ERR_IO, FW_ERR_NOT_FOUND, FW_ERR_TOO_LARGE, FirmwareInfo, MAX_FIRMWARE_SIZE,
    devtree_attach_firmware, devtree_detach_firmware, devtree_get_firmware, fnv1a_32,
};
use crate::framework::fs::{vfs_open, vfs_read};
use crate::framework::syscall::raw as raw_sync;
use alloc::vec::Vec;
use core::ptr;

const MAX_PATH_LEN: usize = 4096;
const MAX_FW_GET_SIZE: usize = 8 * 1024 * 1024;
const FW_BUF_SIZE: usize = 4096;

// POSIX errno (与 QX_* 错误语义一致)
const EFAULT: i64 = -14;
const EINVAL: i64 = -22;
const ENOENT: i64 = -2;

/// 从用户态拷贝字节切片
// SAFETY: ptr/len 经 `check_user_ptr` 校验, 用户态缓冲在 syscall 期间不会被释放
unsafe fn copy_user_bytes(ptr: u64, len: usize) -> Option<Vec<u8>> {
    if !raw_sync::check_user_ptr(ptr) || len == 0 {
        return None;
    }
    let mut buf = alloc::vec![0u8; len];
    // SAFETY: check_user_ptr 已校验 ptr/len 在用户空间, 用户态缓冲在 syscall 期间不会被释放
    unsafe {
        ptr::copy_nonoverlapping(ptr as *const u8, buf.as_mut_ptr(), len);
    }
    Some(buf)
}

/// 从用户态写入字节切片
// SAFETY: ptr 已由调用方/check_user_buf 校验为合法用户缓冲区; syscall 期间用户内存有效.
unsafe fn write_user_bytes(ptr: u64, data: &[u8]) -> bool {
    if data.is_empty() {
        return true;
    }
    if !raw_sync::check_user_buf(ptr, data.len() as u64) {
        return false;
    }
    // SAFETY: check_user_buf 已校验 ptr/len, syscall 期间用户态缓冲不会被释放
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
    }
    true
}

/// 打开文件并读取全部内容 (上限 `MAX_FIRMWARE_SIZE`)
///
/// 使用 `framework::fs::api::vfs_open` / `vfs_read`
fn read_path_data(path: &[u8]) -> Result<Vec<u8>, i64> {
    // 路径必须为 C 字符串 (NUL 结尾); 检查并补齐 NUL
    let mut path_z: Vec<u8> = path.to_vec();
    if path_z.last() != Some(&0) {
        path_z.push(0);
    }
    if core::str::from_utf8(&path_z[..path_z.len() - 1]).is_err() {
        return Err(EINVAL);
    }
    let pwm = 0u64; // 内核侧加载固件使用全权 PWM
    let fd = vfs_open(path_z.as_ptr(), 0 /* O_RDONLY */, pwm);
    if fd < 0 {
        return Err(ENOENT);
    }

    let mut out = Vec::new();
    let mut tmp = alloc::vec![0u8; FW_BUF_SIZE];
    loop {
        if out.len() + tmp.len() > MAX_FIRMWARE_SIZE {
            return Err(i64::from(FW_ERR_TOO_LARGE));
        }
        let n = vfs_read(fd as u32, tmp.as_mut_ptr(), tmp.len() as u32);
        if n < 0 {
            return Err(i64::from(FW_ERR_IO));
        }
        if n == 0 {
            break;
        }
        let n = n as usize;
        out.extend_from_slice(&tmp[..n]);
    }
    Ok(out)
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
/// `sys_fw_load`: 从用户态路径读取并附着固件到 node
///
/// `a0=node_id`, `a1=path_ptr`, `a2=path_len`, a3=version  // 寄存器约定
pub fn sys_fw_load(a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let node_id = a0 as u32;
    let path_len = a2 as usize;
    let version = a3 as u32;

    if path_len == 0 || path_len > MAX_PATH_LEN {
        return EINVAL;
    }

    // 拷贝路径
    // SAFETY: copy_user_bytes 自身 unsafe 封装已校验用户态指针
    let path_opt = unsafe { copy_user_bytes(a1, path_len) };
    let path_bytes = match path_opt {
        Some(b) => b,
        None => return EFAULT,
    };

    // 读取文件
    let data = match read_path_data(&path_bytes) {
        Ok(d) => d,
        Err(e) => return e,
    };

    if data.len() > MAX_FIRMWARE_SIZE {
        return i64::from(FW_ERR_TOO_LARGE);
    }

    // 计算 name hash (使用路径最后一个分量为 "name")
    let path_str = match core::str::from_utf8(&path_bytes) {
        Ok(s) => s,
        Err(_) => return EINVAL,
    };
    let path_str = path_str.trim_end_matches('\0');
    let name = path_str.rsplit('/').next().unwrap_or(path_str);
    let name_hash = fnv1a_32(name);

    if devtree_attach_firmware(node_id, data, name_hash, version) {
        0
    } else {
        i64::from(FW_ERR_NOT_FOUND)
    }
}

#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::borrow_as_ptr,
    reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
/// `sys_fw_get_info`: 将 `FirmwareInfo` 写入用户态 info 指针
///
/// `a0=node_id`, `a1=info_ptr`
pub fn sys_fw_get_info(a0: u64, a1: u64) -> i64 {
    let node_id = a0 as u32;
    if !raw_sync::check_user_buf(a1, core::mem::size_of::<FirmwareInfo>() as u64) {
        return EFAULT;
    }
    let blob = match devtree_get_firmware(node_id) {
        Some(b) => b,
        None => return i64::from(FW_ERR_NOT_FOUND),
    };
    let info = FirmwareInfo {
        size: blob.size() as u32,
        name_hash: blob.name_hash,
        version: blob.version,
        _reserved: 0,
    };
    // SAFETY: FirmwareInfo 是 POD 结构体, 从引用获取字节切片无副作用
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &info as *const _ as *const u8,
            core::mem::size_of::<FirmwareInfo>(),
        )
    };
    // SAFETY: write_user_bytes 自身 unsafe 调用已带 SAFETY 注释, 此处重新封装为表达式返回值
    let write_ok = unsafe { write_user_bytes(a1, bytes) };
    if write_ok { 0 } else { EFAULT }
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
/// `sys_fw_get`: 按 offset 拷贝固件到用户态 buf
///
/// `a0=node_id`, `a1=buf_ptr`, `a2=buf_len`, a3=offset  // 寄存器约定
///
/// 返回值: 实际拷贝字节数; 失败: 负 errno / `FW_ERR`_*
pub fn sys_fw_get(a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let node_id = a0 as u32;
    let buf_len = a2 as usize;
    let offset = a3 as usize;

    if buf_len > MAX_FW_GET_SIZE {
        return i64::from(FW_ERR_TOO_LARGE);
    }
    if buf_len == 0 {
        return 0;
    }
    if !raw_sync::check_user_buf(a1, buf_len as u64) {
        return EFAULT;
    }

    let blob = match devtree_get_firmware(node_id) {
        Some(b) => b,
        None => return i64::from(FW_ERR_NOT_FOUND),
    };

    if offset >= blob.size() {
        return 0;
    }
    let avail = core::cmp::min(buf_len, blob.size() - offset);
    let slice = &blob.data[offset..offset + avail];

    // SAFETY: write_user_bytes 自身 unsafe 调用已带 SAFETY 注释
    let write_ok = unsafe { write_user_bytes(a1, slice) };
    if write_ok { avail as i64 } else { EFAULT }
}

/// `sys_fw_detach`: 移除节点上的固件
///
/// `a0=node_id`
pub fn sys_fw_detach(a0: u64) -> i64 {
    let node_id = a0 as u32;
    if devtree_detach_firmware(node_id) {
        0
    } else {
        i64::from(FW_ERR_NOT_FOUND)
    }
}
