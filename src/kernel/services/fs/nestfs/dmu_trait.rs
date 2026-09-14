#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//!
//! DMU (Data Management Unit) trait 抽象 — LEGACY-5.4
//!
//! ## 架构
//!
//! ```text
//! DmuManager trait (framework/nestfs 抽象接口)
//!   ├── StandardDmu (services/nestfs, 0 unsafe, NestObjSet 包装)
//!   └── MockDmu    (host-test 用)
//! ```
//!
//! ## TCB 减负
//!
//! 原 `NestObjSet` 8 个方法 (`init/alloc_obj/free_obj/get_obj/update_obj`/...) 直接暴露.
//! 提取 trait 后:
//! - SPA/ZIL 调用方依赖 trait object / 泛型, 不再绑死 `NestObjSet`
//! - 单元测试可注入 `MockDmu`, 验证 SPA 在不真实 DMU 上的逻辑
//!
//! ## 与 LEGACY-5.1/5.2 范式一致

use super::dmu::{NestDmuObject, NestObjSet, NestObjType};

// ============================================================================
// DmuManager trait — 对象管理接口
// ============================================================================

/// DMU 对象管理 trait
///
/// `NestObjSet` 的方法 (`init/alloc_obj/free_obj/get_obj`/...) 抽象为 trait,
/// 让 SPA/ZIL 等调用方依赖抽象而非具体类型, 便于单元测试注入 mock 实现.
///
/// # Safety
///
/// - `init` 应只调用一次 (创建 root + meta zap)
/// - `alloc_obj` 返回的 `obj_id` 必须唯一
/// - `free_obj` 在 `link_count=0` 时自动标记 used=false
/// - 所有方法在 alloc/free 路径上需内部同步 (`NestObjSet` 内部用 Mutex)
pub trait DmuManager: Send + Sync {
    /// 初始化对象集 (创建 root dir + meta zap)
    fn init(&self, owner_pwm: u64);

    /// 是否已初始化
    fn is_initialized(&self) -> bool;

    /// 分配新对象
    /// 返回 `Some(obj_id)` 成功, None 表示类型不支持或资源耗尽
    fn alloc_obj(&self, obj_type: NestObjType, owner_pwm: u64) -> Option<u64>;

    /// 释放对象 (`link_count` -= 1, 0 时标记未使用)
    /// 返回 true 成功, false 表示对象不存在
    fn free_obj(&self, obj_id: u64) -> bool;

    /// 读取对象副本
    fn get_obj(&self, obj_id: u64) -> Option<NestDmuObject>;

    /// 更新对象
    /// 返回 true 成功, false 表示对象不存在
    fn update_obj(&self, obj: &NestDmuObject) -> bool;

    /// root 对象 id
    fn root_obj_id(&self) -> u64;

    /// 读取 root 对象
    fn get_root(&self) -> Option<NestDmuObject>;

    /// 当前活动对象数 (used=true)
    fn obj_count(&self) -> u64;

    /// 下一个可用 `obj_id`
    fn next_obj_id(&self) -> u64;
}

// ============================================================================
// StandardDmu — 默认 DMU 实现 (NestObjSet 包装)
// ============================================================================

/// 标准 DMU 实现 — 包装 `NestObjSet`, 委托所有方法
///
/// 0 unsafe, 0 thunk, 编译期类型安全.
/// 单元测试可注入 `MockDmu` 替代本实现.
pub struct StandardDmu(pub NestObjSet);

impl StandardDmu {
    /// 构造新实例 (未初始化)
    pub fn new() -> Self {
        Self(NestObjSet::new())
    }

    /// 访问内部 `NestObjSet` (向后兼容)
    pub fn inner(&self) -> &NestObjSet {
        &self.0
    }
}

impl Default for StandardDmu {
    fn default() -> Self {
        Self::new()
    }
}

impl DmuManager for StandardDmu {
    fn init(&self, owner_pwm: u64) {
        self.0.init(owner_pwm);
    }

