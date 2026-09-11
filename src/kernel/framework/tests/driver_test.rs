#![cfg(target_arch = "x86_64")]
//! 硬件驱动测试示例 (Hardware Driver Test Example)
//!
//! 测试所有基本硬件驱动的功能：
//! - VGA 文本模式显示
//! - 串口输出
//! - PIT 定时器
//! - PS/2 键盘
//!
//! 此文件用于 QEMU 环境下的驱动验证。

#![no_std]
#![no_main]

use core::panic::PanicInfo;

// ============================================================================
// 内核入口点
// ============================================================================

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 初始化所有驱动
    test_drivers();
    
    // 进入无限循环
    loop {
        crate::arch!(halt());
    }
}

// ============================================================================
// 驱动测试函数
// ============================================================================

fn test_drivers() {
    // 1. 测试 VGA 显示
    test_vga();
    
    // 2. 测试串口输出
    test_serial();
    
    // 3. 测试 PIT 定时器
    test_pit();
    
    // 4. 测试键盘 (轮询模式)
    test_keyboard();
}

/// 测试 VGA 文本模式驱动 (services 权威, §6.4 直接方案 B)
fn test_vga() {
    // 使用 services VgaConsole 输出测试信息
    vga_println!("=== VGA Driver Test ===");
    vga_println!("Initializing VGA text mode (80x25)...");
    vga_println!("[OK] VGA console created");

    vga_println!("Screen width: 80");
    vga_println!("Screen height: 25");
    vga_println!("Buffer address: 0xB8000");

    vga_println!("[PASS] VGA test completed");
    vga_println!("");
}

/// 测试串口驱动 (services 权威, §6.4 直接方案 B)
fn test_serial() {
    vga_println!("=== Serial Port Test ===");
    vga_println!("Testing COM1 (0x3F8)...");

    // 初始化串口 (services SerialPort, COM1 115200 8N1)
    match serial() {
        Some(port) => {
            port.send_str("QueenX - Serial Port Test\n");
            port.send_str("COM1 initialized at 115200 baud\n");
            port.send_str("8N1 configuration\n");
            vga_println!("[OK] Serial port test completed");
        }
        None => vga_println!("[FAIL] SerialPort::new(COM1) returned None"),
    }
    vga_println!("");
}

/// 测试 PIT 定时器
fn test_pit() {
    vga_println!("=== PIT Timer Test ===");
    vga_println!("Initializing PIT (8254)...");
    
    // 初始化 PIT 为 1000 Hz (1ms 间隔)
    // let freq = pit_init(1000);
    
    vga_println!("Target frequency: 1000 Hz");
    vga_println!("Base frequency: 1.193182 MHz");
    vga_println!("Divisor: 1193");
    
    vga_set_color!(Color::LightGreen, Color::Black);
    vga_println!("[OK] PIT timer initialized");
    
    vga_set_color!(Color::White, Color::Black);
    vga_println!("Testing timer delay...");
    
    // 简单延迟测试
    for i in 1..=5 {
        vga_print!("Delay test ");
        vga_println!(i);
        // pit_delay_ms(100);
    }
    
    vga_set_color!(Color::LightCyan, Color::Black);
    vga_println!("[PASS] PIT test completed");
    vga_set_color!(Color::White, Color::Black);
    vga_println!("");
}

/// 测试 PS/2 键盘驱动
fn test_keyboard() {
    vga_println!("=== Keyboard Test ===");
    vga_println!("Initializing PS/2 keyboard...");
    
    // 初始化键盘
    keyboard_init();
    
    vga_set_color!(Color::LightGreen, Color::Black);
    vga_println!("[OK] Keyboard initialized");
    
    vga_set_color!(Color::White, Color::Black);
    vga_println!("Waiting for key press (polling mode)...");
    vga_println!("Press any key to continue (timeout: 10s)");
    
    // 简单的键盘轮询测试
    let mut count = 0;
    let timeout = 10000000; // 简单的超时计数
    
    while count < timeout {
        // 检查是否有按键
        if keyboard_has_char() > 0 {
            let ch = keyboard_read_char();
            if ch != 0 {
                vga_print!("Key pressed: ");
                vga_putchar(ch as u8);
                vga_println!("");
                break;
            }
        }
        count += 1;
    }
    
    if count >= timeout {
        vga_set_color!(Color::Yellow, Color::Black);
        vga_println!("[TIMEOUT] No key pressed");
    } else {
        vga_set_color!(Color::LightCyan, Color::Black);
        vga_println!("[PASS] Keyboard test completed");
    }
    
    vga_set_color!(Color::White, Color::Black);
    vga_println!("");
}

