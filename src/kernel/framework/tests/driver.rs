#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::driver::Driver;
#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::driver::keyboard::{
    KB_LED_CAPS_LOCK, KB_LED_NUM_LOCK, KeyboardBuffer, KeyboardDriver, ModifierState,
    SCANCODE_TABLE, SHIFT_TABLE, SpecialKey, get_special_key,
};
#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::driver::{
    ATA_PRIMARY_CTRL, ATA_PRIMARY_IO, ATA_SECONDARY_CTRL, ATA_SECONDARY_IO, AtaController,
    AtaDevice, MAX_ATA_DEVICES, WORDS_PER_SECTOR, get_ctrl_base, get_io_base,
};
use crate::kernel::framework::driver::{DeviceInfo, DeviceType, DriverError, DriverResult};
use crate::kernel::framework::tests::{TestResult, assert_eq_test, check, runner};
use crate::register_tests_inner;

fn driver_error_codes() -> TestResult {
    assert_eq_test!(
        alloc::format!("{}", DriverError::InvalidParameter),
        "Invalid parameter",
        "InvalidParameter"
    );
    assert_eq_test!(
        alloc::format!("{}", DriverError::Timeout),
        "Operation timeout",
        "Timeout"
    );
    check!(
        DriverError::Busy != DriverError::NotInitialized,
        "Busy != NotInitialized"
    );
    TestResult::Pass
}

fn driver_device_types() -> TestResult {
    assert_eq_test!(alloc::format!("{}", DeviceType::Block), "Block", "Block");
    assert_eq_test!(alloc::format!("{}", DeviceType::Char), "Char", "Char");
    assert_eq_test!(
        alloc::format!("{}", DeviceType::Network),
        "Network",
        "Network"
    );
    TestResult::Pass
}

fn driver_device_info_creation() -> TestResult {
    let info = DeviceInfo::new("test_device", DeviceType::Other);
    check!(info.id > 0, "id should be positive");
    assert_eq_test!(info.name, "test_device", "name");
    check!(!info.initialized, "not initialized");
    check!(info.io_base.is_none(), "no io_base");
    check!(info.irq.is_none(), "no irq");
    TestResult::Pass
}

fn driver_device_info_builder() -> TestResult {
    let info = DeviceInfo::new("serial0", DeviceType::Char)
        .with_io_base(0x3F8)
        .with_irq(4);
    assert_eq_test!(info.io_base, Some(0x3F8), "io_base");
    assert_eq_test!(info.irq, Some(4), "irq");
    TestResult::Pass
}

