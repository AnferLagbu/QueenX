#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! xHCI 环形缓冲区 — services 层安全代理 (Phase 2.1.6)
//!
//! 重导出 framework Command Ring + Event Ring 类型.
//! framework `ring.rs` 本身 0 unsafe, 此处作为 services 层统一入口。
//!
//! 评估日期: 2026-07-22

// 重导出 framework ring 全部公共类型/函数
pub use crate::framework::driver::usb::ring::*;
