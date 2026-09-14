#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//!
//! ZIL (ZFS Intent Log) trait 抽象 — LEGACY-5.10
//!
//! ## 架构
//!
//! ```text
//! ZilLog trait (framework/nestfs 抽象接口)
//!   ├── StandardZil (services/nestfs, 0 unsafe, NestZil 包装)
//!   └── MockZilLog (host-test 用)
//! ```
//!
//! ## TCB 减负
//!
//! 原 `NestZil` 8 个公共方法 (`init/add_record/commit/sync/replay`/...) 直接暴露.
//! 提取 trait 后:
//! - SPA/DMU 调用方依赖 trait object / 泛型, 不再绑死 `NestZil`
//! - 单元测试可注入 `MockZilLog`, 验证 `zil_persist` 在不真实 ZIL 上的序列化和重放逻辑
//!
//! ## 与 LEGACY-5.1-5.5/5.7/5.8 范式一致

use super::bp::NestBlockPointer;
use super::zil::{NestZil, NestZilRecord};
use alloc::vec::Vec;

// ============================================================================
// ZilLog trait — Intent Log 接口
// ============================================================================

/// ZIL 写入日志 trait
///
/// `NestZil` 的核心方法 (`init/add_record/commit/sync/replay`) 抽象为 trait,
/// 让 SPA/DMU 等调用方依赖抽象而非具体类型, 便于单元测试注入 mock 实现.
///
/// # Safety
///
/// - `init` 应在系统启动时调用一次
/// - `add_record` 分配 sequence number, 调用方不应预设 seq
/// - `commit(txg)` 提交所有 txg <= 传入值的记录
/// - `sync` 是 commit + 同步到磁盘的组合
/// - `replay` 返回所有未提交记录 (按 seq 排序)
pub trait ZilLog: Send + Sync {
    /// 初始化
    fn init(&self);

    /// 是否启用
    fn is_enabled(&self) -> bool;

    /// 设置启用状态
    fn set_enabled(&self, enabled: bool);

    /// 添加记录 (自动分配 seq)
    fn add_record(&self, record: NestZilRecord);

    /// 当前 sequence (下一个待分配)
    fn current_seq(&self) -> u64;

    /// 已提交 sequence (最大)
    fn committed_seq(&self) -> u64;

    /// 是否有未提交记录
    fn has_uncommitted(&self) -> bool;

    /// 待处理记录数
    fn pending_count(&self) -> usize;

    /// 提交 txg <= 传入值的所有记录
    fn commit(&self, txg: u64);

    /// 同步到磁盘 (commit + flush)
    fn sync(&self, txg: u64);

    /// 是否正在 sync
    fn is_syncing(&self) -> bool;

    /// 重放 (返回未提交记录, 按 seq 排序)
    fn replay(&self) -> Vec<NestZilRecord>;

    /// 日志块指针
    fn log_bp(&self) -> NestBlockPointer;
}

// ============================================================================
// StandardZil — 默认 ZIL 实现 (NestZil 包装)
// ============================================================================

/// 标准 ZIL 实现 — 包装 `NestZil`, 委托所有方法
///
/// 0 unsafe, 0 thunk, 编译期类型安全.
/// 单元测试可注入 `MockZilLog` 替代本实现.
pub struct StandardZil(pub NestZil);

impl StandardZil {
    /// 构造新实例
    pub fn new() -> Self {
        Self(NestZil::new())
    }

    /// 访问内部 `NestZil` (向后兼容)
    pub fn inner(&self) -> &NestZil {
        &self.0
    }
}

impl Default for StandardZil {
    fn default() -> Self {
        Self::new()
    }
}

impl ZilLog for StandardZil {
    fn init(&self) {
        self.0.init();
    }

    fn is_enabled(&self) -> bool {
        self.0.enabled.load(core::sync::atomic::Ordering::Acquire)
    }

