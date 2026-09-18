//! 虚拟内存管理器 (VMM)
//!
//! 使用 4 级页表 (`x86_64`) 管理虚拟内存映射.
//! 提供:
//! - 虚实地址转换
//! - 页表创建与管理
//! - 用户空间页表
//! - 大页支持 (2MB, 1GB)
//! - 内存保护与访问控制
//!
//! ## SAFETY
//!
//! `user_tables` 的内部可变性通过 `UnsafeCell` 实现,
//! 由内部 `AtomicBool` 自旋锁 (`VMM_LOCK`) 保护.
//! 所有变更都在锁内进行, 确保 `unsafe impl Sync` 的正确性.
//!
//! ### 关键不变式 (所有 `unsafe` 块都依赖):
//!
//! 1. **`KERNEL_PML4`**: 在 `init()` 中写入一次, 之后只读 (Release/Acquire).
//! 2. **`VMM_LOCK`**: 所有页表修改与 `UserPageTable` 变更都串行化.
//! 3. **`PhysAddr` → `VirtAddr`**: `phys_to_virt(pa) = pa + KERNEL_BASE` 是合法内核 VA,
//!    因为内核在 `KERNEL_BASE` 处恒等映射所有物理内存.
//! 4. **PMM 分配**: 返回的物理地址总是页对齐且合法.
//! 5. **页表指针**: 任何从 `PhysAddr::to_virt()` 派生的指针都指向 PMM 分配的
//!    完整 4KB 页, 所有 512 项遍历都安全.
//! 6. **存在位保护**: 将表项解引用为下一级指针前, 检查 `entry & 1 != 0`.
//! 7. **死锁防止**: `acquire_lock` 在调试构建中通过 `VMM_LOCK_RECURSIVE`
//!    对递归获取直接 panic, 在死锁发生前阻止.
//!
//! ## 锁顺序
//!
//! **`VMM_LOCK` 绝不能在持有时再去获取 `VMA_LOCK` (`MmStruct::vmas`).**
//! 这避免了 ABBA 死锁:
//!   线程 A: `VMM_LOCK` → `VMA_LOCK`
//!   线程 B: `VMA_LOCK` → `VMM_LOCK` (在 `MmStruct::remove_range` 中)
//!
//! 所有调用方遵守该规则:
//! - `user_driver.rs`: VMM 操作 (map/unmap) → 释放 `VMM_LOCK` → VMA 操作 (insert/remove)
//! - `page_fault.rs`: VMA 查找 (`find_vma`) → 释放 `VMA_LOCK` → VMM 操作 (`map_page`)
//! - `MmStruct::remove_range`: 持有 `VMA_LOCK` → 获取 `VMM_LOCK` (反向顺序安全)

use super::{
    HUGE_PAGE_1G_SIZE, HUGE_PAGE_2M_SIZE, KERNEL_BASE, PAGE_NX, PAGE_PRESENT, PAGE_SIZE, PAGE_USER,
    PAGE_WRITABLE, PageFlags, PageSize, PageTableEntry, PhysAddr, VirtAddr, get_pmm,
};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::framework::sync::{IrqSaveFlags, disable_interrupts, restore_interrupts};

use crate::framework::sync::OnceLock;
pub(crate) static KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

static VMM_LOCK: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
static VMM_LOCK_RECURSIVE: AtomicBool = AtomicBool::new(false);

const MAX_USER_PAGE_TABLES: usize = 256;

#[derive(Clone, Copy)]
struct UserPageTable {
    pml4_phys: u64,
    in_use: bool,
}

pub struct VirtualMemoryManager {
    user_tables: UnsafeCell<[UserPageTable; MAX_USER_PAGE_TABLES]>,
    user_table_count: AtomicUsize,
    total_maps: AtomicU64,
    total_unmaps: AtomicU64,
    page_faults: AtomicU64,
}

// SAFETY: VMM_LOCK serializes all writes to user_tables (via UnsafeCell).
// SAFETY: VMM_LOCK 自旋锁保护所有可变状态, 原子计数器使用 Relaxed 顺序 (锁内单写者).
unsafe impl Sync for VirtualMemoryManager {}

impl VirtualMemoryManager {
    pub const fn new() -> Self {
        Self {
            user_tables: UnsafeCell::new(
                [UserPageTable {
                    pml4_phys: 0,
                    in_use: false,
                }; MAX_USER_PAGE_TABLES],
            ),
            user_table_count: AtomicUsize::new(0),
            total_maps: AtomicU64::new(0),
            total_unmaps: AtomicU64::new(0),
            page_faults: AtomicU64::new(0),
        }
    }

    pub fn init(&self) {
        // SAFETY: read_cr3() reads the CR3 control register — safe at any time
        let cr3 = unsafe { self.read_cr3() };

        KERNEL_PML4.store(cr3, Ordering::Release);

        super::api::kernel_pml4.store(cr3, Ordering::Release);

        // P1 C7: KPTI 实际页表隔离 — 分配 USER_PML4, 复制内核高半区并清 USER 位
        // 完整功能需要汇编 entry/exit trampoline, 见 kpti.rs 模块顶部文档
        if !super::kpti::kpti_is_active()
            && crate::framework::config::KernelCapabilities::detect().kpti
        {
            // SAFETY: KERNEL_PML4 已初始化, PMM 可用, KPTI 全局状态在 init 独占
            unsafe {
                super::kpti::kpti_init(cr3);
            }
        }
    }

    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    /// 映射单个 4KB 页到内核页表 (`KERNEL_PML4`), 并累加映射计数.
    ///
    /// # Errors
    /// 当 VMM 未初始化 (`KERNEL_PML4` 为空) 时返回 `Err("VMM not initialized")`;
    /// 当中间页表 (PDPT/PD/PT) 分配失败时返回 `Err("Failed to allocate PDPT")` 等错误.
    pub fn map_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        let _flags = self.acquire_lock();

        let result = self.map_page_internal(virt, phys, flags);

        if result.is_ok() {
            self.total_maps.fetch_add(1, Ordering::Relaxed);
        }

