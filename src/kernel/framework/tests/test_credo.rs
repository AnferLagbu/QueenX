//! E-06 (2026-09-07): `services::credo::policy` 能力矩阵规范实现用例.
//!
//! 本模块由 host-tests/src/capability.rs 去重载体迁入: B08-12 后该载体仅剩
//! 回归测试 (本地 CapBits/CapabilityMatrix 复刻已删), E-04/E-05 同源双编译后
//! 纯逻辑被测对象的用例应统一为"双端共享" (kernel_test QEMU + host-test host),
//! 故迁移至 framework/tests 套件注册, 删除 host-tests 侧重复载体.
//!
//! 被测对象: `services::credo::policy` — CapBits 位运算 / CapMatrix 快照 /
//! InMemoryMatrix 可变矩阵 / CapabilityMatrix trait / VIABLE_FLOOR / 域边界.
//! 注意: 此处的 `CapBits`/`CapDomain` 是 policy 模块自有类型 (services/credo/
//! policy.rs), 与 `services::credo::types` (framework::credo::types re-export,
//! test_pwm.rs 测) 是不同对象.

use crate::kernel::framework::tests::{TestResult, assert_eq_test, check, runner};
use crate::kernel::services::credo::capability::{
    FS_CAP_CHOWN, FS_CAP_DELETE, FS_CAP_EXECUTE, FS_CAP_READ, FS_CAP_WRITE, PROC_CAP_EXEC,
    PROC_CAP_FORK, PROC_CAP_KILL, SYS_CAP_ALL,
};
use crate::kernel::services::credo::policy::{
    CapBits, CapDomain, CapMatrix, CapabilityMatrix, InMemoryMatrix, VIABLE_FLOOR,
};
use crate::register_tests_inner;

fn cap_bits_has() -> TestResult {
    let cb = CapBits(FS_CAP_READ | FS_CAP_WRITE);
    check!(cb.contains(CapBits(FS_CAP_READ)), "contains READ");
    check!(cb.contains(CapBits(FS_CAP_WRITE)), "contains WRITE");
    check!(!cb.contains(CapBits(FS_CAP_EXECUTE)), "not EXEC");
    TestResult::Pass
}

fn cap_bits_grant() -> TestResult {
    let cb = CapBits(FS_CAP_READ) | CapBits(FS_CAP_WRITE);
    check!(cb.contains(CapBits(FS_CAP_READ)), "contains READ");
    check!(cb.contains(CapBits(FS_CAP_WRITE)), "contains WRITE");
    TestResult::Pass
}

fn cap_bits_revoke() -> TestResult {
    let cb = CapBits(FS_CAP_READ | FS_CAP_WRITE).diff(CapBits(FS_CAP_READ));
    check!(!cb.contains(CapBits(FS_CAP_READ)), "READ revoked");
    check!(cb.contains(CapBits(FS_CAP_WRITE)), "WRITE kept");
    TestResult::Pass
}

fn cap_bits_superset() -> TestResult {
    let full = CapBits(FS_CAP_READ | FS_CAP_WRITE | FS_CAP_EXECUTE);
    let partial = CapBits(FS_CAP_READ);
    check!(full.contains(partial), "full contains partial");
    check!(!partial.contains(full), "partial not contains full");
    TestResult::Pass
}

fn cap_matrix_new_empty() -> TestResult {
    let cm = InMemoryMatrix::new();
    assert_eq_test!(cm.get(CapDomain::FS), Some(CapBits::NONE), "FS NONE");
    assert_eq_test!(cm.get(CapDomain::PROC), Some(CapBits::NONE), "PROC NONE");
    TestResult::Pass
}

fn cap_matrix_grant_revoke() -> TestResult {
    let cm = InMemoryMatrix::new();
    cm.set(CapDomain::FS, CapBits(FS_CAP_READ | FS_CAP_WRITE)).unwrap();
    check!(
        cm.get(CapDomain::FS).unwrap().contains(CapBits(FS_CAP_READ)),
        "FS has READ"
    );
    check!(
        cm.get(CapDomain::FS).unwrap().contains(CapBits(FS_CAP_WRITE)),
        "FS has WRITE"
    );
    cm.set(CapDomain::FS, CapBits(FS_CAP_READ)).unwrap();
    check!(
        cm.get(CapDomain::FS).unwrap().contains(CapBits(FS_CAP_READ)),
        "FS still has READ"
    );
    check!(
        !cm.get(CapDomain::FS).unwrap().contains(CapBits(FS_CAP_WRITE)),
        "FS lost WRITE"
    );
    TestResult::Pass
}

fn cap_matrix_all() -> TestResult {
    let cm = CapMatrix::all();
    for d in 0..16u8 {
        assert_eq_test!(cm.get(CapDomain(d)), CapBits::ALL, "all domains ALL");
    }
    TestResult::Pass
}

