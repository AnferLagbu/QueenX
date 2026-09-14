//! # BBR/BSR/BHR 单元测试
//!
//! 测试 Barrier Base/Soft/Hard Recovery 功能
use crate::register_tests_inner;

use crate::framework::tests::{TestResult, assert_eq_test, check, runner};

fn config_recovery_result() -> TestResult {
    use crate::framework::barrier::reset::config::tests;
    check!(tests::test_recovery_result(), "recovery result");
    check!(tests::test_recovery_layer(), "recovery layer");
    TestResult::Pass
}

fn config_default() -> TestResult {
    use crate::framework::barrier::reset::config::tests;
    check!(tests::test_config_default(), "config default");
    TestResult::Pass
}

fn config_stats() -> TestResult {
    use crate::framework::barrier::reset::config::tests;
    check!(tests::test_stats(), "recovery stats");
    TestResult::Pass
}

fn audit_log_basic() -> TestResult {
    use crate::framework::barrier::reset::audit::tests;
    check!(tests::test_audit_log(), "audit log");
    TestResult::Pass
}

fn audit_log_count() -> TestResult {
    use crate::framework::barrier::reset::audit::tests;
    check!(tests::test_audit_count_by_layer(), "audit count by layer");
    TestResult::Pass
}

fn bbr_fingerprint() -> TestResult {
    use crate::framework::barrier::reset::bbr::tests;
    check!(tests::test_compute_fingerprint(), "compute fingerprint");
    TestResult::Pass
}

fn bbr_should_attempt() -> TestResult {
    use crate::framework::barrier::reset::bbr::tests;
    check!(tests::test_should_attempt(), "should attempt");
    TestResult::Pass
}

fn bsr_freeze_unfreeze() -> TestResult {
    use crate::framework::barrier::reset::bsr::tests;
    check!(tests::test_freeze_unfreeze(), "bsr freeze/unfreeze");
    TestResult::Pass
}

fn parallel_dependency_layer() -> TestResult {
    use crate::framework::barrier::reset::parallel::tests;
    check!(tests::test_dependency_layer(), "dependency layer");
    check!(tests::test_dependency_layers(), "dependency layers");
    TestResult::Pass
}

fn parallel_compute_layers() -> TestResult {
    use crate::framework::barrier::reset::parallel::tests;
    check!(tests::test_compute_layers(), "compute layers");
    TestResult::Pass
}

fn device_type_enum() -> TestResult {
    use crate::framework::barrier::DeviceType;

    assert_eq_test!(DeviceType::Keyboard as u32, 1, "keyboard type");
    assert_eq_test!(DeviceType::Serial as u32, 2, "serial type");
    assert_eq_test!(DeviceType::Timer as u32, 3, "timer type");
    assert_eq_test!(DeviceType::Network as u32, 4, "network type");
    assert_eq_test!(DeviceType::Storage as u32, 5, "storage type");

    assert_eq_test!(
        DeviceType::from_u32(1),
        DeviceType::Keyboard,
        "from_u32 keyboard"
    );
    assert_eq_test!(
        DeviceType::from_u32(99),
        DeviceType::Unknown,
        "from_u32 unknown"
    );

    TestResult::Pass
}

fn recovery_layer_order() -> TestResult {
    use crate::framework::barrier::RecoveryLayer;

    assert_eq_test!(RecoveryLayer::Layer1 as u32, 1, "layer1 value");
    assert_eq_test!(RecoveryLayer::Layer2 as u32, 2, "layer2 value");
    assert_eq_test!(RecoveryLayer::Layer3 as u32, 3, "layer3 value");

    TestResult::Pass
}

fn recovery_result_checks() -> TestResult {
    use crate::framework::barrier::RecoveryResult;

    let success = RecoveryResult::Success;
    let failed = RecoveryResult::Failed;
    let escalate = RecoveryResult::Escalate;

    check!(success.is_success(), "success is_success");
    check!(!success.should_escalate(), "success not escalate");

    check!(!failed.is_success(), "failed not success");
    check!(!failed.should_escalate(), "failed not escalate");

    check!(!escalate.is_success(), "escalate not success");
    check!(escalate.should_escalate(), "escalate should_escalate");

    TestResult::Pass
}

fn rollback_mode_enum() -> TestResult {
    use crate::framework::barrier::RollbackMode;

    assert_eq_test!(RollbackMode::Serial as u32, 0, "serial mode");
    assert_eq_test!(RollbackMode::Parallel as u32, 1, "parallel mode");

    TestResult::Pass
}

fn snapshot_register_api() -> TestResult {
    use crate::framework::barrier::{
        DeviceType, snapshot_register_device, snapshot_unregister_device,
    };

    let registered = snapshot_register_device(999, DeviceType::Timer, "test_dev", 0xF000, 10);
    check!(registered, "register device");

    let unregistered = snapshot_unregister_device(999);
    check!(unregistered, "unregister device");

    TestResult::Pass
}

fn recovery_stats_api() -> TestResult {
    use crate::framework::barrier::{get_stats, reset_stats};

    reset_stats();
    let (bsr, bhr, tick) = get_stats();
    assert_eq_test!(bsr, 0, "bsr count");
    assert_eq_test!(bhr, 0, "bhr count");
    assert_eq_test!(tick, 0, "last tick");

    TestResult::Pass
}

fn recovery_status_api() -> TestResult {
    use crate::framework::barrier::{get_recovery_status, reset_stats};

    reset_stats();
    let status = get_recovery_status();
    assert_eq_test!(status.bbr_count, 0, "bbr count");
    assert_eq_test!(status.bsr_count, 0, "bsr count");
    assert_eq_test!(status.bhr_count, 0, "bhr count");

    TestResult::Pass
}

pub fn register_tests() {
    let r = runner();
    register_tests_inner! { r:
        "barrier::config": {
            "recovery_result": config_recovery_result,
            "default": config_default,
            "stats": config_stats,
        },
        "barrier::audit": {
            "basic": audit_log_basic,
            "count": audit_log_count,
        },
        "barrier::bbr": {
            "fingerprint": bbr_fingerprint,
            "should_attempt": bbr_should_attempt,
        },
        "barrier::bsr": {
            "freeze_unfreeze": bsr_freeze_unfreeze,
        },
        "barrier::parallel": {
            "dependency_layer": parallel_dependency_layer,
            "compute_layers": parallel_compute_layers,
        },
        "barrier::snapshot": {
            "device_type": device_type_enum,
            "register_api": snapshot_register_api,
        },
        "barrier::reset": {
            "layer_order": recovery_layer_order,
            "result_checks": recovery_result_checks,
            "rollback_mode": rollback_mode_enum,
            "stats_api": recovery_stats_api,
            "status_api": recovery_status_api,
        },
    }
}
