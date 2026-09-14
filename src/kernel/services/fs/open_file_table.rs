//! 全局 OpenFile 表 — services 层 re-export 壳
//!
//! ## B09-12/DECISION-H13 P1-B5 迁移记录 (2026-08-31)
//!
//! OpenFileTable 是 VFS 打开文件表机制, 按"机制归 framework"原则迁回
//! `framework::fs::vfs::open_file_table` (完整实现). 本文件仅 re-export
//! 保持调用方兼容 (services→framework 单向依赖).

#![deny(unsafe_code)]

pub use crate::framework::fs::vfs::open_file_table::*;