fn driver_result_type() -> TestResult {
    // J-01 (2026-09-08): returns_ok 专测 Result 的 Ok 路径语义 (is_ok/unwrap),
    // 恒 Ok 返回触发 unnecessary_wraps — 测试辅助语义, 加 expect 保留
    #[expect(
        clippy::unnecessary_wraps,
        reason = "unnecessary_wraps: 测试辅助 fn 专测 DriverResult Ok 路径语义 (is_ok/unwrap); 当前优先 expect"
    )]
    fn returns_ok() -> DriverResult<u32> {
        Ok(42)
    }
    fn returns_err() -> DriverResult<u32> {
        Err(DriverError::DeviceNotFound)
    }
    check!(returns_ok().is_ok(), "ok is ok");
    check!(returns_err().is_err(), "err is err");
    assert_eq_test!(returns_ok().unwrap(), 42, "ok value");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn keyboard_scancode_table() -> TestResult {
    assert_eq_test!(SCANCODE_TABLE[0x02], b'1', "scancode 0x02");
    assert_eq_test!(SCANCODE_TABLE[0x03], b'2', "scancode 0x03");
    assert_eq_test!(SCANCODE_TABLE[0x1E], b'a', "scancode 0x1E");
    assert_eq_test!(SCANCODE_TABLE[0x30], b'b', "scancode 0x30");
    assert_eq_test!(SCANCODE_TABLE[0x39], b' ', "scancode 0x39");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn keyboard_shift_table() -> TestResult {
    assert_eq_test!(SHIFT_TABLE[0x02], b'!', "shift 0x02");
    assert_eq_test!(SHIFT_TABLE[0x03], b'@', "shift 0x03");
    assert_eq_test!(SHIFT_TABLE[0x1E], b'A', "shift 0x1E");
    assert_eq_test!(SHIFT_TABLE[0x30], b'B', "shift 0x30");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn keyboard_special_keys() -> TestResult {
    assert_eq_test!(get_special_key(0x0D), SpecialKey::Enter, "Enter");
    assert_eq_test!(get_special_key(0x0E), SpecialKey::Backspace, "Backspace");
    assert_eq_test!(get_special_key(0x48), SpecialKey::ArrowUp, "ArrowUp");
    assert_eq_test!(get_special_key(0x4B), SpecialKey::ArrowLeft, "ArrowLeft");
    assert_eq_test!(get_special_key(0x57), SpecialKey::F11, "F11");
    assert_eq_test!(get_special_key(0xFF), SpecialKey::None, "invalid scancode");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn keyboard_modifier_default() -> TestResult {
    let mods = ModifierState::default();
    check!(!mods.shift_pressed(), "no shift");
    check!(!mods.ctrl_pressed(), "no ctrl");
    check!(!mods.alt_pressed(), "no alt");
    check!(!mods.caps_lock, "no caps");
    check!(mods.num_lock, "num lock on");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn keyboard_modifier_operations() -> TestResult {
    let mut mods = ModifierState {
        left_shift: true,
        ..Default::default()
    };
    check!(mods.shift_pressed(), "left shift");
    mods.right_shift = true;
    check!(mods.shift_pressed(), "both shift");
    mods.left_shift = false;
    check!(mods.shift_pressed(), "right shift still");
    mods.caps_lock = true;
    check!(mods.caps_lock, "caps lock");
    let led = mods.to_led_byte();
    check!(led & KB_LED_CAPS_LOCK != 0, "caps LED");
    check!(led & KB_LED_NUM_LOCK != 0, "num LED");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn keyboard_buffer() -> TestResult {
    let mut buf = KeyboardBuffer::default();
    check!(buf.is_empty(), "empty initially");
    assert_eq_test!(buf.len(), 0, "len 0");
    check!(buf.push(b'A').is_ok(), "push A");
    check!(buf.push(b'B').is_ok(), "push B");
    check!(!buf.is_empty(), "not empty");
    assert_eq_test!(buf.len(), 2, "len 2");
    assert_eq_test!(buf.pop(), Some(b'A'), "pop A");
    assert_eq_test!(buf.pop(), Some(b'B'), "pop B");
    check!(buf.is_empty(), "empty after pop");
    assert_eq_test!(buf.pop(), None, "pop empty");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn keyboard_driver_trait() -> TestResult {
    let mut driver = KeyboardDriver::new();
    assert_eq_test!(driver.name(), "PS/2 Keyboard", "name");
    assert_eq_test!(driver.device_type(), DeviceType::Input, "type");
    check!(!driver.is_ready(), "not ready");
    let _ = driver.init();
    check!(!driver.status().is_empty(), "status non-empty");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn ata_constants() -> TestResult {
    assert_eq_test!(ATA_PRIMARY_IO, 0x1F0, "primary IO");
    assert_eq_test!(ATA_SECONDARY_IO, 0x170, "secondary IO");
    assert_eq_test!(WORDS_PER_SECTOR, 256, "words per sector");
    assert_eq_test!(MAX_ATA_DEVICES, 4, "max devices");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn ata_device_default() -> TestResult {
    let device = AtaDevice::default();
    check!(!device.present, "not present");
    check!(device.is_master, "is master");
    assert_eq_test!(device.channel, 0, "channel 0");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn ata_controller_creation() -> TestResult {
    let controller = AtaController::new();
    check!(!controller.primary_present, "no primary");
    check!(!controller.secondary_present, "no secondary");
    assert_eq_test!(controller.detected_device_count(), 0, "0 devices");
    check!(!controller.is_ready(), "not ready");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn ata_io_base_calculation() -> TestResult {
    assert_eq_test!(get_io_base(0), ATA_PRIMARY_IO, "ch0 IO");
    assert_eq_test!(get_io_base(1), ATA_PRIMARY_IO, "ch1 IO");
    assert_eq_test!(get_io_base(2), ATA_SECONDARY_IO, "ch2 IO");
    assert_eq_test!(get_io_base(3), ATA_SECONDARY_IO, "ch3 IO");
    assert_eq_test!(get_ctrl_base(0), ATA_PRIMARY_CTRL, "ch0 ctrl");
    assert_eq_test!(get_ctrl_base(3), ATA_SECONDARY_CTRL, "ch3 ctrl");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn ata_disk_present_bounds() -> TestResult {
    let controller = AtaController::new();
    check!(!controller.disk_present(0), "disk 0 not present");
    check!(!controller.disk_present(3), "disk 3 not present");
    check!(!controller.disk_present(4), "disk 4 out of range");
    check!(!controller.disk_present(255), "disk 255 out of range");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
pub fn register_ata_tests() {
    let r = runner();
    register_tests_inner! { r:
        "driver::ata": {
            "constants": ata_constants,
            "device_default": ata_device_default,
            "controller_creation": ata_controller_creation,
            "io_base_calculation": ata_io_base_calculation,
            "disk_present_bounds": ata_disk_present_bounds,
        },
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn register_ata_tests() {}

#[cfg(target_arch = "x86_64")]
pub fn register_keyboard_serial_tests() {
    let r = runner();
    register_tests_inner! { r:
        "driver::keyboard": {
            "scancode_table": keyboard_scancode_table,
            "shift_table": keyboard_shift_table,
            "special_keys": keyboard_special_keys,
            "modifier_default": keyboard_modifier_default,
            "modifier_operations": keyboard_modifier_operations,
            "buffer": keyboard_buffer,
            "driver_trait": keyboard_driver_trait,
        },
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn register_keyboard_serial_tests() {}

pub fn register_tests() {
    let r = runner();
    register_tests_inner! { r:
        "driver::framework": {
            "error_codes": driver_error_codes,
            "device_types": driver_device_types,
            "device_info_creation": driver_device_info_creation,
            "device_info_builder": driver_device_info_builder,
            "result_type": driver_result_type,
        },
    }
    register_keyboard_serial_tests();
    register_ata_tests();
}
