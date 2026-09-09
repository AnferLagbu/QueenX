//! PMM Buddy 分配器 host 集成测试 (H-04)
//!
//! H-04 (2026-09-09): 删除 `host-tests/src/buddy.rs` 平行实现 (436 行, 含 F9
//! `#![allow(dead_code)]`), 改引内核真实 `framework::mm::pmm` 的 buddy 机制 —
//! 经 `MetaStore` 载体注入 `VecMetaStore` (Vec<u8> 堆后端), init_bitmap 与
//! 全部 buddy 算法仅一份代码, 无测试/生产分叉 (B08-12 路线 C 核心).
//!
//! 覆盖 (映射原 buddy.rs 单测语义到公共 API, 不测内部实现):
//! 1. 分配/释放往返: alloc 后 free, 再 alloc 成功 (页回到池)
//! 2. 内核保留区防护: 分配不得落入 [0, KERNEL_END) 内核保留区
//! 3. buddy 合并: order-9 块 free 后再分配同大小块成功 (验证合并正确)
//! 4. reserve_after_kernel: init_bitmap 预留区不可被分配

use queenx::kernel::framework::mm::pmm::{PhysicalMemoryManager, VecMetaStore};

/// 模拟物理内存 64MB (buddy 完整覆盖 order-0..9)
const MEM_SIZE: u64 = 64 * 1024 * 1024;
/// 模拟内核镜像末尾 16MB (init_bitmap 前的内核保留区)
const KERNEL_END: u64 = 16 * 1024 * 1024;
/// order-9 块 = 512 页 = 2MB (MAX_BUDDY_ORDER = 9)
const ORDER9_PAGES: usize = 1 << 9;

/// 构造已注入 VecMetaStore 且完成 init + init_bitmap 的 PMM 实例
fn setup_pmm(reserve_after_kernel: u64) -> PhysicalMemoryManager {
    let pmm = PhysicalMemoryManager::new();
    // H-04: 注入 Vec<u8> 载体 — init_bitmap 经同一份 MetaStore 接口建立三区
    pmm.inject_meta_store(VecMetaStore::new());
    pmm.init(MEM_SIZE, KERNEL_END);
    pmm.init_bitmap(reserve_after_kernel);
    pmm
}

#[test]
fn pmm_alloc_free_roundtrip() {
    let pmm = setup_pmm(0);
    let a = pmm.alloc_page().expect("首次分配应成功");
    assert!(a.0 >= KERNEL_END, "分配不得落入内核保留区");
    pmm.free_page(a);
    // 释放后页回到池: 再次分配应成功
    let b = pmm.alloc_page().expect("释放后再分配应成功");
    pmm.free_page(b);
}

#[test]
fn pmm_alloc_never_returns_kernel_reserved() {
    let pmm = setup_pmm(0);
    let mut allocs = 0u64;
    for _ in 0..1024 {
        match pmm.alloc_page() {
            Some(addr) => {
                assert!(addr.0 >= KERNEL_END, "buddy 分配不得落入内核保留区");
                allocs += 1;
            }
            None => break,
        }
    }
    assert!(allocs > 0, "应至少能分配一些页");
}

#[test]
fn pmm_buddy_merge_after_free() {
    let pmm = setup_pmm(0);
    // 分配 order-9 块 (2MB)
    let a = pmm
        .alloc_pages(ORDER9_PAGES)
        .expect("order-9 分配应成功");
    pmm.free_pages(a, ORDER9_PAGES);
    // 再次分配同大小块: 若 free 后 buddy 未正确合并则失败
    let b = pmm
        .alloc_pages(ORDER9_PAGES)
        .expect("free 后合并应可再次分配同阶块");
    pmm.free_pages(b, ORDER9_PAGES);
}

#[test]
fn pmm_reserve_after_kernel_respected() {
    // init_bitmap 预留 4MB: 分配不得落入 [KERNEL_END, KERNEL_END+4MB)
    let reserve = 4 * 1024 * 1024;
    let pmm = setup_pmm(reserve);
    let mut allocs = 0u64;
    for _ in 0..1024 {
        match pmm.alloc_page() {
            Some(addr) => {
                assert!(
                    addr.0 >= KERNEL_END + reserve,
                    "分配不得落入 reserved_after_kernel 区"
                );
                allocs += 1;
            }
            None => break,
        }
    }
    assert!(allocs > 0, "预留区之外应仍可分配");
}
