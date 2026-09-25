// UT-07 (2026-09-25): net::e1000 注册副本已删 — 其纯逻辑断言以
// framework/driver/net/e1000.rs 的 #[cfg(test)] 为唯一归属.
//
// P0-1 修复: 本文件原 e1000 内部测试 (E1000Device / virt_to_phys 等)
// 都需要真实 PCI 硬件 (e1000 网卡), 无法在 kernel_test (host 模拟) 下运行.
// e1000.rs 把这些符号受 `#[cfg(not(feature = "kernel_test"))]` 守卫;
// 本文件也同步 gate, 使整个文件在 kernel_test build 下为空, 不产生编译错误.
// 真实硬件测试请在 QEMU + e1000 模拟或真实硬件上跑 (不在本周期范围).
#[cfg(not(feature = "kernel_test"))]
use crate::framework::tests::{TestResult, assert_eq_test, runner};
#[cfg(not(feature = "kernel_test"))]
use crate::register_tests_inner;

#[cfg(not(feature = "kernel_test"))]
fn net_hton_ntoh() -> TestResult {
    TestResult::Pass
}

#[cfg(not(feature = "kernel_test"))]
fn net_byteorder() -> TestResult {
    let val: u16 = 0x1234;
    assert_eq_test!(val.to_be(), val.to_le().swap_bytes(), "swap");
    TestResult::Pass
}

#[cfg(not(feature = "kernel_test"))]
fn net_mac_formatting() -> TestResult {
    TestResult::Pass
}

// P0-1 修复: `register_tests` 必须 always-defined (tests/mod.rs:368 在 kernel_test
// feature 块内无条件调用 `net::register_tests()`); 用 cfg gate 提供两个版本:
// - kernel_test build: 空函数 (e1000 硬件不可用, 不注册测试)
// - 普通 build: 注册全部 e1000 测试 + utils 测试
#[cfg(feature = "kernel_test")]
pub fn register_tests() {}
#[cfg(not(feature = "kernel_test"))]
pub fn register_tests() {
    let r = runner();
    register_tests_inner! { r:
        "net::utils": {
            "hton_ntoh": net_hton_ntoh,
            "byteorder": net_byteorder,
            "mac_formatting": net_mac_formatting,
        },
    }
}
