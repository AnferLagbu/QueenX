//! PL011 UART 字符设备驱动 (ARM PrimeCell UART)
//!
//! QEMU virt 机器默认使用 PL011 @ 0x09000000。
//! 寄存器定义基于 ARM DDI 0183G。
//!
//! 此驱动将 PL011 UART 注册为 Chitin 字符设备，
//! 提供统一的设备枚举和 I/O 接口。
//!
//! ## 架构
//!
//! ```text
//! Chitin 框架 (chitin_char_read / chitin_char_write)
//!   └── CharOps { read, write }
//!         └── Pl011Driver::read_byte / write_byte
//!               └── arch::uart (底层 MMIO 硬件访问)
//! ```
//!
//! ## 设计约束
//!
//! 本驱动是**单例驱动**：整个系统仅支持一个 PL011 实例，绑定到全局
//! [PL011_BASE](uart::PL011_BASE) (0x09000000)。所有 I/O 操作委托给
//! [arch::uart] 模块，该模块内部使用同一硬编码基地址。
//!
//! 注意：PL011 UART 在启动阶段由 [boot::entry](super::super::boot) 通过
//! [arch::uart::init] 提前初始化以支持早期控制台输出。
//! 本驱动的 `init()` 会检测此状态，避免重复初始化。

use crate::framework::arch::uart;
use crate::framework::chitin::CharOps;
use crate::framework::driver::{DeviceType, Driver, DriverResult};

/// PL011 UART 字符设备驱动 (单例)
///
/// 作为 [Chitin](crate::framework::chitin) 字符设备的底层驱动，
/// 提供完整的生命周期管理（init/shutdown）和 I/O 能力（read/write）。
///
/// 所有硬件访问委托给 [arch::uart] 模块，该模块硬编码使用
/// [PL011_BASE](uart::PL011_BASE) (0x09000000)。
pub struct Pl011Driver {
    initialized: bool,
}

impl Pl011Driver {
    /// 创建 PL011 驱动实例 (单例)
    ///
    /// 驱动绑定到全局 [PL011_BASE](uart::PL011_BASE) (0x09000000)。
    pub fn new() -> Self {
        Self { initialized: false }
    }

    #[expect(
        clippy::unused_self,
        reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
    )]
    /// 读取单个字节 (阻塞)
    ///
    /// 委托给底层 [uart::getc]。
    fn read_byte(&self) -> u8 {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe { uart::getc() }
    }

    #[expect(
        clippy::unused_self,
        reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
    )]
    /// 写入单个字节 (阻塞)
    ///
    /// 委托给底层 [uart::putc]。
    fn write_byte(&self, c: u8) {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            uart::putc(c);
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
    )]
    /// 检查 UART 硬件是否已启用
    fn is_hw_enabled(&self) -> bool {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let cr = core::ptr::read_volatile((uart::base() + uart::UARTCR) as *const u32);
            cr & uart::UARTCR_UARTEN != 0
        }
    }
}

impl Driver for Pl011Driver {
    fn name(&self) -> &'static str {
        "PL011 UART"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn init(&mut self) -> DriverResult<()> {
        if self.is_hw_enabled() {
            self.initialized = true;
            return Ok(());
        }

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            uart::init();
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> DriverResult<()> {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_volatile((uart::base() + uart::UARTCR) as *mut u32, 0);
        }
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn reset(&mut self) -> DriverResult<()> {
        self.shutdown()?;
        self.init()
    }

    fn status(&self) -> &'static str {
        if self.initialized {
            "PL011 ready @ 0x09000000 (115200-8N1)"
        } else {
            "PL011 not initialized"
        }
    }
}

// ============================================================================
// CharOps 回调 (C ABI 兼容)
// ============================================================================

// SAFETY: 调用方 (CharOps::read) 保证 driver_data 有效, buf 至少 buf.len() 字节。
//         unsafe 逻辑封装在 (self.read)(...) 内。
extern "C" fn pl011_read(driver_data: *mut u8, buf: *mut u8, len: usize) -> usize {
    // SAFETY: driver_data 由 CharOps 契约保证指向有效 Pl011Driver；
    // buf 是调用方提供的 len 字节可写缓冲区。
    let drv = unsafe { &mut *(driver_data as *mut Pl011Driver) };
    let mut count = 0;
    for i in 0..len {
        // SAFETY: i < len 保证写入合法；buf 由调用方契约。
        unsafe {
            let byte = drv.read_byte();
            core::ptr::write_volatile(buf.add(i), byte);
        }
        count += 1;
    }
    count
}

// SAFETY: 调用方 (CharOps::write) 保证 driver_data 有效, buf 至少 len 字节。
extern "C" fn pl011_write(driver_data: *mut u8, buf: *const u8, len: usize) -> usize {
    // SAFETY: driver_data 由 CharOps 契约保证指向有效 Pl011Driver；
    // buf 是调用方提供的 len 字节只读缓冲区。
    let drv = unsafe { &*(driver_data as *const Pl011Driver) };
    for i in 0..len {
        // SAFETY: i < len 保证读合法；buf 由调用方契约。
        unsafe {
            let byte = core::ptr::read_volatile(buf.add(i));
            drv.write_byte(byte);
        }
    }
    len
}

/// PL011 CharOps 静态实例
pub static PL011_CHAR_OPS: CharOps = CharOps {
    read: pl011_read,
    write: pl011_write,
    ioctl: None,
};
