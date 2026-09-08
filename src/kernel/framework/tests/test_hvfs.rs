#![cfg(target_arch = "x86_64")]
use crate::register_tests_inner;

use super::{assert_eq_test, check};
use crate::kernel::framework::fs::hvfs::arc::{HvArc, HvArcBufType, HvArcKey};
use crate::kernel::framework::fs::hvfs::bp::{HvBlockPointer, HvCksumType, HvDva};
use crate::kernel::framework::fs::hvfs::checksum::HvChecksum;
use crate::kernel::framework::fs::hvfs::dmu::{HvDmuObject, HvObjType};
use crate::kernel::framework::fs::hvfs::spa::{HV_SPA_MAGIC, HvSpaConfig, HvUberblock};
use crate::kernel::framework::fs::hvfs::txg::HvTxgGroup;
use crate::kernel::framework::fs::hvfs::zap::HvZap;
use crate::kernel::framework::fs::hvfs::zil::{HvZil, HvZilRecord};
use crate::kernel::framework::tests::{TestResult, runner};

fn test_bp_null() -> TestResult {
    let bp = HvBlockPointer::null();
    check!(bp.is_null(), "null bp should be null");
    check!(bp.get_dva(0).is_none(), "null bp dva should be None");
    TestResult::Pass
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
fn test_bp_dva_set_get() -> TestResult {
    let mut bp = HvBlockPointer::null();
    let dva = HvDva::new(0, 4096, 8192);
    bp.set_dva(0, dva);
    let got = match bp.get_dva(0) {
        Some(v) => v,
        None => return TestResult::Fail("dva not set"),
    };
    check!(got.vdev_id == 0, "vdev_id mismatch");
    check!(got.offset == 4096, "offset mismatch");
    check!(got.asize == 8192, "asize mismatch");
    TestResult::Pass
}

fn test_bp_birth_txg() -> TestResult {
    let mut bp = HvBlockPointer::null();
    bp.set_birth(42);
    check!(bp.birth_txg == 42, "birth txg mismatch");
    TestResult::Pass
}

fn test_checksum_fletcher4_basic() -> TestResult {
    let data = b"hello world test data for checksum verification";
    let ck_a = HvChecksum::compute(HvCksumType::Fletcher4, data);
    let ck_b = HvChecksum::compute(HvCksumType::Fletcher4, data);
    check!(
        ck_a.value == ck_b.value,
        "same data should produce same checksum"
    );
    TestResult::Pass
}

fn test_checksum_different_data() -> TestResult {
    let ck_a = HvChecksum::compute(HvCksumType::Fletcher4, b"hello");
    let ck_b = HvChecksum::compute(HvCksumType::Fletcher4, b"world");
    check!(ck_a.value != ck_b.value, "different data should differ");
    TestResult::Pass
}

// E-06 (2026-09-07): host-tests/src/checksum.rs 去重载体用例合入 (同被测对象
// services::fs::hvfs::checksum::HvChecksum 的双端共享用例统一收口到本套件).
// 覆盖 Fletcher2/4 全族、verify 往返/损坏检测、Off/EdonR/SHA256 变体与边界长度.

fn test_checksum_fletcher2_empty() -> TestResult {
    let ck = HvChecksum::compute(HvCksumType::Fletcher2, b"");
    check!(ck.value[0] == 0 && ck.value[1] == 0, "empty fletcher2 zero");
    TestResult::Pass
}

fn test_checksum_fletcher2_deterministic() -> TestResult {
    let data = b"hello world";
    let ck1 = HvChecksum::compute(HvCksumType::Fletcher2, data);
    let ck2 = HvChecksum::compute(HvCksumType::Fletcher2, data);
    assert_eq_test!(ck1.value, ck2.value, "fletcher2 deterministic");
    TestResult::Pass
}

fn test_checksum_fletcher4_empty() -> TestResult {
    let ck = HvChecksum::compute(HvCksumType::Fletcher4, b"");
    check!(
        ck.value[0] == 0 && ck.value[1] == 0 && ck.value[2] == 0 && ck.value[3] == 0,
        "empty fletcher4 zero"
    );
    TestResult::Pass
}

fn test_checksum_fletcher4_deterministic() -> TestResult {
    let data = b"test data for fletcher4";
    let ck1 = HvChecksum::compute(HvCksumType::Fletcher4, data);
    let ck2 = HvChecksum::compute(HvCksumType::Fletcher4, data);
    assert_eq_test!(ck1.value, ck2.value, "fletcher4 deterministic");
    TestResult::Pass
}

fn test_checksum_verify_roundtrip_fletcher2() -> TestResult {
    let data = b"some test data for verification";
    let ck = HvChecksum::compute(HvCksumType::Fletcher2, data);
    check!(ck.verify(data), "fletcher2 verifies");
    TestResult::Pass
}

fn test_checksum_verify_roundtrip_fletcher4() -> TestResult {
    let data = b"some test data for verification";
    let ck = HvChecksum::compute(HvCksumType::Fletcher4, data);
    check!(ck.verify(data), "fletcher4 verifies");
    TestResult::Pass
}

fn test_checksum_verify_detects_corruption() -> TestResult {
    let data = b"original data";
    let ck = HvChecksum::compute(HvCksumType::Fletcher4, data);
    let corrupted = b"corrupted data";
    check!(!ck.verify(corrupted), "corruption detected");
    TestResult::Pass
}

fn test_checksum_off_always_zero() -> TestResult {
    let ck = HvChecksum::compute(HvCksumType::Off, b"any data");
    assert_eq_test!(ck.value, [0u64; 4], "Off checksum zero");
    TestResult::Pass
}

fn test_checksum_edonr_uses_fletcher4() -> TestResult {
    let data = b"test data";
    let ck_edonr = HvChecksum::compute(HvCksumType::EdonR, data);
    let ck_f4 = HvChecksum::compute(HvCksumType::Fletcher4, data);
    assert_eq_test!(ck_edonr.value, ck_f4.value, "EdonR == Fletcher4");
    TestResult::Pass
}

fn test_checksum_fletcher2_single_byte() -> TestResult {
    let ck = HvChecksum::compute(HvCksumType::Fletcher2, b"A");
    check!(ck.value[0] != 0, "single byte fletcher2 non-zero");
    TestResult::Pass
}

fn test_checksum_fletcher4_single_byte() -> TestResult {
    let ck = HvChecksum::compute(HvCksumType::Fletcher4, b"A");
    check!(ck.value[0] != 0, "single byte fletcher4 non-zero");
    TestResult::Pass
}

fn test_checksum_fletcher2_odd_length() -> TestResult {
    let data = b"hello";
    let ck = HvChecksum::compute(HvCksumType::Fletcher2, data);
    check!(ck.verify(data), "odd-length fletcher2 verifies");
    TestResult::Pass
}

fn test_checksum_fletcher4_odd_length() -> TestResult {
    let data = b"odd data length test";
    let ck = HvChecksum::compute(HvCksumType::Fletcher4, data);
    check!(ck.verify(data), "odd-length fletcher4 verifies");
    TestResult::Pass
}

fn test_checksum_fletcher2_exact_8_bytes() -> TestResult {
    let data = b"12345678";
    let ck = HvChecksum::compute(HvCksumType::Fletcher2, data);
    check!(ck.verify(data), "8-byte fletcher2 verifies");
    TestResult::Pass
}

fn test_checksum_fletcher4_exact_8_bytes() -> TestResult {
    let data = b"abcdefgh";
    let ck = HvChecksum::compute(HvCksumType::Fletcher4, data);
    check!(ck.verify(data), "8-byte fletcher4 verifies");
    TestResult::Pass
}

fn test_checksum_fletcher2_large_data() -> TestResult {
    let data: alloc::vec::Vec<u8> = (0..4096u32).map(|i| (i % 256) as u8).collect();
    let ck = HvChecksum::compute(HvCksumType::Fletcher2, &data);
    check!(ck.verify(&data), "large fletcher2 verifies");
    TestResult::Pass
}

fn test_checksum_fletcher4_large_data() -> TestResult {
    let data: alloc::vec::Vec<u8> = (0..8192u32).map(|i| (i % 256) as u8).collect();
    let ck = HvChecksum::compute(HvCksumType::Fletcher4, &data);
    check!(ck.verify(&data), "large fletcher4 verifies");
    TestResult::Pass
}

fn test_checksum_fletcher2_different_lengths() -> TestResult {
    let ck1 = HvChecksum::compute(HvCksumType::Fletcher2, b"hello");
    let ck2 = HvChecksum::compute(HvCksumType::Fletcher2, b"hello world");
    check!(ck1.value != ck2.value, "different lengths differ");
    TestResult::Pass
}

fn test_checksum_fletcher4_different_lengths() -> TestResult {
    let ck1 = HvChecksum::compute(HvCksumType::Fletcher4, b"hello");
    let ck2 = HvChecksum::compute(HvCksumType::Fletcher4, b"hello world");
    check!(ck1.value != ck2.value, "different lengths differ");
    TestResult::Pass
}

fn test_checksum_verify_single_bit_flip() -> TestResult {
    let data = b"some test data for verification";
    let ck = HvChecksum::compute(HvCksumType::Fletcher4, data);
    let mut corrupted: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    corrupted.extend_from_slice(data);
    corrupted[5] ^= 0x01;
    check!(!ck.verify(&corrupted), "single bit flip detected");
    TestResult::Pass
}

fn test_checksum_off_verify_always_true() -> TestResult {
    let ck = HvChecksum::compute(HvCksumType::Off, b"any data");
    check!(ck.verify(b"different data"), "Off verifies any data");
    TestResult::Pass
}

fn test_checksum_sha256_short_data() -> TestResult {
    let data = b"ab";
    let ck1 = HvChecksum::compute(HvCksumType::SHA256, data);
    let ck2 = HvChecksum::compute(HvCksumType::SHA256, data);
    assert_eq_test!(ck1.value, ck2.value, "SHA256 short deterministic");
    TestResult::Pass
}

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: FIPS 180-4 SHA-256('abc') 测试向量按 u64 四字面值书写, 有明确标准出处"
)]
fn test_checksum_sha256_known_vector() -> TestResult {
    let ck = HvChecksum::compute(HvCksumType::SHA256, b"abc");
    let expected: [u64; 4] = [
        0xba7816bf8f01cfea,
        0x414140de5dae2223,
        0xb00361a396177a9c,
        0xb410ff61f20015ad,
    ];
    assert_eq_test!(ck.value, expected, "SHA256('abc') FIPS vector");
    TestResult::Pass
}

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: FIPS 180-4 SHA-256('') 测试向量按 u64 四字面值书写, 有明确标准出处"
)]
fn test_checksum_sha256_empty() -> TestResult {
    let ck = HvChecksum::compute(HvCksumType::SHA256, b"");
    let expected: [u64; 4] = [
        0xe3b0c44298fc1c14,
        0x9afbf4c8996fb924,
        0x27ae41e4649b934c,
        0xa495991b7852b855,
    ];
    assert_eq_test!(ck.value, expected, "SHA256('') FIPS vector");
    TestResult::Pass
}

