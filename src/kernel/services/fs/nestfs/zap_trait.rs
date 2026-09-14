#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//!
//! ZAP (ZFS Attribute Processor) trait 抽象 — LEGACY-5.1
//!
//! ## 架构
//!
//! ```text
//! ZapStore trait (framework/nestfs 抽象接口)
//!   ├── StandardZap (services/nestfs, 0 unsafe, 当前 NestZap 包装)
//!   └── MockZap    (host-test 用)
//! ```
//!
//! ## TCB 减负
//!
//! 原 `NestZap` 9 个方法 (insert/lookup/remove/...) 直接暴露.
//! 提取 trait 后:
//! - 调用方依赖 trait object / 泛型, 不再绑死 `NestZap`
//! - 单元测试可注入 `MockZap`, 验证 SPA/DMU 逻辑 (触发 zil/snapshot 脱离 vdev)
//!
//! ## 与 REVAL-6.1 范式一致
//!
//! - 0 unsafe (策略层)
//! - trait dispatch (无 thunk)
//! - 编译期类型安全 (实现方必须 impl `ZapStore`)

use super::zap::{NestZap, NestZapType};
use alloc::vec::Vec;

// ============================================================================
// ZapStore trait — 键值存储接口
// ============================================================================

/// ZAP 键值存储 trait
///
/// `NestZap` 的方法 (insert/lookup/remove/...) 抽象为 trait, 让 SPA/DMU 等
/// 调用方依赖抽象而非具体类型, 便于单元测试注入 mock 实现.
///
/// # 实现示例
///
/// ```ignore
/// pub struct StandardZap(NestZap);
/// impl ZapStore for StandardZap {
///     fn insert(&self, name: &str, value: &[u8]) -> bool { self.0.insert(name, value) }
///     // ...
/// }
/// ```
///
/// # Safety
///
/// - 实现方必须保证所有方法的内存安全 (无悬垂引用/双重释放)
/// - `Send + Sync` 约束: 跨线程共享时需内部同步
pub trait ZapStore: Send + Sync {
    /// 插入/更新键值对
    /// 返回 true = 成功, false = 容量满
    fn insert(&self, name: &str, value: &[u8]) -> bool;

    /// 插入 u64 值 (便捷方法)
    fn insert_u64(&self, name: &str, value: u64) -> bool;

    /// 查找键对应的值
    fn lookup(&self, name: &str) -> Option<Vec<u8>>;

    /// 查找键对应的 u64 值
    fn lookup_u64(&self, name: &str) -> Option<u64>;

    /// 删除键
    /// 返回 true = 找到并删除, false = 键不存在
    fn remove(&self, name: &str) -> bool;

    /// 检查键是否存在
    fn contains(&self, name: &str) -> bool;

    /// 当前条目数
    fn len(&self) -> usize;

    /// 是否为空
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 容量上限
    fn capacity(&self) -> usize;

    /// ZAP 类型 (Micro/Normal/Leaf)
    fn zap_type(&self) -> NestZapType;
}

// ============================================================================
// StandardZap — 默认 ZAP 实现 (NestZap 包装)
// ============================================================================

/// 标准 ZAP 实现 — 包装 `NestZap`, 委托所有方法
///
/// 0 unsafe, 0 thunk, 编译期类型安全.
/// 单元测试可注入 `MockZap` 替代本实现.
pub struct StandardZap(pub NestZap);

impl StandardZap {
    /// 构造默认容量
    pub fn new() -> Self {
        Self(NestZap::new())
    }

    /// 构造指定容量
    pub fn with_capacity(capacity: usize) -> Self {
        Self(NestZap::with_capacity(capacity))
    }
}

impl Default for StandardZap {
    fn default() -> Self {
        Self::new()
    }
}

impl ZapStore for StandardZap {
    fn insert(&self, name: &str, value: &[u8]) -> bool {
        self.0.insert(name, value)
    }

    fn insert_u64(&self, name: &str, value: u64) -> bool {
        self.0.insert_u64(name, value)
    }

    fn lookup(&self, name: &str) -> Option<Vec<u8>> {
        self.0.lookup(name)
    }

    fn lookup_u64(&self, name: &str) -> Option<u64> {
        self.0.lookup_u64(name)
    }

    fn remove(&self, name: &str) -> bool {
        self.0.remove(name)
    }

