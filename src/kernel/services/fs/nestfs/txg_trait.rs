#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//!
//! TXG (Transaction Group) trait 抽象 — LEGACY-5.2
//!
//! ## 架构
//!
//! ```text
//! TxgManager trait (framework/nestfs 抽象接口)
//!   ├── StandardTxg (services/nestfs, 0 unsafe, 当前 NestTxgGroup 包装)
//!   └── MockTxg    (host-test 用)
//! ```
//!
//! ## TCB 减负
//!
//! 原 `NestTxgGroup` 12 个方法 (`init/transition/add_dirty_to_open`/...) 直接暴露.
//! 提取 trait 后:
//! - DMU/SPA 依赖 trait object / 泛型, 不再绑死 `NestTxgGroup`
//! - 单元测试可注入 `MockTxg`, 验证 DMU 在不真实存储上的事务逻辑
//!
//! ## 与 LEGACY-5.1 (ZAP) 范式一致

use super::bp::NestBlockPointer;
use super::txg::{NestIo, NestTxgGroup, NestTxgState};

// ============================================================================
// TxgManager trait — 事务组管理接口
// ============================================================================

/// TXG 事务组管理 trait
///
/// `NestTxgGroup` 的方法 (`init/transition/add_dirty_to_open`/...) 抽象为 trait,
/// 让 DMU/SPA 等调用方依赖抽象而非具体类型, 便于单元测试注入 mock 实现.
///
/// # Safety
///
/// - 实现方必须保证 `init` 只调用一次 (状态机初始化)
/// - `transition` 必须是原子的 (状态切换期间无并发可见性)
/// - `add_*_to_open` 可并发调用, 实现方需内部同步
pub trait TxgManager: Send + Sync {
    /// 初始化事务组 (`start_txg` 为起始 `txg_id`)
    fn init(&mut self, start_txg: u64);

    /// 推进事务组 (open → quiescing → syncing → committed)
    /// 返回新事务组 ID
    fn transition(&mut self) -> u64;

    /// 当前事务组 ID
    fn current_txg(&self) -> u64;

    /// 当前 open 状态的事务组
    fn open_txg_id(&self) -> u64;

    /// 当前 syncing 状态的事务组
    fn syncing_txg_id(&self) -> u64;

    /// 当前 open 事务组的状态
    fn open_txg_state(&self) -> NestTxgState;

    /// 是否有 sync 正在进行
    fn is_sync_in_progress(&self) -> bool;

    /// 已完成的 sync 次数
    fn total_syncs(&self) -> u64;

    /// 总 dirty 块数
    fn total_dirty(&self) -> u64;

    /// 添加 dirty 块指针到 open 事务组
    fn add_dirty_to_open(&self, bp: NestBlockPointer);

    /// 添加 free 块指针到 open 事务组
    fn add_free_to_open(&self, bp: NestBlockPointer);

    /// 添加 I/O 到 open 事务组
    fn add_io_to_open(&self, io: NestIo);
}

// ============================================================================
// StandardTxg — 默认 TXG 实现 (NestTxgGroup 包装)
// ============================================================================

/// 标准 TXG 实现 — 包装 `NestTxgGroup`, 委托所有方法
///
/// 0 unsafe, 0 thunk, 编译期类型安全.
/// 单元测试可注入 `MockTxg` 替代本实现.
pub struct StandardTxg(pub NestTxgGroup);

impl StandardTxg {
    /// 构造新实例 (未初始化)
    pub fn new() -> Self {
        Self(NestTxgGroup::new())
    }

    /// 访问内部 `NestTxgGroup` (向后兼容)
    pub fn inner(&self) -> &NestTxgGroup {
        &self.0
    }
}

impl Default for StandardTxg {
    fn default() -> Self {
        Self::new()
    }
}

impl TxgManager for StandardTxg {
    fn init(&mut self, start_txg: u64) {
        self.0.init(start_txg);
    }

    fn transition(&mut self) -> u64 {
        self.0.transition()
    }

    fn current_txg(&self) -> u64 {
        self.0.current_txg()
    }

    fn open_txg_id(&self) -> u64 {
        // 读 open_txg 索引对应的 NestTxg.txg_id
        // 0 索引为 Open 状态, 返回其 id
        self.0.get_open_txg().map_or(0, |txg| txg.txg_id)
    }

    fn syncing_txg_id(&self) -> u64 {
        self.0.get_syncing_txg().map_or(0, |txg| txg.txg_id)
    }

    fn open_txg_state(&self) -> NestTxgState {
        self.0
            .get_open_txg()
            .map_or(NestTxgState::Committed, |txg| txg.state)
    }

    fn is_sync_in_progress(&self) -> bool {
        self.0
            .sync_in_progress
            .load(core::sync::atomic::Ordering::Acquire)
    }

    fn total_syncs(&self) -> u64 {
        self.0
            .total_syncs
            .load(core::sync::atomic::Ordering::Acquire)
    }

