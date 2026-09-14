//! 文件锁 (flock + POSIX record locks) — services re-export 兼容壳
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 完整实现 (728 行, 0 unsafe) 已按"framework VFS 机制内联消费
//! (vfs/path.rs inode 释放路径调用 `posix_lock_release_inode`) → 锁表属
//! VFS 机制状态 → 机制归 framework"判据迁回 `framework::fs::vfs::flock`。
//! 本文件仅 glob re-export 保持 `services::fs::flock::*` 调用方路径不变
//! (services→framework 合法方向)。

pub use crate::framework::fs::vfs::flock::*;