    fn contains(&self, name: &str) -> bool {
        self.0.contains(name)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn capacity(&self) -> usize {
        self.0.capacity
    }

    fn zap_type(&self) -> NestZapType {
        self.0.zap_type
    }
}

// ============================================================================
// 单元测试 — ZapStore trait 契约
// ============================================================================
//
// 验证 StandardZap 的 8 个 trait 方法 + 与 NestZap 行为一致性.

#[cfg(test)]
mod tests {
    use super::*;

    /// 1. insert/lookup: 基本键值操作
    #[test]
    fn test_zap_insert_lookup() {
        let zap = StandardZap::new();
        assert!(zap.insert("key1", b"value1"));
        assert_eq!(zap.lookup("key1"), Some(b"value1".to_vec()));
        // 不存在的键
        assert_eq!(zap.lookup("nokey"), None);
    }

    /// 2. insert_u64 / lookup_u64
    #[test]
    fn test_zap_u64() {
        let zap = StandardZap::new();
        assert!(zap.insert_u64("count", 42));
        assert_eq!(zap.lookup_u64("count"), Some(42));
        assert_eq!(zap.lookup("count"), Some(42u64.to_le_bytes().to_vec()));
    }

    /// 3. update 语义: 重复 insert 覆盖 value
    #[test]
    fn test_zap_update() {
        let zap = StandardZap::new();
        assert!(zap.insert("k", b"v1"));
        assert!(zap.insert("k", b"v2-longer"));
        assert_eq!(zap.lookup("k"), Some(b"v2-longer".to_vec()));
    }

    /// 4. remove / contains
    #[test]
    fn test_zap_remove() {
        let zap = StandardZap::new();
        assert!(zap.insert("a", b"1"));
        assert!(zap.contains("a"));
        // 删除存在的键
        assert!(zap.remove("a"));
        assert!(!zap.contains("a"));
        // 重复删除 → false
        assert!(!zap.remove("a"));
        // 删除不存在的键
        assert!(!zap.remove("nokey"));
    }

    /// 5. len / capacity
    #[test]
    fn test_zap_len_capacity() {
        let zap = StandardZap::new();
        assert_eq!(zap.len(), 0);
        assert_eq!(zap.capacity(), 256);
        zap.insert("a", b"1");
        zap.insert("b", b"2");
        assert_eq!(zap.len(), 2);
    }

    /// 6. with_capacity: 自定义容量
    #[test]
    fn test_zap_custom_capacity() {
        let zap = StandardZap::with_capacity(100);
        assert_eq!(zap.capacity(), 100);
    }

    /// 7. capacity 限制
    #[test]
    fn test_zap_capacity_limit() {
        let zap = StandardZap::with_capacity(2);
        assert!(zap.insert("a", b"1"));
        assert!(zap.insert("b", b"2"));
        // 容量满 → false
        assert!(!zap.insert("c", b"3"));
        assert_eq!(zap.len(), 2);
    }

    /// 8. zap_type 映射
    #[test]
    fn test_zap_type_mapping() {
        let micro = StandardZap::with_capacity(32);
        assert_eq!(micro.zap_type(), NestZapType::Micro);
        let normal = StandardZap::with_capacity(128);
        assert_eq!(normal.zap_type(), NestZapType::Normal);
    }

    /// 9. trait 对象分发 (dyn ZapStore)
    #[test]
    fn test_zap_trait_object() {
        let zap: alloc::sync::Arc<dyn ZapStore> = alloc::sync::Arc::new(StandardZap::new());
        // 通过 trait object 调用
        assert!(zap.insert("dyn", b"works"));
        assert_eq!(zap.lookup("dyn"), Some(b"works".to_vec()));
    }

    /// 10. integration: 模拟 SPA 场景 (多个键)
    #[test]
    fn test_zap_spa_simulation() {
        let zap = StandardZap::new();
        // 模拟 SPA 存 pool 元数据
        zap.insert_u64("pool_guid", 0x12345678);
        zap.insert_u64("txg", 42);
        zap.insert("pool_name", b"tank");
        zap.insert_u64("ashift", 9);
        // 验证
        assert_eq!(zap.lookup_u64("pool_guid"), Some(0x12345678));
        assert_eq!(zap.lookup_u64("txg"), Some(42));
        assert_eq!(zap.lookup("pool_name"), Some(b"tank".to_vec()));
        assert_eq!(zap.lookup_u64("ashift"), Some(9));
        assert_eq!(zap.len(), 4);
    }
}
