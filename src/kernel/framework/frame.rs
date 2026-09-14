//! Frame — 物理页安全抽象 (TCB)
//!
//! 将裸 `PhysAddr` 封装为带引用计数的类型安全句柄，
//! 防止 double-free / use-after-free / DMA 竞争。
//!
//! ## 与 Asterinas OSTD `Frame` 的关系
//!
//! 等价于 OSTD 的 `Frame<M>` 概念：每个物理地址被唯一拥有，
//! 释放时核查引用计数为零。元数据槽位 (`usize`) 可供
//! services 层挂载自定义状态（如 slab 缓存索引、DMA pin 标志）。
//!
//! ## SAFETY 不变量
//!
//! - **唯一所有权**: 任意时刻最多一个 `Frame` 实例持有一个物理地址。
//! - **释放前清理**: 释放 Frame 前确保无 DMA 缓冲区 / 页表条目引用。
//! - **对齐**: Frame 地址始终对齐到 `PAGE_SIZE` 边界。
//! - `from_raw()` 是唯一 unsafe 构造路径；services 层通过 `FrameAlloc::alloc()` 获取。

use core::fmt;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::framework::mm::PAGE_SIZE;
use crate::framework::mm::PhysAddr;

/// 一个带引用计数和自定义元数据的物理帧。
///
/// # Safety Invariant
/// 每个物理地址在同一时刻最多被一个 Frame 实例持有。
#[derive(Debug)]
pub struct Frame {
    phys: PhysAddr,
    ref_count: AtomicU32,
    order: u8,
    meta: usize,
}

impl Frame {
    /// 从裸物理地址构造 Frame。
    ///
    /// # SAFETY
    /// 调用方保证 `phys` 是有效的可分配物理地址，
    /// 且未被其他 `Frame` 实例持有。
    pub unsafe fn from_raw(phys: PhysAddr, order: u8) -> Self {
        debug_assert!(
            phys.as_u64().is_multiple_of(PAGE_SIZE as u64),
            "Frame must be page-aligned"
        );
        Self {
            phys,
            ref_count: AtomicU32::new(1),
            order,
            meta: 0,
        }
    }

    /// 物理地址
    #[inline(always)]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    pub fn phys(&self) -> PhysAddr {
        self.phys
    }

    /// Buddy 阶数 (0 = 4KB, 9 = 2MB)
    #[inline(always)]
    pub fn order(&self) -> u8 {
        self.order
    }

    /// 帧大小 (字节)
    // 有意窄化: 用户内存代理, 指针/长度上下文保证
    #[expect(clippy::cast_possible_truncation)]
    pub fn size(&self) -> usize {
        (PAGE_SIZE as usize) << self.order
    }

    /// 当前引用计数
    #[inline(always)]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    pub fn ref_count(&self) -> u32 {
        self.ref_count.load(Ordering::Acquire)
    }

    /// 增加引用计数 (如被页表映射、DMA 缓冲引用)
    #[inline(always)]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    pub fn inc_ref(&self) {
        self.ref_count.fetch_add(1, Ordering::AcqRel);
    }

    /// 减少引用计数。返回 true 表示计数归零，可物理释放。
    #[inline(always)]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    pub fn dec_ref(&self) -> bool {
        let prev = self.ref_count.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(prev > 0, "Frame ref_count underflow");
        prev == 1
    }

    /// 自定义元数据（services 可挂载任意 usize 值）
    #[inline(always)]
    #[expect(
        clippy::inline_always,
        reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
    )]
    pub fn meta(&self) -> usize {
        self.meta
    }

    /// 设置自定义元数据
    #[inline(always)]
    pub fn set_meta(&mut self, val: usize) {
        self.meta = val;
    }

    /// 转换为内核可访问的虚拟地址
    ///
    /// # B03-26 警告
    /// 返回 `*mut u8` 无生命周期绑定, Frame Drop 后指针悬挂。
    /// 优先使用 `as_virt_slice()` 安全 API (生命周期绑定 `&self`)。
    /// 仅在 FFI / DMA 等需要 raw pointer 的场景保留。
    pub fn as_virt_ptr(&self) -> *mut u8 {
        crate::framework::mm::phys_to_virt(self.phys.as_u64()) as *mut u8
    }

    /// B03-26: 安全 API — 返回生命周期绑定 `&self` 的可变字节切片。
    /// Frame Drop 后切片自动失效, 防止悬挂指针 (I1 安全不变式)。
    pub fn as_virt_slice(&mut self) -> &mut [u8] {
        let ptr = self.as_virt_ptr();
        // SAFETY: Frame 拥有物理页, phys_to_virt 返回唯一内核映射 VA。
        // `&mut self` 保证独占借用, Frame 生命周期内指针有效。
        unsafe {
            core::slice::from_raw_parts_mut(
                ptr,
                crate::framework::mm::PAGE_SIZE as usize,
            )
        }
    }

    /// 零填充帧内容
    pub fn zero(&self) {
        let ptr = self.as_virt_ptr();
        // SAFETY: `as_virt_ptr()` 由 `phys_to_virt` 转换物理地址到内核虚拟地址;
        // `Frame` 持有的 `phys` 由 `FrameAlloc::allocate_frame` 返回, 保证:
        //   1. 物理地址是已分配 (free list 中扣除) 的 4K 对齐页
        //   2. 物理地址在 `phys_to_virt` 线性映射范围内 (内核高半区直接映射)
        //   3. `self.size()` 字节全部可写, 写 0 不会破坏其他数据结构
        //   4. 期间无并发写 (Frame 所有权唯一, 由 FrameAlloc 跟踪)
        unsafe {
            core::ptr::write_bytes(ptr, 0, self.size());
        }
    }
}

impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Frame(phys=0x{:x}, order={}, ref={})",
            self.phys.as_u64(),
            self.order,
            self.ref_count()
        )
    }
}

// SAFETY: Frame 是堆分配对象，Send + Sync 来自 AtomicU32 的安全并发访问。
unsafe impl Send for Frame {}
unsafe impl Sync for Frame {}
