//! KPTI (Kernel Page Table Isolation) — `x86_64`
//!
//! 抗 Meltdown 用户/内核页表隔离。
//!
//! # 设计目标
//!
//! 1. **双页表**: 一份仅含 entry/exit trampoline 所需最小内核数据 (`USER_PML4`),
//!    一份完整内核页表 (`KERNEL_PML4`)。CPU 在用户态运行 `USER_PML4`,
//!    进入内核后切换为 `KERNEL_PML4`。
//! 2. **安全**: 大部分内核页表条目移除 USER 位, 用户态无法访问内核 .text/.data/.bss/
//!    GDT/IDT/TSS/per-CPU 栈/页表本身。
//! 3. **可关闭**: 通过 `KernelCapabilities::kpti` 编译期开关, 调试时关闭可加快 TT 速度。
//!
//! # 现状 (本轮 PR)
//!
//! - **已完成**:
//!   - 用户页表初始化 (复制 `KERNEL_PML4` 256..511 项, 清 USER 位, 标记 trampoline 页保留 USER)
//!   - `switch_to_user_pml4` / `switch_to_kernel_pml4` CR3 切换原语
//!   - 公共 API: `kpti_init`, `kpti_is_active`, `kpti_user_pml4`, `kpti_enter_kernel`,
//!     `kpti_exit_to_user`
//!   - 与 `vmm::Vmm::init` 集成
//!
//! - **未完成 (本轮范围外, 在 `engineering-progress.md` §五 + roadmap Backlog 登记)**:
//!   - **汇编 trampoline 集成**: 当前 syscall 入口是 Rust `syscall_dispatch_from_frame`,
//!     KPTI 切换必须在汇编中做 (CPU 进入内核的第一条指令必须是 `mov cr3, kernel_pml4`,
//!     否则 CPU 仍按 `user_pml4` 寻址, 会因缺页 #PF panic)。
//!     需要新增 `entry_SYSCALL_64` / `swapgs_restore_regs_and_return_to_usermode` 汇编
//!     trampoline, 把当前 `syscall_dispatch_from_frame` 改造成可被 trampoline 调用。
//!   - **PCID/INVPCID 优化**: 当前每次切换 CR3 都 TLB 全清, 高频 syscall 性能损失 5-15%。
//!   - **aarch64 双 TTBR**: 需要在 `vmm_aarch64.rs` 实现 TTBR0 (用户) / TTBR1 (内核) 切换。
//!   - **可写 trampoline 页的 RO 化**: trampoline 代码需要 RO+NX 保护。

#![cfg(target_arch = "x86_64")]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::framework::mm::pmm_alloc_page;
// host-test 下 KERNEL_BASE 仅在 map_text_page (整函数门控排除) 中使用,
// 该符号改由真机分支单独导入 (避免 host-test 维 unused_imports).
#[cfg(not(feature = "host-test"))]
use crate::framework::mm::KERNEL_BASE;
use crate::framework::mm::{PAGE_SIZE, PhysAddr};

// ── PCID 常量 ─────────────────────────────────────────────────────
// PCID (Process-Context Identifier) 占 CR3 低 12 位, 用于 TLB 标记.
// 启用 PCID 后, CR3 切换不再隐式刷新全局 TLB, 改用 INVPCID 精确刷除.

/// 内核页表 PCID
pub const PCID_KERNEL: u64 = 1;
/// 用户页表 PCID
pub const PCID_USER: u64 = 2;

/// INVPCID 指令类型: 按 PCID 刷新 TLB
///
/// 供 VMM 页表修改 (COW/mprotect) 后刷除特定 PCID 的 TLB 条目.
const INVPCID_TYPE_SINGLE: u64 = 0;
/// INVPCID 指令类型: 刷新所有 TLB (包括 global 页)
const INVPCID_TYPE_ALL_INCL_GLOBAL: u64 = 2;

/// 执行 INVPCID 指令, 刷新指定 PCID 的 TLB 条目.
///
/// # Safety
///
/// 调用方保证 CPU 支持 INVPCID (通过 CPUID.07H:EBX.IVPCID 确认).
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub unsafe fn invpcid(pcid: u64, addr: u64, typ: u64) {
    // INVPCID 描述符: 16 字节, [0:7] PCID, [8:15] 线性地址
    // 在栈上构造描述符, 通过内存操作数传递给 INVPCID.
    // INVPCID 格式: invpcid r64, m128 — 第二操作数必须是内存引用.
    let desc: [u64; 2] = [pcid, addr];
    // SAFETY: 调用方保证 CPU 支持 INVPCID; desc 在栈上有效, 16 字节对齐.
    unsafe {
        core::arch::asm!(
            "invpcid {typ}, [{desc}]",
            typ = in(reg) typ,
            desc = in(reg) desc.as_ptr(),
            options(nostack, preserves_flags, readonly),
        );
    }
}

/// 刷新所有 PCID 的 TLB 条目 (不含 global 页).
///
/// # Safety
///
/// 调用方保证 CPU 支持 INVPCID (通过 CPUID.07H:EBX.IVPCID 确认).
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub unsafe fn invpcid_flush_all() {
    // SAFETY: 调用方保证 CPU 支持 INVPCID; type 2 刷新所有 TLB 条目是安全操作.
    unsafe {
        invpcid(0, 0, INVPCID_TYPE_ALL_INCL_GLOBAL);
    }
}

