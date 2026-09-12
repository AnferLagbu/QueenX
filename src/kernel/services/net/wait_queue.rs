#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! Socket WaitQueue 基础设施 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-12)
//!
//! 原策略代码按统一判据"机制持有的数据结构/常量归 framework"迁回
//! `framework/net/wait_queue.rs` (`SOCKET_WAIT_QUEUES` 全局表被 framework
//! net/init `poll_network` 机制直接消费)。framework/net 的 wait_queue 为公开
//! 子模块 (`pub mod wait_queue`), 可直接 glob re-export, 保持 services 侧
//! API 兼容 (services→framework 合法方向)。

pub use crate::kernel::framework::net::wait_queue::*;
