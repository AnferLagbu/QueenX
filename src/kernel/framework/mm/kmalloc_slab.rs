//! Slab-Backed kmalloc (小对象 Slab, 大对象堆)
//!
//! 将 Kmalloc 与 Slab 分配器集成:
//!
//! - 分配 ≤ 2048 bytes → Slab 缓存 (O(1), 无碎片)
//! - 分配 > 2048 bytes → 堆分配器 (first-fit)
//!
//! ## SAFETY
//!
//! `SLAB_CACHES` 使用 `[Option<KmemCache>; 8]` 数组存储，每个缓存独立初始化。
//! 如果某个缓存创建失败，对应的数组元素为 `None`，分配时会自动回退到堆分配器。
//! 这种设计避免了未初始化内存访问，确保内存安全。
//!
//! `SLAB_CACHES` 访问由 `SLAB_LOCK` 自旋锁保护。
//! `slab_init()` 在内核早期初始化阶段（单核）调用，因此初始化路径无竞态。

use super::slab::KmemCache;
use crate::klog_error;
use crate::klog_info_simple;
use core::sync::atomic::{AtomicBool, Ordering};

const CACHE_SIZES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];

/// Slab 缓存数组 - 用 Option 安全处理初始化失败
/// None 表示缓存创建失败, 不应使用
static SLAB_CACHES: crate::framework::sync::IrqSpinLock<[Option<KmemCache>; 8]> =
    crate::framework::sync::IrqSpinLock::new([None, None, None, None, None, None, None, None]);
static SLAB_READY: AtomicBool = AtomicBool::new(false);

pub fn slab_init() {
    let mut success_count = 0;
    let mut caches = SLAB_CACHES.lock();

    for i in 0..8 {
        let name = match i {
            0 => "kmalloc-16",
            1 => "kmalloc-32",
            2 => "kmalloc-64",
            3 => "kmalloc-128",
            4 => "kmalloc-256",
            5 => "kmalloc-512",
            6 => "kmalloc-1024",
            _ => "kmalloc-2048",
        };

        if let Ok(cache) = KmemCache::create(name, CACHE_SIZES[i]) {
            caches[i] = Some(cache);
            success_count += 1;
        } else {
            klog_error!(
                "[SLAB] CRITICAL: Failed to create cache {} (size {}). \
                 This size class will fall back to heap allocator.",
                name,
                CACHE_SIZES[i]
            );
            caches[i] = None;
        }
    }
    drop(caches);

    if success_count == 0 {
        klog_error!("[SLAB] CRITICAL: All slab caches failed to initialize!");
    } else {
        klog_info_simple!("[SLAB] Initialized {}/8 caches successfully", success_count);
    }

    SLAB_READY.store(true, Ordering::Release);
}

fn cache_index(size: usize) -> Option<usize> {
    // T2-3: 委托给 SlabPolicy::find_cache_index
    super::slab_trait::current_slab_policy()
        .find_cache_index(size, &super::slab::GENERAL_CACHE_SIZES)
}

pub fn slab_kmalloc(size: usize) -> Option<*mut u8> {
    if size == 0 || !SLAB_READY.load(Ordering::Acquire) {
        return super::kmalloc::get_kmalloc().allocate(size);
    }

    cache_index(size).map_or(super::kmalloc::get_kmalloc().allocate(size), |idx| {
        let mut caches = SLAB_CACHES.lock();
        caches[idx].as_mut().map_or(
            super::kmalloc::get_kmalloc().allocate(size),
            KmemCache::allocate,
        )
    })
}

pub fn slab_kfree(ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }

    if !SLAB_READY.load(Ordering::Acquire) {
        super::kmalloc::get_kmalloc().deallocate(ptr);
        return;
    }

    if let Some(idx) = cache_index(size) {
        let mut caches = SLAB_CACHES.lock();
        if let Some(ref mut cache) = caches[idx] {
            cache.deallocate(ptr);
        } else {
            drop(caches);
            super::kmalloc::get_kmalloc().deallocate(ptr);
        }
    } else {
        super::kmalloc::get_kmalloc().deallocate(ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_index_selection() {
        assert_eq!(cache_index(8), Some(0));
        assert_eq!(cache_index(16), Some(0));
        assert_eq!(cache_index(20), Some(1));
        assert_eq!(cache_index(100), Some(3));
        assert_eq!(cache_index(2048), Some(7));
        assert_eq!(cache_index(2049), None);
    }
}
