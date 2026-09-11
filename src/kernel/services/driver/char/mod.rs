#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! 字符设备驱动 — services 层 (Phase 2.1.5)
//!
//! 包含文本模式显示的 100% safe API,
//! 为内核早期控制台和字符设备提供统一接口。
//!
//! ## 模块结构
//!
//! - [vga] — VGA 文本模式 (0xB8000 MMIO + 0x3D4/0x3D5 PIO), 0 unsafe
//! - [serial] — 16550 UART 串口 (COM1-COM4 PIO), 0 unsafe
//!
//! ## 后续添加
//!
//! - `pl011.rs` — ARM PL011 UART (Phase 2.1.5 后续)
//!
//! 评估日期: 2026-06-04

pub mod serial;
pub mod vga;

/// 初始化字符设备子系统并注册到 Chitin (§6.4 直接方案 B: services 权威)
///
/// services 层权威实现: 将 VGA 文本控制台 + COM1 串口注册进 Chitin 设备注册表。
/// framework 侧已退位 (删除 driver/char/serial.rs + vga.rs 业务, 保留 IoPort/IoMem
/// 机制与 aarch64 pl011)。
///
/// SIMPLIFIED: 注册走 `chitin_register_driver` (无 CharOps 读写绑定)——Chitin char
/// 读写路径 (`chitin_char_write/read`) 当前无生产消费者 (休眠); 待 devfs char 读写
/// 接入时按 §6.2 补 framework 提供的 CharOps 安全桥 trait (unsafe 转换留 framework)。
#[cfg(target_arch = "x86_64")]
pub fn char_init() {
    use alloc::boxed::Box;
    use crate::kernel::framework::chitin::{ChitinProto, chitin_register_driver};
    use serial::{ComPort, SerialConfig, SerialPort};
    use vga::VgaConsole;

    if let Some(vga) = VgaConsole::new() {
        chitin_register_driver("vga", ChitinProto::Char, None, None, Box::new(vga));
    }
    if let Some(com1) = SerialPort::new(ComPort::Com1, SerialConfig::default_115200_8n1()) {
        chitin_register_driver(
            "serial0",
            ChitinProto::Char,
            Some(u64::from(serial::COM1_BASE)),
            Some(4),
            Box::new(com1),
        );
    }
}
