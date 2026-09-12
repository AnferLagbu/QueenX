//! 文件系统子系统
//!
//! ## 依赖声明
//!
//! framework 内部依赖: syscall, sync, proc, credo, driver
//! services 依赖: `services::fs` (安全代理)

pub mod devfs;
pub mod hvfs;
pub mod initramfs;
pub mod ramfs;
pub mod vfs;
pub mod vfs_poll_trait;

pub use vfs::*;

// DECOUPL-4: 顶层 re-export initramfs unpack 入口, 避免 framework 内部 3+ 层深度访问
pub use initramfs::unpack;
