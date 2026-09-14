#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! USB 核心框架 — services 层安全代理 (Phase 2.1.6)
//!
//! 重导出 framework USB 核心类型 (Descriptors, URB, `HostController` Trait, `UsbDevice`).
//! framework `usb_core.rs` 本身 0 unsafe, 此处作为 services 层统一入口。
//!
//! ## 导出类型
//!
//! - `UsbSpeed` / `DeviceState` / `DeviceClass` — 枚举常量
//! - `DeviceDescriptor` / `ConfigurationDescriptor` / `InterfaceDescriptor` / `EndpointDescriptor` — 描述符
//! - `UsbSetupPacket` / `StandardRequest` — USB 请求
//! - `Urb` / `UrbStatus` — USB 请求块
//! - `HostController` — 主机控制器 Trait
//! - `UsbDevice` — 设备实例
//!
//! 评估日期: 2026-07-22

// 重导出 framework usb_core 全部公共类型 (framework usb_core.rs = 0 unsafe)
pub use crate::framework::driver::usb::usb_core::*;
