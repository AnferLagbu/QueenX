//! VFS 公共类型 — services 层 re-export 壳
//!
//! ## B09-12/DECISION-H13 P1-B3 迁移记录 (2026-08-31)
//!
//! VFS 类型定义 (常量/枚举/结构体/FileSystem trait/OpenFile) 按"机制归
//! framework"原则迁回 `framework::fs::vfs::types` (完整实现). 本文件仅
//! re-export 保持调用方兼容 (services→framework 单向依赖).

#![deny(unsafe_code)]

pub use crate::framework::fs::vfs::types::*;
