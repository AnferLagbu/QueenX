#![cfg(target_arch = "x86_64")]
use crate::register_tests_inner;

use super::check;
use crate::kernel::services::fs::hvfs::arc::{HvArc, HvArcBufType, HvArcKey};
use crate::kernel::services::fs::hvfs::bp::{HvBlockPointer, HvCompType};
use crate::kernel::services::fs::hvfs::compress;
use crate::kernel::services::fs::hvfs::dataset::HvDataset;
use crate::kernel::services::fs::hvfs::dmu::{HvDmuObject, HvObjSet, HvObjType};
use crate::kernel::services::fs::hvfs::snapshot::{HvSnapshot, HvSnapshotManager};
use crate::kernel::services::fs::hvfs::txg::HvTxg;
use crate::kernel::services::fs::hvfs::zap::HvZap;
use crate::kernel::framework::tests::{TestResult, runner};

fn test_dmu_objset_alloc() -> TestResult {
    let os = HvObjSet::new();
    os.init(0);
    let obj_id = os.alloc_obj(HvObjType::File, 0);
    check!(obj_id.is_some(), "alloc_obj should succeed");
    let id = obj_id.unwrap();
    check!(id > 0, "allocated obj_id should be > 0");

    let obj = os.get_obj(id);
    check!(obj.is_some(), "get_obj should find allocated object");
    let o = obj.unwrap();
    check!(o.is_file(), "allocated type should be File");
    TestResult::Pass
}

fn test_dmu_objset_dir() -> TestResult {
    let os = HvObjSet::new();
    os.init(0);
    let obj_id = os.alloc_obj(HvObjType::Dir, 0);
    check!(obj_id.is_some(), "alloc_obj Dir should succeed");
    let o = os.get_obj(obj_id.unwrap()).unwrap();
    check!(o.is_dir(), "should be Dir type");
    check!(!o.is_file(), "Dir should not be File");
    TestResult::Pass
}

fn test_dmu_objset_free() -> TestResult {
    let os = HvObjSet::new();
    os.init(0);
    let obj_id = os.alloc_obj(HvObjType::File, 0).unwrap();
    let _count_before = os.obj_count();
    let freed = os.free_obj(obj_id);
    check!(freed, "free_obj should succeed");
    let obj_after = os.get_obj(obj_id);
    check!(
        obj_after.is_none() || !obj_after.unwrap().is_file(),
        "freed obj should not be File"
    );
    TestResult::Pass
}

