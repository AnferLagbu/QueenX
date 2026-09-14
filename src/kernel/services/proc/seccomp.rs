#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! Seccomp — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 原定义（T1-5 于 2026-06-16 自 framework 迁入）按统一判据迁回
//! `framework/proc/seccomp.rs` — SeccompState 挂在 Process 机制状态,
//! seccomp_check 被 framework syscall 分发消费。glob re-export 保持
//! services 侧 API 兼容。

pub use crate::framework::proc::seccomp::*;
