//! Copy-on-Write (COW) — fork 内存共享与写时复制
//!
//! ## 核心思想
//!
//! `fork()` 时父子进程**共享**物理页，均标记为只读。
//! 任意一方写入时触发 #PF → COW handler → 复制物理页。
//!
//! ## 引用计数
//!
//! 使用 `BTreeMap<PhysFrame, u32>` 跟踪每帧的共享计数:
//! - fork: 父子各 +1 = 2
//! - COW fault: 子分配新页，父引用 -1
//! - munmap / exit: 引用 -1，归零时释放
//!
//! ## SAFETY
//!
//! - COW 跟踪表由 `COW_LOCK` 自旋锁保护。
//! - 所有 `unsafe` 页表访问基于 PMM 分配的有效物理帧，通过 `KERNEL_BASE`
//!   转换为内核虚拟地址，不会产生悬垂指针。
//! - volatile 读写确保编译器不重排 MMIO 相关的页表操作。

use alloc::collections::BTreeMap;

use super::vmm;
use super::{PAGE_SIZE, PageFlags, PhysAddr, VirtAddr};

use crate::framework::sync::IrqSpinLock;
static COW_REFS: IrqSpinLock<Option<BTreeMap<u64, u32>>> = IrqSpinLock::new(None);

pub fn cow_init() {
    *COW_REFS.lock() = Some(BTreeMap::new());
}

fn frame_key(phys: u64) -> u64 {
    phys & !(PAGE_SIZE - 1)
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub fn cow_inc_ref(phys: u64) {
    let key = frame_key(phys);
    let mut guard = COW_REFS.lock();
    let refs = match guard.as_mut() {
        Some(r) => r,
        None => return,
    };
    *refs.entry(key).or_insert(0) += 1;
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
pub fn cow_dec_ref(phys: u64) -> bool {
    let key = frame_key(phys);
    let mut guard = COW_REFS.lock();
    let refs = match guard.as_mut() {
        Some(r) => r,
        None => return false,
    };
    if let Some(count) = refs.get_mut(&key) {
        *count -= 1;
        if *count == 0 {
            refs.remove(&key);
            return true;
        }
    }
    false
}

pub fn cow_ref_count(phys: u64) -> u32 {
    let key = frame_key(phys);
    let guard = COW_REFS.lock();
    guard
        .as_ref()
        .map_or(0, |refs| refs.get(&key).copied().unwrap_or(0))
}

#[expect(
    clippy::used_underscore_binding,
    reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
)]
/// COW 感知的页表克隆: 共享用户空间物理页, 双方标记只读
/// 相比 deep copy 版本, 该版本不分配新物理页, 也不复制数据
///
/// # SMP Safety
/// 本函数持有 `VMM_LOCK` 保护所有页表修改, 确保多核并发安全。
/// 函数返回前刷新全部 TLB 条目使被清除 WRITABLE 位的 PTE 失效。
pub fn clone_user_page_table_cow(parent_pml4: u64) -> Option<u64> {
    let vmm_inst = vmm::get_vmm();
    let _vmm_flags = vmm_inst.acquire_lock();
    let result = clone_user_page_table_cow_inner(parent_pml4);
    vmm_inst.release_lock(&_vmm_flags);
    result
}