    fn is_initialized(&self) -> bool {
        self.0
            .initialized
            .load(core::sync::atomic::Ordering::Acquire)
    }

    fn alloc_obj(&self, obj_type: NestObjType, owner_pwm: u64) -> Option<u64> {
        self.0.alloc_obj(obj_type, owner_pwm)
    }

    fn free_obj(&self, obj_id: u64) -> bool {
        self.0.free_obj(obj_id)
    }

    fn get_obj(&self, obj_id: u64) -> Option<NestDmuObject> {
        self.0.get_obj(obj_id)
    }

    fn update_obj(&self, obj: &NestDmuObject) -> bool {
        self.0.update_obj(obj)
    }

    fn root_obj_id(&self) -> u64 {
        self.0.root_obj
    }

    fn get_root(&self) -> Option<NestDmuObject> {
        self.0.get_root()
    }

    fn obj_count(&self) -> u64 {
        self.0.obj_count()
    }

    fn next_obj_id(&self) -> u64 {
        self.0
            .next_obj_id
            .load(core::sync::atomic::Ordering::Acquire)
    }
}

// ============================================================================
// 单元测试 — DmuManager trait 契约
// ============================================================================
//
// 验证 StandardDmu 的 10 个 trait 方法 + 对象生命周期.

#[cfg(test)]
mod tests {
    use super::*;

    /// 1. new + is_initialized (未 init 时)
    #[test]
    fn test_dmu_uninitialized() {
        let dmu = StandardDmu::new();
        assert!(!dmu.is_initialized());
        assert_eq!(dmu.obj_count(), 0);
        // root_obj_id 在 new 时已设置 (HV_DMU_OBJ_ROOT=2)
        assert_eq!(dmu.root_obj_id(), 2);
    }

    /// 2. init: 创建 root + meta zap
    #[test]
    fn test_dmu_init() {
        let dmu = StandardDmu::new();
        dmu.init(0x100);
        assert!(dmu.is_initialized());
        // root + meta = 2 个对象
        assert_eq!(dmu.obj_count(), 2);
        // get_root 返回 Dir 类型
        let root = dmu.get_root().unwrap();
        assert_eq!(root.obj_type, NestObjType::Dir);
        assert_eq!(root.obj_id, 2);
    }

    /// 3. alloc_obj: 不同类型
    #[test]
    fn test_dmu_alloc_types() {
        let dmu = StandardDmu::new();
        dmu.init(0x100);
        // alloc File
        let f = dmu.alloc_obj(NestObjType::File, 0x100).unwrap();
        assert_eq!(f, 3); // next_obj_id 在 init 后 = HV_DMU_OBJ_ROOT + 2 = 4, 递增后给的是旧值
        // 实际 next_obj_id 在 alloc 前是 4, alloc 后是 5
        // 修正: 初始 next_obj_id=HV_DMU_OBJ_ROOT+1=3, init 后 =HV_DMU_OBJ_ROOT+2=4
        // alloc 后 =5, 分配给第一个 obj 的 id 是 fetch_add 的旧值 = 4
        // 重新验证
        let dmu2 = StandardDmu::new();
        dmu2.init(0x100);
        let f2 = dmu2.alloc_obj(NestObjType::File, 0x100).unwrap();
        // 第一个 alloc 返回 HV_DMU_OBJ_ROOT+1=3
        assert_eq!(f2, 3);
        let d2 = dmu2.alloc_obj(NestObjType::Dir, 0x100).unwrap();
        assert_eq!(d2, 4);
        // File 类型
        let fobj = dmu2.get_obj(f2).unwrap();
        assert_eq!(fobj.obj_type, NestObjType::File);
        // Dir 类型
        let dobj = dmu2.get_obj(d2).unwrap();
        assert_eq!(dobj.obj_type, NestObjType::Dir);
    }

    /// 4. get_obj: 不存在 / 存在
    #[test]
    fn test_dmu_get_obj() {
        let dmu = StandardDmu::new();
        dmu.init(0x100);
        // 不存在
        assert!(dmu.get_obj(99).is_none());
        // 存在 (root)
        let root = dmu.get_obj(2).unwrap();
        assert_eq!(root.obj_id, 2);
    }