fn cap_matrix_viable() -> TestResult {
    let cm = CapMatrix::from_bits(VIABLE_FLOOR);
    check!(
        cm.get(CapDomain::FS).contains(CapBits(FS_CAP_READ)),
        "FS viable has READ"
    );
    check!(
        cm.get(CapDomain::FS).contains(CapBits(FS_CAP_EXECUTE)),
        "FS viable has EXEC"
    );
    check!(
        !cm.get(CapDomain::FS).contains(CapBits(FS_CAP_WRITE)),
        "FS viable lacks WRITE"
    );
    check!(
        cm.get(CapDomain::PROC).contains(CapBits(PROC_CAP_FORK)),
        "PROC viable has FORK"
    );
    check!(
        cm.get(CapDomain::PROC).contains(CapBits(PROC_CAP_EXEC)),
        "PROC viable has EXEC"
    );
    TestResult::Pass
}

fn cap_matrix_superset() -> TestResult {
    let parent = CapMatrix::all();
    let child = CapMatrix::from_bits(VIABLE_FLOOR);
    for d in 0..16u8 {
        check!(
            parent.get(CapDomain(d)).contains(child.get(CapDomain(d))),
            "parent contains child"
        );
        check!(
            !child.get(CapDomain(d)).contains(parent.get(CapDomain(d))),
            "child not contains parent"
        );
    }
    TestResult::Pass
}

fn cap_matrix_out_of_range() -> TestResult {
    let cm = InMemoryMatrix::new();
    check!(!CapDomain(16).is_valid(), "16 invalid");
    check!(!CapDomain(255).is_valid(), "255 invalid");
    assert_eq_test!(cm.get(CapDomain(16)), None, "get 16 None");
    assert_eq_test!(cm.get(CapDomain(255)), None, "get 255 None");
    TestResult::Pass
}

fn cap_bits_empty_has_nothing() -> TestResult {
    let cb = CapBits::NONE;
    check!(!cb.contains(CapBits(FS_CAP_READ)), "NONE lacks READ");
    check!(!cb.contains(CapBits(FS_CAP_WRITE)), "NONE lacks WRITE");
    check!(!cb.contains(CapBits(SYS_CAP_ALL)), "NONE lacks SYS_ALL");
    TestResult::Pass
}

fn cap_bits_grant_all_then_revoke_one() -> TestResult {
    let cb = CapBits::ALL;
    check!(cb.contains(CapBits(FS_CAP_READ)), "ALL has READ");
    check!(cb.contains(CapBits(SYS_CAP_ALL)), "ALL has SYS_ALL");
    let cb = cb.diff(CapBits(FS_CAP_READ));
    check!(!cb.contains(CapBits(FS_CAP_READ)), "READ revoked");
    check!(cb.contains(CapBits(FS_CAP_WRITE)), "WRITE kept");
    TestResult::Pass
}

fn cap_bits_revoke_nonexistent_is_noop() -> TestResult {
    let cb = CapBits(FS_CAP_READ).diff(CapBits(FS_CAP_WRITE));
    check!(cb.contains(CapBits(FS_CAP_READ)), "READ kept");
    check!(!cb.contains(CapBits(FS_CAP_WRITE)), "WRITE absent");
    TestResult::Pass
}

fn cap_bits_grant_idempotent() -> TestResult {
    let cb = CapBits(FS_CAP_READ) | CapBits(FS_CAP_READ);
    check!(cb.contains(CapBits(FS_CAP_READ)), "contains READ");
    assert_eq_test!(cb, CapBits(FS_CAP_READ), "idempotent value");
    TestResult::Pass
}

fn cap_matrix_delegation_chain() -> TestResult {
    let root = CapMatrix::all();
    let admin = InMemoryMatrix::new();
    admin
        .set(
            CapDomain::FS,
            CapBits(FS_CAP_READ | FS_CAP_WRITE | FS_CAP_EXECUTE | (1 << 3) | (1 << 4)),
        )
        .unwrap();
    admin
        .set(
            CapDomain::PROC,
            CapBits(PROC_CAP_FORK | PROC_CAP_EXEC | PROC_CAP_KILL),
        )
        .unwrap();
    let user = InMemoryMatrix::new();
    user.set(CapDomain::FS, CapBits(FS_CAP_READ | FS_CAP_EXECUTE))
        .unwrap();
    user.set(CapDomain::PROC, CapBits(PROC_CAP_FORK | PROC_CAP_EXEC))
        .unwrap();
    check!(
        root.get(CapDomain::FS).contains(admin.get(CapDomain::FS).unwrap()),
        "root contains admin"
    );
    check!(
        admin.get(CapDomain::FS).unwrap().contains(user.get(CapDomain::FS).unwrap()),
        "admin contains user"
    );
    check!(
        !user.get(CapDomain::FS).unwrap().contains(admin.get(CapDomain::FS).unwrap()),
        "user not contains admin"
    );
    TestResult::Pass
}

fn cap_matrix_revocation_partial() -> TestResult {
    let all = CapMatrix::all();
    let fs_bits = all.get(CapDomain::FS).diff(CapBits(FS_CAP_DELETE | FS_CAP_CHOWN));
    check!(fs_bits.contains(CapBits(FS_CAP_READ)), "has READ");
    check!(fs_bits.contains(CapBits(FS_CAP_WRITE)), "has WRITE");
    check!(!fs_bits.contains(CapBits(FS_CAP_DELETE)), "lacks DELETE");
    check!(!fs_bits.contains(CapBits(FS_CAP_CHOWN)), "lacks CHOWN");
    TestResult::Pass
}

