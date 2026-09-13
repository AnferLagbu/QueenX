#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! CFS (Completely Fair Scheduler) 调度策略 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 原定义（T1-1 于 2026-06-16 自 framework 迁入）按统一判据迁回
//! `framework/proc/cfs.rs` — CfsRunQueue/DlRunQueue 是 scheduler 机制
//! 运行队列状态。glob re-export 保持 services 侧 API 兼容。

pub use crate::kernel::framework::proc::cfs::*;
