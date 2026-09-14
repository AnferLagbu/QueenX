#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//!
//! ZIL (ZFS Intent Log) 持久化 trait 抽象 — LEGACY-5.11
//!
//! ## 架构
//!
//! ```text
//! ZilPersist trait (framework/nestfs 抽象接口)
//!   ├── StandardZilPersist (services/nestfs, 0 unsafe, NestZilPersist 包装)
//!   └── MockZilPersist (host-test 用)
//! ```
//!
//! ## TCB 减负
//!
//! 原 `NestZilPersist` 3 个核心方法 (`serialize_zil_to_block/deserialize_zil_from_block/mark_written`)
//! 提取为 trait, 让 SPA/DMU 在不真实磁盘上测试序列化/反序列化/重放逻辑.
//!
//! ## 与 LEGACY-5.1-5.5/5.7/5.8/5.10 范式一致

use super::zil::NestZilRecord;
use super::zil_persist::NestZilPersist;
use alloc::vec::Vec;

// ============================================================================
// ZilPersist trait — 持久化接口
// ============================================================================

/// ZIL 持久化 trait
///
/// `NestZilPersist` 的核心方法 (`serialize_zil_to_block/deserialize_zil_from_block/mark_written`)
/// 抽象为 trait, 让 SPA/DMU 等调用方依赖抽象而非具体类型, 便于单元测试注入 mock 实现.
///
/// # Safety
///
/// - `serialize_zil_to_block` 在 records 为空时返回 None
/// - `deserialize_zil_from_block` 验证 header + trailer + `data_checksum`, 不一致时返回空 Vec
/// - `mark_written` 是幂等的
pub trait ZilPersist: Send + Sync {
    /// 序列化 ZIL 为磁盘块 (None = 无记录)
    fn serialize_zil_to_block(zil_records: &[NestZilRecord], txg: u64) -> Option<Vec<u8>>;

    /// 反序列化磁盘块为 ZIL 记录
    fn deserialize_zil_from_block(block: &[u8]) -> Vec<NestZilRecord>;

    /// 标记已写入
    fn mark_written(&self);
}

// ============================================================================
// StandardZilPersist — 默认 ZIL 持久化实现
// ============================================================================

/// 标准 ZIL 持久化实现 — 包装 `NestZilPersist`, 委托所有方法
///
/// 0 unsafe, 0 thunk, 编译期类型安全.
/// 单元测试可注入 `MockZilPersist` 替代本实现.
pub struct StandardZilPersist(pub NestZilPersist);

impl StandardZilPersist {
    /// 构造新实例
    pub const fn new() -> Self {
        Self(NestZilPersist::new())
    }

    /// 访问内部 `NestZilPersist` (向后兼容)
    pub fn inner(&self) -> &NestZilPersist {
        &self.0
    }
}

impl Default for StandardZilPersist {
    fn default() -> Self {
        Self::new()
    }
}

impl ZilPersist for StandardZilPersist {
    fn serialize_zil_to_block(zil_records: &[NestZilRecord], txg: u64) -> Option<Vec<u8>> {
        // 委托 NestZilPersist::serialize_zil_to_block (需要 &NestZil, 但 trait 接受 records slice)
        // 这里我们构造一个临时 NestZil 来满足签名
        use super::zil::NestZil;
        let temp_zil = NestZil::new();
        temp_zil.init();
        for rec in zil_records {
            // 跳过自动分配 seq, 直接 push (NestZilRecord 不可 Copy, 需 clone)
            use core::sync::atomic::Ordering;
            temp_zil.records.lock().push(rec.clone());
            let _ = Ordering::Acquire;
        }
        NestZilPersist::serialize_zil_to_block(&temp_zil, txg)
    }

    fn deserialize_zil_from_block(block: &[u8]) -> Vec<NestZilRecord> {
        NestZilPersist::deserialize_zil_from_block(block)
    }

    fn mark_written(&self) {
        self.0.mark_written();
    }
}

