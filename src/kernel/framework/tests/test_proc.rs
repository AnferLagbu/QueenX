use super::assert_eq_test;
use super::check;
use crate::kernel::framework::proc::types::{ProcessId, ProcessState, ThreadId};
use crate::kernel::framework::tests::{TestResult, runner};
use crate::register_tests_inner;

fn test_process_state_from_u8() -> TestResult {
    assert_eq_test!(
        ProcessState::from_u8(0),
        ProcessState::Created,
        "state 0 should be Created"
    );
    assert_eq_test!(
        ProcessState::from_u8(1),
        ProcessState::Ready,
        "state 1 should be Ready"
    );
    assert_eq_test!(
        ProcessState::from_u8(2),
        ProcessState::Running,
        "state 2 should be Running"
    );
    assert_eq_test!(
        ProcessState::from_u8(3),
        ProcessState::Blocked,
        "state 3 should be Blocked"
    );
    assert_eq_test!(
        ProcessState::from_u8(4),
        ProcessState::Zombie,
        "state 4 should be Zombie"
    );
    assert_eq_test!(
        ProcessState::from_u8(5),
        ProcessState::Terminated,
        "state 5 should be Terminated"
    );
    assert_eq_test!(
        ProcessState::from_u8(6),
        ProcessState::Frozen,
        "state 6 should be Frozen"
    );
    assert_eq_test!(
        ProcessState::from_u8(255),
        ProcessState::Created,
        "invalid state should fallback to Created"
    );
    TestResult::Pass
}

fn test_process_state_from_u32() -> TestResult {
    assert_eq_test!(
        ProcessState::from_u32(0),
        ProcessState::Created,
        "u32 0 should be Created"
    );
    assert_eq_test!(
        ProcessState::from_u32(2),
        ProcessState::Running,
        "u32 2 should be Running"
    );
    assert_eq_test!(
        ProcessState::from_u32(999),
        ProcessState::Created,
        "invalid u32 should fallback"
    );
    TestResult::Pass
}

fn test_process_state_name() -> TestResult {
    check!(
        ProcessState::Created.name() == "Created",
        "Created name mismatch"
    );
    check!(ProcessState::Ready.name() == "Ready", "Ready name mismatch");
    check!(
        ProcessState::Running.name() == "Running",
        "Running name mismatch"
    );
    check!(
        ProcessState::Blocked.name() == "Blocked",
        "Blocked name mismatch"
    );
    check!(
        ProcessState::Zombie.name() == "Zombie",
        "Zombie name mismatch"
    );
    check!(
        ProcessState::Terminated.name() == "Terminated",
        "Terminated name mismatch"
    );
    check!(
        ProcessState::Frozen.name() == "Frozen",
        "Frozen name mismatch"
    );
    TestResult::Pass
}

// 2026-09-11 实测: Ready==Ready 触发 eq_op, 需豁免
#[allow(clippy::eq_op)]
fn test_process_state_equality() -> TestResult {
    check!(
        ProcessState::Ready == ProcessState::Ready,
        "same states should be equal"
    );
    check!(
        ProcessState::Ready != ProcessState::Running,
        "different states should not be equal"
    );
    TestResult::Pass
}

fn test_process_id() -> TestResult {
    let pid1 = ProcessId(1);
    let pid2 = ProcessId(2);
    let pid1_copy = ProcessId(1);
    check!(pid1 == pid1_copy, "same PIDs should be equal");
    check!(pid1 != pid2, "different PIDs should not be equal");
    check!(pid1.0 == 1, "PID value should be 1");
    TestResult::Pass
}

fn test_thread_id() -> TestResult {
    let tid1 = ThreadId(1);
    let tid2 = ThreadId(100);
    check!(tid1.0 == 1, "TID value should be 1");
    check!(tid2.0 == 100, "TID value should be 100");
    TestResult::Pass
}

fn test_process_state_lifecycle() -> TestResult {
    let states = [
        ProcessState::Created,
        ProcessState::Ready,
        ProcessState::Running,
        ProcessState::Blocked,
        ProcessState::Zombie,
        ProcessState::Terminated,
        ProcessState::Frozen,
    ];
    for (i, &state) in states.iter().enumerate() {
        assert_eq_test!(state as u8, i as u8, "state discriminant mismatch");
    }
    TestResult::Pass
}

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
fn test_stack_canary() -> TestResult {
    use crate::kernel::framework::proc::KERNEL_STACK_CANARY;
    check!(
        KERNEL_STACK_CANARY == 0xDEADBEEF_CAFEBABE,
        "canary value mismatch"
    );
    TestResult::Pass
}

pub fn register_proc_tests() {
    let r = runner();
    register_tests_inner! { r:
        "Proc": {
            "state_from_u8": test_process_state_from_u8,
            "state_from_u32": test_process_state_from_u32,
            "state_name": test_process_state_name,
            "state_equality": test_process_state_equality,
            "process_id": test_process_id,
            "thread_id": test_thread_id,
            "state_lifecycle": test_process_state_lifecycle,
            "stack_canary": test_stack_canary,
        },
    }
}