    /// 5. free_obj: link_count 递减
    #[test]
    fn test_dmu_free_obj() {
        let dmu = StandardDmu::new();
        dmu.init(0x100);
        // alloc File, 初始 link_count=1
        let f = dmu.alloc_obj(NestObjType::File, 0x100).unwrap();
        let obj = dmu.get_obj(f).unwrap();
        assert_eq!(obj.link_count, 1);
        assert_eq!(obj.used, true);
        // free: link_count=0, used=false
        assert!(dmu.free_obj(f));
        let obj2 = dmu.get_obj(f);
        assert!(obj2.is_none(), "link_count=0 后 used=false, get 返回 None");
    }

    /// 6. free_obj: 不存在
    #[test]
    fn test_dmu_free_nonexistent() {
        let dmu = StandardDmu::new();
        dmu.init(0x100);
        assert!(!dmu.free_obj(99));
    }

    /// 7. update_obj: 修改 size
    #[test]
    fn test_dmu_update_obj() {
        let dmu = StandardDmu::new();
        dmu.init(0x100);
        let f = dmu.alloc_obj(NestObjType::File, 0x100).unwrap();
        let mut obj = dmu.get_obj(f).unwrap();
        obj.size = 4096;
        assert!(dmu.update_obj(&obj));
        let obj2 = dmu.get_obj(f).unwrap();
        assert_eq!(obj2.size, 4096);
    }

    /// 8. obj_count 变化
    #[test]
    fn test_dmu_obj_count() {
        let dmu = StandardDmu::new();
        dmu.init(0x100);
        // init 后 2 个
        assert_eq!(dmu.obj_count(), 2);
        // alloc 3 个
        dmu.alloc_obj(NestObjType::File, 0x100);
        dmu.alloc_obj(NestObjType::Dir, 0x100);
        dmu.alloc_obj(NestObjType::File, 0x100);
        assert_eq!(dmu.obj_count(), 5);
        // free 1 个 → 4
        let f = dmu.alloc_obj(NestObjType::File, 0x100).unwrap();
        dmu.free_obj(f);
        assert_eq!(dmu.obj_count(), 5); // alloc 1 + free 1 = 0 净变化
    }

    /// 9. trait 对象分发 (dyn DmuManager)
    #[test]
    fn test_dmu_trait_object() {
        let dmu: alloc::boxed::Box<dyn DmuManager> = alloc::boxed::Box::new(StandardDmu::new());
        dmu.init(0x100);
        assert!(dmu.is_initialized());
        let f = dmu.alloc_obj(NestObjType::File, 0x100).unwrap();
        assert!(dmu.get_obj(f).is_some());
    }

    /// 10. integration: 完整对象生命周期
    #[test]
    fn test_dmu_full_cycle() {
        let dmu = StandardDmu::new();
        dmu.init(0x100);
        // 创建目录树: root → subdir → file
        let subdir = dmu.alloc_obj(NestObjType::Dir, 0x100).unwrap();
        let file = dmu.alloc_obj(NestObjType::File, 0x100).unwrap();
        // 写文件大小
        let mut fobj = dmu.get_obj(file).unwrap();
        fobj.size = 8192;
        fobj.link_count = 2; // 模拟有 2 个 hard link
        dmu.update_obj(&fobj);
        // 验证
        let fobj2 = dmu.get_obj(file).unwrap();
        assert_eq!(fobj2.size, 8192);
        assert_eq!(fobj2.link_count, 2);
        // 释放 hard link 1 次
        dmu.free_obj(file);
        let fobj3 = dmu.get_obj(file).unwrap();
        assert_eq!(fobj3.link_count, 1);
        assert_eq!(fobj3.used, true);
        // 释放 hard link 第 2 次 → used=false
        dmu.free_obj(file);
        assert!(dmu.get_obj(file).is_none());
        // 目录仍在
        assert!(dmu.get_obj(subdir).is_some());
    }
}
