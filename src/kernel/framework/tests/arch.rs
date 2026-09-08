use crate::kernel::framework::cpu::{CacheInfo, CpuSignature, CpuVendor, TopologyInfo};
use crate::kernel::framework::tests::{TestResult, assert_eq_test, check, runner};
use crate::register_tests_inner;

#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::arch::x86_64::gdt::{
    AccessByte, GdtEntry, Granularity, SELECTOR_KERNEL_CODE, SELECTOR_KERNEL_DATA, SELECTOR_NULL,
    SELECTOR_TSS, SELECTOR_USER_CODE, SELECTOR_USER_DATA,
};
#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::arch::x86_64::tss::{DEFAULT_IOMAP_BASE, TSS_SIZE, TaskStateSegment};

#[cfg(target_arch = "x86_64")]
fn gdt_entry_null() -> TestResult {
    let null_desc = GdtEntry::null();
    // SAFETY: `const` 由调用方保证为有效指针; 只读访问
    let bytes = unsafe {
        core::ptr::read_volatile(core::ptr::from_ref(&null_desc) as *const u64)
    };
    check!(bytes == 0, "Null descriptor should be all zeros");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn gdt_access_byte_constants() -> TestResult {
    assert_eq_test!(AccessByte::kernel_code().0, 0x9A, "kernel_code access byte");
    assert_eq_test!(AccessByte::kernel_data().0, 0x92, "kernel_data access byte");
    assert_eq_test!(AccessByte::user_code().0, 0xFA, "user_code access byte");
    assert_eq_test!(AccessByte::user_data().0, 0xF2, "user_data access byte");
    assert_eq_test!(AccessByte::tss().0, 0x89, "tss access byte");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn gdt_granularity_constants() -> TestResult {
    let code_gran = Granularity::code_64bit();
    check!(
        code_gran.0 & Granularity::PAGE_GRANULARITY != 0,
        "code gran should have page granularity"
    );
    check!(
        code_gran.0 & Granularity::LONG_MODE != 0,
        "code gran should have long mode"
    );

    let data_gran = Granularity::data_32bit();
    check!(
        data_gran.0 & Granularity::PAGE_GRANULARITY != 0,
        "data gran should have page granularity"
    );
    check!(
        data_gran.0 & Granularity::SIZE_32BIT != 0,
        "data gran should have size 32bit"
    );
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn gdt_selector_values() -> TestResult {
    assert_eq_test!(SELECTOR_NULL, 0x00, "NULL selector");
    assert_eq_test!(SELECTOR_KERNEL_CODE, 0x08, "kernel code selector");
    assert_eq_test!(SELECTOR_KERNEL_DATA, 0x10, "kernel data selector");
    // 2026-06-29 修复: 原测试 USER_CODE/USER_DATA 位置写反 (与实装相反).
    // 实装: USER_DATA = 0x18, USER_CODE = 0x20 (同 gdt.rs:720-721 内嵌 assert_eq).
    assert_eq_test!(SELECTOR_USER_DATA, 0x18, "user data selector");
    assert_eq_test!(SELECTOR_USER_CODE, 0x20, "user code selector");
    assert_eq_test!(SELECTOR_TSS, 0x28, "TSS selector");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn tss_zeroed() -> TestResult {
    let tss = TaskStateSegment::zeroed();
    assert_eq_test!(tss.rsp0, 0, "rsp0 should be 0");
    assert_eq_test!(tss.rsp1, 0, "rsp1 should be 0");
    assert_eq_test!(tss.rsp2, 0, "rsp2 should be 0");
    assert_eq_test!(tss.iomap_base, DEFAULT_IOMAP_BASE, "iomap_base default");
    for i in 0..7 {
        assert_eq_test!(tss.get_ist(i), Some(0), "IST should be 0");
    }
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn tss_set_kernel_stack() -> TestResult {
    let mut tss = TaskStateSegment::zeroed();
    tss.set_kernel_stack(0xDEAD_BEEF_CAFE_BABE);
    assert_eq_test!(
        tss.get_kernel_stack(),
        0xDEAD_BEEF_CAFE_BABE,
        "kernel stack mismatch"
    );
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn tss_ist_operations() -> TestResult {
    let mut tss = TaskStateSegment::zeroed();
    tss.set_ist(0, 0x1111_2222_3333_4444);
    tss.set_ist(6, 0xAAAA_BBBB_CCCC_DDDD);
    assert_eq_test!(tss.get_ist(0), Some(0x1111_2222_3333_4444), "IST0 mismatch");
    assert_eq_test!(tss.get_ist(6), Some(0xAAAA_BBBB_CCCC_DDDD), "IST6 mismatch");
    assert_eq_test!(tss.get_ist(3), Some(0), "unset IST should be 0");
    assert_eq_test!(tss.get_ist(7), None, "out-of-range IST should be None");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn tss_iomap() -> TestResult {
    let mut tss = TaskStateSegment::zeroed();
    check!(!tss.has_iomap(), "should not have iomap by default");
    let offset = (TSS_SIZE / 2) as u16;
    tss.enable_iomap(offset);
    check!(tss.has_iomap(), "should have iomap after enable");
    assert_eq_test!(tss.iomap_base, offset, "iomap_base mismatch");
    tss.disable_iomap();
    check!(!tss.has_iomap(), "should not have iomap after disable");
    assert_eq_test!(
        tss.iomap_base,
        DEFAULT_IOMAP_BASE,
        "iomap_base should reset"
    );
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn tss_size() -> TestResult {
    check!(TSS_SIZE >= 92, "TSS too small");
    assert_eq_test!(TSS_SIZE % 2, 0, "TSS not 2-byte aligned");
    TestResult::Pass
}

fn cpu_vendor_recognition() -> TestResult {
    assert_eq_test!(
        CpuVendor::from_vendor_string(b"GenuineIntel"),
        CpuVendor::Intel,
        "Intel vendor"
    );
    assert_eq_test!(
        CpuVendor::from_vendor_string(b"AuthenticAMD"),
        CpuVendor::Amd,
        "AMD vendor"
    );
    assert_eq_test!(
        CpuVendor::from_vendor_string(b"CentaurHauls"),
        CpuVendor::Via,
        "VIA vendor"
    );
    assert_eq_test!(
        CpuVendor::from_vendor_string(b"TCGTCGTCG??\0"),
        CpuVendor::Qemu,
        "QEMU vendor"
    );
    assert_eq_test!(
        CpuVendor::from_vendor_string(b"UnknownVend\0"),
        CpuVendor::Unknown,
        "Unknown vendor"
    );
    TestResult::Pass
}

fn cpu_signature_effective_values() -> TestResult {
    let sig = CpuSignature {
        family: 6,
        model: 0x9E,
        ext_family: 0,
        ext_model: 0,
        ..Default::default()
    };
    assert_eq_test!(sig.effective_family(), 6, "effective family");
    assert_eq_test!(sig.effective_model(), 0x9E, "effective model");

    let sig_ext = CpuSignature {
        family: 0xF,
        model: 0x07,
        ext_family: 0x06,
        ext_model: 0x09,
        ..Default::default()
    };
    assert_eq_test!(sig_ext.effective_family(), 0x15, "ext effective family");
    assert_eq_test!(sig_ext.effective_model(), 0x97, "ext effective model");
    TestResult::Pass
}

fn cpu_cache_info_total() -> TestResult {
    let cache = CacheInfo {
        l1d_size: 32 * 1024,
        l1i_size: 32 * 1024,
        l2_size: 256 * 1024,
        l3_size: 8 * 1024 * 1024,
        ..Default::default()
    };
    assert_eq_test!(
        cache.total_size(),
        (32 + 32 + 256 + 8192) * 1024,
        "cache total size"
    );
    check!(cache.has_l3(), "should have L3");

    let no_l3 = CacheInfo {
        l3_size: 0,
        ..Default::default()
    };
    check!(!no_l3.has_l3(), "should not have L3");
    TestResult::Pass
}

fn cpu_topology_threads_per_core() -> TestResult {
    let single = TopologyInfo {
        physical_cores: 4,
        logical_threads: 4,
        hyperthreading_enabled: false,
        ..Default::default()
    };
    assert_eq_test!(single.threads_per_core(), 1, "single thread per core");
    check!(!single.is_single_core(), "4 cores is not single");

    let ht = TopologyInfo {
        physical_cores: 4,
        logical_threads: 8,
        hyperthreading_enabled: true,
        ..Default::default()
    };
    assert_eq_test!(ht.threads_per_core(), 2, "HT threads per core");

    let mono = TopologyInfo {
        physical_cores: 1,
        logical_threads: 1,
        ..Default::default()
    };
    check!(mono.is_single_core(), "1 core is single");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
pub fn register_gdt_tests() {
    let r = runner();
    register_tests_inner! { r:
        "arch::gdt": {
            "null_descriptor": gdt_entry_null,
            "access_byte_constants": gdt_access_byte_constants,
            "granularity_constants": gdt_granularity_constants,
            "selector_values": gdt_selector_values,
        },
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn register_gdt_tests() {}

#[cfg(target_arch = "x86_64")]
pub fn register_tss_tests() {
    let r = runner();
    register_tests_inner! { r:
        "arch::tss": {
            "zeroed": tss_zeroed,
            "set_kernel_stack": tss_set_kernel_stack,
            "ist_operations": tss_ist_operations,
            "iomap": tss_iomap,
            "size": tss_size,
        },
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn register_tss_tests() {}

pub fn register_cpu_tests() {
    let r = runner();
    register_tests_inner! { r:
        "cpu": {
            "vendor_recognition": cpu_vendor_recognition,
            "signature_effective_values": cpu_signature_effective_values,
            "cache_info_total": cpu_cache_info_total,
            "topology_threads_per_core": cpu_topology_threads_per_core,
        },
    }
}

pub fn register_cpuid_tests() {}

pub fn register_tests() {
    register_gdt_tests();
    register_tss_tests();
    register_cpu_tests();
    register_cpuid_tests();
}
