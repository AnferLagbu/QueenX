//! VFS 对外 API (syscall 边界)
//!
//! ## 调用方契约
//! - `syscall::sys_read/write/open/close` —— 用户态文件操作
//! - `proc::exec::load_elf` —— 加载 ELF 时通过 VFS 读文件
//! - `credo::storage` —— 持久化身份数据
//! - `host-tests` —— host 端单元测试
//!
//! ## 内部接口
//! - `vfs_*_internal` 是核心实现,**不对外**;`#[no_mangle]` 的 `vfs_*`
//!   函数将指针参数 (来自用户态/asm) 转为 `&str` 后委托给内部实现。
//!
//! ## 安全约束
//! - 所有 `*_internal` 函数在调用前已验证指针非空与字符串 UTF-8
//!   (委托给 [`CStrExt::as_kstr`])。
//! - `&[u8]` buffer 长度由调用方提供,实现按需截断,不会越界写。
//!
//! ## 性能特征
//! - 静态分发,无 vtable 开销
//! - 字符串路径解析纯栈上,无堆分配 (除路径 split 时 `alloc::string`)
//!
//! ## 模块拆分
//! - 挂载/生命周期/同步/格式化见 [`mount`] (本模块 `pub use mount::*`)
//! - 路径/目录/链接/元数据/cwd 见 [`path`] (本模块 `pub use path::*`)
//! - fd 句柄操作 (open/close/read/write/seek/...) 见 [`handle`] (本模块
//!   `pub use handle::*`)
use super::types::KernelResult;
use crate::framework::lib::CStrExt;
use crate::framework::mm::PAGE_SIZE;

/// B2: 4KB 对齐 read 时的 pcache 命中快路径上限 (16 页 = 64KB)
pub(crate) const PCACHE_FAST_MAX_BYTES: usize = 64 * 1024;
/// B2: 4KB 对齐 read 时的 pcache 命中快路径下限 (1 页 = 4KB)
// 有意窄化: 用户内存代理, 指针/长度上下文保证
#[expect(clippy::cast_possible_truncation)]
pub(crate) const PCACHE_FAST_MIN_BYTES: usize = PAGE_SIZE as usize;

// ============================================================================
// 对外契约: Vfs trait (用于 trait-object 注册 / host 端测试)
// ============================================================================
//
// 注: 此 trait 是 **声明性契约**,不替换现有 #[no_mangle] 函数。内部
// `vfs_*_internal` 仍是真实入口;`Vfs` trait 为未来 hot-swap / mock 测试
// 预留接口边界,impl 见 fs::ramfs/ramfs::RamFs 等。
pub trait Vfs: Send + Sync {
    fn name(&self) -> &'static str;
    /// 挂载文件系统到指定路径。
    /// # Errors
    /// 挂载失败时返回 Err。
    fn mount(&self, path: &str) -> KernelResult<()>;
    /// 卸载文件系统。
    /// # Errors
    /// 卸载失败时返回 Err。
    fn unmount(&self) -> KernelResult<()>;
}

/// 兼容旧 `ptr_to_str(ptr)` 调用语义:
/// - 空指针 → `""`
/// - 非 UTF-8 → `""`(降级)
/// - 超过 `VFS_MAX_PATH` 长度 → 截断到该上限
///
/// 委托给统一抽象 [`CStrExt::as_kstr`],行为完全一致。
///
/// 可见性: `pub(crate)` 供拆分后的兄弟子模块 `mount` / `path` / `handle` 复用。
pub(crate) fn ptr_to_str<'a>(ptr: *const u8) -> &'a str {
    ptr.as_kstr()
}

/// 拆分父路径与文件名, 供兄弟子模块 `mount` / `path` / `handle` 复用。
pub(crate) fn split_parent_name(rel_path: &str) -> (&str, &str) {
    rel_path.rfind('/').map_or(("/", rel_path), |pos| {
        if pos == 0 {
            ("/", &rel_path[1..])
        } else {
            (&rel_path[..pos], &rel_path[pos + 1..])
        }
    })
}

// ============================================================================
// 公共 VFS API
// ============================================================================

/// 将 Rust &str 转换为 null 终止的 C 字符串并调用 VFS 函数
///
/// # Safety
/// 本函数内部处理 unsafe 指针操作，调用方无需 unsafe。
pub fn with_cstr<F, R>(path: &str, f: F) -> R
where
    F: FnOnce(*const u8) -> R,
{
    let mut buf = alloc::vec::Vec::with_capacity(path.len() + 1);
    buf.extend_from_slice(path.as_bytes());
    buf.push(0);
    f(buf.as_ptr())
}

// ============================================================================
// 拆分后的子模块 re-export — 保持对外符号名与调用路径不变
// (`#[no_mangle]` 全局符号不受模块位置影响)
// ============================================================================
pub use super::mount::*;
pub use super::path::*;
pub use super::handle::*;
