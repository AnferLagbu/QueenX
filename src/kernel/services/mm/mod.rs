#![deny(unsafe_code)]
//! 内存管理 — services 层安全代理
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 framework::mm。
//!
//! ## 职责
//!
//! - Page Cache: 文件内容缓存的安全 API
//! - Swap: 页面换出/换入的安全 API
//! - mmap: 文件映射的安全参数验证与 VFS 交互
//! - PMM 策略: 阶数选择/碎片化评估/回收阈值/水位线

pub mod brk;
/// madvise / mlock / mincore 系统调用策略
pub mod madvise_mlock;
/// D9: 内存压力策略 (阈值/分级/判定) — services 层
pub mod memory_pressure;
pub mod mmap;
pub mod mprotect;
pub mod mremap;
/// D3: NUMA 安全封装
pub mod numa;
pub mod pcache;
/// T2-2: PMM 策略 (阶数选择/碎片化/回收阈值/水位线) — services 层
pub mod pmm_policy;
/// T2-3: Slab 策略 (缓存大小选择/对象数计算/分配优先级/大小限制) — services 层
pub mod slab_policy;
pub mod swap;
/// T2-4: Swap 策略 (LRU 管理/回收决策/kswapd 触发) — services 层
pub mod swap_policy;
/// T1 G4: userfaultfd 系统调用/ioctl ABI 层
pub mod uffd;

// memory_pressure 公共接口 re-export — T-02 策略-机制分离
pub use memory_pressure::{PressureAwareAllocPolicy, register_pressure_aware_policy};

/// `services::mm` 初始化 — 注册策略到 framework
///
/// 在 `framework::mm` 初始化完成后调用一次.
pub fn init() {
    // T-02: 注册 services 层分配决策策略
    let _ = memory_pressure::register_pressure_aware_policy();
    // DECISION-O ②: 注册内存压力分级策略 (算法在 services, 状态在 framework)
    let _ = memory_pressure::register_pressure_classifier();
    // T2-2: 注册 services 层 PMM 策略
    let _ = pmm_policy::register_default_pmm_policy();
    // T2-3: 注册 services 层 Slab 策略
    let _ = slab_policy::register_default_slab_policy();
    // T2-4: 注册 services 层 Swap 策略
    let _ = swap_policy::register_default_swap_policy();
}
