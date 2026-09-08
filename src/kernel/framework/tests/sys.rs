use crate::kernel::framework::credo::constant_time_eq;
use crate::kernel::framework::credo::secure_boot::{sha256_extend, sha256_hash};
use crate::kernel::framework::credo::sha256::sha256;
use crate::kernel::framework::errno::{errno_from_i64, Errno};
use crate::kernel::framework::mm::slab::{
    GENERAL_CACHE_SIZES, KmemCache, SLAB_MAX_OBJECT_SIZE, SLAB_MIN_OBJECT_SIZE,
    find_general_cache_index,
};
use crate::kernel::framework::proc::elf::{Elf64Header, Elf64Phdr};
use crate::kernel::framework::tests::{TestResult, assert_eq_test, check, runner};
use crate::kernel::framework::timer::pit::{
    DEFAULT_INTERRUPT_FREQ_HZ, PIT_BASE_FREQUENCY, PIT_MAX_COUNT, PIT_MIN_COUNT,
};
use crate::register_tests_inner;

fn slab_cache_creation() -> TestResult {
    let cache = KmemCache::create("test_cache", 64);
    check!(cache.is_ok(), "cache creation should succeed");
    let cache = cache.unwrap();
    assert_eq_test!(cache.object_size, 64, "object size");
    check!(cache.objects_per_slab > 0, "objects per slab");
    assert_eq_test!(cache.slab_count, 0, "initial slab count");
    TestResult::Pass
}

fn slab_cache_invalid_size() -> TestResult {
    check!(KmemCache::create("zero", 0).is_err(), "size 0 rejected");
    check!(
        KmemCache::create("huge", SLAB_MAX_OBJECT_SIZE + 1).is_err(),
        "oversize rejected"
    );
    TestResult::Pass
}

fn slab_cache_min_size() -> TestResult {
    let cache = KmemCache::create("tiny", 8).unwrap();
    assert_eq_test!(cache.object_size, SLAB_MIN_OBJECT_SIZE, "min size enforced");
    TestResult::Pass
}

fn slab_general_cache_sizes() -> TestResult {
    assert_eq_test!(GENERAL_CACHE_SIZES[0], 16, "size 0");
    assert_eq_test!(GENERAL_CACHE_SIZES[3], 128, "size 3");
    assert_eq_test!(GENERAL_CACHE_SIZES[7], 2048, "size 7");
    TestResult::Pass
}

fn slab_find_general_cache_index() -> TestResult {
    assert_eq_test!(find_general_cache_index(16), Some(0), "idx 16");
    assert_eq_test!(find_general_cache_index(32), Some(1), "idx 32");
    assert_eq_test!(find_general_cache_index(64), Some(2), "idx 64");
    assert_eq_test!(find_general_cache_index(2048), Some(7), "idx 2048");
    assert_eq_test!(find_general_cache_index(3000), None, "idx 3000");
    TestResult::Pass
}

fn syscall_error_conversion() -> TestResult {
    assert_eq_test!(Errno::EPERM.as_ret(), -1, "EPERM");
    assert_eq_test!(Errno::ENOMEM.as_ret(), -12, "ENOMEM");
    assert_eq_test!(Errno::EINVAL.as_ret(), -22, "EINVAL");
    TestResult::Pass
}

fn syscall_error_from_i64() -> TestResult {
    assert_eq_test!(errno_from_i64(-1), Some(Errno::EPERM), "from -1");
    assert_eq_test!(errno_from_i64(-22), Some(Errno::EINVAL), "from -22");
    assert_eq_test!(errno_from_i64(-999), None, "from -999");
    TestResult::Pass
}

fn syscall_error_display() -> TestResult {
    let s = alloc::format!("{}", Errno::EPERM);
    assert_eq_test!(s.as_str(), "Operation not permitted", "EPERM display");
    let s = alloc::format!("{}", Errno::ENOSYS);
    assert_eq_test!(s.as_str(), "Function not implemented", "ENOSYS display");
    TestResult::Pass
}

fn sha256_empty() -> TestResult {
    let expected: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9,
        0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52,
        0xb8, 0x55,
    ];
    assert_eq_test!(sha256(b""), expected, "SHA256 empty");
    TestResult::Pass
}

fn sha256_abc() -> TestResult {
    let expected: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];
    assert_eq_test!(sha256(b"abc"), expected, "SHA256 abc");
    TestResult::Pass
}

// E-06 (2026-09-07): host-tests/src/sha256.rs 去重载体用例合入 (同被测对象
// framework::credo::sha256 的双端共享用例统一收口到本套件, 删除 host-tests 侧重复载体).
// 覆盖 SHA-256 已知长消息向量、块边界 (55/56/63/64)、雪崩效应与确定性/大输入.

