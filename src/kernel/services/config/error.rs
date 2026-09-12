#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! 配置校验结果类型 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-12)
//!
//! `ConfigError` 为 `ConfigValidateHook` trait 返回类型 (DECISION-K 项 2), 被
//! framework config 机制直接消费, 迁回 `framework/config/error.rs`。
//! framework/config 的 error 子模块为私有, 故经其顶层 re-export
//! (`framework::config::ConfigError`) 显式转发, 保持 services 侧 API 兼容
//! (services→framework 合法方向)。

pub use crate::kernel::framework::config::ConfigError;
