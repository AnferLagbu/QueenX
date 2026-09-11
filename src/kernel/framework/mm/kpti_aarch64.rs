//! KPTI (Kernel Page Table Isolation) — AArch64
//!
//! ARMv8-A Meltdown 缓解: TTBR0/TTBR1 双地址空间隔离.
//! trampoline 页表分配使用 PAGE_SIZE → usize 已知转换 (aarch64 usize = u64).
//!
//! # 设计
//!
//! ARMv8-A 硬件自动根据 VA 高位选择 TTBR0_EL1 (低半区, 用户) 或
//! TTBR1_EL1 (高半区, 内核), 无需软件切换. KPTI 的作用是:
//!
//! 在用户态 (EL0) 运行时, TTBR1_EL1 指向最小化的 trampoline 页表
//! (仅含异常入口代码), 减少内核地址空间泄露面. 异常入口时切换到
//! 完整内核页表, eret 返回 EL0 前切回 trampoline 页表.
//!
//! # 现状
//!
//! 框架搭建完成, 等待用户态进程支持后集成到异常入口:
//! - `handle_el0_sync` / `handle_el0_irq` 入口处切换 TTBR1
//! - `eret` 返回 EL0 前切换回 trampoline TTBR1

#![cfg(target_arch = "aarch64")]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::kernel::framework::mm::PAGE_SIZE;
use crate::kernel::framework::mm::pmm_alloc_page;

// ── 公共状态 ──────────────────────────────────────────────────────

/// KPTI 是否已初始化.
static KPTI_READY: AtomicBool = AtomicBool::new(false);

/// Trampoline TTBR1_EL1 物理地址 (最小化内核页表, EL0 运行时使用).
/// `#[unsafe(no_mangle)]` 供汇编直接 `adrp+ldr` 读取.
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
static TRAMP_TTBR1: AtomicU64 = AtomicU64::new(0);

/// 完整内核 TTBR1_EL1 物理地址 (异常入口时切换回).
/// `#[unsafe(no_mangle)]` 供汇编直接 `adrp+ldr` 读取.
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
static KERNEL_TTBR1: AtomicU64 = AtomicU64::new(0);

// ── 公开 API ──────────────────────────────────────────────────────

/// 返回 KPTI 是否已就绪.
#[inline(always)]
pub fn kpti_is_active() -> bool {
    KPTI_READY.load(Ordering::Acquire)
}

/// 返回 trampoline TTBR1 物理地址 (供异常入口汇编读取).
#[inline(always)]
pub fn kpti_trampoline_ttbr1() -> u64 {
    TRAMP_TTBR1.load(Ordering::Acquire)
}

/// 返回完整内核 TTBR1 物理地址.
#[inline(always)]
pub fn kpti_kernel_ttbr1() -> u64 {
    KERNEL_TTBR1.load(Ordering::Acquire)
}

/// 进入内核态: 切换 TTBR1_EL1 到完整内核页表.
///
/// 在 EL0 异常入口汇编中调用 (handle_el0_sync / handle_el0_irq).
///
/// # Safety
///
/// 调用方必须是异常入口 trampoline (EL0→EL1 切换的第一时间).
#[inline(never)]
pub unsafe fn kpti_enter_kernel() {
    let kernel_ttbr1 = KERNEL_TTBR1.load(Ordering::Acquire);
    if kernel_ttbr1 == 0 {
        return;
    }
    // SAFETY: 写入 TTBR1_EL1 是特权操作; kernel_ttbr1 来自 init 阶段可信来源.
    unsafe {
        core::arch::asm!(
            "dsb ish",
            "msr ttbr1_el1, {0}",
            "isb",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            in(reg) kernel_ttbr1,
        );
    }
}

/// 返回用户态: 切换 TTBR1_EL1 到 trampoline 页表.
///
/// 在 eret 返回 EL0 前调用.
///
/// # Safety
///
/// 调用方必须是异常出口 trampoline (即将 eret 返回 EL0).
#[inline(never)]
pub unsafe fn kpti_exit_to_user() {
    let tramp_ttbr1 = TRAMP_TTBR1.load(Ordering::Acquire);
    if tramp_ttbr1 == 0 {
        return;
    }
    // SAFETY: 写入 TTBR1_EL1 是特权操作; tramp_ttbr1 来自 init 阶段可信来源.
    unsafe {
        core::arch::asm!(
            "dsb ish",
            "msr ttbr1_el1, {0}",
            "isb",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            in(reg) tramp_ttbr1,
        );
    }
}

// ── 初始化 ────────────────────────────────────────────────────────

#[expect(
    clippy::missing_panics_doc,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
#[expect(
    clippy::manual_assert,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
/// 初始化 KPTI: 创建 trampoline TTBR1 页表.
///
/// trampoline 页表仅复制异常入口代码所需的 L0 条目,
/// 其余高半区条目置零, 减少内核地址泄露面.
///
/// # Safety
///
/// 调用方保证: KERNEL_TTBR1 已初始化; PMM 可分配页面;
/// KPTI 全局状态在 boot 阶段被独占写入.
pub unsafe fn kpti_init(kernel_ttbr1: u64) {
    if KPTI_READY.load(Ordering::Acquire) {
        return;
    }

    // 1. 分配 trampoline L0 页表
    let tramp_l0_phys = pmm_alloc_page() as u64;
    if tramp_l0_phys == 0 {
        // 不可恢复: KPTI 初始化需要 trampoline L0 页表, 分配失败意味着内存耗尽,
        // 内核无法安全进入用户态, 只能停机
        panic!("[KPTI] failed to allocate trampoline L0 page");
    }

    // 2. 清零
    // SAFETY: pmm 分配的页已对齐, 物理页属于内核.
    unsafe {
        core::ptr::write_bytes(tramp_l0_phys as *mut u8, 0, PAGE_SIZE as usize);
    }

    // 3. 复制内核 L0 中包含异常入口代码的条目
    //    当前策略: 复制所有高半区条目 (L0[256..511]).
    //    后续优化: 仅复制异常向量表所在 L1 条目, 其余置零.
    //
    // SAFETY: kernel_ttbr1 由 vmm_init 写入, tramp_l0 由 pmm 分配, 均有效.
    unsafe {
        let src = kernel_ttbr1 as *const u64;
        let dst = tramp_l0_phys as *mut u64;
        core::ptr::copy_nonoverlapping(src.add(256), dst.add(256), 256);
    }

    // 4. 公开状态
    TRAMP_TTBR1.store(tramp_l0_phys, Ordering::Release);
    KERNEL_TTBR1.store(kernel_ttbr1, Ordering::Release);
    KPTI_READY.store(true, Ordering::Release);
}

/// KPTI 关闭时的占位: 返回完整内核 TTBR1.
#[inline(always)]
pub fn kpti_trampoline_ttbr1_or_kernel(kernel_ttbr1: u64) -> u64 {
    let t = TRAMP_TTBR1.load(Ordering::Acquire);
    if t == 0 { kernel_ttbr1 } else { t }
}
