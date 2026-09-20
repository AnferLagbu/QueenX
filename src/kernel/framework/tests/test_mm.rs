use super::check;
use crate::framework::mm::KERNEL_BASE;
use crate::framework::mm::{
    MemoryInfo, PAGE_SIZE, PageFlags, PageSize, PageTableEntry, PhysAddr, VirtAddr,
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

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
/// VMA 级 NUMA 策略: mbind 落地路径 (设置 / 查询 / 覆盖校验 / 清除)
fn test_vma_numa_policy() -> TestResult {
    use crate::framework::mm::numa::{NumaPolicy, NumaRangePolicy};
    use crate::framework::mm::vma::{MmStruct, Vma, VmaType};

    let mm = MmStruct::new();
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    let vma = Vma::new(0x400000, 0x402000, flags, VmaType::Anonymous);
    check!(mm.insert_vma(vma).is_ok(), "insert_vma failed");

    // 未设置 → None (回退进程级策略)
    check!(
        mm.numa_policy_at(0x400000).is_none(),
        "policy should start empty"
    );

    let policy = NumaRangePolicy {
        mode: NumaPolicy::Bind,
        nodemask: 0b01,
    };
    match mm.set_numa_policy_range(0x400000, 0x402000, Some(policy)) {
        Ok(bytes) => check!(bytes == 0x2000, "covered bytes mismatch"),
        Err(_) => return TestResult::Fail("set_numa_policy_range failed"),
    }
    check!(
        mm.numa_policy_at(0x401000) == Some(policy),
        "policy query mismatch"
    );

    // 空洞范围 (无 VMA 覆盖) → EFAULT
    check!(
        mm.set_numa_policy_range(0x500000, 0x501000, Some(policy))
            .is_err(),
        "hole range should fail"
    );
    // 部分覆盖 (超出 VMA 边界) → EFAULT (SIMPLIFIED: 无 split_vma)
    check!(
        mm.set_numa_policy_range(0x400000, 0x403000, Some(policy))
            .is_err(),
        "partial overlap should fail"
    );
    // 失败调用不得改动已生效策略
    check!(
        mm.numa_policy_at(0x400000) == Some(policy),
        "failed call must not modify policy"
    );

    // MPOL_DEFAULT 语义: 清除范围策略
    check!(
        mm.set_numa_policy_range(0x400000, 0x402000, None).is_ok(),
        "clear failed"
    );
    check!(
        mm.numa_policy_at(0x400000).is_none(),
        "policy should be cleared"
    );
    TestResult::Pass
}

/// `MPOL_*` ABI 模式值 → `NumaPolicy` 映射 (与枚举判别值不同的编码)
fn test_numa_policy_linux_mode() -> TestResult {
    use crate::framework::mm::numa::{
        MPOL_BIND, MPOL_DEFAULT, MPOL_INTERLEAVE, MPOL_PREFERRED, NumaPolicy,
    };

    check!(
        NumaPolicy::from_linux_mode(MPOL_DEFAULT) == Some(NumaPolicy::Default),
        "MPOL_DEFAULT mapping mismatch"
    );
    check!(
        NumaPolicy::from_linux_mode(MPOL_PREFERRED) == Some(NumaPolicy::Preferred),
        "MPOL_PREFERRED mapping mismatch"
    );
    check!(
        NumaPolicy::from_linux_mode(MPOL_BIND) == Some(NumaPolicy::Bind),
        "MPOL_BIND mapping mismatch"
    );
    check!(
        NumaPolicy::from_linux_mode(MPOL_INTERLEAVE) == Some(NumaPolicy::Interleave),
        "MPOL_INTERLEAVE mapping mismatch"
    );
    check!(
        NumaPolicy::from_linux_mode(4).is_none(),
        "unknown mode should be rejected"
    );
    TestResult::Pass
}

// ============================================================================
// T1 G4: userfaultfd
// ============================================================================

/// userfaultfd 实例生命周期 + `UFFDIO_API` 握手
fn test_uffd_instance_lifecycle() -> TestResult {
    use crate::framework::mm::uffd::{
        UFFDIO_COPY, UFFDIO_REGISTER, UFFDIO_UNREGISTER, UFFDIO_WAKE, UFFDIO_ZEROPAGE, UFFD_API,
    };
    use crate::framework::mm::{
        api_negotiate, is_uffd_fd, uffd_create, uffd_is_active, uffd_is_open, uffd_release,
    };

    let Some(fd) = uffd_create(4242) else {
        return TestResult::Fail("uffd_create failed");
    };
    check!(is_uffd_fd(fd), "created fd should be recognised as uffd");
    check!(uffd_is_open(fd), "new instance should be open");
    check!(!uffd_is_active(fd), "should be inactive before UFFDIO_API");

    // 版本校验: 不支持的 api 值 → EINVAL
    check!(
        api_negotiate(fd, UFFD_API + 1, 0).is_err(),
        "unsupported api version should fail"
    );
    match api_negotiate(fd, UFFD_API, 0) {
        Ok(out) => {
            check!(out.api == UFFD_API, "api echo mismatch");
            // SIMPLIFIED: features 恒为 0 (仅支持 MISSING 模式, 见 framework/mm/uffd.rs)
            check!(out.features == 0, "features should be 0");
            let want =
                UFFDIO_REGISTER | UFFDIO_UNREGISTER | UFFDIO_WAKE | UFFDIO_COPY | UFFDIO_ZEROPAGE;
            check!(out.ioctls == want, "ioctls bitmap mismatch");
        }
        Err(_) => return TestResult::Fail("UFFDIO_API handshake failed"),
    }

    check!(uffd_release(fd), "release should succeed");
    check!(!uffd_is_open(fd), "released instance should be closed");
    check!(!uffd_release(fd), "double release should fail");
    check!(!is_uffd_fd(1), "smoltcp fd must not be recognised as uffd");
    TestResult::Pass
}

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
/// userfaultfd 参数校验 (不触及当前地址空间的前置校验分支)
fn test_uffd_register_arg_validation() -> TestResult {
    use crate::framework::mm::uffd::{UFFDIO_REGISTER_MODE_MISSING, UFFD_API};
    use crate::framework::mm::{
        api_negotiate, provide_page, uffd_create, uffd_register, uffd_release, uffd_unregister,
        uffd_wake,
    };

    let Some(fd) = uffd_create(1) else {
        return TestResult::Fail("uffd_create failed");
    };
    check!(api_negotiate(fd, UFFD_API, 0).is_ok(), "handshake failed");

    let mode = UFFDIO_REGISTER_MODE_MISSING;
    check!(
        uffd_register(fd, 0x400000, 0, mode).is_err(),
        "zero length should fail"
    );
    check!(
        uffd_register(fd, 0x400001, PAGE_SIZE, mode).is_err(),
        "unaligned start should fail"
    );
    check!(
        uffd_register(fd, 0x400000, PAGE_SIZE + 1, mode).is_err(),
        "unaligned len should fail"
    );
    check!(
        uffd_register(fd, 0x400000, PAGE_SIZE, 0).is_err(),
        "empty mode should fail"
    );
    check!(
        uffd_register(fd, 0x400000, PAGE_SIZE, 2).is_err(),
        "mode without MISSING should fail"
    );
    check!(
        uffd_register(5, 0x400000, PAGE_SIZE, mode).is_err(),
        "non-uffd fd should fail"
    );

    // 无匹配注册区间 → EINVAL
    check!(
        uffd_unregister(fd, 0x400000, PAGE_SIZE).is_err(),
        "unregister without match should fail"
    );
    // 无挂起缺页 → 唤醒为空操作 (Ok(false))
    match uffd_wake(fd, 0x400000, PAGE_SIZE) {
        Ok(woken) => check!(!woken, "wake without pending fault should be no-op"),
        Err(_) => return TestResult::Fail("wake should not fail"),
    }

    let full_page = [0u8; PAGE_SIZE as usize];
    check!(
        provide_page(fd, 0x400001, None).is_err(),
        "unaligned page should fail"
    );
    check!(
        provide_page(fd, 0x400000, Some(&[0u8; 8][..])).is_err(),
        "short data should fail"
    );
    check!(
        provide_page(fd, 0x400000, Some(&full_page[..])).is_err(),
        "provide without pending fault should fail"
    );

    check!(uffd_release(fd), "release should succeed");
    TestResult::Pass
}

/// host-test 无 PMM 初始化且无当前地址空间 → 跳过 (依赖裸机 #PF 与物理页)
#[cfg(feature = "host-test")]
fn test_uffd_register_and_fault_flow() -> TestResult {
    TestResult::Skip("E-04: host 无 PMM 初始化, 跳过 (依赖裸机物理内存分配)")
}

/// userfaultfd 注册 → 缺页登记 → 用户态提供页 → 重入映射 全流程
///
/// 包装层负责临时安装测试地址空间 (注册/缺页校验依赖 `CURRENT_MM`) 与 fd 回收,
/// 断言主体在 [`uffd_flow_body`] (单一出口保证 fd 不泄漏).
#[cfg(not(feature = "host-test"))]
fn test_uffd_register_and_fault_flow() -> TestResult {
    use crate::framework::mm::vma::{MmStruct, Vma, VmaType};
    use crate::framework::mm::{uffd_create, uffd_release, vma_set_current_mm};

    const BASE: u64 = 0x400000;

    let Some(fd) = uffd_create(1) else {
        return TestResult::Fail("uffd_create failed");
    };
    let mm = MmStruct::new();
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    let vma = Vma::new(
        BASE as usize,
        (BASE + 2 * PAGE_SIZE) as usize,
        flags,
        VmaType::Anonymous,
    );
    if mm.insert_vma(vma).is_err() {
        return TestResult::Fail("insert_vma failed");
    }

    // 临时安装测试地址空间 (测试运行器全程关中断, 无并发切换风险)
    let saved = crate::framework::mm::vma_get_current_mm().map(core::ptr::from_ref);
    vma_set_current_mm(core::ptr::from_ref(&mm));
    let result = uffd_flow_body(fd);
    match saved {
        Some(p) => vma_set_current_mm(p),
        None => vma_set_current_mm(core::ptr::null()),
    }

    if !uffd_release(fd) {
        return TestResult::Fail("release should succeed");
    }
    result
}

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
#[cfg(not(feature = "host-test"))]
fn uffd_flow_body(fd: i32) -> TestResult {
    use crate::framework::mm::uffd::{
        UFFDIO_REGISTER_MODE_MISSING, UFFD_API, UFFD_EVENT_PAGEFAULT, UFFD_PAGEFAULT_FLAG_WRITE,
    };
    use crate::framework::mm::{
        UffdFaultOutcome, api_negotiate, fault_notify, fill_provided_page, pop_event,
        provide_page, uffd_is_active, uffd_register, uffd_unregister,
    };

    /// `UffdInstance::ranges` 容量 (`framework/mm/uffd.rs::MAX_RANGES`)
    const MAX_RANGES: usize = 4;
    const BASE: u64 = 0x400000;
    const MODE: u64 = UFFDIO_REGISTER_MODE_MISSING;

    check!(api_negotiate(fd, UFFD_API, 0).is_ok(), "handshake failed");
    // 未被 VMA 覆盖的区间 → EFAULT
    check!(
        uffd_register(fd, 0x600000, PAGE_SIZE, MODE).is_err(),
        "unmapped range should fail"
    );
    // 同一区间重复注册至容量上限, 再注册 → ENOSPC
    for _ in 0..MAX_RANGES {
        check!(
            uffd_register(fd, BASE, PAGE_SIZE, MODE).is_ok(),
            "register within capacity failed"
        );
    }
    check!(
        uffd_register(fd, BASE, PAGE_SIZE, MODE).is_err(),
        "exceeding MAX_RANGES should fail"
    );
    check!(uffd_is_active(fd), "instance should be active after register");

    // 未注册页 → 交回内核 demand paging
    check!(
        fault_notify(BASE + PAGE_SIZE, 0) == UffdFaultOutcome::NotRegistered,
        "unregistered page should fall back to demand paging"
    );

    // 注册页缺页 → 事件入队 + Waiting
    check!(
        fault_notify(BASE, UFFD_PAGEFAULT_FLAG_WRITE) == UffdFaultOutcome::Waiting,
        "registered page fault should wait for user data"
    );
    match pop_event(fd) {
        Some(ev) => {
            check!(ev.event == UFFD_EVENT_PAGEFAULT, "event code mismatch");
            check!(ev.address == BASE, "fault address mismatch");
            check!(ev.flags == UFFD_PAGEFAULT_FLAG_WRITE, "fault flags mismatch");
        }
        None => return TestResult::Fail("page fault event should be queued"),
    }

    // 非挂起页 / 无挂起缺页 → EINVAL
    check!(
        provide_page(fd, BASE + PAGE_SIZE, None).is_err(),
        "providing a non-pending page should fail"
    );
    // SIMPLIFIED: 单挂起页模型 — 服务线程只能填充当前挂起页
    let filled = [0x5Au8; PAGE_SIZE as usize];
    check!(
        provide_page(fd, BASE, Some(&filled[..])).is_ok(),
        "provide pending page failed"
    );
    check!(
        fault_notify(BASE, 0) == UffdFaultOutcome::Ready,
        "second fault should observe provided data"
    );

    // 重入映射: 物理页填充 staged 数据 + 状态复位
    let pmm_inst = crate::framework::mm::pmm::get_pmm();
    let Some(phys) = pmm_inst.alloc_page() else {
        return TestResult::Fail("pmm alloc_page failed");
    };
    let ok = fill_provided_page(BASE, phys);
    // SAFETY: phys 由 PMM 分配, `to_virt` 给出有效内核虚拟地址; 该页尚未映射给任何
    // 用户地址空间, 测试独占访问, 仅读取校验 staged 数据是否已写入.
    let content_ok = {
        let base_ptr = phys.to_virt().0 as *const u8;
        let probes = [0usize, 1, 2048, PAGE_SIZE as usize - 1];
        probes
            .iter()
            .all(|&i| unsafe { core::ptr::read_volatile(base_ptr.add(i)) } == 0x5A)
    };
    pmm_inst.free_page(phys);
    check!(ok, "fill_provided_page should succeed");
    check!(content_ok, "provided page content mismatch");
    check!(
        provide_page(fd, BASE, None).is_err(),
        "provide after mapping should fail (state reset)"
    );

    // 注销全部区间后 → 交回内核 demand paging
    for _ in 0..MAX_RANGES {
        check!(
            uffd_unregister(fd, BASE, PAGE_SIZE).is_ok(),
            "unregister failed"
        );
    }
    check!(
        fault_notify(BASE, 0) == UffdFaultOutcome::NotRegistered,
        "after unregister should fall back to demand paging"
    );
    TestResult::Pass
}

/// host-test 无 PMM 初始化且无页表物理内存 → 跳过 (依赖裸机页表分配)
#[cfg(feature = "host-test")]
fn test_count_present_user_pages() -> TestResult {
    TestResult::Skip("E-04: host 无 PMM 初始化, 跳过 (依赖裸机页表分配)")
}

/// OOMD RSS 近似 (页表用户页计数) 的行为验证
///
/// 覆盖 `count_present_user_pages` 的三个关键性质:
/// 1. 空根 (`cr3 == 0`, 内核线程/无地址空间) → `Some(0)`, 不触碰任何内存
/// 2. 新建用户页表已含内核为进入用户态预置的低半区页 (GDT/IDT/TSS), 计数取为基线
/// 3. 低半区新增映射 N 个 4 KiB 页 → 计数恰好增加 N (不多不少)
///
/// 非阻塞语义: 该函数锁被占用时返回 `None`; 单测上下文无并发持锁者, 故期望 `Some`
/// (意外 `None` 即失败, 覆盖"非阻塞路径不误判").
#[cfg(not(feature = "host-test"))]
fn test_count_present_user_pages() -> TestResult {
    use crate::framework::mm::count_present_user_pages;
    use crate::framework::mm::mechanism::{
        pmm_alloc_page_phys, vmm_create_user_page_table, vmm_destroy_page_table,
        vmm_map_page_in_table,
    };

    const BASE: u64 = 0x40_0000;

    check!(
        count_present_user_pages(0) == Some(0),
        "null root must count 0"
    );

    let pml4 = vmm_create_user_page_table();
    check!(pml4 != 0, "create_user_page_table failed");

    // 基线非零: 新建用户页表已映射 GDT/IDT/TSS 等低半区页 (ring3 iretq/中断所需).
    // 计数正确性因此由"增量恰好等于新增映射数"判定, 而非绝对值.
    let Some(baseline) = count_present_user_pages(pml4) else {
        vmm_destroy_page_table(pml4);
        return TestResult::Fail("count_present_user_pages: VMM_LOCK 被占用 (单测上下文应可用)");
    };

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    for i in 0..3u64 {
        let Some(phys) = pmm_alloc_page_phys() else {
            vmm_destroy_page_table(pml4);
            return TestResult::Fail("pmm_alloc_page_phys failed");
        };
        vmm_map_page_in_table(pml4, BASE + i * PAGE_SIZE, phys.as_u64(), flags.bits());
    }

    let Some(counted) = count_present_user_pages(pml4) else {
        vmm_destroy_page_table(pml4);
        return TestResult::Fail("count_present_user_pages: VMM_LOCK 被占用 (单测上下文应可用)");
    };
    vmm_destroy_page_table(pml4);
    check!(
        counted == baseline + 3,
        "3 newly mapped 4K pages must increase count by exactly 3"
    );

    TestResult::Pass
}

// ============================================================
// 帧持有计数: COW 语义 (cr3-lifetime-ownership.md §8.1)
// ============================================================

/// COW 测试公共前置: 新建用户页表 + 映射一页 (以 `COW_TEST_BYTE` 填充).
///
/// 返回 `(pml4, 帧物理地址)`; 失败返回 `Err(消息)`. 调用方负责拆除页表.
#[cfg(not(feature = "host-test"))]
pub(super) fn cow_setup_mapped_page() -> Result<(u64, PhysAddr), &'static str> {
    use crate::framework::mm::mechanism::{
        vmm_create_user_page_table, vmm_destroy_page_table, vmm_map_page_in_table,
    };
    use crate::framework::mm::pmm::get_pmm;

    let pml4 = vmm_create_user_page_table();
    if pml4 == 0 {
        return Err("create_user_page_table failed");
    }
    let Some(phys) = get_pmm().alloc_page() else {
        vmm_destroy_page_table(pml4);
        return Err("pmm alloc_page failed");
    };
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    vmm_map_page_in_table(pml4, COW_TEST_VA, phys.as_u64(), flags.bits());
    // SAFETY: phys 由 PMM 分配, to_virt 得到内核直接映射地址; 写满一页不越界
    unsafe {
        core::ptr::write_bytes(phys.to_virt().0 as *mut u8, COW_TEST_BYTE, PAGE_SIZE as usize);
    }
    Ok((pml4, phys))
}