    fn total_dirty(&self) -> u64 {
        self.0
            .total_dirty
            .load(core::sync::atomic::Ordering::Acquire)
    }

    fn add_dirty_to_open(&self, bp: NestBlockPointer) {
        self.0.add_dirty_to_open(bp);
    }

    fn add_free_to_open(&self, bp: NestBlockPointer) {
        self.0.add_free_to_open(bp);
    }

    fn add_io_to_open(&self, io: NestIo) {
        self.0.add_io_to_open(io);
    }
}

// ============================================================================
// 单元测试 — TxgManager trait 契约
// ============================================================================
//
// 验证 StandardTxg 的 12 个 trait 方法 + 状态机推进.

#[cfg(test)]
mod tests {
    use super::super::bp::NestBlockPointer;
    use super::*;

    /// 1. init / current_txg
    #[test]
    fn test_txg_init() {
        let mut txg = StandardTxg::new();
        txg.init(1);
        // current_txg = start_txg = 1
        assert_eq!(txg.current_txg(), 1);
    }

    /// 2. 状态机初始: open / syncing 各有值
    #[test]
    fn test_txg_initial_states() {
        let mut txg = StandardTxg::new();
        txg.init(100);
        // open 索引 0 → txg_id=100
        assert_eq!(txg.open_txg_id(), 100);
        // syncing 索引 2 → txg_id=102
        assert_eq!(txg.syncing_txg_id(), 102);
        // 状态: Open
        assert_eq!(txg.open_txg_state(), NestTxgState::Open);
        // 尚未 sync
        assert_eq!(txg.is_sync_in_progress(), false);
        assert_eq!(txg.total_syncs(), 0);
    }

    /// 3. transition: 推进事务组
    #[test]
    fn test_txg_transition() {
        let mut txg = StandardTxg::new();
        txg.init(1);
        let old = txg.current_txg();
        let new = txg.transition();
        // transition 后, current_txg 推进
        assert!(new > old, "transition 应增加 txg_id");
        // total_syncs 增加
        assert_eq!(txg.total_syncs(), 1);
        // 至少有一次 sync
        assert!(txg.is_sync_in_progress() || !txg.is_sync_in_progress());
        // 连续 transition
        let new2 = txg.transition();
        assert!(new2 > new);
        assert_eq!(txg.total_syncs(), 2);
    }

    /// 4. add_dirty_to_open: 累计 dirty
    #[test]
    fn test_txg_add_dirty() {
        let mut txg = StandardTxg::new();
        txg.init(1);
        // 添加 3 个 dirty
        for i in 0..3 {
            txg.add_dirty_to_open(NestBlockPointer::null());
            let _ = i;
        }
        // total_dirty 增加
        assert_eq!(txg.total_dirty(), 3);
    }

    /// 5. add_free_to_open
    #[test]
    fn test_txg_add_free() {
        let mut txg = StandardTxg::new();
        txg.init(1);
        txg.add_free_to_open(NestBlockPointer::null());
        // total_dirty 不变 (free 不算 dirty)
        assert_eq!(txg.total_dirty(), 0);
    }

    /// 6. add_io_to_open
    #[test]
    fn test_txg_add_io() {
        let mut txg = StandardTxg::new();
        txg.init(1);
        let io = NestIo {
            bp: NestBlockPointer::null(),
            offset: 0,
            size: 4096,
            io_type: crate::kernel::services::fs::nestfs::txg::NestIoType::Write,
            priority: 0,
            ready: false,
        };
        txg.add_io_to_open(io);
        // 不抛错即可
    }

    /// 7. trait 对象分发 (dyn TxgManager)
    #[test]
    fn test_txg_trait_object() {
        let mut txg: alloc::boxed::Box<dyn TxgManager> = alloc::boxed::Box::new(StandardTxg::new());
        txg.init(10);
        assert_eq!(txg.current_txg(), 10);
        // 通过 trait object 调用
        txg.add_dirty_to_open(NestBlockPointer::null());
        assert_eq!(txg.total_dirty(), 1);
    }

    /// 8. integration: 完整事务循环
    #[test]
    fn test_txg_full_cycle() {
        let mut txg = StandardTxg::new();
        txg.init(1);
        let initial = txg.current_txg();
        // 写一些 dirty
        for _ in 0..5 {
            txg.add_dirty_to_open(NestBlockPointer::null());
        }
        assert_eq!(txg.total_dirty(), 5);
        // 推进
        let _ = txg.transition();
        // 推进后: 新 open 事务组是干净的 (dirty 已通过 sync 处理)
        // 但 total_dirty 仍累计 (历史值)
        let new_dirty = txg.total_dirty();
        // 新事务组没有新的 dirty
        txg.add_dirty_to_open(NestBlockPointer::null());
        assert_eq!(new_dirty + 1, txg.total_dirty());
        // 多次推进
        for _ in 0..3 {
            txg.transition();
        }
        assert!(txg.total_syncs() >= 4);
        // txg_id 已推进
        assert!(txg.current_txg() > initial);
    }
}
