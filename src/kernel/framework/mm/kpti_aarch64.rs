//! KPTI (Kernel Page Table Isolation) — AArch64
//!
//! ARMv8-A Meltdown 缓解: 用户态 (EL0) 运行期间令 TTBR1_EL1 指向**最小化内核页表**
//! (trampoline 表), 仅映射异常入口实际用到的页, 缩小内核地址空间在用户态的可见面.
//!
//! # 全切换模型 (对齐 x86_64 CR3 语义)
//!
//! aarch64 内核代码/数据/栈位于低半区 (`KERNEL_BASE=0`, VA==PA), 由 TTBR0_EL1 翻译;
//! 异常向量表经高半区别名 (`VA = PA + 0xFFFF_0000_0000_0000`) 由 TTBR1_EL1 翻译.
//! 故"用户态隔离"必须 TTBR0/TTBR1 **同时**切换:
//!
//! - **EL0 运行期**: TTBR0 = 用户页表; TTBR1 = trampoline 表.
//!   用户页表额外映射内核栈**首页** (EL1-only, 无 USER 位) —— 异常入口在切换 TTBR0
//!   之前需向 SP_EL1 (= 内核栈低半区地址) 压入 280 字节异常帧.
//! - **异常入口** (`exception.rs` 的 `handle_el0_*`): 压帧后立刻切
//!   TTBR0 → 内核恒等表 (`kernel_ttbr0`), TTBR1 → 完整内核根表 (`kernel_ttbr1`),
//!   此后处理器在完整内核地址空间中运行.
//! - **异常出口** (eret 前): 切回 TTBR0 → 用户页表 (`user_ttbr0`),
//!   TTBR1 → trampoline 表. 切换代码必须位于高半区 (`.vectors`) —— TTBR0 切换后
//!   低半区代码即不可取指 (实测: 低半区切换必然 Prefetch Abort).
//! - **进入 EL0**: `arch/aarch64/mod.rs::enter_user` 跳转到 `.vectors` 内的
//!   `kpti_enter_user_trampoline` (高半区) 完成切换后 eret.
//!
//! aarch64 无 SMP (`smp_init.rs` 仅 x86_64), 故全局量用普通 `AtomicU64` 即可.

#![cfg(target_arch = "aarch64")]

use core::sync::atomic::{AtomicU64, Ordering};

use crate::framework::mm::PAGE_SIZE;
use crate::framework::mm::pmm_alloc_page;

// ── 公共状态 ──────────────────────────────────────────────────────

/// KPTI 全局量集合.
///
/// 单独成页 (`repr(align(4096))`) 有两个作用:
/// 1. trampoline 表以**页级最小化**映射本页, 使入口/出口汇编的
///    `adrp KPTI_GLOBALS` 在 tramp 表下可取数;
/// 2. 汇编按固定字节偏移访问各字段, 布局由下方 `offset_of!` 静态断言锁定.
#[repr(C, align(4096))]
pub struct KptiGlobals {
    /// Trampoline TTBR1_EL1 物理地址 (EL0 运行期 TTBR1).
    pub tramp_ttbr1: AtomicU64,
    /// 完整内核 TTBR1_EL1 物理地址 (异常入口切回).
    pub kernel_ttbr1: AtomicU64,
    /// 内核恒等 TTBR0_EL1 物理地址 (异常入口切回).
    pub kernel_ttbr0: AtomicU64,
    /// 当前用户进程 TTBR0_EL1 物理地址 (异常出口 / 进入 EL0 时切回).
    pub user_ttbr0: AtomicU64,
    /// KPTI 是否已初始化 (0 = 未就绪, 1 = 就绪).
    pub ready: AtomicU64,
}

