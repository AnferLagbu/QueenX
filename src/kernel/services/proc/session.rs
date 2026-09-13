#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! 会话 / 进程组 / 控制终端 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 原定义（T1-6 于 2026-06-16 自 framework 迁入）按统一判据迁回
//! `framework/proc/session.rs` — 会话/进程组是进程关系机制状态, 被
//! framework syscall dispatch 消费。glob re-export 保持 services 侧 API
//! 兼容。

pub use crate::kernel::framework::proc::session::*;