fn test_checksum_sha256_long_data() -> TestResult {
    let data = b"sha256 via checksum module - this is a longer string that spans multiple blocks";
    let ck1 = HvChecksum::compute(HvCksumType::SHA256, data);
    let ck2 = HvChecksum::compute(HvCksumType::SHA256, data);
    assert_eq_test!(ck1.value, ck2.value, "SHA256 long deterministic");
    TestResult::Pass
}

fn test_spa_config_name() -> TestResult {
    let cfg = HvSpaConfig::new("test-pool");
    let name = core::str::from_utf8(&cfg.name)
        .unwrap_or("")
        .trim_end_matches('\0');
    check!(name.starts_with("test-pool"), "expected test-pool prefix");
    TestResult::Pass
}

fn test_spa_uberblock_null() -> TestResult {
    let ub = HvUberblock::null();
    check!(!ub.is_valid(), "null uberblock should be invalid");
    TestResult::Pass
}

fn test_spa_uberblock_checksum() -> TestResult {
    let mut ub = HvUberblock {
        txg: 1,
        root_bp: HvBlockPointer::null(),
        timestamp: 100,
        root_dataset_obj: 0,
        pool_guid: 0xABCD,
        checkpoint_txg: 0,
        checksum: [0; 4],
        magic: HV_SPA_MAGIC,
        pwm_domain_id: 0,
        _pad: [0; 2],
    };
    ub.compute_checksum();
    check!(ub.verify_checksum(), "checksum should verify");
    TestResult::Pass
}

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
fn test_spa_uberblock_invalid_magic() -> TestResult {
    let mut ub = HvUberblock::null();
    ub.magic = 0xDEADBEEF;
    check!(!ub.is_valid(), "wrong magic should be invalid");
    TestResult::Pass
}

