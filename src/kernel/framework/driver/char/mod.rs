//! 字符设备驱动子系统 (Character Device Driver Subsystem)
//!
//! ## 架构 (2026 §6.4 直接方案 B 后)
//!
//! x86_64 字符设备业务已下沉 services (services/driver/char: serial/vga 权威)。
//! framework 仅保留 aarch64 PL011 UART 机制 (boot 早期输出)。
//!
//! - pl011.rs — ARM PL011 UART (aarch64 机制)
//!
//! ## 历史
//!
//! - x86_64 serial.rs / vga.rs 已删除 (2026-09-12): 业务迁 services,
//!   framework 保留 IoPort/IoMem 机制; Chitin 注册由 services::driver::char::char_init 完成。

#[cfg(target_arch = "aarch64")]
pub mod pl011;

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化字符设备子系统 (AArch64: PL011 UART)
///
/// x86_64 的字符设备初始化由 services 层 `services::driver::char::char_init()`
/// 负责 (crate root lib.rs 编排调用)。
#[cfg(target_arch = "aarch64")]
pub fn char_init() {
    use crate::kernel::framework::chitin::ChitinOps;

    crate::kernel::framework::chitin::chitin_register_driver_with_ops(
        "pl011",
        crate::kernel::framework::chitin::ChitinProto::Char,
        Some(crate::kernel::framework::arch::uart::base()),
        None,
        alloc::boxed::Box::new(pl011::Pl011Driver::new()),
        ChitinOps::Char(&pl011::PL011_CHAR_OPS),
    );
}