    fn set_enabled(&self, enabled: bool) {
        self.0
            .enabled
            .store(enabled, core::sync::atomic::Ordering::Release);
    }

    fn add_record(&self, record: NestZilRecord) {
        self.0.add_record(record);
    }

    fn current_seq(&self) -> u64 {
        self.0
            .current_seq
            .load(core::sync::atomic::Ordering::Acquire)
    }

    fn committed_seq(&self) -> u64 {
        self.0
            .committed_seq
            .load(core::sync::atomic::Ordering::Acquire)
    }

    fn has_uncommitted(&self) -> bool {
        self.0.has_uncommitted()
    }

    fn pending_count(&self) -> usize {
        self.0.pending_count()
    }

    fn commit(&self, txg: u64) {
        self.0.commit(txg);
    }

    fn sync(&self, txg: u64) {
        self.0.sync(txg);
    }

    fn is_syncing(&self) -> bool {
        self.0.syncing.load(core::sync::atomic::Ordering::Acquire)
    }

    fn replay(&self) -> Vec<NestZilRecord> {
        self.0.replay()
    }

    fn log_bp(&self) -> NestBlockPointer {
        *self.0.log_bp.lock()
    }
}

// ============================================================================
// 单元测试 — ZilLog trait 契约
// ============================================================================
//
// 验证 StandardZil 的 13 个 trait 方法 + 序列化和重放语义.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fs::nestfs::zil::{NestZilRecord, NestZilRecordType};

    /// 1. new + init
    #[test]
    fn test_zil_init() {
        let zil = StandardZil::new();
        // 默认 enabled
        assert!(zil.is_enabled());
        zil.init();
        assert_eq!(zil.current_seq(), 0);
        assert_eq!(zil.committed_seq(), 0);
    }

    /// 2. add_record: 分配 seq
    #[test]
    fn test_zil_add_record() {
        let zil = StandardZil::new();
        zil.init();
        let rec = NestZilRecord::new_write(1, 100, 0, 4096);
        zil.add_record(rec);
        assert_eq!(zil.current_seq(), 1);
        assert_eq!(zil.pending_count(), 1);
    }

    /// 3. add_record: 多记录 seq 递增
    #[test]
    fn test_zil_seq_increment() {
        let zil = StandardZil::new();
        zil.init();
        for _ in 0..5 {
            zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        }
        assert_eq!(zil.current_seq(), 5);
        assert_eq!(zil.pending_count(), 5);
    }

    /// 4. commit: 提交 txg
    #[test]
    fn test_zil_commit() {
        let zil = StandardZil::new();
        zil.init();
        // 添加 3 个不同 txg 的记录
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096)); // txg=1
        zil.add_record(NestZilRecord::new_write(2, 100, 0, 4096)); // txg=2
        zil.add_record(NestZilRecord::new_write(3, 100, 0, 4096)); // txg=3
        // commit txg=2 → 应清除 txg<=2 的记录, 保留 txg=3
        zil.commit(2);
        assert_eq!(zil.pending_count(), 1);
        assert_eq!(zil.committed_seq(), 2);
    }

    /// 5. has_uncommitted
    #[test]
    fn test_zil_has_uncommitted() {
        let zil = StandardZil::new();
        zil.init();
        assert!(!zil.has_uncommitted(), "初始无未提交");
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        assert!(zil.has_uncommitted());
        zil.commit(1);
        assert!(!zil.has_uncommitted());
    }

    /// 6. enabled 控制
    #[test]
    fn test_zil_enabled() {
        let zil = StandardZil::new();
        zil.init();
        assert!(zil.is_enabled());
        // disable 后 add_record 不会分配 seq
        zil.set_enabled(false);
        assert!(!zil.is_enabled());
        let old_seq = zil.current_seq();
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        assert_eq!(zil.current_seq(), old_seq, "disable 时不应分配 seq");
        assert_eq!(zil.pending_count(), 0);
        // 重新 enable
        zil.set_enabled(true);
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        assert_eq!(zil.current_seq(), old_seq + 1);
    }

    /// 7. sync
    #[test]
    fn test_zil_sync() {
        let zil = StandardZil::new();
        zil.init();
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        // sync(txg=1) → commit(txg=1)
        zil.sync(1);
        assert!(!zil.has_uncommitted());
        assert_eq!(zil.pending_count(), 0);
    }

    /// 8. replay: 按 seq 排序
    #[test]
    fn test_zil_replay() {
        let zil = StandardZil::new();
        zil.init();
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        zil.add_record(NestZilRecord::new_write(1, 200, 0, 4096));
        zil.add_record(NestZilRecord::new_write(1, 300, 0, 4096));
        let replayed = zil.replay();
        assert_eq!(replayed.len(), 3);
        // 应按 seq 排序
        for i in 1..replayed.len() {
            assert!(replayed[i].seq > replayed[i - 1].seq);
        }
    }

    /// 9. record types 完整覆盖
    #[test]
    fn test_zil_record_types() {
        let zil = StandardZil::new();
        zil.init();
        // 各种 record type
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        zil.add_record(NestZilRecord::new_create(1, 10, "file1"));
        zil.add_record(NestZilRecord::new_remove(1, 10, "file1"));
        zil.add_record(NestZilRecord::new_mkdir(1, 10, "subdir"));
        zil.add_record(NestZilRecord::new_setattr(1, 100));
        zil.add_record(NestZilRecord::new_link(1, 10, "link1", 100));
        zil.add_record(NestZilRecord::new_rename(1, 10, "old", "new"));
        zil.add_record(NestZilRecord::new_symlink(1, 10, "lnk", "/target"));
        assert_eq!(zil.pending_count(), 8);
        // 验证 replay 返回 8 条
        let replayed = zil.replay();
        assert_eq!(replayed.len(), 8);
    }

    /// 10. trait 对象分发 (dyn ZilLog)
    #[test]
    fn test_zil_trait_object() {
        let zil: alloc::boxed::Box<dyn ZilLog> = alloc::boxed::Box::new(StandardZil::new());
        zil.init();
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        assert_eq!(zil.current_seq(), 1);
        assert!(zil.has_uncommitted());
    }

    /// 11. integration: 完整事务生命周期
    #[test]
    fn test_zil_full_cycle() {
        let zil = StandardZil::new();
        zil.init();
        // 1. 模拟事务组 1: 3 个写
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        zil.add_record(NestZilRecord::new_write(1, 200, 0, 4096));
        zil.add_record(NestZilRecord::new_create(1, 10, "file"));
        assert_eq!(zil.pending_count(), 3);
        // 2. sync txg=1
        zil.sync(1);
        assert_eq!(zil.pending_count(), 0);
        assert!(!zil.has_uncommitted());
        // 3. 模拟事务组 2: 1 个写
        zil.add_record(NestZilRecord::new_write(2, 200, 4096, 4096));
        assert_eq!(zil.current_seq(), 4);
        // 4. commit txg=2
        zil.commit(2);
        assert_eq!(zil.pending_count(), 0);
    }

    /// 12. integration: replay 跨多次 commit
    #[test]
    fn test_zil_replay_across_commits() {
        let zil = StandardZil::new();
        zil.init();
        // 添加 3 个, commit txg=2
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096)); // seq 1
        zil.add_record(NestZilRecord::new_write(2, 100, 0, 4096)); // seq 2
        zil.add_record(NestZilRecord::new_write(3, 100, 0, 4096)); // seq 3
        zil.commit(2);
        // 现在只剩 1 条 (txg=3)
        let replayed = zil.replay();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].seq, 3);
        // 全 commit
        zil.commit(3);
        let replayed2 = zil.replay();
        assert_eq!(replayed2.len(), 0);
    }
}