// 有意窄化: 显式收窄, 调用方保证值域
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::similar_names,
    reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
)]
#[expect(
    clippy::too_many_lines,
    reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
)]
#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
fn clone_user_page_table_cow_inner(parent_pml4: u64) -> Option<u64> {
    if parent_pml4 == 0 {
        return None;
    }

    let pmm = super::pmm::get_pmm();
    let child_pml4_phys = pmm.alloc_page()?;
    let kernel_pml4 = vmm::get_kernel_pml4();

    // SAFETY: child_pml4_phys 刚由 PMM 分配, 物理地址有效;
    // KERNEL_BASE 偏移后得到可访问的内核虚拟地址
    let child_pml4_virt = child_pml4_phys.to_virt().0 as *mut u64;
    unsafe {
        // B05-55C 根治: 必须 cast *mut u8, 否则 *mut u64 的 write_bytes 按
        // count × size_of::<u64>() 清零 32768 字节 (8 页), 覆盖后续物理页
        // (可能含 parent 页表结构页) → parent 页表被清 → fork 崩溃.
        core::ptr::write_bytes(child_pml4_virt as *mut u8, 0, PAGE_SIZE as usize);
    }

    // SAFETY: kernel_pml4 由 vmm_init 写入, 指向有效页表;
    // 复制高半区 (索引 256-511) 使子进程页表共享内核映射
    let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt().0 as *const u64;
    unsafe {
        core::ptr::copy_nonoverlapping(kernel_pml4_virt.add(256), child_pml4_virt.add(256), 256);
    }

    // SAFETY: parent_pml4 是已注册用户页表的物理地址
    let parent_pml4_virt = PhysAddr(parent_pml4).to_virt().0 as *const u64;

    for i in 0..256usize {
        // SAFETY: 索引 0-255 在 PML4 页范围内; volatile 确保读取真实的页表内容
        let parent_pml4e = unsafe { parent_pml4_virt.add(i).read_volatile() };
        if (parent_pml4e & 1) == 0 {
            continue;
        }

        let child_pdpt_phys = pmm.alloc_page()?;
        // SAFETY: 刚分配的页, 通过 KERNEL_BASE 映射有效
        let child_pdpt_virt = child_pdpt_phys.to_virt().0 as *mut u64;
        unsafe {
            // B05-55C 根治: cast *mut u8, 否则清零 8 页覆盖后续物理页
            core::ptr::write_bytes(child_pdpt_virt as *mut u8, 0, PAGE_SIZE as usize);
        }

        let mut child_pml4e = parent_pml4e;
        child_pml4e = (child_pml4e & 0xFFF) | (child_pdpt_phys.as_u64() & 0x000FFFFFFFFFF000);
        // SAFETY: child_pml4_virt 指向有效 PML4 页
        unsafe {
            child_pml4_virt.add(i).write_volatile(child_pml4e);
        }

        // SAFETY: parent_pml4e 已检验 present, phys_to_virt 映射有效
        let parent_pdpt_virt =
            PhysAddr(parent_pml4e & 0x000FFFFFFFFFF000).to_virt().0 as *const u64;

        for j in 0..512usize {
            // SAFETY: pdpt 索引在页范围内
            let parent_pdpte = unsafe { parent_pdpt_virt.add(j).read_volatile() };
            if (parent_pdpte & 1) == 0 || (parent_pdpte & 0x80) != 0 {
                continue;
            }

            let child_pd_phys = pmm.alloc_page()?;
            // SAFETY: 刚分配的页
            let child_pd_virt = child_pd_phys.to_virt().0 as *mut u64;
            unsafe {
                // B05-55C 根治: cast *mut u8, 否则清零 8 页覆盖后续物理页
                core::ptr::write_bytes(child_pd_virt as *mut u8, 0, PAGE_SIZE as usize);
            }

            let mut child_pdpte = parent_pdpte;
            child_pdpte = (child_pdpte & 0xFFF) | (child_pd_phys.as_u64() & 0x000FFFFFFFFFF000);
            unsafe {
                child_pdpt_virt.add(j).write_volatile(child_pdpte);
            }

            let parent_pd_virt =
                PhysAddr(parent_pdpte & 0x000FFFFFFFFFF000).to_virt().0 as *const u64;

            for k in 0..512usize {
                // SAFETY: pd 索引在页范围内
                let parent_pde = unsafe { parent_pd_virt.add(k).read_volatile() };
                if (parent_pde & 1) == 0 {
                    continue;
                }

                if (parent_pde & 0x80) != 0 {
                    // 2MB huge page: 保持原样, 不参与 COW
                    let child_pde_v = parent_pde;
                    // SAFETY: child_pd_virt 指向有效 PD 页
                    unsafe {
                        child_pd_virt.add(k).write_volatile(child_pde_v);
                    }
                    continue;
                }

                let child_pt_phys = pmm.alloc_page()?;
                let child_pt_virt = child_pt_phys.to_virt().0 as *mut u64;
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe {
                    // B05-55C 根治: cast *mut u8, 否则清零 8 页覆盖后续物理页
                    core::ptr::write_bytes(child_pt_virt as *mut u8, 0, PAGE_SIZE as usize);
                }

                let mut child_pde = parent_pde;
                child_pde = (child_pde & 0xFFF) | (child_pt_phys.as_u64() & 0x000FFFFFFFFFF000);
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe {
                    child_pd_virt.add(k).write_volatile(child_pde);
                }

                // SAFETY: parent_pt 物理地址来自有效 PDE;
                // 声明为 *mut 因为 COW 会写回清除 WRITABLE 位
                let parent_pt_virt =
                    PhysAddr(parent_pde & 0x000FFFFFFFFFF000).to_virt().0 as *mut u64;

                for l in 0..512usize {
                    // SAFETY: pt 索引在页范围内
                    let parent_pte = unsafe { parent_pt_virt.add(l).read_volatile() };
                    if (parent_pte & 1) == 0 {
                        continue;
                    }

                    let parent_phys = parent_pte & 0x000FFFFFFFFFF000;
                    let parent_flags = parent_pte & 0xFFF;

                    // B05-55 根治: COW 仅应用于 USER 可写页 (P=1, W=1, U=1).
                    // 用户页表低半区还含 KPTI 映射的 supervisor 页 (USER_CR3_SAVE,
                    // SyscallPerCpu, GDT/IDT/TSS, IST 栈, RSP0 栈), 这些页无 USER 位.
                    // 原实现仅判 W 位, fork 时把这些内核页 WRITABLE 清除 → 用户态异常
                    // 入口写 USER_CR3_SAVE → 写保护 #PF → 死循环/Triple Fault.
                    if (parent_flags & 2) != 0 && (parent_flags & 4) != 0 {
                        // SAFETY: 该 PTE 由本函数独占访问 (外层持有 VMM_LOCK)
                        unsafe {
                            let mut pte = parent_pt_virt.add(l).read_volatile();
                            pte &= !2u64; // clear WRITABLE
                            parent_pt_virt.add(l).write_volatile(pte);
                        }

                        let mut child_pte = parent_pte;
                        child_pte &= !2u64;
                        // SAFETY: child_pt_virt 指向有效 PT 页
                        unsafe {
                            child_pt_virt.add(l).write_volatile(child_pte);
                        }

                        // fork: 父子各持引用, count 从 1 变为 2
                        cow_inc_ref(parent_phys);
                    } else {
                        // supervisor 页 (KPTI 数据/IDT/GDT/IST/RSP0) 或已只读的页:
                        // 直接共享 PTE 内容, 不参与 COW
                        // SAFETY: 已只读的页直接共享 PTE 内容
                        unsafe {
                            child_pt_virt.add(l).write_volatile(parent_pte);
                        }
                    }
                }
            }
        }
    }

    // SMP: 刷新 TLB 使所有被清除 WRITABLE 位的 PTE 失效
    // 父进程可能在其他 CPU 上运行, 完整的 TLB shootdown 需要 IPI
    // 此处至少刷新本地 TLB 确保当前 CPU 的一致性
    crate::arch!(tlb_flush_all());

    Some(child_pml4_phys.as_u64())
}