fn sha256_long_message() -> TestResult {
    let expected: [u8; 32] = [
        0x24, 0x8d, 0x6a, 0x61, 0xd2, 0x06, 0x38, 0xb8, 0xe5, 0xc0, 0x26, 0x93, 0x0c, 0x3e,
        0x60, 0x39, 0xa3, 0x3c, 0xe4, 0x59, 0x64, 0xff, 0x21, 0x67, 0xf6, 0xec, 0xed, 0xd4,
        0x19, 0xdb, 0x06, 0xc1,
    ];
    assert_eq_test!(
        sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        expected,
        "SHA256 long message"
    );
    TestResult::Pass
}

fn sha256_deterministic() -> TestResult {
    let h1 = sha256(b"hello world");
    let h2 = sha256(b"hello world");
    assert_eq_test!(h1, h2, "same input same hash");
    TestResult::Pass
}

fn sha256_different_inputs() -> TestResult {
    let h1 = sha256(b"hello");
    let h2 = sha256(b"world");
    check!(h1 != h2, "different inputs differ");
    TestResult::Pass
}

fn sha256_single_byte() -> TestResult {
    let h = sha256(b"a");
    check!(h != [0u8; 32], "single byte non-zero");
    TestResult::Pass
}

fn sha256_exactly_55_bytes() -> TestResult {
    let data: alloc::vec::Vec<u8> = (0..55u32).map(|i| i as u8).collect();
    let h = sha256(&data);
    check!(h != [0u8; 32], "55 bytes non-zero");
    TestResult::Pass
}

fn sha256_exactly_56_bytes() -> TestResult {
    let data: alloc::vec::Vec<u8> = (0..56u32).map(|i| i as u8).collect();
    let h = sha256(&data);
    check!(h != [0u8; 32], "56 bytes non-zero");
    TestResult::Pass
}

fn sha256_exactly_63_bytes() -> TestResult {
    let data: alloc::vec::Vec<u8> = (0..63u32).map(|i| i as u8).collect();
    let h = sha256(&data);
    check!(h != [0u8; 32], "63 bytes non-zero");
    TestResult::Pass
}

fn sha256_exactly_64_bytes() -> TestResult {
    let data: alloc::vec::Vec<u8> = (0..64u32).map(|i| i as u8).collect();
    let h = sha256(&data);
    check!(h != [0u8; 32], "64 bytes non-zero");
    TestResult::Pass
}

fn sha256_multi_block() -> TestResult {
    let data: alloc::vec::Vec<u8> = (0..200u32).map(|i| (i % 256) as u8).collect();
    let h1 = sha256(&data);
    let h2 = sha256(&data);
    assert_eq_test!(h1, h2, "multi-block deterministic");
    TestResult::Pass
}

fn sha256_all_zeros() -> TestResult {
    let data = [0u8; 64];
    let h = sha256(&data);
    check!(h != [0u8; 32], "all zeros non-zero");
    TestResult::Pass
}

fn sha256_all_ones() -> TestResult {
    let data = [0xFFu8; 64];
    let h = sha256(&data);
    check!(h != [0u8; 32], "all ones non-zero");
    TestResult::Pass
}

fn sha256_avalanche_effect() -> TestResult {
    let h1 = sha256(b"hello");
    let h2 = sha256(b"hellp");
    let diff_bits: u32 = h1
        .iter()
        .zip(h2.iter())
        .map(|(a, b)| (a ^ b).count_ones())
        .sum();
    check!(diff_bits > 32, "avalanche flips many bits");
    TestResult::Pass
}

fn sha256_large_input() -> TestResult {
    let data: alloc::vec::Vec<u8> = (0..10000u32).map(|i| (i % 256) as u8).collect();
    let h = sha256(&data);
    check!(h != [0u8; 32], "large input non-zero");
    TestResult::Pass
}

// B08-19 专项: secure_boot 侧 sha256_hash 委托规范实现, 输出与已知向量一致
fn secure_boot_sha256_hash_consistency() -> TestResult {
    let expected: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];
    assert_eq_test!(sha256_hash(b"abc"), expected, "secure_boot sha256_hash == 规范向量");
    TestResult::Pass
}

// B08-19 专项: sha256_extend(A, B) == sha256(A || B) 组合语义
fn secure_boot_sha256_extend_combine() -> TestResult {
    let a = *b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let b = *b"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
    let mut combined = [0u8; 64];
    combined[..32].copy_from_slice(&a);
    combined[32..].copy_from_slice(&b);
    assert_eq_test!(sha256_extend(&a, &b), sha256(&combined), "extend == hash(A||B)");
    TestResult::Pass
}

