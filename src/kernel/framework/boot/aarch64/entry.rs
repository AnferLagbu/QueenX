//! AArch64 启动入口 (Rust 侧)
//!
//! 从 start.S 跳转后的第一个 Rust 函数。负责:
//!   1. BSS 清零 + 栈 canary 写入
//!   2. MMU 初始化 (identity mapping + TTBR1, 必须在 UART 之前)
//!   3. PL011 UART 初始化 (使用 TTBR1 高半区地址 0xFFFF_0000_0900_0000)
//!   4. 异常向量表设置
//!   5. GICv3 初始化 (使用 TTBR1 高半区地址)
//!   6. Timer 初始化
//!   7. 跳转 kernel_init()

use crate::framework::arch::uart;

// ============================================================================
// 启动入口
// ============================================================================

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
/// AArch64 启动入口。
///
/// # Safety
///
/// 仅由汇编 `start.S` 在 EL1 启动阶段调用，调用前需确保：
/// - 栈指针 (SP) 已设置
/// - BSS 段可写
/// - 运行在 EL1（内核特权级）
pub unsafe extern "C" fn entry() -> ! {
    unsafe {
        // 0. 启用 FP/SIMD (编译器会生成 NEON 指令如 movi v0.2d)
        //    CPACR_EL1.FPEN[21:20] = 0b11 → 不 trap FP/SIMD
        core::arch::asm!("mrs x0, cpacr_el1", "orr x0, x0, #(0x3 << 20)", "msr cpacr_el1, x0", out("x0") _);

        // 1. BSS 清零
        clear_bss();

        // 1.1 写入 boot 栈 canary 到 stack_bottom (栈溢出检测)
        // 与 x86_64 boot.asm trampoline64_high 对齐
        // 必须在 clear_bss 之后, 否则 canary 会被清零覆盖
        crate::framework::proc::write_boot_stack_canary();

        // 2. 初始化 MMU (identity mapping + TTBR1)
        //    必须在 UART 之前, 因为 UART 使用 TTBR1 高半区地址 (0xFFFF_0000_0900_0000),
        //    在 MMU 启用前该地址为无效物理地址, 会导致立即崩溃.
        crate::framework::arch::mmu::init();

        // 3. 初始化 UART (使用 TTBR1 高半区地址, 依赖 MMU)
        uart::init();
        uart::puts("[BOOT] QueenX starting...");

        // 3.1 验证 canary (UART 已可用, 异常向量表尚未设置, 崩了就是真崩)
        let canary_ok = crate::framework::proc::check_boot_stack_canary();
        if !canary_ok {
            uart::puts("[BOOT] FATAL: canary lost between write and kernel_init!");
            loop {}
        }

        // 4. 初始化异常向量表
        uart::puts("[BOOT] Setting up exception vectors...");
        crate::framework::arch::exception::init();

        // 5. 初始化 GICv3 (使用 TTBR1 高半区地址, 依赖 MMU)
        uart::puts("[BOOT] Initializing GICv3...");
        crate::framework::arch::gic::init();

        // 6. 初始化定时器 (仅配置, 不启用 — 稍后在 kernel_init 中启用)
        uart::puts("[BOOT] Initializing timer...");
        let (_freq, interval) = crate::framework::arch::timer::init_deferred();
        crate::framework::arch::exception::TIMER_INTERVAL_TICKS
            .store(interval, core::sync::atomic::Ordering::Relaxed);

        // 7. 跳转统一内核入口
        uart::puts("[BOOT] Booting kernel...");
        crate::kernel_init();

        // 不应该到达这里
        loop {
            crate::arch!(halt());
        }
    }
}

// ============================================================================
// BSS 清零
// ============================================================================

// SAFETY: C ABI 互操作，函数签名与外部代码约定一致
unsafe extern "C" {
    static mut __bss_start: u8;
    static _kernel_end: u8;
}

// SAFETY: `clear_bss` 是有效的 C ABI 函数指针; 参数列表与声明一致
#[expect(
    clippy::borrow_as_ptr,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
unsafe fn clear_bss() {
    unsafe {
        let bss_start = &mut __bss_start as *mut u8;
        let bss_end = &_kernel_end as *const u8 as usize;

        if bss_start as usize >= bss_end {
            return;
        }

        let size = bss_end - bss_start as usize;
        core::ptr::write_bytes(bss_start, 0, size);
    }
}