fn cap_matrix_viable_is_not_all() -> TestResult {
    let viable = CapMatrix::from_bits(VIABLE_FLOOR);
    let all = CapMatrix::all();
    check!(
        all.get(CapDomain::FS).contains(viable.get(CapDomain::FS)),
        "all contains viable"
    );
    check!(
        !viable.get(CapDomain::FS).contains(all.get(CapDomain::FS)),
        "viable not contains all"
    );
    TestResult::Pass
}

fn cap_matrix_grant_out_of_range_silent() -> TestResult {
    let cm = InMemoryMatrix::new();
    check!(cm.set(CapDomain(16), CapBits(0xFF)).is_err(), "16 set err");
    check!(cm.set(CapDomain(255), CapBits(0xFF)).is_err(), "255 set err");
    assert_eq_test!(cm.get(CapDomain(16)), None, "16 still None");
    assert_eq_test!(cm.get(CapDomain(255)), None, "255 still None");
    TestResult::Pass
}

fn cap_matrix_revoke_out_of_range_silent() -> TestResult {
    // 内核 set 对非法域返回 Err, 矩阵内容不变 (等价原 revoke 静默失败)
    let cm = InMemoryMatrix::new();
    check!(cm.set(CapDomain(16), CapBits::ALL).is_err(), "16 set err");
    check!(cm.set(CapDomain(255), CapBits::ALL).is_err(), "255 set err");
    for d in 0..16u8 {
        assert_eq_test!(cm.get(CapDomain(d)), Some(CapBits::NONE), "all NONE");
    }
    TestResult::Pass
}

fn cap_matrix_cross_domain_isolation() -> TestResult {
    let cm = InMemoryMatrix::new();
    cm.set(CapDomain::FS, CapBits(FS_CAP_READ)).unwrap();
    assert_eq_test!(cm.get(CapDomain::PROC), Some(CapBits::NONE), "PROC NONE");
    assert_eq_test!(cm.get(CapDomain::NET), Some(CapBits::NONE), "NET NONE");
    TestResult::Pass
}

fn cap_matrix_empty_not_superset_of_viable() -> TestResult {
    let empty = CapMatrix::empty();
    let viable = CapMatrix::from_bits(VIABLE_FLOOR);
    check!(
        !empty.get(CapDomain::FS).contains(viable.get(CapDomain::FS)),
        "empty not contains viable"
    );
    TestResult::Pass
}

fn cap_bits_superset_reflexive() -> TestResult {
    let cb = CapBits(FS_CAP_READ | FS_CAP_WRITE);
    check!(cb.contains(cb), "reflexive");
    TestResult::Pass
}

fn cap_matrix_superset_reflexive() -> TestResult {
    let cm = CapMatrix::from_bits(VIABLE_FLOOR);
    check!(
        cm.get(CapDomain::FS).contains(cm.get(CapDomain::FS)),
        "reflexive"
    );
    TestResult::Pass
}

pub fn register_credo_tests() {
    let r = runner();
    register_tests_inner! { r:
        "pwm::policy": {
            "cap_bits_has": cap_bits_has,
            "cap_bits_grant": cap_bits_grant,
            "cap_bits_revoke": cap_bits_revoke,
            "cap_bits_superset": cap_bits_superset,
            "cap_matrix_new_empty": cap_matrix_new_empty,
            "cap_matrix_grant_revoke": cap_matrix_grant_revoke,
            "cap_matrix_all": cap_matrix_all,
            "cap_matrix_viable": cap_matrix_viable,
            "cap_matrix_superset": cap_matrix_superset,
            "cap_matrix_out_of_range": cap_matrix_out_of_range,
            "cap_bits_empty_has_nothing": cap_bits_empty_has_nothing,
            "cap_bits_grant_all_then_revoke_one": cap_bits_grant_all_then_revoke_one,
            "cap_bits_revoke_nonexistent_is_noop": cap_bits_revoke_nonexistent_is_noop,
            "cap_bits_grant_idempotent": cap_bits_grant_idempotent,
            "cap_matrix_delegation_chain": cap_matrix_delegation_chain,
            "cap_matrix_revocation_partial": cap_matrix_revocation_partial,
            "cap_matrix_viable_is_not_all": cap_matrix_viable_is_not_all,
            "cap_matrix_grant_out_of_range_silent": cap_matrix_grant_out_of_range_silent,
            "cap_matrix_revoke_out_of_range_silent": cap_matrix_revoke_out_of_range_silent,
            "cap_matrix_cross_domain_isolation": cap_matrix_cross_domain_isolation,
            "cap_matrix_empty_not_superset_of_viable": cap_matrix_empty_not_superset_of_viable,
            "cap_bits_superset_reflexive": cap_bits_superset_reflexive,
            "cap_matrix_superset_reflexive": cap_matrix_superset_reflexive,
        },
    }
}
