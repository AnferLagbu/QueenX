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
//! 5. 帧持有计数语义 (cr3 所有权, `docs/plan/cr3-lifetime-ownership.md` §3.1/§5.6):
//!    alloc 置 1、inc/dec 配对、归零恰好一次、未归零不归还、未计数帧拒绝登记、
//!    `should_reuse` 判据边界 (§8.1)、pfn 越界 fail-closed (MMIO 地址)

use queenx::kernel::framework::mm::PhysAddr;
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

// ---- 帧持有计数语义 (cr3-lifetime-ownership.md §3.1) ----
// 计数面: 0 = 未计数帧, n >= 1 = n 个持有者. 唯一销毁/归还判据是"计数归零".

#[test]
fn pmm_frame_count_alloc_starts_at_one() {
    let pmm = setup_pmm(0);
    let a = pmm.alloc_page().expect("分配应成功");
    assert_eq!(pmm.frame_ref_count(a), 1, "alloc_page 后分配者即唯一持有者");
    pmm.free_page(a);
    assert_eq!(pmm.frame_ref_count(a), 0, "归还后计数归零");
}

#[test]
fn pmm_frame_inc_dec_paired() {
    let pmm = setup_pmm(0);
    let a = pmm.alloc_page().expect("分配应成功");
    // 共享方登记: 计数 1 -> 2
    assert!(pmm.frame_inc(a), "计数态帧应可登记持有者");
    assert_eq!(pmm.frame_ref_count(a), 2);
    // 注销一个持有者: 仍有持有者时不报告归零
    assert!(!pmm.frame_dec(a), "仍有持有者时不得报告归零");
    assert_eq!(pmm.frame_ref_count(a), 1);
    // 注销最后一个持有者: 报告归零
    assert!(pmm.frame_dec(a), "最后一个持有者注销应报告归零");
    assert_eq!(pmm.frame_ref_count(a), 0);
    pmm.free_page(a);
}

#[test]
fn pmm_frame_zero_reported_exactly_once() {
    let pmm = setup_pmm(0);
    let a = pmm.alloc_page().expect("分配应成功");
    assert!(pmm.frame_dec(a), "首个 dec 应归零");
    // fail-closed: 已归零 (等价未计数态) 的帧不得再次报告归零
    assert!(!pmm.frame_dec(a), "计数 0 上不得报告归零");
    assert!(!pmm.frame_dec(a), "重复 dec 仍不得报告归零");
    assert_eq!(pmm.frame_ref_count(a), 0);
    pmm.free_page(a);
}

#[test]
fn pmm_frame_release_only_when_zero() {
    let pmm = setup_pmm(0);
    let a = pmm.alloc_page().expect("分配应成功");
    assert!(pmm.frame_inc(a), "登记第二个持有者");
    // 丢弃一个持有者: 计数 2 -> 1, 仍有持有者 ⇒ 不得归还 PMM
    pmm.free_page(a);
    assert_eq!(pmm.frame_ref_count(a), 1);
    let mut held: Vec<PhysAddr> = Vec::new();
    for _ in 0..256 {
        let p = pmm.alloc_page().expect("仍可分配其他页");
        assert_ne!(p.0, a.0, "仍有持有者的帧不得被重新分配");
        held.push(p);
    }
    assert_eq!(held.len(), 256, "持续分配应成功");
    // 丢弃最后一个持有者: 计数 1 -> 0, 页归还
    pmm.free_page(a);
    assert_eq!(pmm.frame_ref_count(a), 0);
}

#[test]
fn pmm_frame_transfer_keeps_single_owner() {
    let pmm = setup_pmm(0);
    let a = pmm.alloc_page().expect("分配应成功");
    // 转移语义 = 不计数只移动 (§3.1 `cr3_transfer`): 计数保持 1, 不得翻倍
    // (翻倍即成为永不归还的泄漏).
    let moved = a;
    assert_eq!(
        pmm.frame_ref_count(moved),
        1,
        "转移不得改变持有计数 (否则永不归还)"
    );
    // 新持有者释放: 恰好一次归零
    pmm.free_page(moved);
    assert_eq!(pmm.frame_ref_count(moved), 0, "单所有者释放应归零");
}

/// `should_reuse` 判据边界 (§8.1): 计数 = 持有者数, `<= 1` 就地可写, `>= 2` 必须复制.
///
/// 判别力: 旧 `COW_REFS` 用"额外注册数"表达, 单次 fork 后计数仍为 1 ⇒ 误判唯一引用.
/// 本用例断言持有者语义下的判据取值, 与 [cow.rs] `cow_handle_fault` 的判据同源.
#[test]
fn pmm_should_reuse_predicate_boundary() {
    let pmm = setup_pmm(0);
    let a = pmm.alloc_page().expect("分配应成功");
    // 单一映射: 计数 1 ⇒ 判据成立 (就地恢复可写, 计数不变)
    assert!(
        pmm.frame_ref_count(a) <= 1,
        "单一映射应落在 should_reuse 分支"
    );
    // fork 的 COW 共享 (每 leaf 一次): 计数 1 -> 2 ⇒ 判据不成立 (必须复制)
    assert!(pmm.frame_inc(a), "登记共享持有者");
    assert!(
        pmm.frame_ref_count(a) > 1,
        "共享后计数 > 1 ⇒ 不得就地可写"
    );
    // 复制分支对旧帧的净效果: 递减 1 (仍被原持有者引用, 不报告归零)
    assert!(!pmm.frame_dec(a), "复制后旧帧仍被原持有者引用");
    assert_eq!(pmm.frame_ref_count(a), 1, "旧帧计数应递减 1");
    assert!(pmm.frame_dec(a), "最后持有者注销归零");
    pmm.free_page(a);
}

/// pfn 越界 fail-closed (§8.1 规则 4): 设备/MMIO 物理地址不参与帧计数.
///
/// MMIO 物理地址的 pfn 远超 `total_pages`, 若不加校验会越界读写计数区.
#[test]
fn pmm_frame_out_of_range_pfn_rejected() {
    let pmm = setup_pmm(0);
    // 0xFE00_0000 (framebuffer 典型地址) 远超模拟的 64MB 物理内存
    let mmio = PhysAddr(0xFE00_0000);
    assert_eq!(pmm.frame_ref_count(mmio), 0, "越界物理地址无计数");
    assert!(!pmm.frame_inc(mmio), "越界物理地址不得被登记持有者");
    assert!(!pmm.frame_dec(mmio), "越界物理地址不得报告归零 (防误释放)");
}

#[test]
fn pmm_frame_uncounted_block_rejected() {
    let pmm = setup_pmm(0);
    let blk = pmm.alloc_pages(ORDER9_PAGES).expect("order-9 分配应成功");
    // §3.2: 连续多帧块视为单一 holder, 不逐页计数 ⇒ 块内帧计数 0, 拒绝登记/注销
    assert_eq!(pmm.frame_ref_count(blk), 0, "order > 0 的块首帧不逐页计数");
    assert!(!pmm.frame_inc(blk), "未计数帧不得被登记持有者");
    assert!(!pmm.frame_dec(blk), "未计数帧不得报告归零 (防止误销毁)");
    pmm.free_pages(blk, ORDER9_PAGES);
}