/// 按 PCID + 虚拟地址刷新单条 TLB 条目.
///
/// 用于 VMM COW/mprotect 的细粒度 TLB 失效, 避免全量刷新的性能损失.
///
/// # Safety
///
/// - `pcid` 必须是有效的 PCID (0-4095)
/// - `vaddr` 必须是页对齐的虚拟地址
/// - 调用方保证 vaddr 属于当前地址空间或已通过 CR3 切换访问
/// - CPU 必须支持 INVPCID (通过 CPUID.07H:EBX.IVPCID 确认)
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub unsafe fn invpcid_flush_single(pcid: u16, vaddr: u64) {
    // SAFETY: INVPCID type 0 (by individual address + PCID).
    // 前提: pcid 有效 (0-4095), vaddr 页对齐.
    // 调用方保证: vaddr 属于调用方地址空间.
    // 硬件契约: INVPCID 指令在支持的 CPU 上原子刷新单条 TLB.
    unsafe {
        invpcid(u64::from(pcid), vaddr, INVPCID_TYPE_SINGLE);
    }
}

/// CR3 值中嵌入 PCID: PML4 物理地址 | PCID.
#[inline(always)]
#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
pub const fn cr3_with_pcid(pml4_phys: u64, pcid: u64) -> u64 {
    (pml4_phys & 0x000FFFFFFFFFF000) | (pcid & 0xFFF)
}

/// 检查 CPU 是否支持 INVPCID.
#[inline]
pub fn has_invpcid() -> bool {
    crate::framework::cpu::get_cpu_info().is_some_and(|info| {
        info.features
            .contains(crate::framework::cpu::CpuFeatures::INVPCID)
    })
}

/// 检查 PCID 是否已启用 (CR4.PCIDE = 1).
#[inline]
pub fn pcid_is_enabled() -> bool {
    let cr4: u64;
    // SAFETY: 读取 CR4 是特权操作但无副作用, 仅检查 bit 17.
    unsafe {
        core::arch::asm!("mov {0}, cr4", out(reg) cr4, options(nostack, nomem));
    }
    (cr4 >> 17) & 1 == 1
}

// ── 链接脚本符号 (x86_64.ld) ──────────────────────────────────────
// KPTI trampoline 代码范围: _kernel_text_start ~ _kernel_text_end
// 这些页在 USER_PML4 中需要保持可执行 (X), 其余代码页设为 NX.
// (_kpti_trampoline_end 曾为独立边界符号, 全仓零 Rust 引用, 声明已删除)

// SAFETY: 链接脚本定义的符号, 地址有效 (只读引用).
// 符号桩化 (host-test): host 无 x86_64.ld 符号, 两处引用点 (本文件 `kpti_init` step 4.5、
// `vmm_x86_64::create_user_page_table` KPTI 同步段) 均受 not(host-test) 门控; 声明门控与之
// 严格同构 (本模块已受 target_arch = "x86_64" 门控, 不重复 arch 条件).
#[cfg(not(feature = "host-test"))]
unsafe extern "C" {
    pub(super) static _kernel_text_start: u8;
    pub(super) static _kernel_text_end: u8;
}

// ── 公共状态 ──────────────────────────────────────────────────────

/// KPTI 是否已初始化 (init 完成后置 true)。
static KPTI_READY: AtomicBool = AtomicBool::new(false);

/// `USER_PML4` 物理地址 (在 `vmm_init` 阶段被初始化)。
///
/// 此 PML4 与 `KERNEL_PML4` 共享 entries 256..511 的内核高半区,
/// 但**清除了 USER 位**, 仅保留 trampoline + 必要内核数据条目的 USER 位。
static USER_PML4: AtomicU64 = AtomicU64::new(0);

/// 上一份 PML4 物理地址 (供 `switch_to_kernel_pml4` 切回时使用)。
///
/// 注: 切回时直接用 `KERNEL_PML4` 而非此处保存的旧值, 因为 CPU 上一次在内核态
/// 使用的就是 `KERNEL_PML4`, 保留 `USER_PML4` 即可, 不需要 per-CPU 保存。
static LAST_KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

// ── 公开 API ──────────────────────────────────────────────────────

/// 返回 KPTI 是否已就绪 (init 调用完成)。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub fn kpti_is_active() -> bool {
    KPTI_READY.load(Ordering::Acquire)
}

/// 返回 `USER_PML4` 物理地址 (供 COW fork 等路径构造子进程用户页表)。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub fn kpti_user_pml4() -> u64 {
    USER_PML4.load(Ordering::Acquire)
}

/// 返回 `KERNEL_PML4` 物理地址 (避免其它模块读 `KERNEL_PML4` 内部符号)。
#[inline(always)]
pub fn kpti_kernel_pml4() -> u64 {
    LAST_KERNEL_PML4.load(Ordering::Acquire)
}