/// COW fault 处理: 为写入分配新页
///
/// # SMP Safety
/// 所有页表修改通过 VMM 的 `map_page_in_table` 进行, 该函数内部持有 `VMM_LOCK`
/// 并执行 TLB 刷新, 保证多核并发安全。
// 有意窄化: 显式收窄, 调用方保证值域
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
pub fn cow_handle_fault(pml4: u64, fault_addr: u64) -> Option<u64> {
    let vmm_inst = vmm::get_vmm();
    let page_aligned = fault_addr & !(PAGE_SIZE - 1);

    let old_phys = vmm_inst.get_physical_in_pml4(pml4, VirtAddr(page_aligned))?;
    let old_frame = old_phys.as_u64() & 0x000FFFFFFFFFF000;

    let should_reuse = {
        let mut guard = COW_REFS.lock();
        let refs = guard.as_mut()?;
        match refs.get_mut(&old_frame) {
            Some(count) if *count <= 1 => {
                refs.remove(&old_frame);
                true
            }
            Some(_) => false,
            None => true,
        }
    };

    if should_reuse {
        // 引用计数 ≤ 1: 直接恢复 WRITABLE 位, 无需分配新页
        // map_page_in_table 内部持有 VMM_LOCK + TLB 刷新, SMP 安全
        let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
        vmm_inst.map_page_in_table(pml4, VirtAddr(page_aligned), old_phys, flags);
        return Some(old_phys.as_u64());
    }

    let pmm_inst = super::pmm::get_pmm();
    let new_phys = pmm_inst.alloc_page()?;
    let new_virt = new_phys.to_virt();

    let old_virt = PhysAddr(old_frame).to_virt();
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::ptr::copy_nonoverlapping(
            old_virt.0 as *const u8,
            new_virt.0 as *mut u8,
            PAGE_SIZE as usize,
        );
    }

    if cow_dec_ref(old_frame) {
        pmm_inst.free_page(PhysAddr(old_frame));
    }

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    // map_page_in_table 内部持有 VMM_LOCK + TLB 刷新, SMP 安全
    vmm_inst.map_page_in_table(pml4, VirtAddr(page_aligned), new_phys, flags);

    Some(new_phys.as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_key_alignment() {
        assert_eq!(frame_key(0x1000), 0x1000);
        assert_eq!(frame_key(0x1FFF), 0x1000);
        assert_eq!(frame_key(0x2000), 0x2000);
    }

    #[test]
    fn test_virt_index_functions() {
        let v = VirtAddr(0x0000_7FFF_0000_0000u64);
        assert_eq!(v.pml4_idx(), 0);
        assert_eq!(v.pt_idx(), 0);
    }
}