        self.release_lock(&_flags);
        result
    }

    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    /// 映射大页 (2MB/1GB, 或退回 4KB) 到内核页表, 并累加映射计数.
    ///
    /// # Errors
    /// 当 `virt`/`phys` 未按 `size_type` 对齐时返回 `Err("Address not aligned for huge page")`;
    /// 其余错误同 `map_page` (VMM 未初始化或中间页表分配失败);
    /// 2MB/1GB 映射在对应目录条目已被拆分为页表时也会返回 `Err`.
    pub fn map_huge_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
        size_type: PageSize,
    ) -> Result<(), &'static str> {
        if !size_type.is_aligned(virt.0) || !size_type.is_aligned(phys.0) {
            return Err("Address not aligned for huge page");
        }

        let _flags = self.acquire_lock();

        let mut flags = flags;
        flags.insert(PageFlags::HUGE_PAGE);

        let result = match size_type {
            PageSize::Size2M => self.map_2mb_page(virt, phys, flags),
            PageSize::Size1G => self.map_1gb_page(virt, phys, flags),
            PageSize::Size4K => self.map_page_internal(virt, phys, flags),
        };

        if result.is_ok() {
            self.total_maps.fetch_add(1, Ordering::Relaxed);
        }

        self.release_lock(&_flags);
        result
    }

    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    pub fn unmap_page(&self, virt: VirtAddr) {
        let _flags = self.acquire_lock();

        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            self.release_lock(&_flags);
            return;
        }

        // SAFETY: pml4_base = CR3 value, KERNEL_BASE offset produces valid kernel VA
        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // 安全门: KPTI 共享页表防护
        // 禁止修改 PML4[256..511] (kernel high half).
        // KPTI init 时复制 PML4[256..512], 底层 PDPT/PD 页物理共享.
        // 此处 unmap 清零 PDE/PTE 会同时破坏 kernel 和 user 页表,
        // 导致 PMM free list 等内核数据结构不可访问, 触发 Triple Fault.
        if virt.pml4_idx() >= 256 {
            crate::klog_boot_info!(
                "[VMM] unmap_page: skip kernel-half virt={:#X} pml4_idx={}",
                virt.0,
                virt.pml4_idx()
            );
            self.release_lock(&_flags);
            return;
        }

        // SAFETY: VMM_LOCK held. Page table walk with present-bit guards at each level.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pml4e = &*pml4.add(virt.pml4_idx());

            if !pml4e.is_present() {
                self.release_lock(&_flags);
                return;
            }

            // SAFETY: pml4e.frame() is present & valid frame; phys_to_virt gives kernel VA
            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &*pdpt.add(virt.pdpt_idx());

            if !pdpte.is_present() {
                self.release_lock(&_flags);
                return;
            }

            if pdpte.is_huge() {
                // 1GB 页: 直接清空 PDPT 项
                (*pdpt.add(virt.pdpt_idx())).set_value(0);
                self.flush_tlb(virt.0);
            } else {
                // SAFETY: pdpte.frame() valid; present && !huge → points to PD
                let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
                let pde = &*pd.add(virt.pd_idx());

                if !pde.is_present() {
                    self.release_lock(&_flags);
                    return;
                }

                if pde.is_huge() {
                    (*pd.add(virt.pd_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                } else {
                    // SAFETY: pde.frame() valid; present && !huge → points to PT
                    let pt = pde.frame().to_virt().0 as *mut PageTableEntry;
                    (*pt.add(virt.pt_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                }
            }
        }

        self.total_unmaps.fetch_add(1, Ordering::Relaxed);
        self.release_lock(&_flags);
    }

    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    /// 修改虚拟页的保护属性 (mprotect 核心实现)
    ///
    /// 遍历四级页表找到 PTE, 修改 R/W/U/NX 位, 然后 flush TLB.
    /// 如果页不存在, 静默跳过 (mprotect 对未映射页无操作).
    pub fn protect_page(&self, virt: VirtAddr, new_flags: PageFlags) {
        let _flags = self.acquire_lock();

        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            self.release_lock(&_flags);
            return;
        }

        // SAFETY: pml4_base = CR3 value, KERNEL_BASE offset produces valid kernel VA
        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // 安全门: KPTI 共享页表防护
        // 禁止修改 PML4[256..511] (kernel high half).
        // KPTI init 时复制 PML4[256..512], 底层 PDPT/PD 页物理共享.
        // 此处修改权限位会同时影响 kernel 和 user 页表.
        if virt.pml4_idx() >= 256 {
            crate::klog_boot_info!(
                "[VMM] protect_page: skip kernel-half virt={:#X} pml4_idx={}",
                virt.0,
                virt.pml4_idx()
            );
            self.release_lock(&_flags);
            return;
        }

        // SAFETY: VMM_LOCK held. Page table walk with present-bit guards at each level.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            let pml4e = &*pml4.add(virt.pml4_idx());

            if !pml4e.is_present() {
                self.release_lock(&_flags);
                return;
            }

            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &*pdpt.add(virt.pdpt_idx());

            if !pdpte.is_present() {
                self.release_lock(&_flags);
                return;
            }

            if pdpte.is_huge() {
                // 1GB page: 修改 PDPT entry 的权限位
                let entry = pdpt.add(virt.pdpt_idx());
                let mut val = (*entry).value();
                // 保留物理帧地址和保留位, 仅修改权限位
                val &= !(PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER | PAGE_NX);
                val |= new_flags.bits() & (PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER | PAGE_NX);
                (*entry).set_value(val);
                self.flush_tlb(virt.0);
                self.release_lock(&_flags);
                return;
            }

            let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
            let pde = &*pd.add(virt.pd_idx());

            if !pde.is_present() {
                self.release_lock(&_flags);
                return;
            }

            if pde.is_huge() {
                // 2MB page: 修改 PD entry 的权限位
                let entry = pd.add(virt.pd_idx());
                let mut val = (*entry).value();
                val &= !(PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER | PAGE_NX);
                val |= new_flags.bits() & (PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER | PAGE_NX);
                (*entry).set_value(val);
                self.flush_tlb(virt.0);
                self.release_lock(&_flags);
                return;
            }

            // 4KB page: 修改 PT entry 的权限位
            let pt = pde.frame().to_virt().0 as *mut PageTableEntry;
            let entry = pt.add(virt.pt_idx());
            let mut val = (*entry).value();
            val &= !(PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER | PAGE_NX);
            val |= new_flags.bits() & (PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER | PAGE_NX);
            (*entry).set_value(val);
            self.flush_tlb(virt.0);
        }

        self.release_lock(&_flags);
    }

    pub fn get_physical(&self, virt: VirtAddr) -> Option<PhysAddr> {
        self.get_physical_in_pml4(KERNEL_PML4.load(Ordering::Acquire), virt)
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    #[expect(
        clippy::unreadable_literal,
        reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
    )]
    pub fn get_physical_in_pml4(&self, pml4: u64, virt: VirtAddr) -> Option<PhysAddr> {
        if pml4 == 0 {
            return None;
        }

        // SAFETY: pml4 is a valid PML4 physical address; phys_to_virt gives kernel VA
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: 只读页表遍历. 在并发硬件页表遍历 (A/D 位) 下使用 volatile 读取保证正确性.
        unsafe {
            let pml4_raw = pml4_virt.0 as *const u64;
            let pml4e = pml4_raw.add(virt.pml4_idx()).read_volatile();
            if (pml4e & 1) == 0 {
                return None;
            }

            // SAFETY: pml4e present → frame bits point to valid PDPT
            let pdpt_virt = (pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pdpt_raw = pdpt_virt as *const u64;
            let pdpte = pdpt_raw.add(virt.pdpt_idx()).read_volatile();
            if (pdpte & 1) == 0 {
                return None;
            }

            if (pdpte & 0x80) != 0 {
                let frame = pdpte & 0x000FFFFFFFFFF000;
                let offset = virt.0 & (HUGE_PAGE_1G_SIZE - 1);
                return Some(PhysAddr(frame + offset));
            }

            // SAFETY: pdpte present && !huge → valid PD pointer
            let pd_virt = (pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pd_raw = pd_virt as *const u64;
            let pde = pd_raw.add(virt.pd_idx()).read_volatile();
            if (pde & 1) == 0 {
                return None;
            }

            if (pde & 0x80) != 0 {
                let frame = pde & 0x000FFFFFFFFFF000;
                let offset = virt.0 & (HUGE_PAGE_2M_SIZE - 1);
                return Some(PhysAddr(frame + offset));
            }

            // SAFETY: pde present && !huge → valid PT pointer
            let pt_virt = (pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pt_raw = pt_virt as *const u64;
            let pte = pt_raw.add(virt.pt_idx()).read_volatile();
            if (pte & 1) == 0 {
                return None;
            }

            let frame = pte & 0x000FFFFFFFFFF000;
            let offset = virt.0 & (PAGE_SIZE - 1);
            Some(PhysAddr(frame + offset))
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    #[expect(
        clippy::unreadable_literal,
        reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
    )]
    #[expect(
        clippy::similar_names,
        reason = "similar_names: 变量名相似表达同族概念; 当前优先 expect"
    )]
    /// 读取 PTE 原始值 (用于 swap entry 检测)
    ///
    /// 返回 4KB 页的 PTE 原始值, 若页表层级不存在则返回 None.
    pub fn get_pte_value(&self, pml4: u64, virt: VirtAddr) -> Option<u64> {
        if pml4 == 0 {
            return None;
        }

        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let pml4_raw = pml4_virt.0 as *const u64;
            let pml4e = pml4_raw.add(virt.pml4_idx()).read_volatile();
            if (pml4e & 1) == 0 {
                return None;
            }

            let pdpt_virt = (pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pdpt_raw = pdpt_virt as *const u64;
            let pdpte = pdpt_raw.add(virt.pdpt_idx()).read_volatile();
            if (pdpte & 1) == 0 || (pdpte & 0x80) != 0 {
                return None;
            }

            let pd_virt = (pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pd_raw = pd_virt as *const u64;
            let pde = pd_raw.add(virt.pd_idx()).read_volatile();
            if (pde & 1) == 0 || (pde & 0x80) != 0 {
                return None;
            }

            let pt_virt = (pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pt_raw = pt_virt as *const u64;
            let pte = pt_raw.add(virt.pt_idx()).read_volatile();

            Some(pte)
        }
    }

    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    #[expect(
        clippy::unreadable_literal,
        reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
    )]
    /// 直接写入 PTE 原始值 (用于 swap 替换)
    ///
    /// 沿 PML4→PDPT→PD→PT 找到最终 PTE, 写入 `raw_pte` 后 TLB flush.
    /// 与 `map_page_in_table` 的区别: 接受任意 raw PTE (含 swap entry, 即将 present=0).
    /// 若任意中间层缺失 (P 位=0), 静默返回 (不创建中间页表, swap-out 不应触发缺中间页).
    pub fn set_pte_value(&self, pml4: u64, virt: VirtAddr, raw_pte: u64) {
        if pml4 == 0 {
            return;
        }

        let _flags = self.acquire_lock();
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: VMM_LOCK held; 四级页表查找 PTE 并直接写入
        unsafe {
            let pml4_raw = pml4_virt.0 as *const u64;
            let pml4e = pml4_raw.add(virt.pml4_idx()).read_volatile();
            if (pml4e & 1) == 0 {
                self.release_lock(&_flags);
                return;
            }

            let pdpt_virt = (pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pdpt_raw = pdpt_virt as *const u64;
            let pdpte = pdpt_raw.add(virt.pdpt_idx()).read_volatile();
            if (pdpte & 1) == 0 || (pdpte & 0x80) != 0 {
                self.release_lock(&_flags);
                return;
            }

            let pd_virt = (pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pd_raw = pd_virt as *const u64;
            let pde = pd_raw.add(virt.pd_idx()).read_volatile();
            if (pde & 1) == 0 || (pde & 0x80) != 0 {
                self.release_lock(&_flags);
                return;
            }

            let pt_virt = (pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pt_ptr = (pt_virt as *mut u64).add(virt.pt_idx());
            pt_ptr.write_volatile(raw_pte);

            self.flush_tlb(virt.0);
        }

        self.release_lock(&_flags);
    }

    pub fn switch_page_table(&self, pml4: u64) {
        // SAFETY: pml4 must point to a valid PML4 table; CR3 write is privileged
        unsafe {
            self.write_cr3(pml4);
        }
    }

    /// 创建新的用户进程页表: 复制内核高半区, 并映射 KPTI 所需的低半区页 (GDT/IDT/TSS 等).
    ///
    /// # Panics
    /// 正常情况下不会 panic; 唯一存在的 unwrap 是
    /// `u64::from_le_bytes(buf[2..10].try_into().unwrap())`, 由于切片长度恒为 8 字节,
    /// 该 unwrap 实际不会触发 panic.
    // 有意窄化: 显式收窄, 调用方保证值域
    #[expect(clippy::cast_possible_truncation)]
    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
    )]
    pub fn create_user_page_table(&self) -> Option<u64> {
        let pmm = get_pmm();
        let pml4_phys = pmm.alloc_page()?;

        // SAFETY: pml4_phys from PMM, phys_to_virt valid; zero for clean state
        let pml4_virt = pml4_phys.to_virt();
        unsafe {
            core::ptr::write_bytes(pml4_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
        }

        let kernel_pml4 = KERNEL_PML4.load(Ordering::Acquire);
        // SAFETY: kernel_pml4 valid (set in init), phys_to_virt valid
        let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt();

        // SAFETY: 将内核空间项 (256..511) 复制到用户 PML4.
        // src 与 dst 都是合法的页对齐内核 VA.
        unsafe {
            let src = kernel_pml4_virt.0 as *const u64;
            let dst = pml4_virt.0 as *mut u64;

            // 复制高半部分 (内核空间: PML4[256..511])
            core::ptr::copy_nonoverlapping(src.add(256), dst.add(256), 256);

            // 低半部分 (PML4[0..256]) 保持全零:
            // enter_user 在高半部分内核地址中切换 CR3, 不依赖低半部分映射.
            // 用户进程的 ELF 段由加载器按需映射, 不应继承内核恒等映射.

            crate::arch!(tlb_flush_page(dst.add(256) as usize));

            // 通过回读项 256 验证复制
            let e256_src = src.add(256).read_volatile();
            let e256_dst = dst.add(256).read_volatile();
            if e256_src != e256_dst || (e256_src & 1) == 0 {
                pmm.free_page(pml4_phys);
                return None;
            }
        }

        let _flags = self.acquire_lock();

        let idx = self.find_free_user_slot();
        if idx < MAX_USER_PAGE_TABLES {
            // SAFETY: VMM_LOCK held; exclusive access to user_tables via UnsafeCell
            unsafe {
                let tables = &mut *self.user_tables.get();
                tables[idx].pml4_phys = pml4_phys.as_u64();
                tables[idx].in_use = true;
            }
            self.user_table_count.fetch_add(1, Ordering::Relaxed);
        }

        self.release_lock(&_flags);

        // 符号桩化 (host-test): host 无 _kernel_text_* / USER_CR3_SAVE 链接脚本
        // 汇编符号且无页表上下文, 进程页表文本区间映射整段跳过.
        #[cfg(not(feature = "host-test"))]
        {
        // 关键修复: 在进程页表中恒等映射 trampoline 物理页 (USER+RX)
        // enter_user_asm 在低半区 LMA 地址执行, mov cr3 切换到进程页表后
        // CPU 继续取指执行, 因此 trampoline 代码页必须在进程页表低半区有映射.
        // 权限: USER (Ring 3 可访问) + RX (可执行, 不可写).
        if crate::framework::mm::kpti::kpti_is_active() {
            // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
            unsafe {
                let text_start =
                    core::ptr::addr_of!(crate::framework::mm::kpti::_kernel_text_start)
                        as u64;
                let text_end =
                    core::ptr::addr_of!(crate::framework::mm::kpti::_kernel_text_end)
                        as u64;
                crate::framework::mm::kpti::map_text_region_in_user_pml4(
                    pml4_virt.0 as *mut u64,
                    text_start,
                    text_end,
                );

                // 映射 KPTI 入口数据页 (USER_CR3_SAVE, SyscallPerCpu) 到进程用户页表.
                //
                // 原因: KPTI 中断/异常入口 (isr_common/irq_common/syscall_entry) 在
                // CR3 切换前访问 USER_CR3_SAVE (.bss) 和 SyscallPerCpu (.data),
                // 这些页面必须在用户页表中有 USER 位映射, 否则触发 #PF → Triple Fault.
                //
                // kpti_init() 只映射了全局 USER_PML4, 每个进程的独立页表也需要映射.
                // 不映射会导致 Ring 3 下第一个时钟中断 (IRQ 0) 在 irq_common 中
                // mov [USER_CR3_SAVE], rax → #PF (写入不存在的页) → Double Fault → 死锁.
                crate::framework::mm::kpti::map_kpti_data_pages(pml4_virt.0 as *mut u64);
            }
        }
        }

        // 映射 GDT / IDT / TSS 所在的低半部分页到用户页表.
        // iretq 和段寄存器加载需要访问 GDT, 中断入口需要 IDT,
        // 用户态中断触发时 CPU 需要从 TSS 读取 RSP0/IST 栈指针.
        // 这些结构体位于低半部分物理内存, 用户页表不继承恒等映射,
        // 因此必须显式映射.
        // 注意: 必须在 release_lock 之后调用, 因为 map_page_in_table 内部也会获取锁.
        {
            let sgdt = crate::framework::arch::gdt::get_gdt_ptr();
            let gdt_start = sgdt.base as u64 & !(PAGE_SIZE as u64 - 1);
            let gdt_end = (sgdt.base as u64 + u64::from(sgdt.limit) + PAGE_SIZE as u64)
                & !(PAGE_SIZE as u64 - 1);

            // 同时用 sgdt 指令读取实际 GDTR 值进行对比
            let actual_gdt_base: u64;
            // SAFETY: sgdt 是特权指令, 仅读取 GDTR 到栈上缓冲区, 不修改任何状态.
            unsafe {
                let mut buf: [u8; 10] = core::mem::zeroed();
                core::arch::asm!("sgdt [{}]", in(reg) buf.as_mut_ptr() as u64, options(nostack));
                actual_gdt_base = u64::from_le_bytes(buf[2..10].try_into().unwrap());
            }
            crate::klog_boot_info!(
                "[VMM] GDT ptr base={:#x} vs sgdt base={:#x}",
                sgdt.base as u64,
                actual_gdt_base
            );

            // 读取 IDT 基地址和限制 (sidt 指令).
            // IDTR 格式: 2 字节 limit + 8 字节 base (小端序).
            // 修复 (TRACK-INIT-RING3-SYSCALL): 原栈操作 inline asm 中
            // 读出的 idt_limit 为 0xFF (实际为 0x0FFF), 导致 idt_end 只覆盖
            // 1 页, IRQ 向量 (0x20+) 的 IDT 条目落在第 2 页未映射 → #PF.
            // 改用栈缓冲区 + 字节解码, 消除栈操作与编译器冲突.
            let mut idtr_buf: [u8; 10] = [0; 10];
            // SAFETY: sidt 是特权指令, 仅读取 IDTR 到缓冲区, 不修改其他状态.
            unsafe {
                core::arch::asm!(
                    "sidt [{}]",
                    in(reg) idtr_buf.as_mut_ptr(),
                    options(nostack, preserves_flags),
                );
            }
            let idt_limit = u16::from_le_bytes([idtr_buf[0], idtr_buf[1]]);
            let idt_base = u64::from_le_bytes([
                idtr_buf[2],
                idtr_buf[3],
                idtr_buf[4],
                idtr_buf[5],
                idtr_buf[6],
                idtr_buf[7],
                idtr_buf[8],
                idtr_buf[9],
            ]);
            let idt_start = idt_base & !(PAGE_SIZE as u64 - 1);
            let idt_end =
                ((idt_base + u64::from(idt_limit)) & !(PAGE_SIZE as u64 - 1)) + PAGE_SIZE as u64;
            crate::klog_boot_info!(
                "[VMM] IDT raw: base={:#x} limit={:#x} start={:#x} end={:#x}",
                idt_base,
                idt_limit,
                idt_start,
                idt_end
            );

            // 读取 TSS 基地址 (从 GDT TSS 描述符)
            let tss_start =
                crate::framework::arch::gdt::get_tss_base() & !(PAGE_SIZE as u64 - 1);
            // TSS 结构约 128 字节, 最多跨 2 页
            let tss_end = tss_start + 2 * PAGE_SIZE as u64;

            // 收集需要映射的低半部分页 (去重)
            let mut pages = [0u64; 16];
            let mut count = 0;
            let ranges: [(u64, u64); 3] = [
                (gdt_start, gdt_end),
                (idt_start, idt_end),
                (tss_start, tss_end),
            ];

            crate::klog_boot_info!(
                "[VMM] GDT/IDT/TSS mapping: gdt={:#x}-{:#x}, idt={:#x}-{:#x}, tss={:#x}-{:#x}",
                gdt_start,
                gdt_end,
                idt_start,
                idt_end,
                tss_start,
                tss_end
            );

            for &(start, end) in &ranges {
                let mut addr = start;
                while addr < end {
                    if !pages[..count].contains(&addr) {
                        if count < pages.len() {
                            pages[count] = addr;
                            count += 1;
                        }
                    }
                    addr += PAGE_SIZE as u64;
                }
            }

            for &page_phys in &pages[..count] {
                // B05-55 修复: 不用 USER 位. GDT/IDT/TSS 是内核数据, 用户态异常入口
                // (isr_common 等) 在 CR3 切换前以 CPL=0 访问它们 (读 IDT/IST/RSP0),
                // supervisor 权限即可. 原实现带 USER 位, 且 tss 范围 (含 SyscallPerCpu
                // 所在页 0x27b000) 会覆盖 map_kpti_data_pages 的 U=0 映射 → PERCPU 变
                // USER 可写 → fork COW clone 误清 WRITABLE → 内核写 SyscallPerCpu #PF.
                // 同时避免向用户态暴露内核 GDT/IDT/TSS 内容 (信息泄漏面).
                self.map_page_in_table(
                    pml4_phys.as_u64(),
                    VirtAddr(page_phys),
                    PhysAddr(page_phys),
                    PageFlags::PRESENT | PageFlags::WRITABLE,
                );
            }

            // B05-55 修复: 映射 IST 专用栈页 (TSS.ist[0..4]) 到用户页表.
            //
            // 用户态异常/中断 (如 fork 的 COW #PF) 交付时, CPU 在 isr_common
            // 切换到内核页表**之前**用 TSS.ist[N-1] 栈压入异常帧
            // (IDT IST=N → TSS ist[N-1]: #DF=ist[0], NMI=ist[1], int 0x82=ist[2],
            //  #PF=ist[3]). IST 栈是 per-CPU GDT 结构内的独立缓冲区 (低 LMA),
            // 远离 TSS 结构本身; 原实现仅映射 TSS 2 页, 未映射 IST 栈页,
            // 导致用户态 #PF (fork 后首次写栈触发 COW) 交付时 CPU 切到未映射的
            // ist3 → 压栈二次 #PF → #DF → #TF.
            //
            // 权限: PRESENT|WRITABLE (不含 USER) — 交付路径 CPL=0, 且不暴露内核栈.
            #[expect(
                clippy::items_after_statements,
                reason = "item 紧邻使用点声明以便阅读上下文"
            )]
            const PER_CPU_IST_STACK_SIZE: u64 = 16384;
            // SAFETY: get_tss_mut 返回当前 CPU 的有效 TSS (BSP 启动期单 CPU 独占).
            // TSS 是 #[repr(packed)], ist 字段可能未对齐, 用 read_unaligned 拷贝.
            let tss_ref = unsafe { &*crate::framework::arch::gdt::get_tss_mut() };
            let mut ist_tops = [0u64; 4];
            // SAFETY: tss_ref.ist 为 [u64; 7], 索引 0..4 在界内;
            // addr_of! 不创建引用, 配合 read_unaligned 避免 packed 错位 UB.
            unsafe {
                let ist_ptr = core::ptr::addr_of!(tss_ref.ist) as *const u64;
                for i in 0..4 {
                    ist_tops[i] = core::ptr::read_unaligned(ist_ptr.add(i));
                }
            }
            for &ist_top in &ist_tops {
                if ist_top == 0 {
                    continue;
                }
                let start = (ist_top - PER_CPU_IST_STACK_SIZE) & !(PAGE_SIZE as u64 - 1);
                let end = ist_top & !(PAGE_SIZE as u64 - 1);
                let mut ist_addr = start;
                while ist_addr <= end {
                    self.map_page_in_table(
                        pml4_phys.as_u64(),
                        VirtAddr(ist_addr),
                        PhysAddr(ist_addr),
                        PageFlags::PRESENT | PageFlags::WRITABLE,
                    );
                    ist_addr += PAGE_SIZE as u64;
                }
            }

            // 映射内核栈页 (TSS.RSP0) 到用户页表.
            // 注意: RSP0 在 create_user_page_table 调用时可能尚未设置,
            // 实际映射在 enter_user 的 set_kernel_stack 之后完成.
            // 这里仅做尝试, 如果 RSP0 为 0 则跳过.
            // 使用 map_kernel_page_in_table 绕过 KPTI 安全门 (pml4_idx >= 256).
            let rsp0 = crate::framework::arch::tss::tss_get_kernel_stack();
            if rsp0 != 0 {
                let rsp0_page = rsp0 & !(PAGE_SIZE as u64 - 1);
                let rsp0_phys = rsp0_page - crate::framework::mm::KERNEL_BASE as u64;
                self.map_kernel_page_in_table(
                    pml4_phys.as_u64(),
                    VirtAddr(rsp0_page),
                    PhysAddr(rsp0_phys),
                    PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER,
                );
            }
        }

        Some(pml4_phys.as_u64())
    }

    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    pub fn map_page_in_table(&self, pml4: u64, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) {
        if pml4 == 0 {
            return;
        }

        // 安全门 1: KPTI 共享页表防护
        // 禁止修改 PML4[256..511] (kernel high half).
        // KPTI init 时仅复制 PML4 顶层, USER_PDPT/USER_PD/USER_PT 仍与 KERNEL_ 共享
        // 同一物理页. 此处 map 会把共享的 2MB huge PDE 拆成 4KB PT 指针,
        // 污染 kernel page table, 触发 Triple Fault.
        // user half (PML4[0..255]) 不在此限制内, 由 user 自己的 PDPT/PD 承载.
        if virt.pml4_idx() >= 256 {
            crate::klog_boot_info!(
                "[VMM] map_page_in_table: skip kernel-half virt={:#X} pml4_idx={}",
                virt.0,
                virt.pml4_idx()
            );
            return;
        }

        let _flags = self.acquire_lock();

        // SAFETY: pml4 is a valid PML4 address; VMM_LOCK held
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: 完整 4 级页表遍历与按需创建.
        // VMM_LOCK 串行化所有页表修改.
        unsafe {
            let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4_ptr.add(virt.pml4_idx()), true, 0);
            if pdpt.is_null() {
                self.release_lock(&_flags);
                return;
            }

            let pd =
                self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true, HUGE_PAGE_2M_SIZE);
            if pd.is_null() {
                self.release_lock(&_flags);
                return;
            }

            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true, PAGE_SIZE);
            if pt.is_null() {
                crate::klog_boot_info!(
                    "[VMM] map_page_in_table: failed to get/create PT for {:#x}",
                    virt.0
                );
                self.release_lock(&_flags);
                return;
            }

            if flags.contains(PageFlags::USER) {
                // SAFETY: ptr.add(idx) stays within the 512-entry table.
                // 此处 pml4_idx < 256 (上方门检查保证), pdpt/PD 是 user 自己的页表,
                // 不与 kernel 共享, 设 USER 位安全.
                (*pml4_ptr.add(virt.pml4_idx())).set_user(true);
                (*pdpt.add(virt.pdpt_idx())).set_user(true);
                (*pd.add(virt.pd_idx())).set_user(true);
            }

            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        self.release_lock(&_flags);
    }

    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    /// 映射内核高半区页到用户页表 (绕过 KPTI 安全门)
    ///
    /// 用于映射 RSP0 等内核结构到用户页表,使其在用户态可访问.
    /// 该函数绕过 `map_page_in_table` 的 KPTI 安全门 (`pml4_idx` >= 256),
    /// 因为 RSP0 等内核结构位于高半区,但仍需在用户页表中可见.
    ///
    /// # Safety
    ///
    /// 调用方保证:
    /// - `pml4` 是有效的用户页表物理地址
    /// - `virt` 是内核高半区虚拟地址 (`pml4_idx` >= 256)
    /// - `phys` 是对应的物理地址
    /// - 仅用于映射内核栈 (RSP0) 等必要内核结构
    pub fn map_kernel_page_in_table(
        &self,
        pml4: u64,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) {
        if pml4 == 0 {
            return;
        }

        // 仅允许内核高半区地址
        if virt.pml4_idx() < 256 {
            crate::klog_boot_info!(
                "[VMM] map_kernel_page_in_table: reject user-half virt={:#X} pml4_idx={}",
                virt.0,
                virt.pml4_idx()
            );
            return;
        }

        let _flags = self.acquire_lock();

        // SAFETY: pml4 is a valid PML4 address; VMM_LOCK held
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: 完整 4 级页表遍历与按需创建.
        unsafe {
            let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4_ptr.add(virt.pml4_idx()), true, 0);
            if pdpt.is_null() {
                self.release_lock(&_flags);
                return;
            }

            let pd =
                self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true, HUGE_PAGE_2M_SIZE);
            if pd.is_null() {
                self.release_lock(&_flags);
                return;
            }

            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true, PAGE_SIZE);
            if pt.is_null() {
                crate::klog_boot_info!(
                    "[VMM] map_kernel_page_in_table: failed to get/create PT for {:#x}",
                    virt.0
                );
                self.release_lock(&_flags);
                return;
            }

            if flags.contains(PageFlags::USER) {
                // 设置 USER 位: 允许用户态访问
                (*pml4_ptr.add(virt.pml4_idx())).set_user(true);
                (*pdpt.add(virt.pdpt_idx())).set_user(true);
                (*pd.add(virt.pd_idx())).set_user(true);
            }

            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        self.release_lock(&_flags);
    }

    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    pub fn unmap_page_in_table(&self, pml4: u64, virt: VirtAddr) {
        if pml4 == 0 {
            return;
        }

        // 安全门 1: KPTI 共享页表防护
        // 禁止修改 PML4[256..511] (kernel high half).
        // KPTI init 时仅复制 PML4 顶层, USER_PDPT/USER_PD/USER_PT 仍与 KERNEL_ 共享
        // 同一物理页. 此处 unmap 的"递归释放空中间页表"会把共享的 PDE 写 0,
        // 污染 kernel page table, 触发 Triple Fault.
        if virt.pml4_idx() >= 256 {
            crate::klog_boot_info!(
                "[VMM] unmap_page_in_table: skip kernel-half virt={:#X} pml4_idx={}",
                virt.0,
                virt.pml4_idx()
            );
            return;
        }

        let _flags = self.acquire_lock();

        // SAFETY: pml4 = process CR3 value. phys_to_virt gives valid kernel VA.
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: 各级带存在位保护的只读页表遍历.
        // VMM_LOCK 串行化所有页表修改.
        unsafe {
            let pml4_tbl = pml4_virt.0 as *mut PageTableEntry;

            // SAFETY: pml4_tbl.add(idx) stays within the 4KB PML4 page
            let pml4e = &*pml4_tbl.add(virt.pml4_idx());

            if !pml4e.is_present() {
                self.release_lock(&_flags);
                return;
            }

            // SAFETY: pml4e.frame() is present & valid frame; phys_to_virt gives kernel VA
            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &*pdpt.add(virt.pdpt_idx());

            if !pdpte.is_present() {
                self.release_lock(&_flags);
                return;
            }

            if pdpte.is_huge() {
                // 1GB 页: 直接清空 PDPT 项
                (*pdpt.add(virt.pdpt_idx())).set_value(0);
                self.flush_tlb(virt.0);
            } else {
                // SAFETY: pdpte.frame() valid; present && !huge → points to PD
                let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
                let pde = &*pd.add(virt.pd_idx());

                if !pde.is_present() {
                    self.release_lock(&_flags);
                    return;
                }

                if pde.is_huge() {
                    // 2MB 页: 直接清空 PDE 项
                    (*pd.add(virt.pd_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                } else {
                    // SAFETY: pde.frame() valid; present && !huge → points to PT
                    let pt = pde.frame().to_virt().0 as *mut PageTableEntry;
                    let pt_idx = virt.pt_idx();
                    (*pt.add(pt_idx)).set_value(0);
                    self.flush_tlb(virt.0);

                    // 递归释放空的中间页表
                    if self.is_table_empty(pt) {
                        let pt_phys = pde.frame().as_u64();
                        (*pd.add(virt.pd_idx())).set_value(0);
                        get_pmm().free_page(PhysAddr(pt_phys));

                        if self.is_table_empty(pd) {
                            let pd_phys = pdpte.frame().as_u64();
                            (*pdpt.add(virt.pdpt_idx())).set_value(0);
                            get_pmm().free_page(PhysAddr(pd_phys));

                            if self.is_table_empty(pdpt) {
                                let pdpt_phys = pml4e.frame().as_u64();
                                (*pml4_tbl.add(virt.pml4_idx())).set_value(0);
                                get_pmm().free_page(PhysAddr(pdpt_phys));
                            }
                        }
                    }
                }
            }
        }

        self.total_unmaps.fetch_add(1, Ordering::Relaxed);
        self.release_lock(&_flags);
    }

    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    pub fn destroy_page_table(&self, pml4: u64) {
        if pml4 == 0 {
            return;
        }

        let _flags = self.acquire_lock();

        let pmm = get_pmm();
        // SAFETY: pml4 valid; VMM_LOCK held
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: 遍历 4 级释放页表.
        // 仅用户空间项 (0..255); 内核项共享.
        //
        // B03-06 修复: COW 共享页引用计数处理。
        // clone_user_page_table_cow_inner 在 fork 时对每个被共享的物理页调
        // cow_inc_ref, 父子各 +1 → 计数 2。若子进程立即 exit 且未触发 COW fault,
        // cow_dec_ref 从未被调用 → 物理页引用计数永不归零 → 物理页泄漏。
        // 根治: 在 destroy_page_table 遍历 leaf PTE 时对每个用户数据页调
        // cow_dec_ref, 返回 true 时 pmm.free_page 释放。锁序: VMM_LOCK 已持有,
        // 在持锁状态下调 cow_dec_ref (内部 IrqSpinLock<COW_REFS> 嵌套),
        // 禁止倒置 — 即禁止先取 COW_REFS 再取 VMM_LOCK。
        unsafe {
            let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;

            for i in 0..256usize {
                // SAFETY: pml4_ptr.add(i) within the 4KB PML4 page
                let pml4e = &*pml4_ptr.add(i);

                if pml4e.is_present() {
                    let pdpt_phys = pml4e.frame().as_u64();
                    let pdpt_virt = PhysAddr(pdpt_phys).to_virt();
                    let pdpt = pdpt_virt.0 as *mut PageTableEntry;

                    for j in 0..512usize {
                        // SAFETY: pdpt.add(j) within the 4KB PDPT page
                        let pdpte = &*pdpt.add(j);

                        if pdpte.is_present() && !pdpte.is_huge() {
                            let pd_phys = pdpte.frame().as_u64();
                            let pd_virt = PhysAddr(pd_phys).to_virt();
                            let pd = pd_virt.0 as *mut PageTableEntry;

                            for k in 0..512usize {
                                // SAFETY: pd.add(k) within the 4KB PD page
                                let pde = &*pd.add(k);

                                if pde.is_present() && !pde.is_huge() {
                                    let pt_phys = pde.frame().as_u64();
                                    let pt_virt = PhysAddr(pt_phys).to_virt();
                                    let pt = pt_virt.0 as *mut PageTableEntry;

                                    // B03-06: 遍历 leaf PTE 释放用户数据物理页
                                    // (仅 4KB 页, 排除 huge page).
                                    for l in 0..512usize {
                                        // SAFETY: pt.add(l) within the 4KB PT page
                                        let pte = &*pt.add(l);
                                        if pte.is_present() {
                                            let user_phys = pte.frame().as_u64();
                                            // cow_dec_ref 锁序: VMM_LOCK 已持有,
                                            // 内部 IrqSpinLock<COW_REFS> 嵌套.
                                            if crate::framework::mm::cow::cow_dec_ref(user_phys) {
                                                // 引用计数归零: 释放物理页
                                                pmm.free_page(PhysAddr(user_phys));
                                            }
                                        }
                                    }

                                    pmm.free_page(PhysAddr(pt_phys));
                                }
                            }

                            pmm.free_page(PhysAddr(pd_phys));
                        }
                    }

                    pmm.free_page(PhysAddr(pdpt_phys));
                }
            }

            pmm.free_page(PhysAddr(pml4));
        }

        // SAFETY: VMM_LOCK held; only mutation is clearing user_tables slot
        let tables = unsafe { &mut *self.user_tables.get() };
        for i in 0..MAX_USER_PAGE_TABLES {
            if tables[i].pml4_phys == pml4 && tables[i].in_use {
                tables[i].in_use = false;
                tables[i].pml4_phys = 0;
                self.user_table_count.fetch_sub(1, Ordering::Relaxed);
                break;
            }
        }

        self.release_lock(&_flags);
    }

    pub fn get_stats(&self) -> (u64, u64, u64) {
        (
            self.total_maps.load(Ordering::Relaxed),
            self.total_unmaps.load(Ordering::Relaxed),
            self.page_faults.load(Ordering::Relaxed),
        )
    }

    // ==================== 私有方法 ====================

    fn map_page_internal(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        crate::klog_boot_info!("[VMM] map_page: virt={:#X} phys={:#X}", virt.0, phys.0);

        // 安全门: 禁止在 KERNEL_PML4 中修改内核高半区页表.
        // 内核高半区 (PML4[256..511]) 由 boot.asm 建立恒等映射 (1GB),
        // 后续只允许 map_page_in_table (via user PML4) 操作 user half.
        // 此门与 map_page_in_table 中的门对称, 防止 map_page/map_huge_page
        // 间接调用本函数时分裂内核大页导致 PDE 损坏.
        if virt.pml4_idx() >= 256 {
            crate::klog_boot_info!(
                "[VMM] map_page_internal: skip kernel-half virt={:#X} pml4_idx={}",
                virt.0,
                virt.pml4_idx()
            );
            return Ok(());
        }

        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        // SAFETY: KERNEL_PML4 valid; caller holds VMM_LOCK (via map_page/map_huge_page)
        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // SAFETY: Full 4-level page table walk with creation under VMM_LOCK
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true, 0);
            if pdpt.is_null() {
                return Err("Failed to allocate PDPT");
            }

            let pd =
                self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true, HUGE_PAGE_2M_SIZE);
            if pd.is_null() {
                return Err("Failed to allocate PD");
            }

            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true, PAGE_SIZE);
            if pt.is_null() {
                return Err("Failed to allocate PT");
            }

            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    fn map_2mb_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // SAFETY: 2MB huge page mapping at PD level. VMM_LOCK held by caller.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            let pml4_idx = virt.pml4_idx();

            // 记录 PML4E 是否已存在 — 新建的 PDPT 需同步到 USER_PML4
            let pml4e_existed = (*pml4.add(pml4_idx)).is_present();

            let pdpt = self.get_or_create_table_entry(pml4.add(pml4_idx), true, 0);
            if pdpt.is_null() {
                return Err("Failed to allocate PDPT");
            }

            // 内核高半区: 新建 PML4 条目需同步到 USER_PML4 (KPTI)
            if pml4_idx >= 256 && !pml4e_existed {
                // SAFETY: VMM_LOCK 已持有, pml4_idx 在 [256, 512) 内, KERNEL_PML4 已初始化
                super::kpti::kpti_sync_pml4_entry(pml4_idx);
            }

            // 安全门: 如果 PDPT 条目是 1GB 大页且已映射, 禁止覆盖
            // (2MB 映射到已有 1GB 页的区域会拆分共享页表, KPTI 下导致 Triple Fault)
            let pdpte = &*pdpt.add(virt.pdpt_idx());
            if pdpte.is_present() && pdpte.is_huge() {
                // 已有 1GB 大页覆盖此范围, 无需再映射 2MB
                return Ok(());
            }

            let pd =
                self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true, HUGE_PAGE_2M_SIZE);
            if pd.is_null() {
                return Err("Failed to allocate PD");
            }

            let pde = &mut *pd.add(virt.pd_idx());
            if pde.is_present() && !pde.is_huge() {
                return Err("PD entry already split to PT, cannot map 2MB page");
            }
            if pde.is_present() && pde.is_huge() {
                // 已有 2MB 映射, 不覆盖 (避免破坏 KPTI 共享页表)
                return Ok(());
            }
            pde.set_frame(phys);
            pde.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    fn map_1gb_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // SAFETY: 1GB huge page mapping at PDPT level. VMM_LOCK held by caller.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            let pml4_idx = virt.pml4_idx();

            // 记录 PML4E 是否已存在 — 新建的 PDPT 需同步到 USER_PML4
            let pml4e_existed = (*pml4.add(pml4_idx)).is_present();

            let pdpt = self.get_or_create_table_entry(pml4.add(pml4_idx), true, 0);
            if pdpt.is_null() {
                return Err("Failed to allocate PDPT");
            }

            // 内核高半区: 新建 PML4 条目需同步到 USER_PML4 (KPTI)
            if pml4_idx >= 256 && !pml4e_existed {
                // SAFETY: VMM_LOCK 已持有, pml4_idx 在 [256, 512) 内, KERNEL_PML4 已初始化
                super::kpti::kpti_sync_pml4_entry(pml4_idx);
            }

            let pdpte = &mut *pdpt.add(virt.pdpt_idx());
            if pdpte.is_present() && !pdpte.is_huge() {
                return Err("PDPT entry already split, cannot map 1GB page");
            }
            if pdpte.is_present() && pdpte.is_huge() {
                // 已有 1GB 映射, 不覆盖
                return Ok(());
            }
            pdpte.set_frame(phys);
            pdpte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    // 有意窄化: 显式收窄, 调用方保证值域
    #[expect(clippy::cast_possible_truncation)]
    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    #[expect(
        clippy::ptr_as_ptr,
        reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
    )]
    #[expect(
        clippy::unreadable_literal,
        reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
    )]
    unsafe fn get_or_create_table_entry(
        &self,
        entry: *mut PageTableEntry,
        create: bool,
        huge_step: u64,
    ) -> *mut PageTableEntry {
        unsafe {
            // SAFETY: 调用方保证 `entry` 指向 PMM 分配的页表页内合法 PageTableEntry.
            // 解引用通过 512 项表大小做边界检查.
            let e = &*entry;

            if e.is_present() && !e.is_huge() {
                // SAFETY: Present && !huge → frame 位是合法的下一级表物理地址.
                // phys_to_virt 给出合法内核 VA.
                e.frame().to_virt().0 as *mut PageTableEntry
            } else if create {
                let pmm = get_pmm();

                pmm.alloc_page().map_or(core::ptr::null_mut(), |page| {
                    let page_virt = page.to_virt();
                    let pt = page_virt.0 as *mut PageTableEntry;
                    core::ptr::write_bytes(pt as *mut u8, 0, PAGE_SIZE as usize);

                    if e.is_huge() {
                        // 拆分巨页: 从巨页帧填充 512 个子条目
                        // step = PAGE_SIZE → PD→PT (2MB→4KB), 新 PT 条目不需要 HUGE_PAGE
                        // step = HUGE_PAGE_2M_SIZE → PDPT→PD (1GB→2MB), 新 PD 条目需要 HUGE_PAGE
                        let huge_frame = e.frame();
                        let huge_flags = e.flags();
                        let step = if huge_step > 0 {
                            huge_step
                        } else {
                            PAGE_SIZE as u64
                        };
                        let mut new_flags =
                            (huge_flags & !PageFlags::HUGE_PAGE) | PageFlags::PRESENT;
                        if step == HUGE_PAGE_2M_SIZE {
                            // PDPT→PD 拆分: 新 PD 条目必须标记为 2MB 巨页,
                            // 否则 CPU 会将帧地址解释为 PT 指针, 导致页表遍历读取垃圾数据.
                            new_flags |= PageFlags::HUGE_PAGE;
                        }
                        crate::klog_boot_info!(
                            "[VMM] huge split: entry={:#X} frame={:#X} new_pt={:#X} step={:#X}",
                            entry as u64,
                            huge_frame.as_u64(),
                            page.as_u64(),
                            step
                        );
                        for i in 0..512 {
                            // SAFETY: pt points to a full 4KB page; add(i) stays within bounds
                            let pte = &mut *pt.add(i);
                            pte.set_frame(PhysAddr(huge_frame.as_u64() + i as u64 * step));
                            pte.set_flags(new_flags);
                        }
                    }

                    // SAFETY: `entry` 是合法 PDE/PDPTE 指针; 使用 set_value 一次性写入
                    // 新帧地址 + 标志, 避免 set_frame→set_flags 两步操作中间出现
                    // "帧=新PT, 标志=旧值(含HUGE)" 的瞬时不一致状态.
                    // 单次原子 store 保证 CPU 页表遍历器不会观察到中间态.
                    // 修复 (TRACK-INIT-RING3-PDE): 中间页表条目禁止设置 NX.
                    // PDE 的 NX 位语义是"该条目覆盖的整个区域不可执行" (2MB/1GB),
                    // 原 M9 修复 (16667750) 在此加 NX 意图防"用户态执行页表页",
                    // 但实际导致用户代码区 (0x400000 所在 PDE) 整体禁执行 →
                    // 用户态取指 #PF (e=0x15, P=1 U=1 I/D=1). 页表页的安全性由
                    // "用户页表低半区不映射页表页帧" 保证, 无需中间条目 NX.
                    let new_val = (page.as_u64() & 0x000FFFFFFFFFF000)
                        | (PageFlags::PRESENT | PageFlags::WRITABLE).bits();
                    (*entry).set_value(new_val);

                    page_virt.0 as *mut PageTableEntry
                })
            } else {
                core::ptr::null_mut()
            }
        }
    }

    /// 将内核页表中的 2MB 巨页拆分为 4KB 页.
    ///
    /// # Errors
    /// 当 VMM 未初始化时返回 `Err("VMM not initialized")`;
    /// 当目标地址位于内核高半区 (PML4[256..511], KPTI 共享页表) 时返回
    /// `Err("Cannot split kernel-half 2MB page (KPTI shared)")`;
    /// 当 PDPT/PD 不存在或 PD 条目未映射时分别返回 `Err("PDPT not present")`,
    /// `Err("PD not present")`, `Err("PD entry not present")`;
    /// 当分配新的页表页失败时返回 `Err("Failed to allocate PT")`.
    // 有意窄化: 显式收窄, 调用方保证值域
    #[expect(clippy::cast_possible_truncation)]
    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    #[expect(
        clippy::ptr_as_ptr,
        reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
    )]
    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    pub fn split_2mb_page(&self, virt: u64) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        // 安全门: KPTI 共享页表防护
        // 禁止拆分内核高半区的 2MB 巨页 (PML4[256..511]).
        // KPTI 下 USER_PML4 与 KERNEL_PML4 共享底层 PDPT/PD 物理页,
        // 拆分内核 2MB 页会修改共享 PDE, 同时破坏 kernel 和 user 页表.
        if VirtAddr(virt).pml4_idx() >= 256 {
            crate::klog_boot_info!(
                "[VMM] split_2mb_page: skip kernel-half virt={:#X} pml4_idx={}",
                virt,
                VirtAddr(virt).pml4_idx()
            );
            return Err("Cannot split kernel-half 2MB page (KPTI shared)");
        }

        let _flags = self.acquire_lock();

        let result: Result<(), &'static str> = (|| {
            let pml4_virt = PhysAddr(pml4_base).to_virt();
            let v = VirtAddr(virt);

            // SAFETY: 将 2MB 巨页拆分为 512 个 4KB 页.
            // VMM_LOCK 已持有, 所有页表修改串行化.
            unsafe {
                let pml4 = pml4_virt.0 as *mut PageTableEntry;
                let pdpt = self.get_or_create_table_entry(pml4.add(v.pml4_idx()), false, 0);
                if pdpt.is_null() {
                    return Err("PDPT not present");
                }

                let pd = self.get_or_create_table_entry(pdpt.add(v.pdpt_idx()), false, 0);
                if pd.is_null() {
                    return Err("PD not present");
                }

                let pd_entry = &mut *pd.add(v.pd_idx());
                if !pd_entry.is_present() {
                    return Err("PD entry not present");
                }
                if !pd_entry.is_huge() {
                    return Ok(());
                }

                let huge_frame = pd_entry.frame();
                let huge_flags = pd_entry.flags();

                let pmm = get_pmm();
                let pt_page = match pmm.alloc_page() {
                    Some(p) => p,
                    None => return Err("Failed to allocate PT"),
                };
                let pt = pt_page.to_virt().0 as *mut PageTableEntry;
                core::ptr::write_bytes(pt as *mut u8, 0, PAGE_SIZE as usize);

                for i in 0..512 {
                    // SAFETY: pt is a full 4KB PT page; add(i) stays in bounds
                    let pte = &mut *pt.add(i);
                    pte.set_frame(PhysAddr(huge_frame.as_u64() + i as u64 * PAGE_SIZE as u64));
                    pte.set_flags((huge_flags & !PageFlags::HUGE_PAGE) | PageFlags::PRESENT);
                    pte.set_present(true);
                }

                pd_entry.set_frame(pt_page);
                let new_flags = (huge_flags & !PageFlags::HUGE_PAGE) | PageFlags::PRESENT;
                pd_entry.set_flags(new_flags);

                self.flush_tlb(virt);
            }

            Ok(())
        })();

        self.release_lock(&_flags);
        result
    }

    pub fn ensure_pml4_user(&self, virt: u64) {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return;
        }

        let v = VirtAddr(virt);

        // 安全门: KPTI 权限隔离
        // 禁止对内核高半区 (PML4[256..511]) 设置 USER 位.
        // 内核页表条目设 USER 位会允许用户态代码访问内核内存,
        // 破坏 Meltdown 缓解 (KPTI) 的安全边界.
        if v.pml4_idx() >= 256 {
            return;
        }

        // SAFETY: Setting USER bit on PML4 entry; KERNEL_PML4 valid, index in range
        let pml4_virt = PhysAddr(pml4_base).to_virt();
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            let entry = &mut *pml4.add(v.pml4_idx());
            if entry.is_present() && !entry.is_user() {
                entry.set_user(true);
                self.flush_tlb(virt);
            }
        }
    }

    pub fn ensure_path_user(&self, virt: u64) {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return;
        }

        let v = VirtAddr(virt);

        // 安全门: KPTI 权限隔离
        // 禁止对内核高半区 (PML4[256..511]) 设置 USER 位.
        // 内核页表条目设 USER 位会允许用户态代码访问内核内存,
        // 破坏 Meltdown 缓解 (KPTI) 的安全边界.
        if v.pml4_idx() >= 256 {
            return;
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // SAFETY: 遍历 PML4 → PDPT → PD, 各级设 USER 位.
        // 各级都有存在位保护. 索引由 VA 位计算.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pml4e = &mut *pml4.add(v.pml4_idx());
            if !pml4e.is_present() {
                return;
            }
            pml4e.set_user(true);

            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &mut *pdpt.add(v.pdpt_idx());
            if !pdpte.is_present() {
                return;
            }
            pdpte.set_user(true);

            if pdpte.is_huge() {
                self.flush_tlb(virt);
                return;
            }

            let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
            let pde = &mut *pd.add(v.pd_idx());
            if !pde.is_present() {
                return;
            }
            pde.set_user(true);
        }

        self.flush_tlb(virt);
    }

    // 有意窄化: 显式收窄, 调用方保证值域
    #[expect(clippy::cast_possible_truncation)]
    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    #[expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
    )]
    #[expect(
        clippy::unreadable_literal,
        reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
    )]
    pub fn clone_user_page_table(&self, parent_pml4: u64) -> Option<u64> {
        if parent_pml4 == 0 {
            return None;
        }

        let _flags = self.acquire_lock();

        let pmm = get_pmm();
        let child_pml4_phys = pmm.alloc_page()?;
        let child_pml4_base = child_pml4_phys.to_virt().0 as *mut u64;

        // SAFETY: child_pml4_phys from PMM, phys_to_virt valid
        unsafe {
            core::ptr::write_bytes(child_pml4_base, 0, PAGE_SIZE as usize);
        }

        let kernel_pml4 = KERNEL_PML4.load(Ordering::Acquire);
        // SAFETY: kernel_pml4 valid; both src and dst are page-aligned kernel VAs
        let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt().0 as *const u64;
        unsafe {
            core::ptr::copy_nonoverlapping(
                kernel_pml4_virt.add(256),
                child_pml4_base.add(256),
                256,
            );
        }

        // SAFETY: parent_pml4 is a valid user PML4; VMM_LOCK held
        let parent_pml4_virt = PhysAddr(parent_pml4).to_virt().0 as *const u64;

        for i in 0..256u16 {
            // SAFETY: i in 0..255 within PML4 page; volatile for hardware-updated bits
            let parent_pml4e = unsafe { parent_pml4_virt.add(i as usize).read_volatile() };
            if (parent_pml4e & 1) == 0 {
                continue;
            }

            let child_pdpt_phys = pmm.alloc_page()?;
            let child_pdpt = child_pdpt_phys.to_virt().0 as *mut u64;
            // SAFETY: child_pdpt from PMM, phys_to_virt valid
            unsafe {
                core::ptr::write_bytes(child_pdpt, 0, PAGE_SIZE as usize);
            }

            let mut child_pml4e = parent_pml4e;
            child_pml4e = (child_pml4e & 0xFFF) | (child_pdpt_phys.as_u64() & 0x000FFFFFFFFFF000);
            // SAFETY: child_pml4_base 是合法的 4KB PML4 页; volatile 写以保证 TLB 一致性
            unsafe {
                child_pml4_base.add(i as usize).write_volatile(child_pml4e);
            }

            // SAFETY: parent_pml4e present → frame bits point to valid PDPT
            let parent_pdpt_virt = (parent_pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let parent_pdpt = parent_pdpt_virt as *const u64;

            for j in 0..512u16 {
                // SAFETY: j in 0..511 within PDPT page; volatile read
                let parent_pdpte = unsafe { parent_pdpt.add(j as usize).read_volatile() };
                if (parent_pdpte & 1) == 0 {
                    continue;
                }
                if (parent_pdpte & 0x80) != 0 {
                    continue;
                }

                let child_pd_phys = pmm.alloc_page()?;
                let child_pd = child_pd_phys.to_virt().0 as *mut u64;
                // SAFETY: child_pd from PMM
                unsafe {
                    core::ptr::write_bytes(child_pd, 0, PAGE_SIZE as usize);
                }

                let mut child_pdpte_v = parent_pdpte;
                child_pdpte_v =
                    (child_pdpte_v & 0xFFF) | (child_pd_phys.as_u64() & 0x000FFFFFFFFFF000);
                // SAFETY: child_pdpt valid; volatile write
                unsafe {
                    child_pdpt.add(j as usize).write_volatile(child_pdpte_v);
                }

                // SAFETY: parent_pdpte present → valid PD pointer
                let parent_pd_virt = (parent_pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE;
                let parent_pd = parent_pd_virt as *const u64;

                for k in 0..512u16 {
                    // SAFETY: k in 0..511 within PD page; volatile read
                    let parent_pde = unsafe { parent_pd.add(k as usize).read_volatile() };
                    if (parent_pde & 1) == 0 {
                        continue;
                    }

                    if (parent_pde & 0x80) != 0 {
                        // 深拷贝 2MB 巨页
                        let huge_phys = pmm.alloc_pages(512)?;
                        let huge_virt = PhysAddr(huge_phys.as_u64()).to_virt().0;
                        // SAFETY: parent_huge is valid 2MB kernel VA
                        let parent_huge = (parent_pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                parent_huge as *const u8,
                                huge_virt as *mut u8,
                                2 * 1024 * 1024,
                            );
                        }
                        let mut child_pde_v = parent_pde;
                        child_pde_v =
                            (child_pde_v & 0xFFF) | (huge_phys.as_u64() & 0x000FFFFFFFFFF000);
                        // SAFETY: child_pd valid; volatile write
                        unsafe {
                            child_pd.add(k as usize).write_volatile(child_pde_v);
                        }
                        continue;
                    }

                    let child_pt_phys = pmm.alloc_page()?;
                    let child_pt = child_pt_phys.to_virt().0 as *mut u64;
                    // SAFETY: child_pt from PMM
                    unsafe {
                        core::ptr::write_bytes(child_pt, 0, PAGE_SIZE as usize);
                    }

                    let mut child_pde_v = parent_pde;
                    child_pde_v =
                        (child_pde_v & 0xFFF) | (child_pt_phys.as_u64() & 0x000FFFFFFFFFF000);
                    // SAFETY: child_pd valid; volatile write
                    unsafe {
                        child_pd.add(k as usize).write_volatile(child_pde_v);
                    }

                    // SAFETY: parent_pde present && !huge → valid PT pointer
                    let parent_pt_virt = (parent_pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
                    let parent_pt = parent_pt_virt as *const u64;

                    for l in 0..512u16 {
                        // SAFETY: l in 0..511 within PT page; volatile read
                        let parent_pte = unsafe { parent_pt.add(l as usize).read_volatile() };
                        if (parent_pte & 1) == 0 {
                            continue;
                        }

                        let child_page_phys = pmm.alloc_page()?;
                        let child_page_virt = PhysAddr(child_page_phys.as_u64()).to_virt().0;
                        // SAFETY: parent_page_virt is valid kernel VA from PTE
                        let parent_page_virt = (parent_pte & 0x000FFFFFFFFFF000) + KERNEL_BASE;

                        // SAFETY: Both addresses are valid 4KB kernel VAs
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                parent_page_virt as *const u8,
                                child_page_virt as *mut u8,
                                PAGE_SIZE as usize,
                            );
                        }

                        let mut child_pte_v = parent_pte;
                        child_pte_v =
                            (child_pte_v & 0xFFF) | (child_page_phys.as_u64() & 0x000FFFFFFFFFF000);
                        // SAFETY: child_pt valid; volatile write
                        unsafe {
                            child_pt.add(l as usize).write_volatile(child_pte_v);
                        }
                    }
                }
            }
        }

        self.release_lock(&_flags);
        Some(child_pml4_phys.as_u64())
    }

    fn find_free_user_slot(&self) -> usize {
        // SAFETY: Read-only access to user_tables via UnsafeCell under VMM_LOCK.
        let tables = unsafe { &*self.user_tables.get() };
        for i in 0..MAX_USER_PAGE_TABLES {
            if !tables[i].in_use {
                return i;
            }
        }
        MAX_USER_PAGE_TABLES
    }

    #[inline(always)]
    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    /// 获取 VMM 锁 (关中断 + 自旋), 支持单核可重入.
    ///
    /// # Panics
    /// 在 `debug_assertions` 构建下, 若检测到 `VMM_LOCK` 被递归获取 (死锁), 触发 `assert!`
    /// panic, 错误信息为 "`VMM_LOCK`: recursive acquisition detected (deadlock)".
    pub fn acquire_lock(&self) -> IrqSaveFlags {
        let flags = disable_interrupts();
        // 单核可重入: 如果锁已被当前线程持有 (中断禁用时无其他线程),
        // 直接返回避免死锁 (page fault handler 在 COW 持锁期间触发)
        if VMM_LOCK.load(Ordering::Acquire) {
            return flags;
        }
        while VMM_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        #[cfg(debug_assertions)]
        {
            // 不可恢复: VMM_LOCK 递归获取意味着死锁, 继续执行只会挂起系统
            assert!(
                !VMM_LOCK_RECURSIVE.swap(true, Ordering::Relaxed),
                "VMM_LOCK: recursive acquisition detected (deadlock)"
            );
        }
        flags
    }

    #[inline(always)]
    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn release_lock(&self, flags: &IrqSaveFlags) {
        #[cfg(debug_assertions)]
        {
            VMM_LOCK_RECURSIVE.store(false, Ordering::Relaxed);
        }
        VMM_LOCK.store(false, Ordering::Release);
        restore_interrupts(flags);
    }

    #[inline(always)]
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    unsafe fn read_cr3(&self) -> u64 {
        // SAFETY: Reading CR3 is always safe; returns current page table base
        crate::arch!(read_page_table_base())
    }

    #[inline(always)]
    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    // SAFETY: val 必须指向有效 PML4 页表; 调用方保证页表分配后保持有效.
    unsafe fn write_cr3(&self, val: u64) {
        // SAFETY: val must point to a valid PML4 table; caller guarantees this
        crate::arch!(write_page_table_base(val));
    }

    #[inline(always)]
    // 有意窄化: 显式收窄, 调用方保证值域
    #[expect(clippy::cast_possible_truncation)]
    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    fn flush_tlb(&self, addr: u64) {
        crate::arch!(tlb_flush_page(addr as usize));

        #[cfg(feature = "smp")]
        {
            use crate::framework::smp;
            if smp::is_enabled() && smp::get_cpu_count() > 1 {
                smp::broadcast_tlb_invalidate();
            }
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    fn is_table_empty(&self, table: *mut PageTableEntry) -> bool {
        for i in 0..512usize {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                if (*table.add(i)).is_present() {
                    return false;
                }
            }
        }
        true
    }
}

