//! CPU 层面架构抽象封装
//!
//! `Arch` trait 方法的薄封装, 用于 CPU 级操作.
//! 所有对 `Arch` trait 的调用集中在此文件，方便未来多架构移植.
//!
//! ## Phase 1 状态
//! - [x] `cpu_id()` — 获取当前 CPU ID
//! - [x] `timestamp()` — 高精度时间戳
//!
//! ## 设计原则
//! - 零开销: 所有调用通过 `arch!()` 宏展开为静态分发
//! - 零依赖: 不引入额外的 trait 或抽象层
//! - 可替换: Phase 2/3 只需更新 Arch impl，此处无需改动

use crate::framework::arch::Arch;
use crate::framework::config::MAX_CPUS;

/// 获取当前 CPU ID (APIC ID / `MPIDR_EL1`)。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub fn cpu_id() -> u32 {
    <crate::framework::arch::CurrentArch as Arch>::cpu_id()
}

/// 获取高精度时间戳 (rdtsc / `CNTVCT_EL0`)。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub fn timestamp() -> u64 {
    <crate::framework::arch::CurrentArch as Arch>::timestamp()
}

/// CPU 暂停直到中断 (hlt / wfi)。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub fn halt() {
    <crate::framework::arch::CurrentArch as Arch>::halt();
}

/// 发送核间中断到目标 CPU。
///
/// # B03-20 修复
/// 添加 `target_cpu < MAX_CPUS` 越界校验。越界 IPI 会发送到不存在的 CPU,
/// 静默丢失 (无错误返回)。校验后越界直接返回, 调用方需检查返回值或
/// 在传入前确保索引有效。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub fn send_ipi(target_cpu: u32, vector: u8) {
    if target_cpu as usize >= MAX_CPUS {
        // 越界: 静默丢弃 (与原行为一致, 但显式校验避免 silently 失败)
        return;
    }
    <crate::framework::arch::CurrentArch as Arch>::send_ipi(target_cpu, vector);
}

/// 广播核间中断到所有 CPU。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub fn broadcast_ipi(vector: u8) {
    <crate::framework::arch::CurrentArch as Arch>::broadcast_ipi(vector);
}

/// 设置当前 CPU 的内核栈指针。
///
/// `x86_64`: 写入 TSS 的 RSP0 字段 (ring 0 栈)。
/// aarch64: 无操作 — `SP_EL1` 由上下文切换直接管理。
#[inline(always)]
#[expect(
    clippy::used_underscore_binding,
    reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
)]
pub fn set_kernel_stack(_stack: u64) {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        crate::framework::arch::tss::tss_set_kernel_stack(_stack);
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = _stack;
    }
}
