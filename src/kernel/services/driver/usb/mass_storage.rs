#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! USB 大容量存储类驱动 — services 层安全代理 (Phase 2.1.6)
//!
//! 重导出 framework Mass Storage 类型 (CBW/CSW, SCSI 命令构造, `MassStorageDriver`).
//! framework `mass_storage.rs` 本身 0 unsafe, 此处作为 services 层统一入口。
//!
//! 评估日期: 2026-07-22

// 重导出 framework mass_storage 全部公共类型/函数
pub use crate::framework::driver::usb::mass_storage::*;
