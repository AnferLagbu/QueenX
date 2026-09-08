use crate::kernel::framework::sync::{
    AtomicBool, CondVar, IrqSaveFlags, Mutex, MutexInner, RwLock, RwLockInner, SeqLock, SpinLock,
    SpinLockInner, TryLockResult, atomic_add, atomic_cmpxchg, atomic_dec, atomic_inc, atomic_read,
    atomic_set, atomic_sub,
};
use crate::kernel::framework::tests::{TestResult, assert_eq_test, check, runner};
use crate::register_tests_inner;
use core::sync::atomic::Ordering;

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

fn seqlock_basic() -> TestResult {
    let lock = SeqLock::new(42u32);
    {
        let guard = lock.read();
        check!(guard.is_valid(), "read should be valid");
        assert_eq_test!(*guard, 42, "value mismatch");
    }
    TestResult::Pass
}

fn seqlock_write() -> TestResult {
    let lock = SeqLock::new(0u32);
    {
        let mut w = lock.write();
        *w = 100;
    }
    let r = lock.read();
    check!(r.is_valid(), "read should be valid after write");
    assert_eq_test!(*r, 100, "value after write");
    TestResult::Pass
}

fn seqlock_sequence_increments() -> TestResult {
    let lock = SeqLock::new(0u32);
    assert_eq_test!(lock.sequence.load(Ordering::Relaxed), 0, "initial seq");
    {
        let _w = lock.write();
        assert_eq_test!(lock.sequence.load(Ordering::Relaxed), 1, "seq during write");
    }
    assert_eq_test!(lock.sequence.load(Ordering::Relaxed), 2, "seq after write");
    TestResult::Pass
}

fn spinlock_basic() -> TestResult {
    let mut lock = SpinLock::new();
    check!(!lock.is_locked(), "should not be locked");
    lock.raw_lock();
    check!(lock.is_locked(), "should be locked");
    lock.raw_unlock();
    check!(!lock.is_locked(), "should be unlocked");
    TestResult::Pass
}

fn spinlock_trylock() -> TestResult {
    let mut lock = SpinLock::new();
    assert_eq_test!(lock.try_lock(), TryLockResult::Acquired, "first trylock");
    check!(lock.is_locked(), "should be locked");
    assert_eq_test!(lock.try_lock(), TryLockResult::WouldBlock, "second trylock");
    lock.raw_unlock();
    TestResult::Pass
}

fn spinlock_irqsave() -> TestResult {
    let mut lock = SpinLock::new();
    let flags = lock.lock_irqsave();
    check!(lock.is_locked(), "should be locked after irqsave");
    lock.unlock_irqrestore(&flags);
    check!(!lock.is_locked(), "should be unlocked after irqrestore");
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

fn spinlock_inner_default() -> TestResult {
    let lock = SpinLockInner::default();
    assert_eq_test!(
        lock.locked.load(Ordering::Relaxed),
        0,
        "spinlock inner default"
    );
    TestResult::Pass
}

fn mutex_inner_default() -> TestResult {
    let m = MutexInner::default();
    assert_eq_test!(m.locked.load(Ordering::Relaxed), 0, "mutex inner locked");
    assert_eq_test!(m.owner.load(Ordering::Relaxed), -1, "mutex inner owner");
    assert_eq_test!(m.depth.load(Ordering::Relaxed), 0, "mutex inner depth");
    TestResult::Pass
}

fn rwlock_inner_default() -> TestResult {
    let rw = RwLockInner::default();
    assert_eq_test!(
        rw.readers.load(Ordering::Relaxed),
        0,
        "rwlock inner readers"
    );
    assert_eq_test!(rw.writer.load(Ordering::Relaxed), 0, "rwlock inner writer");
    assert_eq_test!(
        rw.pending_writers.load(Ordering::Relaxed),
        0,
        "rwlock inner pending"
    );
    TestResult::Pass
}

fn irq_save_flags() -> TestResult {
    let flags = IrqSaveFlags(0x202);
    check!(flags.interrupts_enabled(), "IF=1 should be enabled");
    let flags_disabled = IrqSaveFlags(0x002);
    check!(
        !flags_disabled.interrupts_enabled(),
        "IF=0 should be disabled"
    );
    TestResult::Pass
}

fn try_lock_result_variants() -> TestResult {
    let acquired = TryLockResult::Acquired;
    let would_block = TryLockResult::WouldBlock;
    assert_eq_test!(acquired, TryLockResult::Acquired, "acquired eq");
    check!(acquired != would_block, "acquired != would_block");
    TestResult::Pass
}

fn atomic_bool_basic() -> TestResult {
    let b = AtomicBool::new(false);
    check!(!b.load(Ordering::Relaxed), "initial false");
    b.swap(true, Ordering::Relaxed);
    check!(b.load(Ordering::Relaxed), "after swap true");
    check!(
        b.compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst),
        "cas success"
    );
    check!(!b.load(Ordering::Relaxed), "after cas false");
    check!(
        !b.compare_exchange(true, true, Ordering::SeqCst, Ordering::SeqCst),
        "cas fail"
    );
    TestResult::Pass
}

fn atomic_operations() -> TestResult {
    let mut val: i32 = 10;
    let ptr = &raw mut val;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        assert_eq_test!(atomic_inc(ptr), 10, "inc returns old");
        assert_eq_test!(*ptr, 11, "inc result");
        assert_eq_test!(atomic_dec(ptr), 11, "dec returns old");
        assert_eq_test!(*ptr, 10, "dec result");
        assert_eq_test!(atomic_add(ptr, 5), 10, "add returns old");
        assert_eq_test!(*ptr, 15, "add result");
        assert_eq_test!(atomic_sub(ptr, 3), 15, "sub returns old");
        assert_eq_test!(*ptr, 12, "sub result");
        atomic_set(ptr, 42);
        assert_eq_test!(*ptr, 42, "set result");
        assert_eq_test!(atomic_read(ptr), 42, "read result");
        check!(atomic_cmpxchg(ptr, 42, 100), "cmpxchg success");
        assert_eq_test!(*ptr, 100, "cmpxchg result");
        check!(!atomic_cmpxchg(ptr, 99, 200), "cmpxchg fail");
        assert_eq_test!(*ptr, 100, "cmpxchg fail no change");
    }
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

pub fn register_seqlock_tests() {
    let r = runner();
    register_tests_inner! { r:
        "sync::seqlock": {
            "basic": seqlock_basic,
            "write": seqlock_write,
            "sequence_increments": seqlock_sequence_increments,
        },
    }
}

pub fn register_spinlock_tests() {
    let r = runner();
    register_tests_inner! { r:
        "sync::spinlock": {
            "basic": spinlock_basic,
            "trylock": spinlock_trylock,
            "irqsave": spinlock_irqsave,
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

pub fn register_sync_types_tests() {
    let r = runner();
    register_tests_inner! { r:
        "sync::types": {
            "spinlock_inner_default": spinlock_inner_default,
            "mutex_inner_default": mutex_inner_default,
            "rwlock_inner_default": rwlock_inner_default,
            "irq_save_flags": irq_save_flags,
            "try_lock_result_variants": try_lock_result_variants,
        },
    }
}

pub fn register_atomic_tests() {
    let r = runner();
    register_tests_inner! { r:
        "sync::atomic": {
            "bool_basic": atomic_bool_basic,
            "operations": atomic_operations,
        },
    }
}

pub fn register_tests() {
    register_mutex_tests();
    register_seqlock_tests();
    register_spinlock_tests();
    register_rwlock_tests();
    register_sync_types_tests();
    register_atomic_tests();
}
