//! 目录项缓存 (dcache) + inode 缓存 (icache) — services 层 re-export 壳
//!
//! ## B09-12/DECISION-H13 P1-B1 迁移记录 (2026-08-31)
//!
//! dcache/icache 是 VFS 缓存机制, 按"机制归 framework"原则迁回
//! `framework::fs::vfs::dcache` (完整实现). 本文件仅 re-export 保持
//! 调用方兼容 (services→framework 单向依赖).

#![deny(unsafe_code)]

pub use crate::framework::fs::vfs::dcache::*;
