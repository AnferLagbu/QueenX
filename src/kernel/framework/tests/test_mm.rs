use super::check;
use crate::framework::mm::KERNEL_BASE;
use crate::framework::mm::{
    MemoryInfo, PageFlags, PageSize, PageTableEntry, PhysAddr, VirtAddr,
};
use crate::framework::mm::{
    pd_index, pdpt_index, phys_to_virt, pml4_index, pt_index, virt_to_phys,
};
use crate::framework::tests::{TestResult, runner};
use crate::register_tests_inner;

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
fn test_phys_addr() -> TestResult {
    let pa = PhysAddr::new(0x1000);
    check!(pa.as_u64() == 0x1000, "PhysAddr as_u64 mismatch");

    let aligned = pa.align_up(0x200000);
    check!(aligned.as_u64() == 0x200000, "align_up 2M mismatch");

    let down = pa.align_down(0x200000);
    check!(down.as_u64() == 0, "align_down 2M mismatch");

    let va = pa.to_virt();
    check!(va.as_u64() == KERNEL_BASE + 0x1000, "phys_to_virt mismatch");
    TestResult::Pass
}

fn test_virt_addr() -> TestResult {
    let va = VirtAddr::new(KERNEL_BASE + 0x2000);
    let pa = va.to_phys();
    check!(pa.as_u64() == 0x2000, "virt_to_phys mismatch");

    let idx = va.pml4_idx();
    check!(idx < 512, "pml4_idx should be < 512");

    let idx2 = va.pdpt_idx();
    check!(idx2 < 512, "pdpt_idx should be < 512");
    TestResult::Pass
}

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
fn test_page_size() -> TestResult {
    let s4k = PageSize::Size4K;
    check!(s4k.size() == 4096, "4K size mismatch");
    check!(s4k.shift() == 12, "4K shift mismatch");
    check!(s4k.is_aligned(0x1000), "0x1000 should be 4K aligned");
    check!(!s4k.is_aligned(0x100), "0x100 should not be 4K aligned");

    let s2m = PageSize::Size2M;
    check!(s2m.size() == 2 * 1024 * 1024, "2M size mismatch");
    check!(s2m.is_aligned(0x200000), "0x200000 should be 2M aligned");
    TestResult::Pass
}

fn test_page_table_entry() -> TestResult {
    let pte = PageTableEntry::new();
    check!(!pte.is_present(), "new PTE should not be present");

    pte.set_present(true);
    check!(pte.is_present(), "should be present after set");

    pte.set_writable(true);
    check!(pte.is_writable(), "should be writable after set");

    pte.set_frame(PhysAddr::new(0x1000));
    check!(pte.frame().as_u64() == 0x1000, "frame mismatch");

    let flags = pte.flags();
    check!(
        flags.contains(PageFlags::PRESENT),
        "flags should have PRESENT"
    );
    check!(
        flags.contains(PageFlags::WRITABLE),
        "flags should have WRITABLE"
    );
    TestResult::Pass
}

fn test_page_flags() -> TestResult {
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    check!(flags.contains(PageFlags::PRESENT), "should have PRESENT");
    check!(flags.contains(PageFlags::WRITABLE), "should have WRITABLE");
    check!(flags.contains(PageFlags::USER), "should have USER");
    check!(!flags.contains(PageFlags::NX), "should not have NX");

    let no_user = flags & !PageFlags::USER;
    check!(
        !no_user.contains(PageFlags::USER),
        "should not have USER after removal"
    );
    TestResult::Pass
}

fn test_memory_info() -> TestResult {
    let info = MemoryInfo::const_default();
    check!(info.total_pages == 0, "default total should be 0");
    check!(info.free_pages == 0, "default free should be 0");
    check!(info.used_pages == 0, "default used should be 0");
    TestResult::Pass
}

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
fn test_address_translation() -> TestResult {
    let phys: u64 = 0x1234000;
    let virt = phys_to_virt(phys);
    check!(virt == KERNEL_BASE + phys, "phys_to_virt mismatch");

    let back = virt_to_phys(virt);
    check!(back == phys, "virt_to_phys roundtrip mismatch");
    TestResult::Pass
}

fn test_page_index_helpers() -> TestResult {
    let addr: u64 = KERNEL_BASE | (1u64 << 39) | (2u64 << 30) | (3u64 << 21) | (4u64 << 12);
    let pml4 = pml4_index(addr);
    let pdpt = pdpt_index(addr);
    let pd = pd_index(addr);
    let pt = pt_index(addr);
    check!(pml4 < 512, "pml4 index out of range");
    check!(pdpt < 512, "pdpt index out of range");
    check!(pd < 512, "pd index out of range");
    check!(pt < 512, "pt index out of range");
    TestResult::Pass
}

pub fn register_mm_tests() {
    let r = runner();
    register_tests_inner! { r:
        "mm::phys_addr": {
            "basic": test_phys_addr,
        },
        "mm::virt_addr": {
            "basic": test_virt_addr,
        },
        "mm::page_size": {
            "basic": test_page_size,
        },
        "mm::pte": {
            "basic": test_page_table_entry,
        },
        "mm::page_flags": {
            "basic": test_page_flags,
        },
        "mm::memory_info": {
            "default": test_memory_info,
        },
        "mm::addr": {
            "translation": test_address_translation,
            "page_index": test_page_index_helpers,
        },
    }
}
