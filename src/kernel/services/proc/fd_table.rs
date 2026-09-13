#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! Per-process 文件描述符表 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 原定义（P1-I-01/D8 提取）按统一判据"机制持有的数据结构/常量归
//! framework"迁回 `framework/proc/fd_table.rs` — FdTable 是 Process
//! 机制字段。glob re-export 保持 services 侧 API 兼容。

pub use crate::kernel::framework::proc::fd_table::*;