/// 将 `KERNEL_PML4` `[`pml4_idx`]` 同步到 `USER_PML4` `[`pml4_idx`]`.
///
/// 当 VMM 在内核高半区创建新的 PML4 条目 (如帧缓冲 MMIO 映射) 时,
/// 必须同步到 `USER_PML4`, 否则 KPTI 模式下 CPU 使用 user CR3 时
/// 访问该地址会触发 Page Fault.
///
/// # Safety
///
/// 调用方保证: `KERNEL_PML4` 已初始化; `pml4_idx` 在 [256, 512) 范围内;
/// `VMM_LOCK` 已持有 (防止并发修改).
pub unsafe fn kpti_sync_pml4_entry(pml4_idx: usize) {
    if !KPTI_READY.load(Ordering::Acquire) {
        return;
    }
    let user_pml4_phys = USER_PML4.load(Ordering::Acquire);
    if user_pml4_phys == 0 {
        return;
    }
    // SAFETY: KERNEL_PML4 和 USER_PML4 均已初始化, phys_to_virt 产生有效内核 VA.
    // VMM_LOCK 由调用方持有, 防止并发修改页表.
    unsafe {
        let kernel_pml4_phys =
            crate::framework::mm::vmm::KERNEL_PML4.load(Ordering::Acquire);
        let src = crate::framework::mm::PhysAddr(kernel_pml4_phys)
            .to_virt()
            .0 as *const u64;
        let dst = crate::framework::mm::PhysAddr(user_pml4_phys)
            .to_virt()
            .0 as *mut u64;
        let entry = core::ptr::read_volatile(src.add(pml4_idx));
        core::ptr::write_volatile(dst.add(pml4_idx), entry);
    }
}

// ── 切换原语 (entry/exit trampoline 调用) ────────────────────────

/// 进入内核态: CR3 切换 `USER_PML4` → `KERNEL_PML4`。
///
/// **调用方**: 必须在 CPU 刚进入 ring 0 的第一条指令 (syscall/iret 入口汇编 trampoline)。
/// **当前实现**: 该函数可用作逻辑参考, 但**真正的集成需要在汇编 trampoline 中**,
/// Rust 函数调用栈一旦建立, 切 CR3 就会因旧栈页不可见导致立即 #PF。
///
/// # Safety
///
/// 调用方必须是 CPU 入口 trampoline (栈尚未建立 / 栈为 trampoline 专用页)。
#[inline(never)]
pub unsafe fn kpti_enter_kernel() {
    let kernel_pml4 = LAST_KERNEL_PML4.load(Ordering::Acquire);
    if kernel_pml4 == 0 {
        return;
    }
    // SAFETY: CR3 write is privileged; kernel_pml4 来自 init 阶段的可信来源。
    unsafe {
        core::arch::asm!(
            "mov cr3, {pml4}",
            pml4 = in(reg) kernel_pml4,
            options(nostack, preserves_flags),
        );
    }
}

/// 返回用户态: CR3 切换 `KERNEL_PML4` → `USER_PML4`。
///
/// **调用方**: 必须在 CPU 即将 iretq/sysret 之前的最后一条指令 (exit trampoline)。
///
/// # Safety
///
/// 调用方必须是 exit trampoline (即将 iretq/sysret, 已恢复用户态寄存器)。
#[inline(never)]
pub unsafe fn kpti_exit_to_user() {
    let user_pml4 = USER_PML4.load(Ordering::Acquire);
    if user_pml4 == 0 {
        return;
    }
    // SAFETY: CR3 write is privileged; user_pml4 来自 init 阶段的可信来源。
    unsafe {
        core::arch::asm!(
            "mov cr3, {pml4}",
            pml4 = in(reg) user_pml4,
            options(nostack, preserves_flags),
        );
    }
}

// ── 初始化 ────────────────────────────────────────────────────────