/// COW 测试用虚拟地址 (低半区, 页对齐; 与 `count_present_user_pages` 用例同区)
#[cfg(not(feature = "host-test"))]
const COW_TEST_VA: u64 = 0x40_0000;
/// COW 测试用父页填充字节
#[cfg(not(feature = "host-test"))]
const COW_TEST_BYTE: u8 = 0xAA;
/// COW 测试用子页改写字节
#[cfg(not(feature = "host-test"))]
const COW_TEST_BYTE_CHILD: u8 = 0xBB;

/// host-test 无 PMM/VMM 初始化 → 跳过 (依赖裸机页表与物理页)
#[cfg(feature = "host-test")]
fn test_cow_child_write_isolated_from_parent() -> TestResult {
    TestResult::Skip("E-04: host 无 VMM/PMM 初始化, 跳过 (依赖裸机页表分配)")
}

/// fork 后子写不污染父页 (§8.1 规则 2/3 的隔离判据).
///
/// **判别力**: 旧 `COW_REFS` 的"额外注册数"判据在单次 fork 后仍为 1 ⇒ 误判"唯一引用"
/// ⇒ 父子写同一物理页 (隔离失效). 新判据以**持有者数** (`>= 2` ⇒ 复制) 纠正之.
#[cfg(not(feature = "host-test"))]
fn test_cow_child_write_isolated_from_parent() -> TestResult {
    use crate::framework::mm::cow::{clone_user_page_table_cow, cow_handle_fault};
    use crate::framework::mm::mechanism::{vmm_destroy_page_table, vmm_get_physical_in_table};
    use crate::framework::mm::pmm::get_pmm;

    let pmm = get_pmm();
    let (parent, phys) = match cow_setup_mapped_page() {
        Ok(v) => v,
        Err(msg) => return TestResult::Fail(msg),
    };
    check!(
        pmm.frame_ref_count(phys) == 1,
        "首个映射不得额外 inc (§8.1 规则 1)"
    );

    // fork: 共享 leaf ⇒ 计数 1 -> 2
    let Some(child) = clone_user_page_table_cow(parent) else {
        vmm_destroy_page_table(parent);
        return TestResult::Fail("clone_user_page_table_cow failed");
    };
    check!(pmm.frame_ref_count(phys) == 2, "fork 共享后持有者数应为 2");
    check!(
        vmm_get_physical_in_table(child, COW_TEST_VA) == phys.as_u64(),
        "fork 后子页表应先共享同一帧"
    );

    // 子进程写入 ⇒ COW fault (计数 >= 2 ⇒ 必须复制)
    let Some(new_phys) = cow_handle_fault(child, COW_TEST_VA) else {
        vmm_destroy_page_table(child);
        vmm_destroy_page_table(parent);
        return TestResult::Fail("cow_handle_fault 未返回新帧");
    };
    check!(
        new_phys != phys.as_u64(),
        "计数 >= 2 时必须复制 (否则父子写同一页)"
    );
    check!(
        pmm.frame_ref_count(phys) == 1,
        "复制后旧帧应递减为 1 (父仍持有)"
    );
    check!(
        vmm_get_physical_in_table(child, COW_TEST_VA) == new_phys,
        "子页表应改指新帧"
    );

    // SAFETY: 两帧均由 PMM 分配, 内核直接映射可读同一字节偏移
    let parent_after = unsafe { *(phys.to_virt().0 as *const u8) };
    let child_copied = unsafe { *(PhysAddr(new_phys).to_virt().0 as *const u8) };
    check!(parent_after == COW_TEST_BYTE, "父页内容不得被子的复制扰动");
    check!(child_copied == COW_TEST_BYTE, "复制帧应含旧页内容");

    // SAFETY: 新帧由 PMM 分配, 写满一页不越界
    unsafe {
        core::ptr::write_bytes(
            PhysAddr(new_phys).to_virt().0 as *mut u8,
            COW_TEST_BYTE_CHILD,
            PAGE_SIZE as usize,
        );
    }
    // SAFETY: 父帧由 PMM 分配, 内核直接映射可读
    let parent_final = unsafe { *(phys.to_virt().0 as *const u8) };
    check!(parent_final == COW_TEST_BYTE, "子写新帧不得污染父页");

    // 拆除: 子先注销新帧与旧帧各一次, 父再注销旧帧 ⇒ 归零恰好一次
    vmm_destroy_page_table(child);
    check!(
        pmm.frame_ref_count(phys) == 1,
        "仅销毁子不得释放父仍在用的帧"
    );
    vmm_destroy_page_table(parent);
    check!(pmm.frame_ref_count(phys) == 0, "最后一个持有者拆除后计数归零");
    TestResult::Pass
}