/// KPTI 全局状态实例. `#[unsafe(no_mangle)]` 供汇编按符号名/`sym` 操作数访问.
// SAFETY: FFI 导出静态量，通过 C ABI 与汇编代码互操作
#[unsafe(no_mangle)]
pub static KPTI_GLOBALS: KptiGlobals = KptiGlobals {
    tramp_ttbr1: AtomicU64::new(0),
    kernel_ttbr1: AtomicU64::new(0),
    kernel_ttbr0: AtomicU64::new(0),
    user_ttbr0: AtomicU64::new(0),
    ready: AtomicU64::new(0),
};

// 汇编按字面偏移 (0/8/16/24/32) 访问上述字段; 调整字段顺序/宽度会在此编译失败.
const _: () = assert!(core::mem::offset_of!(KptiGlobals, tramp_ttbr1) == 0);
const _: () = assert!(core::mem::offset_of!(KptiGlobals, kernel_ttbr1) == 8);
const _: () = assert!(core::mem::offset_of!(KptiGlobals, kernel_ttbr0) == 16);
const _: () = assert!(core::mem::offset_of!(KptiGlobals, user_ttbr0) == 24);
const _: () = assert!(core::mem::offset_of!(KptiGlobals, ready) == 32);

/// 高半区别名基址: `VA = PA + HIGH_ALIAS_BASE`.
pub const HIGH_ALIAS_BASE: u64 = 0xFFFF_0000_0000_0000;

// ── 公开 API ──────────────────────────────────────────────────────

/// 返回 KPTI 是否已就绪.
#[inline(always)]
pub fn kpti_is_active() -> bool {
    KPTI_GLOBALS.ready.load(Ordering::Acquire) != 0
}

/// 返回 trampoline TTBR1 物理地址 (供异常入口汇编读取).
#[inline(always)]
pub fn kpti_trampoline_ttbr1() -> u64 {
    KPTI_GLOBALS.tramp_ttbr1.load(Ordering::Acquire)
}

/// 返回完整内核 TTBR1 物理地址.
#[inline(always)]
pub fn kpti_kernel_ttbr1() -> u64 {
    KPTI_GLOBALS.kernel_ttbr1.load(Ordering::Acquire)
}

/// 返回内核恒等 TTBR0 物理地址.
#[inline(always)]
pub fn kpti_kernel_ttbr0() -> u64 {
    KPTI_GLOBALS.kernel_ttbr0.load(Ordering::Acquire)
}

/// 记录当前用户进程 TTBR0 物理地址 (由 `enter_user` 在进入 EL0 前调用).
#[inline(always)]
pub fn kpti_set_user_ttbr0(ttbr0: u64) {
    KPTI_GLOBALS.user_ttbr0.store(ttbr0, Ordering::Release);
}

/// 返回当前用户进程 TTBR0 物理地址.
#[inline(always)]
pub fn kpti_user_ttbr0() -> u64 {
    KPTI_GLOBALS.user_ttbr0.load(Ordering::Acquire)
}

/// 把内核栈**顶页**以 EL1-only 页级映射进用户页表 (`cr3`).
///
/// 依据 (全切换模型): EL0→EL1 异常入口 (`exception.rs::handle_el0_*`) 在切换
/// `TTBR0` **之前**就把 280 字节异常帧压入 `SP_EL1` (= 内核栈顶), 故该页在用户
/// 页表下必须可写 —— 与 `x86_64` 把 TSS.RSP0 栈页映射进用户页表同因.
///
/// 映射不带 `USER` 位 ⇒ EL0 不可访问; 且 `is_user_leaf` 判据 (bit6 = AP\[1\])
/// 不将其视为用户 leaf, 故 COW 克隆与拆表都不会对其做帧计数 (不误释放共享帧).
///
/// 每个用户页表都需具备该映射: 新建页表 (`create_user_process`) 与 COW 克隆出的
/// 子页表 (fork) 各调用一次.
pub fn map_kernel_stack_top_page(cr3: u64, kstack_top: u64) {
    if cr3 == 0 || kstack_top == 0 {
        return;
    }
    // aarch64 `KERNEL_BASE == 0` (恒等映射) ⇒ 顶页虚拟地址即物理地址
    let top_page = (kstack_top - 1) & !(PAGE_SIZE - 1);
    super::vmm::get_vmm().map_page_in_table(
        cr3,
        super::VirtAddr(top_page),
        super::PhysAddr(top_page),
        super::PageFlags::PRESENT | super::PageFlags::WRITABLE,
    );
}