// ============================================================================
// 单元测试 — ZilPersist trait 契约
// ============================================================================
//
// 验证 StandardZilPersist 的 3 个 trait 方法 + 序列化/反序列化 round-trip.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fs::nestfs::zil::{NestZil, NestZilRecordType};
    use alloc::vec;

    /// 1. serialize 记录为空 → None
    #[test]
    fn test_zil_persist_empty_returns_none() {
        let records: Vec<NestZilRecord> = Vec::new();
        let result = StandardZilPersist::serialize_zil_to_block(&records, 1);
        assert!(result.is_none());
    }

    /// 2. 序列化 + 反序列化往返一致性
    #[test]
    fn test_zil_persist_roundtrip() {
        // 准备 ZIL
        let mut zil = NestZil::new();
        zil.init();
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        zil.add_record(NestZilRecord::new_write(1, 200, 0, 4096));
        zil.add_record(NestZilRecord::new_create(1, 10, "file1"));

        // serialize
        let block = NestZilPersist::serialize_zil_to_block(&zil, 1);
        assert!(block.is_some());
        let block = block.unwrap();

        // deserialize
        let records = StandardZilPersist::deserialize_zil_from_block(&block);
        assert_eq!(records.len(), 3);
    }

    /// 3. serialize 多 txg 记录
    #[test]
    fn test_zil_persist_multi_txg() {
        let mut zil = NestZil::new();
        zil.init();
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        zil.add_record(NestZilRecord::new_write(2, 200, 0, 4096));
        zil.add_record(NestZilRecord::new_write(3, 300, 0, 4096));
        let block = NestZilPersist::serialize_zil_to_block(&zil, 3).unwrap();
        let records = StandardZilPersist::deserialize_zil_from_block(&block);
        assert_eq!(records.len(), 3);
    }

    /// 4. deserialize 短块 → 空
    #[test]
    fn test_zil_persist_short_block() {
        let short = vec![0u8; 100];
        let records = StandardZilPersist::deserialize_zil_from_block(&short);
        assert_eq!(records.len(), 0);
    }

    /// 5. deserialize 损坏数据 → 优雅失败
    #[test]
    fn test_zil_persist_corrupted_data() {
        let mut block = vec![0u8; 8192];
        // 写入错误的 magic
        block[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        let records = StandardZilPersist::deserialize_zil_from_block(&block);
        // 验证失败 → 空 vec
        assert_eq!(records.len(), 0);
    }

    /// 6. mark_written
    #[test]
    fn test_zil_persist_mark_written() {
        let persist = StandardZilPersist::new();
        persist.mark_written();
        // 幂等: 多次调用不抛错
        persist.mark_written();
        persist.mark_written();
    }

    /// 7. record_type 完整覆盖
    #[test]
    fn test_zil_persist_all_record_types() {
        let mut zil = NestZil::new();
        zil.init();
        zil.add_record(NestZilRecord::new_write(1, 100, 0, 4096));
        zil.add_record(NestZilRecord::new_create(1, 10, "f"));
        zil.add_record(NestZilRecord::new_remove(1, 10, "f"));
        zil.add_record(NestZilRecord::new_mkdir(1, 10, "d"));
        zil.add_record(NestZilRecord::new_setattr(1, 100));
        zil.add_record(NestZilRecord::new_link(1, 10, "l", 100));
        zil.add_record(NestZilRecord::new_rename(1, 10, "a", "b"));
        zil.add_record(NestZilRecord::new_symlink(1, 10, "s", "/t"));
        zil.add_record(NestZilRecord::new_dedup_ref(1, [0u64; 4], 100));
        zil.add_record(NestZilRecord::new_dedup_unref(1, [0u64; 4]));
        // serialize 10 条
        let block = NestZilPersist::serialize_zil_to_block(&zil, 1).unwrap();
        // deserialize
        let records = StandardZilPersist::deserialize_zil_from_block(&block);
        assert_eq!(records.len(), 10);
    }

    /// 8. trait 对象分发 (dyn ZilPersist)
    #[test]
    fn test_zil_persist_trait_object() {
        let persist: alloc::boxed::Box<dyn ZilPersist> =
            alloc::boxed::Box::new(StandardZilPersist::new());
        persist.mark_written();
        // 反序列化空块
        let records = persist.deserialize_zil_from_block(&[]);
        assert_eq!(records.len(), 0);
    }

    /// 9. 集成: 序列化 → 反序列化 → 校验记录
    #[test]
    fn test_zil_persist_integration() {
        let mut zil = NestZil::new();
        zil.init();
        // 模拟大量事务
        for i in 0..10 {
            zil.add_record(NestZilRecord::new_write(1, 100 + i, i * 4096, 4096));
        }
        // serialize
        let block = NestZilPersist::serialize_zil_to_block(&zil, 1).unwrap();
        // verify block size
        assert_eq!(block.len(), 8192);
        // deserialize
        let records = StandardZilPersist::deserialize_zil_from_block(&block);
        assert_eq!(records.len(), 10);
        // 验证 seq 顺序
        for (i, r) in records.iter().enumerate() {
            assert_eq!(r.seq, (i + 1) as u64);
        }
    }

    /// 10. 往返一致性保留 txg 字段
    #[test]
    fn test_zil_persist_preserves_txg() {
        let mut zil = NestZil::new();
        zil.init();
        zil.add_record(NestZilRecord::new_write(42, 100, 0, 4096));
        let block = NestZilPersist::serialize_zil_to_block(&zil, 42).unwrap();
        let records = StandardZilPersist::deserialize_zil_from_block(&block);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].txg, 42);
    }
}