/// host-test 无 PMM/VMM 初始化 → 跳过 (依赖裸机页表与物理页)
#[cfg(feature = "host-test")]
fn test_cow_shared_frame_survives_owner_exit() -> TestResult {
    TestResult::Skip("E-04: host 无 VMM/PMM 初始化, 跳过 (依赖裸机页表分配)")
}

/// "共享者仍在运行时所有者退出": 共享帧不得被归还 (§8.1 规则 3).
#[cfg(not(feature = "host-test"))]
fn test_cow_shared_frame_survives_owner_exit() -> TestResult {
    use crate::framework::mm::cow::clone_user_page_table_cow;
    use crate::framework::mm::mechanism::{vmm_destroy_page_table, vmm_get_physical_in_table};
    use crate::framework::mm::pmm::get_pmm;

    let pmm = get_pmm();
    let (parent, phys) = match cow_setup_mapped_page() {
        Ok(v) => v,
        Err(msg) => return TestResult::Fail(msg),
    };
    let Some(child) = clone_user_page_table_cow(parent) else {
        vmm_destroy_page_table(parent);
        return TestResult::Fail("clone_user_page_table_cow failed");
    };
    check!(pmm.frame_ref_count(phys) == 2, "fork 共享后持有者数应为 2");

    // 所有者 (父) 退出, 共享者 (子) 仍在运行
    vmm_destroy_page_table(parent);
    check!(
        pmm.frame_ref_count(phys) == 1,
        "所有者退出不得释放仍被共享的帧"
    );
    check!(
        vmm_get_physical_in_table(child, COW_TEST_VA) == phys.as_u64(),
        "共享者页表仍应指向该帧"
    );
    // SAFETY: 帧由 PMM 分配, 内核直接映射可读
    let content = unsafe { *(phys.to_virt().0 as *const u8) };
    check!(content == COW_TEST_BYTE, "共享帧内容在所有者退出后应保持");

    // 帧不得被归还 PMM: 连续分配不得再取到该帧
    let mut reissued = false;
    for _ in 0..64 {
        if let Some(p) = pmm.alloc_page() {
            if p.0 == phys.0 {
                reissued = true;
            }
            pmm.free_page(p);
        }
    }
    check!(!reissued, "仍被共享的帧不得被重新分配");

    // 共享者退出 (最后持有者) ⇒ 归零
    vmm_destroy_page_table(child);
    check!(
        pmm.frame_ref_count(phys) == 0,
        "最后一个持有者退出后计数归零"
    );
    TestResult::Pass
}

