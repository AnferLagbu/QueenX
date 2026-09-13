//! inotify — 文件系统事件通知 — services re-export 兼容壳
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 完整实现 (596 行) 已按"framework VFS 机制内联消费 (vfs/path.rs 与
//! vfs/handle.rs 文件操作路径直接调用 `inotify_notify`) → 事件队列属 VFS
//! 机制状态 → 机制归 framework"判据迁回 `framework::fs::vfs::inotify`
//! (含 `sys_inotify_read` 用户缓冲区写入)。
//! 本文件仅 glob re-export 保持 `services::fs::inotify::*` 调用方路径不变
//! (services→framework 合法方向)。

pub use crate::kernel::framework::fs::vfs::inotify::*;
