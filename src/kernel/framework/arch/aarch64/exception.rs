//! AArch64 异常向量表 (Exception Vector Table)
//!
//! ARMv8-A 异常级别 EL1 (内核) 异常向量表。
//! 每个异常类型有 4 个入口: 同异常级别使用 SP_EL0/SP_ELx, 不同异常级别使用 SP_EL0/SP_ELx。
//! 本文件 cast 多为异常返回码/中断号转换, 硬件已知安全.
//!
//! 向量表布局 (VBAR_EL1):
//! | 偏移 | 类型 | 级别 | 说明 |
//! |------|------|------|------|
//! | +0x000 | Synchronous | EL1t | current EL, SP_EL0 |
//! | +0x080 | IRQ | EL1t | — |
//! | +0x100 | FIQ | EL1t | — |
//! | +0x180 | SError | EL1t | — |
//! | +0x200 | Synchronous | EL1h | current EL, SP_ELx |
//! | +0x280 | IRQ | EL1h | — |
//! | +0x300 | FIQ | EL1h | — |
//! | +0x380 | SError | EL1h | — |
//! | +0x400 | Synchronous | EL0 in AArch64 | — |
//! | +0x480 | IRQ | EL0 in AArch64 | — |
//! | +0x500 | FIQ | EL0 in AArch64 | — |
//! | +0x580 | SError | EL0 in AArch64 | — |
//! | +0x600 | Synchronous | EL0 in AArch32 | — |
//! | +0x680 | IRQ | EL0 in AArch32 | — |
//! | +0x700 | FIQ | EL0 in AArch32 | — |
//! | +0x780 | SError | EL0 in AArch32 | — |

use core::arch::global_asm;
use core::sync::atomic::{AtomicU64, Ordering};

/// 定时器中断间隔 (ticks), 在 boot 时由 timer::init() 设置
pub static TIMER_INTERVAL_TICKS: AtomicU64 = AtomicU64::new(0);

// ============================================================================
// 异常向量表 (global_asm)
// ============================================================================