/// 进入内核态: 切换 TTBR0/TTBR1 到完整内核地址空间.
///
/// 异常入口汇编 (`handle_el0_*`) 已内联同一序列; 本函数供 Rust 侧诊断/兜底使用.
///
/// # Safety
///
/// 调用方必须是异常入口 trampoline (EL0→EL1 切换的第一时间), 且切换后
/// 的取指必须位于高半区映射内.
#[inline(never)]
pub unsafe fn kpti_enter_kernel() {
    let kernel_ttbr0 = KPTI_GLOBALS.kernel_ttbr0.load(Ordering::Acquire);
    let kernel_ttbr1 = KPTI_GLOBALS.kernel_ttbr1.load(Ordering::Acquire);
    if kernel_ttbr0 == 0 || kernel_ttbr1 == 0 {
        return;
    }
    // SAFETY: 写入 TTBR0/TTBR1_EL1 是特权操作; 两个值来自 init 阶段可信来源.
    unsafe {
        core::arch::asm!(
            "dsb ish",
            "msr ttbr0_el1, {0}",
            "msr ttbr1_el1, {1}",
            "isb",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            in(reg) kernel_ttbr0,
            in(reg) kernel_ttbr1,
        );
    }
}

/// 返回用户态: 切换 TTBR0/TTBR1 到 (用户页表 + trampoline 表).
///
/// # Safety
///
/// 调用方必须是异常出口 trampoline (即将 eret 返回 EL0), 且所在代码
/// 必须位于高半区 (.vectors); 调用方随后立即 eret.
#[inline(never)]
pub unsafe fn kpti_exit_to_user() {
    let user_ttbr0 = KPTI_GLOBALS.user_ttbr0.load(Ordering::Acquire);
    let tramp_ttbr1 = KPTI_GLOBALS.tramp_ttbr1.load(Ordering::Acquire);
    if user_ttbr0 == 0 || tramp_ttbr1 == 0 {
        return;
    }
    // SAFETY: 写入 TTBR0/TTBR1_EL1 是特权操作; 两个值来自 kernel→user 边界可信来源.
    unsafe {
        core::arch::asm!(
            "dsb ish",
            "msr ttbr1_el1, {0}",
            "msr ttbr0_el1, {1}",
            "isb",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            in(reg) tramp_ttbr1,
            in(reg) user_ttbr0,
        );
    }
}

// ── 初始化 ────────────────────────────────────────────────────────

// SAFETY: C ABI 互操作，符号由 link/aarch64.ld 定义
unsafe extern "C" {
    /// 异常向量表段起点 (link/aarch64.ld 的 `.vectors`)
    static _vectors_start: u8;
    /// 异常向量表段终点 (4KB 对齐)
    static _vectors_end: u8;
}

#[expect(
    clippy::missing_panics_doc,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
#[expect(
    clippy::manual_assert,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