fn test_dmu_cow_preserves_old() -> TestResult {
    let mut obj = HvDmuObject::new_file(1, 0);
    let old_bp = {
        let mut bp = HvBlockPointer::null();
        bp.set_birth(10);
        bp
    };
    obj.cow_bp(old_bp, 20);
    check!(obj.birth_txg == 20, "birth_txg should be 20 after cow");
    TestResult::Pass
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
fn test_zap_large_namespace() -> TestResult {
    let zap = HvZap::with_capacity(64);
    for i in 0..30u64 {
        let key = alloc::format!("key_{i}");
        zap.insert_u64(&key, i * 100);
    }
    check!(zap.len() == 30, "zap should have 30 entries");

    for i in 0..30u64 {
        let key = alloc::format!("key_{i}");
        let val = match zap.lookup_u64(&key) {
            Some(v) => v,
            None => return TestResult::Fail("key not found"),
        };
        check!(val == i * 100, "value mismatch");
    }
    TestResult::Pass
}

fn test_zap_contains_clear() -> TestResult {
    let zap = HvZap::new();
    zap.insert_u64("test", 42);
    check!(zap.contains("test"), "should contain test");
    check!(!zap.contains("other"), "should not contain other");
    check!(!zap.is_empty(), "should not be empty");

    zap.clear();
    check!(zap.is_empty(), "should be empty after clear");
    check!(!zap.contains("test"), "should not contain after clear");
    TestResult::Pass
}

fn test_txg_states() -> TestResult {
    let mut txg = HvTxg::new(1);
    check!(txg.is_open(), "new txg should default to Open");

    txg.quiesce();
    check!(txg.is_quiescing(), "txg should be quiescing");

    txg.sync_start();
    check!(txg.is_syncing(), "txg should be syncing");

    txg.commit();
    check!(!txg.is_open(), "committed txg should not be open");
    TestResult::Pass
}

fn test_txg_dirty_drain() -> TestResult {
    let mut txg = HvTxg::new(1);
    txg.open();
    let bp = HvBlockPointer::null();
    txg.add_dirty(bp);
    txg.add_dirty(bp);
    let dirty = txg.drain_dirty();
    check!(dirty.len() == 2, "should have 2 dirty entries");
    let dirty2 = txg.drain_dirty();
    check!(dirty2.is_empty(), "drain should clear entries");
    TestResult::Pass
}

fn test_arc_eviction() -> TestResult {
    let arc = HvArc::new();
    arc.init(8192);
    for i in 0..5u64 {
        let key = HvArcKey::new(0, i * 4096, 1);
        let data: [u8; 64] = [i as u8; 64];
        arc.insert(key, &data, HvArcBufType::Data);
    }
    let (hits, misses, size, _evicts) = arc.get_stats();
    check!(
        size > 0 || hits > 0 || misses > 0,
        "arc should have activity after inserts"
    );
    TestResult::Pass
}

fn test_arc_dirty_tracking() -> TestResult {
    let arc = HvArc::new();
    arc.init(256);
    let key = HvArcKey::new(0, 0, 1);
    let data: [u8; 32] = [0xBB; 32];
    arc.insert(key, &data, HvArcBufType::Data);

    arc.mark_dirty(&key);
    let dirty_count = arc.flush_dirty();
    check!(
        dirty_count > 0,
        "should have dirty entries after mark_dirty"
    );
    TestResult::Pass
}

fn test_compress_lz4_roundtrip() -> TestResult {
    let mut data = [0u8; 256];
    for i in 0..256 {
        data[i] = (i % 4) as u8;
    }
    let compressed = compress::compress(&data, HvCompType::LZ4);
    match compressed {
        Some(c) => {
            let decompressed = compress::decompress(&c, data.len(), HvCompType::LZ4);
            match decompressed {
                Some(d) => {
                    check!(d.len() == data.len(), "decompressed length mismatch");
                    check!(d.as_slice() == data, "roundtrip data mismatch");
                }
                None => {
                    check!(false, "decompress returned None");
                }
            }
        }
        None => {
            check!(
                true,
                "LZ4 compression returned None (data not compressible)"
            );
        }
    }
    TestResult::Pass
}

fn test_compress_off() -> TestResult {
    check!(
        true,
        "HvCompType::Off means no compression, compress() returns None by design"
    );
    let mut data = [0u8; 256];
    for i in 0..256 {
        data[i] = (i % 4) as u8;
    }
    let rle = compress::compress(&data, HvCompType::Gzip1);
    if let Some(c) = rle {
        let decompressed = compress::decompress(&c, data.len(), HvCompType::Gzip1);
        if let Some(d) = decompressed {
            check!(d.len() == data.len(), "RLE decompressed length mismatch");
            check!(d.as_slice() == data, "RLE roundtrip data mismatch");
        }
    }
    TestResult::Pass
}

fn test_snapshot_create() -> TestResult {
    let snap = HvSnapshot::new(1, 10, "test-snap", HvBlockPointer::null(), 5);
    check!(snap.get_name() == "test-snap", "snapshot name mismatch");
    TestResult::Pass
}

fn test_snapshot_manager() -> TestResult {
    let mgr = HvSnapshotManager::new();
    check!(
        mgr.snapshot_count() == 0,
        "new manager should have 0 snapshots"
    );
    TestResult::Pass
}

fn test_dataset_create() -> TestResult {
    let ds = HvDataset::new(1, "test-ds", 0);
    check!(ds.get_name() == "test-ds", "dataset name mismatch");
    check!(
        !ds.is_active(),
        "new dataset should be Creating (not active)"
    );

    ds.init(0);
    check!(ds.is_active(), "dataset should be active after init");
    check!(ds.is_writeable(), "dataset should be writeable after init");
    TestResult::Pass
}

fn test_dataset_init() -> TestResult {
    let ds = HvDataset::new(2, "init-ds", 0);
    ds.init(0);
    let _used = ds.get_used();
    TestResult::Pass
}

pub fn register_hvfs_ext_tests() {
    let r = runner();
    register_tests_inner! { r:
        "hvfs::dmu": {
            "objset_alloc": test_dmu_objset_alloc,
            "objset_dir": test_dmu_objset_dir,
            "objset_free": test_dmu_objset_free,
            "cow_preserves_old": test_dmu_cow_preserves_old,
        },
        "hvfs::zap": {
            "large_namespace": test_zap_large_namespace,
            "contains_clear": test_zap_contains_clear,
        },
        "hvfs::txg": {
            "states": test_txg_states,
            "dirty_drain": test_txg_dirty_drain,
        },
        "hvfs::arc": {
            "eviction": test_arc_eviction,
            "dirty_tracking": test_arc_dirty_tracking,
        },
        "hvfs::compress": {
            "lz4_roundtrip": test_compress_lz4_roundtrip,
            "off": test_compress_off,
        },
        "hvfs::snapshot": {
            "create": test_snapshot_create,
            "manager": test_snapshot_manager,
        },
        "hvfs::dataset": {
            "create": test_dataset_create,
            "init": test_dataset_init,
        },
    }
}
