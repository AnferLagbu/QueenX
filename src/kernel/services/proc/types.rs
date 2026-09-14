#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! 进程类型定义 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 原定义（T6-2 于 2026-06-16 自 framework 迁入）按统一判据"机制持有的
//! 数据结构/常量归 framework"迁回 `framework/proc/types.rs` — 进程核心
//! 类型被 framework user_proc/process/scheduler 消费。framework/proc 的
//! types 为 `pub mod`, 直接 glob re-export, 保持 services 侧 API 兼容
//! (services→framework 合法方向)。

pub use crate::framework::proc::types::*;