// B08-22a 专项: framework 权威 constant_time_eq 行为 (services ct_eq 委托链由 crypto.rs 单测覆盖)
fn constant_time_eq_authority() -> TestResult {
    check!(constant_time_eq(b"abc", b"abc"), "equal 相等");
    check!(!constant_time_eq(b"abc", b"abd"), "末尾位不同");
    check!(!constant_time_eq(b"abc", b"ab"), "长度不同");
    check!(constant_time_eq(b"", b""), "空切片相等");
    TestResult::Pass
}

// B08-22b 专项: ELF64 头统一定义布局 (coredump 与 loader 共用 elf/mod.rs)
fn elf64_header_layout() -> TestResult {
    assert_eq_test!(core::mem::size_of::<Elf64Header>(), 64, "ELF64 header 64B");
    assert_eq_test!(core::mem::size_of::<Elf64Phdr>(), 56, "ELF64 phdr 56B");
    TestResult::Pass
}

fn pit_constants() -> TestResult {
    assert_eq_test!(PIT_BASE_FREQUENCY, 1_193_182, "PIT base freq");
    check!(DEFAULT_INTERRUPT_FREQ_HZ > 0, "default freq positive");
    assert_eq_test!(PIT_MAX_COUNT, 65535, "max count");
    assert_eq_test!(PIT_MIN_COUNT, 1, "min count");
    TestResult::Pass
}

fn pit_divisor_calculation() -> TestResult {
    let divisor = PIT_BASE_FREQUENCY / 1000;
    assert_eq_test!(divisor, 1193, "1000Hz divisor");
    let actual_freq = PIT_BASE_FREQUENCY / divisor;
    check!(
        (actual_freq as i64 - 1000).abs() < 5,
        "actual freq close to 1000"
    );
    TestResult::Pass
}

fn pit_frequency_bounds() -> TestResult {
    check!(PIT_MIN_COUNT >= 1, "min count >= 1");
    assert_eq_test!(u64::from(PIT_MAX_COUNT), 65535, "max count value");
    let max_freq = PIT_BASE_FREQUENCY / u64::from(PIT_MIN_COUNT);
    check!(max_freq > 1_000_000, "max freq > 1MHz");
    let min_freq = PIT_BASE_FREQUENCY / u64::from(PIT_MAX_COUNT);
    check!(min_freq < 20, "min freq < 20Hz");
    TestResult::Pass
}

pub fn register_slab_tests() {
    let r = runner();
    register_tests_inner! { r:
        "mm::slab": {
            "cache_creation": slab_cache_creation,
            "cache_invalid_size": slab_cache_invalid_size,
            "cache_min_size": slab_cache_min_size,
            "general_cache_sizes": slab_general_cache_sizes,
            "find_general_cache_index": slab_find_general_cache_index,
        },
    }
}

pub fn register_syscall_ffi_tests() {
    let r = runner();
    register_tests_inner! { r:
        "syscall::ffi": {
            "error_conversion": syscall_error_conversion,
            "error_from_i64": syscall_error_from_i64,
            "error_display": syscall_error_display,
        },
    }
}

pub fn register_sha256_tests() {
    let r = runner();
    register_tests_inner! { r:
        "pwm::sha256": {
            "empty": sha256_empty,
            "abc": sha256_abc,
            // E-06 (2026-09-07): host-tests/src/sha256.rs 去重载体用例合入
            "long_message": sha256_long_message,
            "deterministic": sha256_deterministic,
            "different_inputs": sha256_different_inputs,
            "single_byte": sha256_single_byte,
            "boundary_55": sha256_exactly_55_bytes,
            "boundary_56": sha256_exactly_56_bytes,
            "boundary_63": sha256_exactly_63_bytes,
            "boundary_64": sha256_exactly_64_bytes,
            "multi_block": sha256_multi_block,
            "all_zeros": sha256_all_zeros,
            "all_ones": sha256_all_ones,
            "avalanche": sha256_avalanche_effect,
            "large_input": sha256_large_input,
        },
        // B08-19/B08-22 专项 (重复实现合并后的一致性测试)
        "pwm::secure_boot_sha256": {
            "hash_consistency": secure_boot_sha256_hash_consistency,
            "extend_combine": secure_boot_sha256_extend_combine,
        },
        "pwm::ct_eq_authority": {
            "constant_time_eq": constant_time_eq_authority,
        },
        "proc::elf64_header": {
            "layout": elf64_header_layout,
        },
    }
}

pub fn register_pit_tests() {
    let r = runner();
    register_tests_inner! { r:
        "timer::pit": {
            "constants": pit_constants,
            "divisor_calculation": pit_divisor_calculation,
            "frequency_bounds": pit_frequency_bounds,
        },
    }
}

pub fn register_tests() {
    register_slab_tests();
    register_syscall_ffi_tests();
    register_sha256_tests();
    register_pit_tests();
}