global_asm!(
    r#"
// ============================================================
// AArch64 异常向量表 (VBAR_EL1, 每个入口 32 条指令 = 128 bytes)
// ARM 要求向量表按 2KB (2048 bytes) 对齐
//
// 关键设计: 每个 128-byte 槽位只放一条 b 指令跳转到外部 handler,
// 确保所有 16 个入口精确处于 VBAR + idx*128 偏移处, 不会溢出错位。
// ============================================================
.section .vectors, "ax"
.balign 2048
.global exception_vector_table
exception_vector_table:

// -------- EL1t: 当前 EL, 使用 SP_EL0 --------
.balign 128
    b   unexpected_exception       // curr_el_sp0_sync
.balign 128
    b   unexpected_exception       // curr_el_sp0_irq
.balign 128
    b   unexpected_exception       // curr_el_sp0_fiq
.balign 128
    b   unexpected_exception       // curr_el_sp0_serror

// -------- EL1h: current EL with SP_ELx (标准内核路径) --------
.balign 128
    b   handle_el1h_sync           // curr_el_spx_sync
.balign 128
    b   handle_el1h_irq            // curr_el_spx_irq
.balign 128
    b   unexpected_exception       // curr_el_spx_fiq
.balign 128
    b   unexpected_exception       // curr_el_spx_serror

// -------- EL0 in AArch64 --------
.balign 128
    b   handle_el0_sync            // lower_el_aarch64_sync
.balign 128
    b   handle_el0_irq             // lower_el_aarch64_irq
.balign 128
    b   unexpected_exception       // lower_el_aarch64_fiq
.balign 128
    b   unexpected_exception       // lower_el_aarch64_serror

// -------- EL0 in AArch32 (未使用) --------
.balign 128
    b   unexpected_exception       // lower_el_aarch32_sync
.balign 128
    b   unexpected_exception       // lower_el_aarch32_irq
.balign 128
    b   unexpected_exception       // lower_el_aarch32_fiq
.balign 128
    b   unexpected_exception       // lower_el_aarch32_serror

// ============================================================
// Handler code (位于向量表外部, 不受 128-byte 槽位限制)
// ============================================================

// -------- EL1h 同步异常处理 --------
handle_el1h_sync:
    sub  sp, sp, #(8 * 35)
    stp  x0, x1, [sp, #(8 * 0)]
    stp  x2, x3, [sp, #(8 * 2)]
    stp  x4, x5, [sp, #(8 * 4)]
    stp  x6, x7, [sp, #(8 * 6)]
    stp  x8, x9, [sp, #(8 * 8)]
    stp  x10, x11, [sp, #(8 * 10)]
    stp  x12, x13, [sp, #(8 * 12)]
    stp  x14, x15, [sp, #(8 * 14)]
    stp  x16, x17, [sp, #(8 * 16)]
    stp  x18, x19, [sp, #(8 * 18)]
    stp  x20, x21, [sp, #(8 * 20)]
    stp  x22, x23, [sp, #(8 * 22)]
    stp  x24, x25, [sp, #(8 * 24)]
    stp  x26, x27, [sp, #(8 * 26)]
    stp  x28, x29, [sp, #(8 * 28)]
    str  x30, [sp, #(8 * 30)]

    mrs  x0, elr_el1
    mrs  x1, spsr_el1
    stp  x0, x1, [sp, #(8 * 31)]
    mov  x1, sp
    add  x1, x1, #(8 * 35)
    str  x1, [sp, #(8 * 33)]

    mov  x0, sp
    bl   sync_exception_handler

    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    ldp  x20, x21, [sp, #(8 * 20)]
    ldp  x22, x23, [sp, #(8 * 22)]
    ldp  x24, x25, [sp, #(8 * 24)]
    ldp  x26, x27, [sp, #(8 * 26)]
    ldp  x28, x29, [sp, #(8 * 28)]
    add  sp, sp, #(8 * 35)
    eret

// -------- EL1h IRQ handler --------
handle_el1h_irq:
    sub  sp, sp, #(8 * 35)
    stp  x0, x1, [sp, #(8 * 0)]
    stp  x2, x3, [sp, #(8 * 2)]
    stp  x4, x5, [sp, #(8 * 4)]
    stp  x6, x7, [sp, #(8 * 6)]
    stp  x8, x9, [sp, #(8 * 8)]
    stp  x10, x11, [sp, #(8 * 10)]
    stp  x12, x13, [sp, #(8 * 12)]
    stp  x14, x15, [sp, #(8 * 14)]
    stp  x16, x17, [sp, #(8 * 16)]
    stp  x18, x19, [sp, #(8 * 18)]
    stp  x20, x21, [sp, #(8 * 20)]
    stp  x22, x23, [sp, #(8 * 22)]
    stp  x24, x25, [sp, #(8 * 24)]
    stp  x26, x27, [sp, #(8 * 26)]
    stp  x28, x29, [sp, #(8 * 28)]
    str  x30, [sp, #(8 * 30)]

    mrs  x0, elr_el1
    mrs  x1, spsr_el1
    stp  x0, x1, [sp, #(8 * 31)]
    mov  x1, sp
    add  x1, x1, #(8 * 35)
    str  x1, [sp, #(8 * 33)]

    mov  x0, sp
    bl   irq_handler

    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    ldp  x20, x21, [sp, #(8 * 20)]
    ldp  x22, x23, [sp, #(8 * 22)]
    ldp  x24, x25, [sp, #(8 * 24)]
    ldp  x26, x27, [sp, #(8 * 26)]
    ldp  x28, x29, [sp, #(8 * 28)]
    add  sp, sp, #(8 * 35)
    eret

// -------- EL0 sync handler (SVC / 数据异常) --------
handle_el0_sync:
    sub  sp, sp, #(8 * 35)
    stp  x0, x1, [sp, #(8 * 0)]
    stp  x2, x3, [sp, #(8 * 2)]
    stp  x4, x5, [sp, #(8 * 4)]
    stp  x6, x7, [sp, #(8 * 6)]
    stp  x8, x9, [sp, #(8 * 8)]
    stp  x10, x11, [sp, #(8 * 10)]
    stp  x12, x13, [sp, #(8 * 12)]
    stp  x14, x15, [sp, #(8 * 14)]
    stp  x16, x17, [sp, #(8 * 16)]
    stp  x18, x19, [sp, #(8 * 18)]
    stp  x20, x21, [sp, #(8 * 20)]
    stp  x22, x23, [sp, #(8 * 22)]
    stp  x24, x25, [sp, #(8 * 24)]
    stp  x26, x27, [sp, #(8 * 26)]
    stp  x28, x29, [sp, #(8 * 28)]
    str  x30, [sp, #(8 * 30)]

    mrs  x0, elr_el1
    mrs  x1, spsr_el1
    stp  x0, x1, [sp, #(8 * 31)]
    mrs  x1, sp_el0
    str  x1, [sp, #(8 * 33)]

    // ── KPTI: 切换 TTBR1_EL1 到完整内核页表 ──────────────────────
    // 从 EL0 进入时, TTBR1 可能指向 trampoline 页表.
    // 读取 KERNEL_TTBR1 全局变量, 若非 0 则写入 TTBR1_EL1.
    adrp x0, KERNEL_TTBR1
    ldr  x0, [x0, #:lo12:KERNEL_TTBR1]
    cbz  x0, 1f
    dsb  ish
    msr  ttbr1_el1, x0
    isb
1:

    // 检查 ESR_EL1.EC 判断异常类型
    mrs  x0, esr_el1
    lsr  x0, x0, #26        // EC = ESR[31:26]
    cmp  x0, #0x15          // SVC from AArch64
    beq  handle_svc

    // 其他 EL0 同步异常 → sync_exception_handler
    mov  x0, sp
    bl   sync_exception_handler

    // ── KPTI: 切换 TTBR1_EL1 到 trampoline 页表 ──────────────────
    // 返回 EL0 前, 读取 TRAMP_TTBR1, 若非 0 则写入 TTBR1_EL1.
    adrp x0, TRAMP_TTBR1
    ldr  x0, [x0, #:lo12:TRAMP_TTBR1]
    cbz  x0, 2f
    dsb  ish
    msr  ttbr1_el1, x0
    isb
2:
    // 恢复上下文并返回 EL0
    ldr  x1, [sp, #(8 * 33)]
    msr  sp_el0, x1
    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    ldp  x20, x21, [sp, #(8 * 20)]
    ldp  x22, x23, [sp, #(8 * 22)]
    ldp  x24, x25, [sp, #(8 * 24)]
    ldp  x26, x27, [sp, #(8 * 26)]
    ldp  x28, x29, [sp, #(8 * 28)]
    add  sp, sp, #(8 * 35)
    eret

handle_svc:
    // SVC #0: x0 = 系统调用号, x1-x5 = 参数
    mov  x0, sp             // x0 = ExceptionFrame*
    bl   svc_handler

    // 把返回值存入帧内 x0
    str  x0, [sp, #(8 * 0)]

    // ── KPTI: 切换 TTBR1_EL1 到 trampoline 页表 ──────────────────
    // SVC 返回 EL0 前, 读取 TRAMP_TTBR1, 若非 0 则写入 TTBR1_EL1.
    adrp x0, TRAMP_TTBR1
    ldr  x0, [x0, #:lo12:TRAMP_TTBR1]
    cbz  x0, 3f
    dsb  ish
    msr  ttbr1_el1, x0
    isb
3:
    ldr  x1, [sp, #(8 * 33)]
    msr  sp_el0, x1
    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    ldp  x20, x21, [sp, #(8 * 20)]
    ldp  x22, x23, [sp, #(8 * 22)]
    ldp  x24, x25, [sp, #(8 * 24)]
    ldp  x26, x27, [sp, #(8 * 26)]
    ldp  x28, x29, [sp, #(8 * 28)]
    add  sp, sp, #(8 * 35)
    eret

// -------- EL0 IRQ handler --------
handle_el0_irq:
    sub  sp, sp, #(8 * 35)
    stp  x0, x1, [sp, #(8 * 0)]
    stp  x2, x3, [sp, #(8 * 2)]
    stp  x4, x5, [sp, #(8 * 4)]
    stp  x6, x7, [sp, #(8 * 6)]
    stp  x8, x9, [sp, #(8 * 8)]
    stp  x10, x11, [sp, #(8 * 10)]
    stp  x12, x13, [sp, #(8 * 12)]
    stp  x14, x15, [sp, #(8 * 14)]
    stp  x16, x17, [sp, #(8 * 16)]
    stp  x18, x19, [sp, #(8 * 18)]
    str  x30, [sp, #(8 * 30)]

    mrs  x0, elr_el1
    mrs  x1, spsr_el1
    stp  x0, x1, [sp, #(8 * 31)]
    mrs  x1, sp_el0
    str  x1, [sp, #(8 * 33)]

    // ── KPTI: 切换 TTBR1_EL1 到完整内核页表 ──────────────────────
    adrp x0, KERNEL_TTBR1
    ldr  x0, [x0, #:lo12:KERNEL_TTBR1]
    cbz  x0, 4f
    dsb  ish
    msr  ttbr1_el1, x0
    isb
4:
    mov  x0, sp
    bl   irq_handler_el0

    // ── KPTI: 切换 TTBR1_EL1 到 trampoline 页表 ──────────────────
    adrp x0, TRAMP_TTBR1
    ldr  x0, [x0, #:lo12:TRAMP_TTBR1]
    cbz  x0, 5f
    dsb  ish
    msr  ttbr1_el1, x0
    isb
5:
    ldr  x1, [sp, #(8 * 33)]
    msr  sp_el0, x1
    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    add  sp, sp, #(8 * 35)
    eret

// -------- 未预期异常处理 --------
unexpected_exception:
    mrs  x0, esr_el1
    mrs  x1, elr_el1
    mrs  x2, far_el1
    wfi
    b    unexpected_exception
"#
);

// ============================================================================
// 异常上下文 (保存/恢复)
// ============================================================================

/// 异常帧 (保存在 SPSR_EL1 + ELR_EL1)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ExceptionFrame {
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
    pub x4: u64,
    pub x5: u64,
    pub x6: u64,
    pub x7: u64,
    pub x8: u64,
    pub x9: u64,
    pub x10: u64,
    pub x11: u64,
    pub x12: u64,
    pub x13: u64,
    pub x14: u64,
    pub x15: u64,
    pub x16: u64,
    pub x17: u64,
    pub x18: u64,
    pub x19: u64,
    pub x20: u64,
    pub x21: u64,
    pub x22: u64,
    pub x23: u64,
    pub x24: u64,
    pub x25: u64,
    pub x26: u64,
    pub x27: u64,
    pub x28: u64,
    pub x29: u64, // FP
    pub x30: u64, // LR
    pub elr: u64,
    pub spsr: u64,
    pub sp: u64,
}

// ============================================================================
// 异常向量表导出 (供 boot 入口设置 VBAR_EL1)
// ============================================================================

// SAFETY: C ABI 互操作，函数签名与外部代码约定一致
unsafe extern "C" {
    /// 异常向量表起始地址 (定义在 asm)
    pub static exception_vector_table: u8;
}

// ============================================================================
// 异常处理函数
// ============================================================================

/// SVC 系统调用处理。
///
/// EL0 SVC 系统调用处理器。
/// 从 EL0 通过 `svc #0` 进入。
/// QueenX aarch64 系统调用约定: x0=syscall_num, x1-x4=args, 返回 x0。
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn svc_handler(frame: &mut ExceptionFrame) -> u64 {
    let syscall_num = frame.x0;

    // rt_sigreturn 特殊处理: 需要直接修改 frame, 不走正常 dispatch.
    // aarch64: SYS_rt_sigreturn = 139
    // aarch64 信号投递 (do_signal_deliver) 尚未实现, 此拦截为预留.
    // 当信号投递实现后, 从用户栈读取 Aarch64SignalFrame 并恢复 x0-x30/elr/spsr/sp.
    if syscall_num == 139 {
        // 清除 SS_ONSTACK 标记
        if let Some(pid) =
            Some(crate::kernel::framework::proc::process_get_current_pid()).filter(|&p| p != 0)
        {
            crate::kernel::framework::proc::process_with_mut(pid, |proc| {
                use core::sync::atomic::Ordering;
                let flags = proc.sigaltstack_flags.load(Ordering::Acquire);
                proc.sigaltstack_flags.store(
                    flags & !crate::kernel::framework::proc::SS_ONSTACK,
                    Ordering::Release,
                );
            });
        }
        // sigreturn 不返回值, 保持 x0 原值
        return frame.x0;
    }

    let arg0 = frame.x1;
    let arg1 = frame.x2;
    let arg2 = frame.x3;
    let arg3 = frame.x4;
    let arg4 = frame.x5;
    let arg5 = frame.x6;

    // 调用通用 syscall 分发器 (syscall 模块已全局化)
    let result =
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe { crate::kernel::framework::syscall::syscall_dispatch(syscall_num, arg0, arg1, arg2, arg3, arg4, arg5) };

    // 返回值写入 x0
    result as u64
}

/// EL0 IRQ 处理器
#[unsafe(no_mangle)]
#[expect(
    clippy::items_after_statements,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
pub extern "C" fn irq_handler_el0(_frame: &ExceptionFrame) {
    // GIC ACK + handle + EOI
    let intid = super::gic::acknowledge();
    if intid >= 1020 {
        // 伪中断, 无需 EOI
        return;
    }

    // Timer interrupt (PPI 30 = non-secure physical timer)
    if intid == 30 {
        // 重新装载定时器 (ARM Generic Timer 是一次性的)
        super::timer::reload(TIMER_INTERVAL_TICKS.load(Ordering::Relaxed));

        static TIMER_COUNT_EL0: AtomicU64 = AtomicU64::new(0);
        let el0count = TIMER_COUNT_EL0.fetch_add(1, Ordering::Relaxed) + 1;
        if el0count <= 5 {
            crate::klog_info!(
                Boot,
                "TIMER IRQ (EL0) count={} ready={}",
                el0count,
                crate::kernel::framework::net::NET_READY
                    .load(core::sync::atomic::Ordering::Acquire)
            );
        }

        crate::kernel::framework::timer::on_timer_interrupt();

        // I-50: hrtimer_run_queues 已在 on_timer_interrupt 内统一触发, 此处不再显式调用.

        // smoltcp: 始终轮询（DHCP 需要在 poll 中完成握手）
        #[cfg(not(feature = "kernel_test"))]
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            crate::kernel::framework::net::poll_network();
        }

        // 仅当 scheduler 已初始化时触发调度
        if crate::kernel::framework::proc::SCHEDULER_READY.load(Ordering::Acquire) {
            // SAFETY: scheduler_tick 由框架调度器提供, 中断路径触发调度
            unsafe extern "C" {
                fn scheduler_tick();
            }
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                scheduler_tick();
            }
        }
    }

    super::gic::end_of_interrupt(intid);

    crate::kernel::framework::irq::do_softirq();
}

/// 默认同步异常处理 (EL1h)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::no_effect_underscore_binding,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
pub extern "C" fn sync_exception_handler(_frame: &ExceptionFrame) {
    let esr: u64;
    let far: u64;
    let elr: u64;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr);
        core::arch::asm!("mrs {}, far_el1", out(reg) far);
        core::arch::asm!("mrs {}, elr_el1", out(reg) elr);
    }
    let _ec = (esr >> 26) & 0x3F;

    // 直接 UART 输出以确保能观察同步异常
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        super::uart::putc(b'S');
        super::uart::putc(b'Y');
        super::uart::putc(b'N');
        super::uart::putc(b'C');
        super::uart::putc(b'!');
        super::uart::putc(b' ');
        super::uart::putc(b'E');
        super::uart::putc(b'S');
        super::uart::putc(b'R');
        super::uart::putc(b'=');
        exc_puthex(esr);
        super::uart::putc(b' ');
        super::uart::putc(b'F');
        super::uart::putc(b'A');
        super::uart::putc(b'R');
        super::uart::putc(b'=');
        exc_puthex(far);
        super::uart::putc(b' ');
        super::uart::putc(b'E');
        super::uart::putc(b'L');
        super::uart::putc(b'R');
        super::uart::putc(b'=');
        exc_puthex(elr);
        super::uart::putc(b'\r');
        super::uart::putc(b'\n');
    }

    loop {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}

/// 辅助函数: 通过 UART 以十六进制输出 u64
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe fn exc_puthex(val: u64) {
    unsafe {
        for shift in (0..16).rev() {
            let nibble = ((val >> (shift * 4)) & 0xF) as u8;
            let c = if nibble < 10 {
                b'0' + nibble
            } else {
                b'A' + nibble - 10
            };
            super::uart::putc(c);
        }
    }
}

/// 默认 IRQ 处理 (EL1h)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::items_after_statements,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
pub extern "C" fn irq_handler(_frame: &ExceptionFrame) {
    // GIC ACK
    let intid = super::gic::acknowledge();

    // 诊断: 记录所有 IRQ 以追踪崩溃点
    {
        static IRQ_COUNT: AtomicU64 = AtomicU64::new(0);
        let count = IRQ_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if count <= 10 {
            crate::klog_info!(Boot, "IRQ: intid={} count={}", intid, count);
        }
    }

    if intid >= 1020 {
        return;
    }

    // ── 栏栈恢复 SGI 7 (aarch64 等价于 x86_64 int 0x82) ─────────────────
    if intid == super::barrier::BARRIER_RECOVERY_SGI as u32 {
        let result = super::barrier::barrier_sgi_handler();
        if result < 0 {
            crate::klog_info!(Boot, "Barrier recovery SGI failed: {}", result);
        }
        super::gic::end_of_interrupt(intid);
        return;
    }

    // Timer interrupt (PPI 30 = non-secure physical timer)
    if intid == 30 {
        // 重新装载定时器 (ARM Generic Timer 是一次性的)
        super::timer::reload(TIMER_INTERVAL_TICKS.load(Ordering::Relaxed));

        static TIMER_COUNT: AtomicU64 = AtomicU64::new(0);
        let tcount = TIMER_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if tcount <= 5 {
            crate::klog_info!(
                Boot,
                "TIMER IRQ count={} ready={}",
                tcount,
                crate::kernel::framework::net::NET_READY
                    .load(core::sync::atomic::Ordering::Acquire)
            );
        }

        crate::kernel::framework::timer::on_timer_interrupt();

        // 网络轮询
        #[cfg(not(feature = "kernel_test"))]
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            crate::kernel::framework::net::poll_network();
        }

        // 仅当 scheduler 已初始化时触发调度
        if crate::kernel::framework::proc::SCHEDULER_READY
            .load(core::sync::atomic::Ordering::Acquire)
        {
            // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
            unsafe extern "C" {
                fn scheduler_tick();
            }
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                scheduler_tick();
            }
        }
    }

    super::gic::end_of_interrupt(intid);
}

/// 默认 FIQ 处理 (EL1h)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn fiq_handler(_frame: &ExceptionFrame) {}

/// 默认 SError 处理 (EL1h)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn serror_handler(_frame: &ExceptionFrame) {
    loop {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}

#[expect(
    clippy::borrow_as_ptr,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
/// 初始化异常: 设置 VBAR_EL1 指向向量表, 清除 DAIF
///
/// # Safety
///
/// 仅在启动阶段调用，调用前需确保向量表已链接到内核镜像中。
///
/// VBAR_EL1 必须使用 TTBR1 高地址 (0xFFFF_0000_...), 因为进入 EL0 后
/// TTBR0_EL1 指向用户页表, 低地址无法通过 TTBR0 访问。TTBR1_EL1 映射:
/// VA = PA + 0xFFFF_0000_0000_0000, 所有相对跳转 (b/bl/adrp) 自动适配。
pub unsafe fn init() {
    unsafe {
        let vbar_low = &exception_vector_table as *const u8 as u64;
        // 转换为 TTBR1 高地址: VA = PA + 0xFFFF_0000_0000_0000
        let vbar = 0xFFFF_0000_0000_0000u64 + vbar_low;
        core::arch::asm!("msr vbar_el1, {}", in(reg) vbar);

        // 清除 DAIF (Debug/SError/IRQ/FIQ 掩码), 使能中断
        core::arch::asm!("msr daifclr, #0xF");

        // ISB 确保写 VBAR 在取指前完成
        core::arch::asm!("isb");
    }
}
