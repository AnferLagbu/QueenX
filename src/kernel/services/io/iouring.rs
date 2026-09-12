#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! `io_uring` 异步 I/O 框架 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-12)
//!
//! 原策略代码按统一判据"机制持有的数据结构/常量归 framework"迁回
//! `framework/io/iouring.rs` (`sys_io_uring_*` 被 framework syscall dispatch
//! 机制直接调用, 依赖闭包全在 framework 内)。framework/io 的 iouring 为公开
//! 子模块 (`pub mod iouring`), 可直接 glob re-export, 保持 services 侧 API
//! 兼容 (services→framework 合法方向)。

pub use crate::kernel::framework::io::iouring::*;
