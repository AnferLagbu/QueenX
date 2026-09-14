#![deny(unsafe_code)]
//! Page Cache — services 层安全代理
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 `framework::mm::pcache`。
//!
//! ## 职责
//!
//! - 提供类型安全的 Page Cache API
//! - 参数验证 (`inode_id` 非零, `page_index` 合法)
//! - 委托 framework 层执行底层操作

use crate::framework::mm::pcache as fw_pcache;

// ============================================================================
// Page Cache 安全 API
// ============================================================================

/// 查找或创建缓存页
///
/// 若缓存命中, 返回物理地址并增加引用计数.
/// 若未命中, 分配物理页并插入缓存.
///
/// # 参数验证
///
/// - `inode_id` 必须非零
pub fn pcache_get(inode_id: u32, page_index: u64) -> Option<u64> {
    if inode_id == 0 {
        return None;
    }
    fw_pcache::pcache_get(inode_id, page_index)
}

/// 查找缓存页 (不增加引用计数)
pub fn pcache_lookup(inode_id: u32, page_index: u64) -> Option<u64> {
    if inode_id == 0 {
        return None;
    }
    fw_pcache::pcache_lookup(inode_id, page_index)
}

/// 标记脏页 (`MAP_SHARED` 写入后调用)
pub fn pcache_mark_dirty(inode_id: u32, page_index: u64) {
    if inode_id == 0 {
        return;
    }
    fw_pcache::pcache_mark_dirty(inode_id, page_index);
}

/// 释放缓存页引用 (munmap 时调用)
pub fn pcache_put(inode_id: u32, page_index: u64) {
    if inode_id == 0 {
        return;
    }
    fw_pcache::pcache_put(inode_id, page_index);
}

/// 释放 inode 的所有缓存页 (文件关闭时调用)
pub fn pcache_invalidate_inode(inode_id: u32) {
    if inode_id == 0 {
        return;
    }
    fw_pcache::pcache_invalidate_inode(inode_id);
}