/// host-test 无 PMM/VMM 初始化 → 跳过 (依赖裸机页表与物理页)
#[cfg(feature = "host-test")]
fn test_cow_unique_mapping_fault_reuses_frame() -> TestResult {
    TestResult::Skip("E-04: host 无 VMM/PMM 初始化, 跳过 (依赖裸机页表分配)")
}

/// 唯一引用 ⇒ 就地恢复可写 (§8.1 `should_reuse` 判据的 `<= 1` 分支).
#[cfg(not(feature = "host-test"))]
fn test_cow_unique_mapping_fault_reuses_frame() -> TestResult {
    use crate::framework::mm::cow::cow_handle_fault;
    use crate::framework::mm::mechanism::{vmm_destroy_page_table, vmm_get_physical_in_table};
    use crate::framework::mm::pmm::get_pmm;

    let pmm = get_pmm();
    let (pml4, phys) = match cow_setup_mapped_page() {
        Ok(v) => v,
        Err(msg) => return TestResult::Fail(msg),
    };

    // 唯一映射 (计数 1): 判据判为"本映射即唯一引用" ⇒ 就地恢复可写, 不复制
    let reused = cow_handle_fault(pml4, COW_TEST_VA);
    check!(
        reused == Some(phys.as_u64()),
        "唯一引用必须就地恢复可写 (不得复制)"
    );
    check!(
        pmm.frame_ref_count(phys) == 1,
        "就地恢复不得改变持有者数"
    );
    check!(
        vmm_get_physical_in_table(pml4, COW_TEST_VA) == phys.as_u64(),
        "就地恢复后映射仍指向同一帧"
    );

    // 仍计入一次拆除
    vmm_destroy_page_table(pml4);
    check!(pmm.frame_ref_count(phys) == 0, "拆除后计数归零");
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
        "mm::numa": {
            "vma_policy": test_vma_numa_policy,
            "linux_mode": test_numa_policy_linux_mode,
        },
        "mm::vmm": {
            "count_present_user_pages": test_count_present_user_pages,
        },
        "mm::cow": {
            "child_write_isolated_from_parent": test_cow_child_write_isolated_from_parent,
            "shared_frame_survives_owner_exit": test_cow_shared_frame_survives_owner_exit,
            "unique_mapping_fault_reuses_frame": test_cow_unique_mapping_fault_reuses_frame,
        },
        "mm::uffd": {
            "lifecycle": test_uffd_instance_lifecycle,
            "arg_validation": test_uffd_register_arg_validation,
            "fault_flow": test_uffd_register_and_fault_flow,
        },
    }
}