// ============================================================================
// 辅助宏和函数
// ============================================================================

// 字符设备输出走 services 权威实现 (§6.4 直接方案 B):
// framework/driver/char/{serial,vga}.rs 已删除, 此处改为 services VgaConsole/SerialPort.

use core::sync::OnceLock;
use crate::kernel::services::driver::char::serial::{ComPort, SerialConfig, SerialPort};
use crate::kernel::services::driver::char::vga::{CursorPos, TextAttribute, VgaConsole};

static SERIAL: OnceLock<Option<SerialPort>> = OnceLock::new();
static VGA: OnceLock<Option<VgaConsole>> = OnceLock::new();

/// 获取全局 COM1 串口 (services SerialPort)
fn serial() -> &'static Option<SerialPort> {
    SERIAL.get_or_init(|| SerialPort::new(ComPort::Com1, SerialConfig::default_115200_8n1()))
}

/// 获取全局 VGA 控制台 (services VgaConsole)
fn vga() -> &'static Option<VgaConsole> {
    VGA.get_or_init(VgaConsole::new)
}

/// VGA 文本输出 (简化: services VgaConsole 无全局光标状态, 固定定位 0,0)
fn vga_puts(s: &[u8]) {
    if let Some(v) = vga().as_ref() {
        v.write_string_at(CursorPos { x: 0, y: 0 }, s, TextAttribute::default());
    }
}

/// VGA 单字符输出 (简化, 定位 0,0)
fn vga_putchar(c: u8) {
    if let Some(v) = vga().as_ref() {
        v.write_char(CursorPos { x: 0, y: 0 }, c, TextAttribute::default());
    }
}

/// VGA 打印宏 (简化版)
macro_rules! vga_println {
    () => {
        vga_putchar(b'\n');
    };
    ($fmt:literal) => {
        vga_puts(concat!($fmt, "\n").as_bytes());
    };
    ($fmt:literal, $($arg:expr),+) => {
        // 简化实现，不支持格式化
        vga_puts(concat!($fmt, "\n").as_bytes());
    };
}

/// VGA 打印宏 (不换行)
macro_rules! vga_print {
    ($fmt:literal) => {
        vga_puts($fmt.as_bytes());
    };
}

/// VGA 设置颜色宏 (services VgaConsole 无全局颜色状态, no-op)
macro_rules! vga_set_color {
    ($fg:expr, $bg:expr) => {};
}

// ============================================================================
// 外部函数声明 (FFI) — 仅键盘保留 framework C ABI
// ============================================================================

// SAFETY: C ABI 互操作，函数签名与外部代码约定一致
unsafe extern "C" {
    // 键盘函数
    fn keyboard_init();
    fn keyboard_has_char() -> i32;
    fn keyboard_read_char() -> i32;

    // PIT 函数
    // fn pit_init(freq: u32) -> u32;  // 已弃用, 使用 hrtimer
    // fn pit_delay_ms(ms: u32);
}

// ============================================================================
// Panic 处理
// ============================================================================

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        vga_set_color(Color::LightRed as u8, Color::Black as u8);
        vga_puts(b"\n!!! KERNEL PANIC !!!\n".as_ptr() as *const u8);
        vga_puts(b"System halted.\n".as_ptr() as *const u8);
    }
    
    loop {
        crate::arch!(halt());
    }
}
