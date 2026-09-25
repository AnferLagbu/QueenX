// UT-07 (2026-09-25): sync::{seqlock,spinlock,types,atomic} 四组注册副本已删 —
// 其纯逻辑断言以源文件 #[cfg(test)] 为唯一归属 (见
// docs/plan/kernel-unit-test-harness-unification.md 的 UT-07 A 类清单).
use crate::framework::sync::{CondVar, Mutex, RwLock};
use crate::framework::tests::{TestResult, assert_eq_test, check, runner};
use crate::register_tests_inner;

fn mutex_basic() -> TestResult {
    let mutex = Mutex::new(42i32);
    check!(!mutex.is_locked(), "should not be locked initially");
    {
        let guard = mutex.lock();
        check!(mutex.is_locked(), "should be locked after lock");
        assert_eq_test!(*guard, 42, "value mismatch");
    }
    check!(!mutex.is_locked(), "should be unlocked after drop");
    TestResult::Pass
}

fn mutex_trylock() -> TestResult {
    let mutex = Mutex::new(100u32);
    let guard1 = mutex.try_lock().expect("first try should succeed");
    check!(mutex.is_locked(), "should be locked");
    assert_eq_test!(*guard1, 100, "value mismatch");
    check!(mutex.try_lock().is_none(), "second trylock should fail");
    drop(guard1);
    check!(!mutex.is_locked(), "should be unlocked after drop");
    TestResult::Pass
}

// G-17 (2026-09-08) 回归: Mutex 递归锁定 — 同一线程对已持有 Mutex 再次 lock.
// 修复前 raw_lock 无 owner 重入检测, 二次 lock 在 slow path 死等自死锁
// (G-11 kill 广播 / G-12 signalfd 受害). 修复后 depth 递增, 逐层 drop 释放.
fn mutex_reentrant() -> TestResult {
    let mutex = Mutex::new(42i32);
    let guard1 = mutex.lock();
    let guard2 = mutex.lock();
    check!(mutex.is_locked(), "reentrant lock keeps locked");
    check!(mutex.depth() == 2, "reentrant depth = 2");
    assert_eq_test!(*guard2, 42, "data accessible via inner guard");
    drop(guard2);
    check!(mutex.is_locked(), "still locked after first drop");
    check!(mutex.depth() == 1, "depth back to 1");
    drop(guard1);
    check!(!mutex.is_locked(), "unlocked after outer drop");
    check!(mutex.owner() == -1, "owner cleared");
    TestResult::Pass
}

fn condvar_creation() -> TestResult {
    let _cond = CondVar::new();
    TestResult::Pass
}

fn rwlock_basic_read() -> TestResult {
    let rwlock = RwLock::new(42i32);
    {
        let reader = rwlock.read();
        assert_eq_test!(*reader, 42, "read value mismatch");
        assert_eq_test!(rwlock.reader_count(), 1, "reader count");
        check!(!rwlock.has_writer(), "should not have writer");
    }
    assert_eq_test!(rwlock.reader_count(), 0, "reader count after drop");
    TestResult::Pass
}

fn rwlock_basic_write() -> TestResult {
    let rwlock = RwLock::new(100u32);
    {
        let writer = rwlock.write();
        assert_eq_test!(*writer, 100, "write value mismatch");
        check!(rwlock.has_writer(), "should have writer");
        assert_eq_test!(rwlock.reader_count(), 0, "no readers during write");
    }
    check!(!rwlock.has_writer(), "no writer after drop");
    TestResult::Pass
}

fn rwlock_try_operations() -> TestResult {
    let rwlock = RwLock::new(0u32);
    check!(rwlock.try_read().is_some(), "try_read should succeed");
    check!(rwlock.try_write().is_some(), "try_write should succeed");
    TestResult::Pass
}

fn rwlock_multiple_readers() -> TestResult {
    let rwlock = RwLock::new(0i32);
    let r1 = rwlock.try_read();
    check!(r1.is_some(), "first read");
    let r2 = rwlock.try_read();
    check!(r2.is_some(), "second read");
    assert_eq_test!(rwlock.reader_count(), 2, "two readers");
    drop(r1);
    drop(r2);
    assert_eq_test!(rwlock.reader_count(), 0, "zero after drop");
    TestResult::Pass
}

fn rwlock_write_blocks_read() -> TestResult {
    let rwlock = RwLock::new(0i32);
    let writer = rwlock.try_write();
    check!(writer.is_some(), "write acquired");
    let reader = rwlock.try_read();
    check!(reader.is_none(), "read blocked by writer");
    drop(writer);
    let reader2 = rwlock.try_read();
    check!(reader2.is_some(), "read after write release");
    TestResult::Pass
}

fn rwlock_read_blocks_write() -> TestResult {
    let rwlock = RwLock::new(0i32);
    let reader = rwlock.try_read();
    check!(reader.is_some(), "read acquired");
    let writer = rwlock.try_write();
    check!(writer.is_none(), "write blocked by reader");
    drop(reader);
    let writer2 = rwlock.try_write();
    check!(writer2.is_some(), "write after read release");
    TestResult::Pass
}

pub fn register_mutex_tests() {
    let r = runner();
    register_tests_inner! { r:
        "sync::mutex": {
            "basic": mutex_basic,
            "trylock": mutex_trylock,
            // G-17 (2026-09-08): 递归锁定回归
            "reentrant": mutex_reentrant,
            "condvar_creation": condvar_creation,
        },
    }
}

pub fn register_rwlock_tests() {
    let r = runner();
    register_tests_inner! { r:
        "sync::rwlock": {
            "basic_read": rwlock_basic_read,
            "basic_write": rwlock_basic_write,
            "try_operations": rwlock_try_operations,
            "multiple_readers": rwlock_multiple_readers,
            "write_blocks_read": rwlock_write_blocks_read,
            "read_blocks_write": rwlock_read_blocks_write,
        },
    }
}

pub fn register_tests() {
    register_mutex_tests();
    register_rwlock_tests();
}
