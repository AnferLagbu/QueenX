//! Per-CPU `RunQueue` — SMP 调度基础
//!
//! 在现有全局 MLFQ 之上添加 per-CPU 状态追踪：
//! - 每个 CPU 跟踪自己的 `current` PID
//! - per-CPU `need_reschedule` 标志
//! - 跨 CPU 重新调度 IPI (通过 `SOftirq::Sched`)
//!
//! ## 架构
//!
//! ```text
//! CPU 0                     CPU 1
//! schedule()               schedule()
//!   ├─ CpuQueue`[0]`          ├─ CpuQueue`[1]`
//!   │  current/need_resched  │  current/need_resched
//!   └─ SCHEDULER (global)   └─ SCHEDULER (global)
//!                                ↑
//!                          resched_ipi() → raise_softirq(Sched)
//! ```

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::types::Pid;

pub struct CpuQueue {
    pub current: AtomicU32,
    pub need_reschedule: AtomicBool,
    pub idle_pid: AtomicU32,
    pub online: AtomicBool,
}

// 所有字段 (AtomicU32, AtomicBool) 自动实现 Send + Sync.

impl CpuQueue {
    pub const fn new() -> Self {
        Self {
            current: AtomicU32::new(0),
            need_reschedule: AtomicBool::new(false),
            idle_pid: AtomicU32::new(0),
            online: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn get_current(&self) -> Option<Pid> {
        let pid = self.current.load(Ordering::Acquire);
        if pid == 0 { None } else { Some(pid) }
    }

    #[inline]
    pub fn set_current(&self, pid: Pid) {
        self.current.store(pid, Ordering::Release);
    }

    #[inline]
    pub fn set_need_reschedule(&self) {
        self.need_reschedule.store(true, Ordering::Release);
    }

    #[inline]
    pub fn take_need_reschedule(&self) -> bool {
        self.need_reschedule.swap(false, Ordering::AcqRel)
    }
}

struct CpuQueues {
    queues: UnsafeCell<[CpuQueue; crate::framework::config::MAX_CPUS]>,
}

// SAFETY: CpuQueues 包装 UnsafeCell<[CpuQueue; MAX_CPUS]>.
// 每个 CpuQueue[i] 仅由 CPU i 访问 (每 CPU 数据).
// 调用方在访问队列条目时必须确保 CPU 亲和性.
unsafe impl Sync for CpuQueues {}

static CPU_QUEUES: CpuQueues = CpuQueues {
    queues: UnsafeCell::new(
        [const { CpuQueue::new() }; crate::framework::config::MAX_CPUS],
    ),
};

pub fn cpu_queue(cpu_id: u32) -> &'static CpuQueue {
    let idx = cpu_id as usize % crate::framework::config::MAX_CPUS;
    // SAFETY: `CPU_QUEUES` 由调用方保证为有效指针; 只读访问
    unsafe { &(&*CPU_QUEUES.queues.get())[idx] }
}

pub fn current_cpu_queue() -> &'static CpuQueue {
    let cpu_id = crate::framework::smp::get_current_cpu();
    cpu_queue(cpu_id)
}

pub fn init_cpu_queue(cpu_id: u32, idle_pid: Pid) {
    let q = cpu_queue(cpu_id);
    q.idle_pid.store(idle_pid, Ordering::Release);
    q.current.store(idle_pid, Ordering::Release);
    q.online.store(true, Ordering::Release);
}

/// 向目标 CPU 发送重新调度 IPI
pub fn resched_cpu(target_cpu: u32) {
    let current = crate::framework::smp::get_current_cpu();
    if target_cpu == current {
        current_cpu_queue().set_need_reschedule();
        return;
    }

    cpu_queue(target_cpu).set_need_reschedule();

    let target_apic_id = crate::framework::smp::get_apic_id(target_cpu);
    if target_apic_id != 0xFFFF {
        crate::arch!(send_ipi(target_apic_id, 0xFE));
    }
}

/// IPI 重新调度入口 (由 IPI handler 调用，在目标 CPU 上执行)
/// 通过 softirq 延迟执行 `schedule()`
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn resched_ipi_handler() {
    crate::framework::irq::raise_softirq(crate::framework::irq::SoftirqVec::Sched);
}

/// 注册 softirq Sched handler (在 scheduler init 时调用)
pub fn register_sched_softirq() {
    crate::framework::irq::open_softirq(
        crate::framework::irq::SoftirqVec::Sched,
        sched_softirq_handler,
    );
}

fn sched_softirq_handler() {
    let q = current_cpu_queue();
    if q.take_need_reschedule() {
        super::scheduler::SCHEDULER.schedule();
    }
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn cpq_init(cpu_id: u32, idle_pid: Pid) {
    init_cpu_queue(cpu_id, idle_pid);
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn cpq_resched_cpu(target_cpu: u32) {
    resched_cpu(target_cpu);
}