fn test_dmu_object_default() -> TestResult {
    let obj = HvDmuObject::new_file(1, 0);
    check!(obj.obj_id == 1, "obj_id mismatch");
    check!(obj.obj_type == HvObjType::File, "obj_type should be File");
    check!(obj.size == 0, "new object size should be 0");
    TestResult::Pass
}

fn test_dmu_object_cow() -> TestResult {
    let mut obj = HvDmuObject::new_file(2, 0);
    let new_bp = HvBlockPointer::null();
    obj.cow_bp(new_bp, 5);
    check!(obj.birth_txg == 5, "birth txg should be 5");
    TestResult::Pass
}

fn test_dmu_object_dir_type() -> TestResult {
    let obj = HvDmuObject::new_dir(3, 0);
    check!(obj.is_dir(), "Dir should report as dir");
    TestResult::Pass
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
fn test_zap_insert_lookup() -> TestResult {
    let zap = HvZap::new();
    zap.insert_u64("key1", 42);
    let val = match zap.lookup_u64("key1") {
        Some(v) => v,
        None => return TestResult::Fail("key1 not found"),
    };
    check!(val == 42, "value mismatch");
    TestResult::Pass
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
fn test_zap_overwrite() -> TestResult {
    let zap = HvZap::new();
    zap.insert_u64("key1", 10);
    zap.insert_u64("key1", 99);
    let val = match zap.lookup_u64("key1") {
        Some(v) => v,
        None => return TestResult::Fail("key1 not found after overwrite"),
    };
    check!(val == 99, "overwrite should set 99");
    TestResult::Pass
}

fn test_zap_nonexistent() -> TestResult {
    let zap = HvZap::new();
    check!(
        zap.lookup_u64("no_such_key").is_none(),
        "nonexistent should be None"
    );
    TestResult::Pass
}

fn test_zap_remove() -> TestResult {
    let zap = HvZap::new();
    zap.insert_u64("rm_me", 7);
    check!(
        zap.lookup_u64("rm_me").is_some(),
        "should exist before remove"
    );
    zap.remove("rm_me");
    check!(
        zap.lookup_u64("rm_me").is_none(),
        "should not exist after remove"
    );
    TestResult::Pass
}

fn test_txg_group_init() -> TestResult {
    let mut tg = HvTxgGroup::new();
    tg.init(1);
    check!(tg.current_txg() >= 1, "txg current should be at least 1");
    TestResult::Pass
}

fn test_txg_group_transition() -> TestResult {
    let mut tg = HvTxgGroup::new();
    tg.init(1);
    let new_txg = tg.transition();
    check!(new_txg >= 2, "txg should advance");
    TestResult::Pass
}

fn test_zil_record_create() -> TestResult {
    let rec = HvZilRecord::new_create(1, 0, "test_file");
    check!(rec.txg == 1, "txg mismatch");
    check!(rec.obj_id == 0, "obj_id should be 0");
    TestResult::Pass
}

fn test_zil_record_write() -> TestResult {
    let rec = HvZilRecord::new_write(2, 10, 0, 1024);
    check!(rec.txg == 2, "txg mismatch");
    check!(rec.obj_id == 10, "obj_id mismatch");
    TestResult::Pass
}

fn test_zil_add_and_sync() -> TestResult {
    let zil = HvZil::new();
    zil.init();
    zil.add_record(HvZilRecord::new_write(1, 5, 0, 512));
    zil.sync(1);
    check!(
        zil.committed_seq.load(core::sync::atomic::Ordering::SeqCst) >= 1,
        "committed_seq should advance"
    );
    TestResult::Pass
}

fn test_arc_init() -> TestResult {
    let arc = HvArc::new();
    arc.init(128);
    check!(arc.is_initialized(), "arc should be initialized");
    TestResult::Pass
}

fn test_arc_lookup_miss() -> TestResult {
    let arc = HvArc::new();
    arc.init(128);
    let key = HvArcKey::new(0, 0, 0);
    check!(arc.lookup(&key).is_none(), "empty arc should return None");
    TestResult::Pass
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
fn test_arc_insert_lookup() -> TestResult {
    let arc = HvArc::new();
    arc.init(128);
    let key = HvArcKey::new(0, 4096, 1);
    let data: [u8; 16] = [0xAA; 16];
    arc.insert(key, &data, HvArcBufType::Data);
    let ptr = match arc.lookup(&key) {
        Some(p) => p,
        None => return TestResult::Fail("should find inserted entry"),
    };
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let found = unsafe { core::slice::from_raw_parts(ptr, 16) };
    check!(found[0] == 0xAA, "data mismatch");
    TestResult::Pass
}

pub fn register_hvfs_tests() {
    let r = runner();
    register_tests_inner! { r:
        "hvfs::bp": {
            "null": test_bp_null,
            "dva_set_get": test_bp_dva_set_get,
            "birth_txg": test_bp_birth_txg,
        },
        "hvfs::checksum": {
            "fletcher4_basic": test_checksum_fletcher4_basic,
            "different_data": test_checksum_different_data,
            // E-06 (2026-09-07): host-tests/src/checksum.rs 去重载体用例合入
            "fletcher2_empty": test_checksum_fletcher2_empty,
            "fletcher2_deterministic": test_checksum_fletcher2_deterministic,
            "fletcher4_empty": test_checksum_fletcher4_empty,
            "fletcher4_deterministic": test_checksum_fletcher4_deterministic,
            "verify_roundtrip_fletcher2": test_checksum_verify_roundtrip_fletcher2,
            "verify_roundtrip_fletcher4": test_checksum_verify_roundtrip_fletcher4,
            "verify_detects_corruption": test_checksum_verify_detects_corruption,
            "off_always_zero": test_checksum_off_always_zero,
            "edonr_uses_fletcher4": test_checksum_edonr_uses_fletcher4,
            "fletcher2_single_byte": test_checksum_fletcher2_single_byte,
            "fletcher4_single_byte": test_checksum_fletcher4_single_byte,
            "fletcher2_odd_length": test_checksum_fletcher2_odd_length,
            "fletcher4_odd_length": test_checksum_fletcher4_odd_length,
            "fletcher2_exact_8_bytes": test_checksum_fletcher2_exact_8_bytes,
            "fletcher4_exact_8_bytes": test_checksum_fletcher4_exact_8_bytes,
            "fletcher2_large_data": test_checksum_fletcher2_large_data,
            "fletcher4_large_data": test_checksum_fletcher4_large_data,
            "fletcher2_different_lengths": test_checksum_fletcher2_different_lengths,
            "fletcher4_different_lengths": test_checksum_fletcher4_different_lengths,
            "verify_single_bit_flip": test_checksum_verify_single_bit_flip,
            "off_verify_always_true": test_checksum_off_verify_always_true,
            "sha256_short_data": test_checksum_sha256_short_data,
            "sha256_known_vector": test_checksum_sha256_known_vector,
            "sha256_empty": test_checksum_sha256_empty,
            "sha256_long_data": test_checksum_sha256_long_data,
        },
        "hvfs::spa": {
            "config_name": test_spa_config_name,
            "uberblock_null": test_spa_uberblock_null,
            "uberblock_checksum": test_spa_uberblock_checksum,
            "uberblock_invalid": test_spa_uberblock_invalid_magic,
        },
        "hvfs::dmu": {
            "default": test_dmu_object_default,
            "cow": test_dmu_object_cow,
            "dir_type": test_dmu_object_dir_type,
        },
        "hvfs::zap": {
            "insert_lookup": test_zap_insert_lookup,
            "overwrite": test_zap_overwrite,
            "nonexistent": test_zap_nonexistent,
            "remove": test_zap_remove,
        },
        "hvfs::txg": {
            "group_init": test_txg_group_init,
            "transition": test_txg_group_transition,
        },
        "hvfs::zil": {
            "record_create": test_zil_record_create,
            "record_write": test_zil_record_write,
            "add_and_sync": test_zil_add_and_sync,
        },
        "hvfs::arc": {
            "init": test_arc_init,
            "lookup_miss": test_arc_lookup_miss,
            "insert_lookup": test_arc_insert_lookup,
        },
    }
}
