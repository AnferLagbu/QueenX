#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! ConfigSummary 启动期编码 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-12)
//!
//! 原策略代码按统一判据"机制持有的数据结构/常量归 framework"迁回
//! `framework/config/boot_image.rs` (`encode_boot_image` 被 framework config
//! `init()` 机制直接调用, 依赖闭包全在 framework 内)。framework/config 的
//! boot_image 为公开子模块 (`pub mod boot_image`), 可直接 glob re-export,
//! 保持 services 侧 API 兼容 (services→framework 合法方向)。

pub use crate::kernel::framework::config::boot_image::*;
