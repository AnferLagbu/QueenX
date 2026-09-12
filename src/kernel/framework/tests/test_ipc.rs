use super::check;
use crate::kernel::framework::ipc::types::{
    IPC_MAX_MSG_QUEUES, IPC_MAX_PIPES, IPC_MAX_SEMAPHORES, IPC_MAX_SHM_SEGS, IpcId, IpcNamespace,
    MsgQueue, Pipe, Semaphore, ShmSegment,
};
// T6-1: pipe/shm 策略函数已迁移到 services; DECISION-J: sem 壳已删, 亦走 services
use crate::kernel::framework::tests::{TestResult, runner};
use crate::kernel::services::ipc::{pipe, sem, shm};
use crate::register_tests_inner;

// J-01 (2026-09-08): large_stack_arrays expect 仅在裸机 (非测试模式) 生效 —
// IPC_MAX_* 在 any(kernel_test, host-test) 下缩减至 2 (services/ipc/types.rs),
// 数组随之变小不触发 lint; 裸机大值数组 (64×Pipe 等) 才需要 expect.
#[cfg_attr(
    not(any(feature = "kernel_test", feature = "host-test")),
    expect(
        clippy::large_stack_arrays,
        reason = "large_stack_arrays: 大栈数组是性能权衡 (避免堆分配); 当前优先 expect"
    )
)]
fn create_test_namespace() -> IpcNamespace {
    IpcNamespace {
        pipes: [const { Pipe::new() }; IPC_MAX_PIPES],
        shm_segs: [const { ShmSegment::new() }; IPC_MAX_SHM_SEGS],
        msg_queues: [const { MsgQueue::new() }; IPC_MAX_MSG_QUEUES],
        semaphores: [const { Semaphore::new() }; IPC_MAX_SEMAPHORES],
    }
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
fn test_pipe_basic() -> TestResult {
    let mut ns = create_test_namespace();
    let mut next_id: IpcId = 1;
    let pid: u32 = 500;

    let (rfd, wfd) = match pipe::pipe_create_safe(&mut ns, &mut next_id, pid) {
        Ok(pair) => pair,
        Err(_) => return TestResult::Fail("pipe_create failed"),
    };

    let data = [0x41u8; 4];
    if pipe::pipe_write_safe(&mut ns, wfd, &data, data.len() as u32).is_err() {
        return TestResult::Fail("pipe_write failed");
    }

    let mut buf = [0u8; 64];
    if pipe::pipe_read_safe(&mut ns, rfd, &mut buf, 4).is_err() {
        return TestResult::Fail("pipe_read failed");
    }

    check!(buf[0] == 0x41, "read data matches");

    let _ = pipe::pipe_close_safe(&mut ns, rfd);
    let _ = pipe::pipe_close_safe(&mut ns, wfd);
    TestResult::Pass
}

// E-04 (2026-09-06): 测试运行器双端适配 — host-test 下 shm 创建依赖裸机 PMM
// (shm_create_safe → pmm_alloc_pages, extern "C" FFI 内 panic 为 abort, catch_unwind
// 无法捕获 → 进程会 SIGABRT), host 无 PMM 初始化 → 直接 Skip 并说明原因.
// kernel_test (QEMU) 下走下方原实现, 行为与改造前完全一致.
#[cfg(feature = "host-test")]
fn test_shm_rapid_attach_detach() -> TestResult {
    TestResult::Skip("E-04: host 无 PMM 初始化, 跳过 (依赖裸机物理内存分配)")
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
#[cfg(not(feature = "host-test"))]
fn test_shm_rapid_attach_detach() -> TestResult {
    crate::klog_ffi!(klog_ffi_info, "[SHM] creating ns");
    let mut ns = create_test_namespace();
    crate::klog_ffi!(klog_ffi_info, "[SHM] ns ok");
    let mut next_id: IpcId = 1;
    let pid: u32 = 700;

    crate::klog_ffi!(klog_ffi_info, "[SHM] create");
    let id = match shm::shm_create_safe(&mut ns, &mut next_id, 4096, 0o666, pid) {
        Ok(id) => id,
        Err(-1) => return TestResult::Fail("shm_create: invalid size"),
        Err(-2) => return TestResult::Fail("shm_create: no free slot"),
        Err(-3) => {
            return TestResult::Skip("shm_create: pmm alloc failed (OOM in test env)");
        }
        Err(_) => return TestResult::Fail("shm_create: unknown error"),
    };
    crate::klog_ffi!(klog_ffi_info, "[SHM] create ok");

    for _i in 0..100 {
        let addr = match shm::shm_attach_safe(&mut ns, id, pid) {
            Ok(a) => a,
            Err(_) => return TestResult::Fail("shm_attach failed"),
        };
        check!(addr != 0, "attach returned null");
        if shm::shm_detach_safe(&mut ns, id, pid).is_err() {
            return TestResult::Fail("shm_detach failed");
        }
    }
    crate::klog_ffi!(klog_ffi_info, "[SHM] loop ok");

    let _ = shm::shm_destroy_safe(&mut ns, id);
    crate::klog_ffi!(klog_ffi_info, "[SHM] destroy ok");
    TestResult::Pass
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
fn test_semaphore_high_concurrency() -> TestResult {
    let mut ns = create_test_namespace();
    let mut next_id: IpcId = 1;
    let pid: u32 = 800;

    let id = match sem::sem_create_safe(&mut ns, &mut next_id, 10, 20, pid) {
        Ok(id) => id,
        Err(_) => return TestResult::Fail("sem_create failed"),
    };

    for _ in 0..1000 {
        if sem::sem_wait_safe(&mut ns, id).is_err() {
            return TestResult::Fail("sem_wait failed");
        }
        if sem::sem_post_safe(&mut ns, id).is_err() {
            return TestResult::Fail("sem_post failed");
        }
    }

    let _ = sem::sem_destroy_safe(&mut ns, id);
    TestResult::Pass
}

#[expect(
    clippy::similar_names,
    reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
)]
fn test_invalid_ids() -> TestResult {
    let mut ns = create_test_namespace();
    let invalid_fd: i32 = 99999;
    let invalid_id: IpcId = 99999;
    let pid: u32 = 1200;

    check!(
        pipe::pipe_write_safe(&mut ns, invalid_fd, &[0u8; 1], 1).is_err(),
        "pipe_write should fail with invalid fd"
    );
    check!(
        pipe::pipe_read_safe(&mut ns, invalid_fd, &mut [0u8; 1], 1).is_err(),
        "pipe_read should fail with invalid fd"
    );
    check!(
        shm::shm_attach_safe(&mut ns, invalid_id, pid).is_err(),
        "shm_attach should fail with invalid id"
    );
    check!(
        sem::sem_wait_safe(&mut ns, invalid_id).is_err(),
        "sem_wait should fail with invalid id"
    );

    TestResult::Pass
}

#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
fn test_duplicate_close() -> TestResult {
    let mut ns = create_test_namespace();
    let mut next_id: IpcId = 1;
    let pid: u32 = 1300;

    let (rfd, wfd) = match pipe::pipe_create_safe(&mut ns, &mut next_id, pid) {
        Ok(pair) => pair,
        Err(_) => return TestResult::Fail("pipe_create failed"),
    };

    check!(
        pipe::pipe_close_safe(&mut ns, rfd).is_ok(),
        "first close should succeed"
    );
    check!(
        pipe::pipe_close_safe(&mut ns, rfd).is_err(),
        "second close should fail"
    );

    let _ = pipe::pipe_close_safe(&mut ns, wfd);
    TestResult::Pass
}

pub fn register_ipc_tests() {
    let r = runner();
    register_tests_inner! { r:
        "IPC": {
            "pipe_basic": test_pipe_basic,
            "shm_rapid_attach_detach": test_shm_rapid_attach_detach,
            "semaphore_high_concurrency": test_semaphore_high_concurrency,
            "invalid_ids": test_invalid_ids,
            "duplicate_close": test_duplicate_close,
        },
    }
}