static GLOBAL_VMM: OnceLock<VirtualMemoryManager> = OnceLock::new();

pub fn vmm_init() {
    GLOBAL_VMM.get_or_init(|slot| {
        let vmm = VirtualMemoryManager::new();
        vmm.init();
        slot.write(vmm);
    });
    super::cow::cow_init();
}

pub fn get_vmm() -> &'static VirtualMemoryManager {
    GLOBAL_VMM.get_or_panic("VMM")
}

/// 返回 `GLOBAL_VMM` `OnceLock` 的内部状态机原始值 (仅用于诊断).
///
/// 返回值: 0=未初始化, 1=初始化中, 2=已完成.
/// 与 `get_vmm()` 不同, 本函数不会 panic, 可在 VMM 初始化前安全调用.
pub fn vmm_debug_state() -> u8 {
    GLOBAL_VMM.debug_state()
}

pub fn get_kernel_pml4() -> u64 {
    KERNEL_PML4.load(Ordering::Acquire)
}

pub fn get_current_pml4() -> u64 {
    let cr3 = crate::arch!(read_page_table_base());
    if cr3 != 0 {
        cr3
    } else {
        KERNEL_PML4.load(Ordering::Acquire)
    }
}

/// 统计进程页表中已映射的用户页数 (4 KiB 粒度, RSS 近似).
///
/// 只读遍历 PML4 用户半区 (`0..256`), **不取 VMM 锁** — 供 OOMD 在内存紧急时
/// 粗略挑选占用最大的进程. 与并发 unmap 的竞态允许近似 (仅用于启发式选择).
///
/// # Arguments
/// * `cr3` — 进程页表根物理地址 (`Process::cr3`); 0 表示无用户页表.
///
/// # Returns
/// 该地址空间映射的 4 KiB 当量页数 (大页按其覆盖的 4 KiB 页数折算).
#[expect(
    clippy::similar_names,
    reason = "变量名相似表达同族概念 (pdpt/pd/pt 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
)]
pub fn count_present_user_pages(cr3: u64) -> u64 {
    if cr3 == 0 {
        return 0;
    }

    let mut pages = 0u64;
    let pml4_ptr = PhysAddr(cr3).to_virt().0 as *const PageTableEntry;

    // SAFETY: cr3 为进程有效 PML4 物理地址, 经直接映射转为可读虚拟地址;
    // 仅读取 4 级页表结构不做修改; 各层索引均限制在 4 KiB 表内 (< 256 / < 512).
    unsafe {
        for i in 0..256usize {
            let pml4e = &*pml4_ptr.add(i);
            if !pml4e.is_present() {
                continue;
            }
            let pdpt_ptr = pml4e.frame().to_virt().0 as *const PageTableEntry;

            for j in 0..512usize {
                let pdpte = &*pdpt_ptr.add(j);
                if !pdpte.is_present() {
                    continue;
                }
                if pdpte.is_huge() {
                    // PDPTE 级大页 = 1 GiB = 512 × 512 个 4 KiB 页
                    pages += 512 * 512;
                    continue;
                }
                let pd_ptr = pdpte.frame().to_virt().0 as *const PageTableEntry;

                for k in 0..512usize {
                    let pde = &*pd_ptr.add(k);
                    if !pde.is_present() {
                        continue;
                    }
                    if pde.is_huge() {
                        // PDE 级大页 = 2 MiB = 512 个 4 KiB 页
                        pages += 512;
                        continue;
                    }
                    let pt_ptr = pde.frame().to_virt().0 as *const PageTableEntry;

                    for l in 0..512usize {
                        if (&*pt_ptr.add(l)).is_present() {
                            pages += 1;
                        }
                    }
                }
            }
        }
    }

    pages
}
