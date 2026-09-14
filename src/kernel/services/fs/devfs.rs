#![deny(unsafe_code)]
//! 设备文件系统 (`DevFS`) — services re-export 兼容壳
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 完整实现 (863 行, 0 unsafe) 已按"framework VFS 挂载机制内联消费
//! (vfs/mount.rs 直接引用 `DEVFS_DATA`/`DevfsData`) → 设备表属 VFS 机制
//! 状态 → 机制归 framework"判据迁回 `framework::fs::devfs`。
//! 本文件仅 glob re-export 保持 `services::fs::devfs::*` 调用方路径不变
//! (services→framework 合法方向)。

pub use crate::framework::fs::devfs::*;