/// 初始化 KPTI: 创建**页级最小化**的 trampoline TTBR1 页表.
///
/// trampoline 表 (4 级根) 仅映射 EL0↔EL1 边界真正用到的页 (高半区别名):
/// - `.vectors` 全部页 (异常入口/出口 + 进入 EL0 trampoline 的取指面)
/// - `KPTI_GLOBALS` 所在页 (入口/出口汇编读写的全局量)
///
/// 其余高半区条目为 0 ⇒ 用户态不可见内核代码/数据. TTBR1 由 T1SZ=16 决定硬件
/// **从 level 0 起**遍历, 故 tramp 根必须是 4 级表 (与 `mmu.rs::init_kernel_ttbr1`
/// 同一归因, 勿重犯历史上"指向 L1 形态表"的错误).
///
/// # Safety
///
/// 调用方保证: `kernel_ttbr1` 是当前 TTBR1_EL1 的有效物理地址; PMM 已初始化;
/// KPTI 全局状态在 boot 阶段被独占写入.
///
/// `vmm` 由调用方以 `&self` 传入 (而非经 `get_vmm()`): 本函数在
/// `vmm_init` 的 `OnceLock` 初始化**过程中**被调用, 此刻全局实例尚不可获取.
pub unsafe fn kpti_init(vmm: &super::vmm::Aarch64Vmm, kernel_ttbr1: u64) {
    if kpti_is_active() {
        return;
    }

    // 1. 读取当前 TTBR0_EL1 (mmu::init 建立的内核恒等表)
    let kernel_ttbr0: u64;
    // SAFETY: mrs 只读系统寄存器, 无副作用.
    unsafe {
        core::arch::asm!("mrs {}, ttbr0_el1", out(reg) kernel_ttbr0);
    }

    // 2. 分配并清零 trampoline 4 级根表
    let tramp_l0_phys = pmm_alloc_page() as u64;
    if tramp_l0_phys == 0 {
        // 不可恢复: KPTI 初始化需要 trampoline 根表, 分配失败意味着内存耗尽,
        // 内核无法安全进入用户态, 只能停机
        panic!("[KPTI] failed to allocate trampoline L0 page");
    }
    // SAFETY: pmm 分配的页 4KB 对齐且属内核; phys_to_virt 给出可写内核 VA.
    unsafe {
        core::ptr::write_bytes(phys_to_virt(tramp_l0_phys) as *mut u8, 0, PAGE_SIZE as usize);
    }

    // 3. 页级最小化映射 (高半区别名 VA → 同物理页, EL1 RW 可执行, 无 USER 位)
    let mut pa = &raw const _vectors_start as u64;
    let vectors_end = &raw const _vectors_end as u64;
    while pa < vectors_end {
        vmm.map_page_in_table(
            tramp_l0_phys,
            super::VirtAddr(pa + HIGH_ALIAS_BASE),
            super::PhysAddr(pa),
            super::PageFlags::PRESENT | super::PageFlags::WRITABLE,
        );
        pa += PAGE_SIZE;
    }
    let globals_pa = (core::ptr::addr_of!(KPTI_GLOBALS) as u64) & !(PAGE_SIZE - 1);
    vmm.map_page_in_table(
        tramp_l0_phys,
        super::VirtAddr(globals_pa + HIGH_ALIAS_BASE),
        super::PhysAddr(globals_pa),
        super::PageFlags::PRESENT | super::PageFlags::WRITABLE,
    );

    // 4. 公开状态 (写入顺序: 先数据后 ready, ready 兼作 Release 屏障)
    KPTI_GLOBALS.kernel_ttbr0.store(kernel_ttbr0, Ordering::Release);
    KPTI_GLOBALS.kernel_ttbr1.store(kernel_ttbr1, Ordering::Release);
    KPTI_GLOBALS.tramp_ttbr1.store(tramp_l0_phys, Ordering::Release);
    KPTI_GLOBALS.ready.store(1, Ordering::Release);
}

/// KPTI 关闭时的占位: 返回 trampoline TTBR1, 未就绪时退回完整内核 TTBR1.
#[inline(always)]
pub fn kpti_trampoline_ttbr1_or_kernel(kernel_ttbr1: u64) -> u64 {
    let t = KPTI_GLOBALS.tramp_ttbr1.load(Ordering::Acquire);
    if t == 0 { kernel_ttbr1 } else { t }
}

/// 物理地址 → 内核可访问虚拟地址 (`KERNEL_BASE=0` ⇒ 恒等).
#[inline(always)]
fn phys_to_virt(phys: u64) -> u64 {
    phys + super::KERNEL_BASE
}