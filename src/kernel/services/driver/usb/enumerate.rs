#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! USB 设备枚举 — services 层安全代理 (Phase 2.1.6)
//!
//! 重导出 framework 枚举类型 (描述符解析 + 设备枚举流程).
//! framework `enumerate.rs` 本身 0 unsafe, 此处作为 services 层统一入口。
//!
//! 评估日期: 2026-07-22

// 重导出 framework enumerate 全部公共函数/类型
pub use crate::framework::driver::usb::enumerate::*;