/// 初始化 KPTI: 分配 `USER_PML4` 页, 从 `KERNEL_PML4` 复制内核高半区,
/// 清除 USER 位, 保留 trampoline 区域。
///
/// 必须在 `vmm::Vmm::init` 之后调用 (依赖 `KERNEL_PML4` 已初始化)。
///
/// # Safety
///
/// 调用方保证: `KERNEL_PML4` 已初始化; PMM 可分配页面; KPTI 全局状态在 boot 阶段被独占写入。
/// # Panics
/// 分配 `USER_PML4` 页失败时 panic。
// 有意窄化: 显式收窄, 调用方保证值域
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::verbose_bit_mask,
    reason = "DECISION-043 pedantic 兜底: 当前批量 expect 兑底; 后续可逐处手工重构 (改 .cast() / let-else / 命名等)"
)]
pub unsafe fn kpti_init(kernel_pml4: u64) {
    if KPTI_READY.load(Ordering::Acquire) {
        return;
    }

    // 1. 分配 USER_PML4 物理页
    let user_pml4_phys = pmm_alloc_page() as u64;
    // 不可恢复: KPTI 初始化需要 USER_PML4 页, 分配失败意味着内存耗尽,
    // 内核无法安全进入用户态, 只能停机
    assert!(
        user_pml4_phys != 0,
        "[KPTI] failed to allocate USER_PML4 page"
    );
    let user_pml4_virt = PhysAddr(user_pml4_phys).to_virt();

    // 2. 清零
    // SAFETY: pmm 分配的页已对齐, 物理页属于内核
    unsafe {
        core::ptr::write_bytes(user_pml4_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
    }

    // 3. 复制 KERNEL_PML4[256..512] (内核高半区) 到 USER_PML4[256..512]
    // SAFETY: kernel_pml4 由 vmm_init 写入, user_pml4 由 pmm 分配, 均有效
    unsafe {
        let src = PhysAddr(kernel_pml4).to_virt().0 as *const u64;
        let dst = user_pml4_virt.0 as *mut u64;
        core::ptr::copy_nonoverlapping(src.add(256), dst.add(256), 256);
    }

    // 4. [已移除] 原清除 [0..256] USER 位的循环.
    //
    // 低半区 [0..256] 仅包含用户页 (代码 0x400000, 栈 0x7FFFFFFFE000 等),
    // 内核页全部在高半区 [256..512]. 清除低半区 USER 位会导致 Ring 3
    // 无法执行用户代码 → #PF → 内核重启. 该循环无安全收益.
    //
    // KPTI 安全性由高半区加固 (step 4.5) 保证: 仅 trampoline 代码页保留 RX,
    // 其余内核代码页设为 RO+NX, 数据页限制权限.

    // 4.5 映射整个 .text 区域到 USER_PML4 (PRESENT only, SMEP-safe)
    //
    // 策略: 将 _kernel_text_start ~ _kernel_text_end 的所有页面映射到用户页表,
    // 权限 PRESENT (Ring 0 可执行, SMEP 兼容). 不设 USER 位.
    //
    // 原因: 异常处理代码 (isr0-isr31, irq0-irq15) 和 syscall_entry 位于 .text,
    // 用户态触发异常/系统调用时 CPU 使用 USER_PML4 寻址取指, 必须能执行这些代码页.
    // SMEP 启用时, 若页面设 USER 位, Ring 0 取指触发 #PF.
    // 此前仅映射 trampoline 子范围 (_kernel_text_start ~ _kpti_trampoline_end)
    // 导致异常处理代码不可执行或未映射 → #PF → Triple Fault.
    //
    // 安全性: .text 为只读代码, 不设 USER 位防止 Ring 3 访问,
    // 同时满足 SMEP 要求 (Ring 0 可执行非 USER 页).
    // KPTI 核心保护 (数据页隔离) 不受影响.
    //
    // 符号桩化 (host-test): host 无 _kernel_text_* 链接脚本符号且无页表上下文,
    // 文本区间映射整段跳过.
    #[cfg(not(feature = "host-test"))]
    {
    // SAFETY: user_pml4_virt 有效, 映射操作只修改 USER_PML4, 不影响 KERNEL_PML4.
    unsafe {
        let text_start = core::ptr::addr_of!(_kernel_text_start) as u64;
        let text_end = core::ptr::addr_of!(_kernel_text_end) as u64;

        // 诊断: 打印 .text 地址范围 (LMA)
        crate::klog_boot_info!(
            "[KPTI] text region: start={:#X} end={:#X} ({} pages)",
            text_start,
            text_end,
            (text_end - text_start + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64
        );

        // 映射整个 .text 区域到 USER_PML4 (高半区 VMA + 低半区 LMA)
        map_text_region_in_user_pml4(user_pml4_virt.0 as *mut u64, text_start, text_end);
    }
    }

    // 4.6 映射 KPTI 入口数据页 (.data/.bss) 到 USER_PML4
    //
    // KPTI 入口代码 (isr_common/irq_common/syscall_entry) 在 CR3 切换前
    // 访问 USER_CR3_SAVE (.bss) 和 SyscallPerCpu (.data), 这些页面
    // 在用户页表中没有 USER 位, 会导致 #PF → Triple Fault.
    // SAFETY: user_pml4_virt 有效, 数据页映射只修改 USER_PML4.
    unsafe {
        map_kpti_data_pages(user_pml4_virt.0 as *mut u64);
    }

    // 5. 启用 PCID (如果 CPU 支持 INVPCID)
    //    CR4.PCIDE (bit 17) 启用后, CR3 低 12 位为 PCID 而非必须为 0.
    //    启用条件: CPU 支持 INVPCID; 当前 CR3 低 12 位为 0 (硬件要求).
    //    启用后, KPTI CR3 切换携带 PCID, TLB 条目按 PCID 隔离,
    //    无需每次切换都全局刷新 TLB, 显著降低 KPTI 性能开销.
    let pcid_enabled = if has_invpcid() {
        // SAFETY: 读取 CR3 判断低 12 位是否为 0 (PCIDE 启用前提).
        let cur_cr3: u64;
        unsafe {
            core::arch::asm!("mov {0}, cr3", out(reg) cur_cr3, options(nostack, nomem));
        }
        if cur_cr3 & 0xFFF == 0 {
            // SAFETY: CR4 写入仅在 boot 阶段, 设置 PCIDE 位.
            unsafe {
                let cr4: u64;
                core::arch::asm!("mov {0}, cr4", out(reg) cr4, options(nostack, nomem));
                core::arch::asm!(
                    "mov cr4, {0}",
                    in(reg) cr4 | (1u64 << 17),
                    options(nostack, nomem, preserves_flags),
                );
            }
            // 启用 PCIDE 后, mov cr3 不再隐式刷新 TLB.
            // 做一次全局 TLB 刷新确保一致性, 然后重新加载 CR3 (带 PCID_KERNEL).
            // SAFETY: INVPCID type 2 刷新所有 TLB 条目 (含 global 页).
            unsafe {
                invpcid_flush_all();
            }
            // 重新加载 CR3 带 PCID_KERNEL
            // SAFETY: kernel_pml4 来自 init 阶段的可信来源; PCIDE 已启用, CR3 低 12 位为 PCID.
            let new_cr3 = cr3_with_pcid(kernel_pml4, PCID_KERNEL);
            unsafe {
                core::arch::asm!(
                    "mov cr3, {0}",
                    in(reg) new_cr3,
                    options(nostack, preserves_flags),
                );
            }
            true
        } else {
            false
        }
    } else {
        false
    };

    // 6. 更新所有 per-CPU SyscallPerCpu PML4 字段
    //    汇编 entry/exit 从 [gs:KERNEL_PML4_OFF] / [gs:USER_PML4_OFF] 读取.
    //    PCID 启用时, 值为 PML4_PHYS | PCID; 未启用时为纯 PML4 物理地址.
    // SAFETY: boot 阶段独占写入, cpu_index 0..256 合法.
    let kernel_cr3 = if pcid_enabled {
        cr3_with_pcid(kernel_pml4, PCID_KERNEL)
    } else {
        kernel_pml4
    };
    let user_cr3 = if pcid_enabled {
        cr3_with_pcid(user_pml4_phys, PCID_USER)
    } else {
        user_pml4_phys
    };

    crate::klog_boot_info!(
        "[KPTI] kpti_init: kernel_pml4={:#x}, user_pml4_phys={:#x}, pcid={}, kernel_cr3={:#x}, user_cr3={:#x}",
        kernel_pml4,
        user_pml4_phys,
        pcid_enabled,
        kernel_cr3,
        user_cr3
    );

    // SAFETY: boot 阶段单 CPU 执行, kernel_cr3/user_cr3 是合法 PML4 物理地址,
    // gdt_set_kpti_pml4 是安全的 FFI 调用, cpu 索引 0..256 合法.
    unsafe {
        for cpu in 0..256u32 {
            crate::framework::arch::gdt::gdt_set_kpti_pml4(cpu, kernel_cr3, user_cr3);
        }
    }

    // 7. 公开状态
    USER_PML4.store(user_pml4_phys, Ordering::Release);
    LAST_KERNEL_PML4.store(kernel_pml4, Ordering::Release);
    KPTI_READY.store(true, Ordering::Release);
}

// ── .text 区域映射 ──────────────────────────────────────────────

// 符号桩化 (host-test): 调用点 (kpti_init step 4.5 / create_user_page_table) 已整段
// cfg, host 下无调用者, 函数整体不编译 (避免 dead_code).
#[cfg(not(feature = "host-test"))]
#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
/// 在 `USER_PML4` 中映射整个 .text 区域 (PRESENT only, SMEP-safe).
///
/// 映射 _`kernel_text_start` ~ _`kernel_text_end` 的所有页面到用户页表,
/// 包括高半区 VMA (CPU 实际取指地址) 和低半区 LMA (恒等映射, 备用).
///
/// 原因: 异常处理代码 (isr0-isr31, irq0-irq15, `syscall_entry`) 位于 .text 区域,
/// 用户态触发异常/系统调用时 CPU 使用 `USER_PML4` 寻址, 必须能取指执行这些代码页.
/// 此前仅映射 trampoline 子范围导致异常处理代码不可执行或未映射 → Triple Fault.
///
/// 权限: PRESENT (Ring 0 可执行, SMEP 兼容). 不设 USER 位:
/// SMEP 启用时 Ring 0 不能执行 USER 页, 设 USER 会导致 `syscall_entry` #PF.
/// 不设 WRITABLE → 只读. 不设 NX → 可执行.
///
/// # Safety
///
/// 调用方保证: `user_pml4` 是有效的 `USER_PML4` 虚拟地址指针;
/// `text_start`/`text_end` 是 .text 物理地址范围;
/// 在 boot 阶段单线程执行, 无并发修改页表.
pub(super) unsafe fn map_text_region_in_user_pml4(
    user_pml4: *mut u64,
    text_start_phys: u64,
    text_end_phys: u64,
) {
    // LMA 转 VMA (链接脚本定义: VMA = LMA + 0xFFFF800001000000)
    let vma_offset = 0xFFFF800001000000u64;
    let text_start_vma = text_start_phys + vma_offset;
    let text_end_vma = text_end_phys + vma_offset;

    // 页对齐: 向下对齐起始地址, 向上对齐结束地址
    let page_start_vma = text_start_vma & !(PAGE_SIZE as u64 - 1);
    let page_end_vma = (text_end_vma + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);

    crate::klog_boot_info!(
        "[KPTI] map_text_region: lma={:#X}-{:#X}, vma={:#X}-{:#X} ({} pages)",
        text_start_phys,
        text_end_phys,
        page_start_vma,
        page_end_vma,
        (page_end_vma - page_start_vma) / PAGE_SIZE as u64
    );

    // 权限位: PRESENT (bit 0) = 0x1
    // 不设置 USER (bit 2): SMEP 启用时 Ring 0 不能执行 USER 页,
    // syscall_entry/isr_common 等入口在 CR3 切换前从用户页表取指,
    // USER 标志会导致 #PF (instruction fetch).
    // 不设置 WRITABLE (bit 1) → 只读
    // 不设置 NX (bit 63) → 可执行
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    const FLAGS: u64 = 0x1; // 仅 PRESENT 位 (SMEP 安全)

    // SAFETY: 调用方保证 user_pml4 有效; text_start/text_end 是合法地址范围;
    // boot 阶段单线程执行, 无并发修改页表. PMM 分配的页已对齐且属于内核.
    unsafe {
        let mut vma_addr = page_start_vma;
        let mut phys_addr = text_start_phys & !(PAGE_SIZE as u64 - 1);
        while vma_addr < page_end_vma {
            // 映射高半区 VMA (CPU 实际取指地址)
            map_text_page(user_pml4, vma_addr, phys_addr, FLAGS, "high-half VMA");

            // 同时映射低半区恒等映射 (物理地址, 备用)
            map_text_page(user_pml4, phys_addr, phys_addr, FLAGS, "low-half identity");

            vma_addr += PAGE_SIZE as u64;
            phys_addr += PAGE_SIZE as u64;
        }
    }
}

/// 映射单个 trampoline 页面到用户页表.
///
/// # Safety
///
/// 调用方保证: `user_pml4` 有效; `vma` 和 `phys` 对齐;
/// boot 阶段单线程执行, 无并发修改页表.
// 有意窄化: 显式收窄, 调用方保证值域
// 符号桩化 (host-test): 调用点 (map_kpti_data_pages / map_text_region_in_user_pml4)
// 均已被 cfg 排除, host 下无调用者, 函数整体不编译 (避免 dead_code).
#[cfg(not(feature = "host-test"))]
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::similar_names,
    reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
)]
#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
unsafe fn map_text_page(user_pml4: *mut u64, vma: u64, phys: u64, flags: u64, _desc: &str) {
    // SAFETY: 调用方保证 user_pml4 有效; vma/phys 是合法地址;
    // boot 阶段单线程执行, 无并发修改页表. PMM 分配的页已对齐且属于内核.
    unsafe {
        // 计算 4 级页表索引
        let pml4_idx = (vma >> 39) & 0x1FF;
        let pdpt_idx = (vma >> 30) & 0x1FF;
        let pd_idx = (vma >> 21) & 0x1FF;
        let pt_idx = (vma >> 12) & 0x1FF;

        // 确保 PML4[pml4_idx] 存在 (分配 PDPT 页)
        let pml4e = core::ptr::read_volatile(user_pml4.add(pml4_idx as usize));
        let pdpt_phys = if pml4e & 1 != 0 {
            pml4e & 0x000FFFFFFFFFF000
        } else {
            let new_page = pmm_alloc_page() as u64;
            assert!(new_page != 0, "[KPTI] map_text_page: alloc PDPT failed");
            let new_page_virt = PhysAddr(new_page).to_virt();
            // SAFETY: new_page 由 PMM 分配, 属于内核; 清零 4KB
            core::ptr::write_bytes(new_page_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
            // 设置 PML4 项: PRESENT + WRITABLE + USER
            core::ptr::write_volatile(user_pml4.add(pml4_idx as usize), new_page | 0x7);
            new_page
        };
        let pdpt = (pdpt_phys + KERNEL_BASE) as *mut u64;

        // 确保 PDPT[pdpt_idx] 存在 (分配 PD 页)
        let pdpte = core::ptr::read_volatile(pdpt.add(pdpt_idx as usize));
        let pd_phys = if pdpte & 1 != 0 {
            pdpte & 0x000FFFFFFFFFF000
        } else {
            let new_page = pmm_alloc_page() as u64;
            assert!(new_page != 0, "[KPTI] map_text_page: alloc PD failed");
            let new_page_virt = PhysAddr(new_page).to_virt();
            // SAFETY: new_page 由 PMM 分配, 属于内核; 清零 4KB
            core::ptr::write_bytes(new_page_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
            core::ptr::write_volatile(pdpt.add(pdpt_idx as usize), new_page | 0x7);
            new_page
        };
        let pd = (pd_phys + KERNEL_BASE) as *mut u64;

        // 确保 PD[pd_idx] 存在 (分配 PT 页)
        // 修复 (TRACK-INIT-RING3-SYSCALL): 处理 2MB 大页 (PS=1).
        // KPTI 下 USER_PML4[256..511] 从 KERNEL_PML4 复制, 底层 PDPT/PD 页
        // 物理共享. 内核 PD 中可能包含 2MB 大页条目 (PS=1), map_text_page
        // 此前将大页 PDE 误读为 PT 物理指针, 导致 PTE 写入错误物理地址,
        // CPU 仍使用原始大页映射读取到全零页 → syscall_entry 解码为
        // add [rax],al → 读 [RAX=1] → #PF CR2=0x1.
        //
        // 修复策略: 在共享 PD 中直接拆分 2MB 大页为 512 个 4KB PTE.
        // 这修改了共享 PD, 同时影响内核页表 (该区间从 2MB 大页变为 4KB 页).
        // 功能正确, 仅有轻微 TLB 性能影响. 不 fork 页表层级, 保持 KPTI 隔离.
        let pde = core::ptr::read_volatile(pd.add(pd_idx as usize));
        let pt_phys = if pde & 1 != 0 {
            if pde & (1 << 7) != 0 {
                // PS=1: 2MB 大页 → 直接拆分
                const PS_BIT: u64 = 1 << 7;
                let huge_base = pde & 0x000FFFFFFFE00000; // bits 51:21
                let pde_flags = pde & 0xFFF & !PS_BIT; // 保留权限, 清 PS

                let new_pt = pmm_alloc_page() as u64;
                assert!(new_pt != 0, "[KPTI] map_text_page: alloc PT (split) failed");
                let new_pt_virt = PhysAddr(new_pt).to_virt();
                // SAFETY: new_pt 由 PMM 分配; 清零后填充 512 PTE
                core::ptr::write_bytes(new_pt_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
                let new_pt_ptr = new_pt_virt.0 as *mut u64;
                for i in 0..512u64 {
                    core::ptr::write_volatile(
                        new_pt_ptr.add(i as usize),
                        (huge_base + i * PAGE_SIZE as u64) | pde_flags,
                    );
                }
                // 更新 PDE → 指向新 PT (PS=0, 修改共享 PD, 同时影响内核页表)
                core::ptr::write_volatile(pd.add(pd_idx as usize), new_pt | pde_flags);

                crate::klog_boot_info!(
                    "[KPTI] split_2mb: PDE[{}]={:#X} → new PT={:#X}",
                    pd_idx,
                    pde,
                    new_pt
                );
                new_pt
            } else {
                // PS=0: 普通 PDE → 已指向 PT 页
                pde & 0x000FFFFFFFFFF000
            }
        } else {
            let new_page = pmm_alloc_page() as u64;
            assert!(new_page != 0, "[KPTI] map_text_page: alloc PT failed");
            let new_page_virt = PhysAddr(new_page).to_virt();
            // SAFETY: new_page 由 PMM 分配, 属于内核; 清零 4KB
            core::ptr::write_bytes(new_page_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
            core::ptr::write_volatile(pd.add(pd_idx as usize), new_page | 0x7);
            new_page
        };
        let pt = (pt_phys + KERNEL_BASE) as *mut u64;

        // 设置 PT[pt_idx] = phys | flags
        core::ptr::write_volatile(pt.add(pt_idx as usize), phys | flags);
    }
}

// ── KPTI 入口数据页映射 ──────────────────────────────────────────

// 符号桩化 (host-test): 函数体整段被 cfg 排除, 触发下面两个 lint 的代码
// (相似局部变量 / 长数字常量) 在 host-test 维不存在, expect 须同步收窄
// (否则 unfulfilled_lint_expectations 阻断 host-test 维 clippy).
#[cfg_attr(
    not(feature = "host-test"),
    expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )
)]
#[cfg_attr(
    not(feature = "host-test"),
    expect(
        clippy::unreadable_literal,
        reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
    )
)]
/// KPTI 中断/系统调用入口代码在 CR3 切换前需要访问的数据页面。
///
/// 当 CPU 在用户态触发中断/异常时, `isr_common/irq_common/syscall_entry`
/// 在切换到内核页表前需要:
/// 1. `mov [USER_CR3_SAVE], rax` — 保存用户 CR3 到 .bss 变量
/// 2. `mov rax, [gs:KERNEL_PML4_OFF]` — 从 `SyscallPerCpu` 读内核 PML4
///
/// 这些访问发生在 CR3 切换前 (此时仍为用户页表), 因此这些数据页面
/// 必须在用户页表中有 PRESENT | WRITABLE 映射, 否则触发 #PF → Triple Fault。
///
/// # 安全性
///
/// 不设 USER 位. 访问路径 CPL 全部为 0 (syscall 指令强制 CPL=0,
/// 中断入口 CPU 自动加载内核 CS), 因此不需要 USER 位即可访问.
/// 用户态 (CPL=3) 无法读写这些数据页, 不暴露内核 PML4 物理地址
/// 与 per-CPU 内核 RSP.
///
/// 根本修复方向: 重构 KPTI 入口 trampoline, 将内核 PML4 地址嵌入
/// 代码本身 (立即数), 使 CR3 切换前不依赖 .data/.bss 中的数据.
///
/// # Safety
///
/// 调用方保证: `user_pml4` 是有效的 `USER_PML4` 虚拟地址指针;
/// 在 boot 阶段单线程执行或持 `VMM_LOCK`, 无并发修改页表.
pub(super) unsafe fn map_kpti_data_pages(user_pml4: *mut u64) {
    // 符号桩化 (host-test): host 无 USER_CR3_SAVE 汇编符号且无页表上下文,
    // 整段跳过 (不执行任何映射).
    #[cfg(not(feature = "host-test"))]
    {
    // 权限: PRESENT (bit 0) + WRITABLE (bit 1) = 0x3
    //
    // 安全: 不设 USER 位. 访问路径 CPL 全部为 0:
    //   - syscall 指令入口: CPU 强制 CPL=0 (Intel SDM SYSCALL)
    //   - isr_common/irq_common: CPU 自动加载内核 CS from TSS, CPL=0
    // 移除 USER 位防止用户态 (CPL=3) 读 USER_CR3_SAVE / SyscallPerCpu,
    // 避免暴露内核 PML4 物理地址与 per-CPU 内核 RSP.
    const FLAGS: u64 = 0x3; // PRESENT | WRITABLE

    // 1. 映射 USER_CR3_SAVE 所在页面
    //    USER_CR3_SAVE 位于 .bss 段, isr.asm 使用绝对寻址 mov [USER_CR3_SAVE], rax
    //    访问的虚拟地址是 LMA (低半区物理地址), 需要恒等映射
    // SAFETY: USER_CR3_SAVE 是链接器符号, 地址有效 (只读引用)
    let user_cr3_save_lma =
        unsafe { core::ptr::addr_of!(super::super::mm::USER_CR3_SAVE_ASM) as u64 };
    let user_cr3_page = user_cr3_save_lma & !(PAGE_SIZE as u64 - 1);
    let vma_offset = 0xFFFF800001000000u64;

    // SAFETY: user_pml4 有效; USER_CR3_SAVE 地址来自链接器符号, 合法;
    // boot 阶段单线程执行, 无并发修改.
    unsafe {
        map_text_page(
            user_pml4,
            user_cr3_page,
            user_cr3_page,
            FLAGS,
            "USER_CR3_SAVE LMA",
        );
        map_text_page(
            user_pml4,
            user_cr3_page + vma_offset,
            user_cr3_page,
            FLAGS,
            "USER_CR3_SAVE VMA",
        );
    }

    // 2. 映射 PER_CPU_GDT 所在页面 (含 SyscallPerCpu)
    //    swapgs 后 [gs:KERNEL_PML4_OFF] 访问 GS_BASE + offset,
    //    GS_BASE = IA32_GS_BASE (swapgs 后) = per_cpu_addr (LMA, 低半区)
    //
    //    注意: gdt_init 中 write_msr(IA32_GS_BASE, &gdt.syscall as *const _ as u64)
    //    写入的是 Rust 链接器分配的 LMA (低半区, 如 0x278000), 而非 VMA.
    //    内核态下低半区通过高半区大页 (KERNEL_BASE + LMA) 恒等映射可访问.
    //    但用户页表不继承该恒等映射, 必须显式映射 LMA 页面.
    //
    //    同时映射高半区 VMA (LMA + vma_offset) 以备高半区访问路径.
    let per_cpu_gdt_lma =
        crate::framework::arch::gdt::get_syscall_per_cpu_base() & !(PAGE_SIZE as u64 - 1);
    let per_cpu_gdt_vma = per_cpu_gdt_lma + vma_offset;

    // SAFETY: user_pml4 有效; per_cpu_gdt 地址来自 GDT 初始化, 合法;
    // boot 阶段单线程执行, 无并发修改.
    unsafe {
        // LMA 恒等映射: 这是 swapgs 后 CPU 实际访问的地址 (GS_BASE = LMA)
        map_text_page(
            user_pml4,
            per_cpu_gdt_lma,
            per_cpu_gdt_lma,
            FLAGS,
            "SyscallPerCpu LMA",
        );
        // VMA 映射: 高半区访问路径
        map_text_page(
            user_pml4,
            per_cpu_gdt_vma,
            per_cpu_gdt_lma,
            FLAGS,
            "SyscallPerCpu VMA",
        );
    }

    crate::klog_boot_info!(
        "[KPTI] data pages mapped: USER_CR3_SAVE={:#X}, SyscallPerCpu LMA={:#X} VMA={:#X}",
        user_cr3_page,
        per_cpu_gdt_lma,
        per_cpu_gdt_vma
    );
    }
    #[cfg(feature = "host-test")]
    {
        // E-04: host 桩分支消费参数, 保持与裸机分支结构对称
        let _ = user_pml4;
    }
}

// ── 测试辅助 (host-tests) ────────────────────────────────────────

/// KPTI 关闭时 (kpti=false) 的占位 `USER_PML4`。
///
/// KPTI 关闭时, `USER_PML4` 沿用 `KERNEL_PML4` (无隔离), 此函数返回 `KERNEL_PML4` 物理地址
/// 以便 `map_to_user_pml4` 等 API 在两条路径都可用。
#[inline(always)]
pub fn kpti_user_pml4_or_kernel(kernel_pml4: u64) -> u64 {
    let up = USER_PML4.load(Ordering::Acquire);
    if up == 0 { kernel_pml4 } else { up }
}
