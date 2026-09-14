//! 输入设备驱动子系统 (Input Device Driver Subsystem)
//!
//! 提供输入设备支持：
//! - **键盘**: PS/2键盘驱动
//! - **鼠标**: PS/2鼠标驱动 (未来)
//! - **游戏手柄**: 游戏控制器 (未来)
//!
//! ## 架构
//!
//! ```text
//! Input Device Subsystem
//! ├── keyboard.rs  # 键盘驱动
//! ├── mouse.rs    # 鼠标驱动 (未来)
//! └── joystick.rs # 游戏手柄 (未来)
//! ```

#[cfg(target_arch = "x86_64")]
pub mod keyboard;

#[cfg(target_arch = "x86_64")]
pub use keyboard::KeyboardDriver;

#[cfg(target_arch = "x86_64")]
pub fn input_init() {
    keyboard::keyboard_init();
    crate::framework::chitin::chitin_register_driver(
        "ps2_keyboard",
        crate::framework::chitin::ChitinProto::Input,
        None,
        None,
        alloc::boxed::Box::new(keyboard::KeyboardDriver::new()),
    );
}

/// AArch64 输入初始化 (暂无 PS/2 设备)。
#[cfg(not(target_arch = "x86_64"))]
pub fn input_init() {}
