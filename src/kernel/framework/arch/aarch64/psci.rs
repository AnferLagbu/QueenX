//! PSCI (Power State Coordination Interface) — 电源状态协调接口
//!
//! ARM 电源管理标准接口，通过 SMC/HVC 调用实现关机/重启。
//! QEMU virt 机器使用 PSCI v0.2+。
//! PSCI_VERSION 返回 (major<<16 | minor<<8 | patch), 取高位/低位都是 u32 已知安全.

/// PSCI 函数 ID (SMC64)
const PSCI_SYSTEM_OFF: u32 = 0x84000008;
const PSCI_SYSTEM_RESET: u32 = 0x84000009;
const PSCI_VERSION: u32 = 0x84000000;

/// 调用 PSCI (SMC calling convention)
///
/// x0 = function_id
/// 返回: x0 = return value
#[inline(always)]
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
#[expect(
    clippy::cast_lossless,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
unsafe fn smc(func: u32) -> i64 {
    unsafe {
        let ret: i64;
        core::arch::asm!(
            "smc #0",
            in("x0") func as u64,
            lateout("x0") ret,
            options(nostack),
        );
        ret
    }
}

#[expect(
    clippy::cast_lossless,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
/// 检查 PSCI 版本。返回 (major, minor) 或 None。
fn psci_version() -> Option<(u32, u32)> {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let ver = unsafe { smc(PSCI_VERSION) } as u64;
    if ver == u32::MAX as u64 {
        // PSCI not available
        None
    } else {
        Some(((ver >> 16) as u32, (ver & 0xFFFF) as u32))
    }
}

/// PSCI 关机 — 不会返回
pub fn system_off() -> ! {
    // 尝试 PSCI
    match psci_version() {
        Some((_major, _minor)) => {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe { smc(PSCI_SYSTEM_OFF) };
        }
        None => {}
    }

    // PSCI 不可用时，触发异常 (通过写入零地址)
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::arch::asm!("mov x0, #0; str x0, [x0]", options(nostack));
    }
    loop {}
}

/// PSCI 重启 — 不会返回
pub fn system_reset() -> ! {
    match psci_version() {
        Some((_major, _minor)) => {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe { smc(PSCI_SYSTEM_RESET) };
        }
        None => {}
    }

    // PSCI 不可用时，fallback
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::arch::asm!("mov x0, #0; str x0, [x0]", options(nostack));
    }
    loop {}
}
