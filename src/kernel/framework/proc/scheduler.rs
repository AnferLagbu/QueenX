//! # Framekernel 调度器
//!
//! ## 调度策略 (I-35: MLFQ 与 CFS 二选一)
//!
//! Framekernel 采用三类调度器并存的分层架构, **没有冗余**:
//!
//! | 策略 | `SchedPolicy` | 实现 | 适用进程 |
//! |------|-------------|------|----------|
//! | DL   | `Deadline`  | Earliest-Deadline-First (EDF) + CBS | `SCHED_DEADLINE` 实时 |
//! | RT   | `Fifo`/`Rr` | 固定优先级 FIFO + 时间片 RR       | `SCHED_FIFO/RR` 实时 |
//! | CFS  | `Normal`    | vruntime 红黑树 (Linux CFS 风格)  | `SCHED_NORMAL` 普通进程 |
//!
//! **MLFQ 已退役**: 历史上 MLFQ 的多级反馈队列 (level 0..3 + 时间片 `[10,20,40,80]` ms)
//! 已完全被 CFS 取代 (注释中保留 "preserved from MLFQ" 仅为历史可追溯性).
//! `add_to_run_queue` 路径已重定向到 `cfs_enqueue`; `boost_priority` 死代码已删除
//! (与 `boost_all_vruntime` 逻辑 100% 等价), 周期性 boost 统一走 `boost_all_vruntime`.
//!
//! ## 调度决策链
//!
//! `schedule()` 严格按 DL → RT → CFS 顺序回退, 每个层级内部找不到可运行任务时
//! 立即降级到下一层级, 与 Linux `pick_next_task` 行为一致.

use crate::framework::sync::{IrqSpinLock as Mutex, OnceLock};
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::cfs::{
    CFS_BOOST_INTERVAL_TICKS, CfsRunQueue, DL_MAX_UTILIZATION_PCT, DeadlineParams, DlRunQueue,
    LOAD_BALANCE_THRESHOLD, NICE0_WEIGHT, TARGET_LATENCY_TICKS, calc_vruntime_delta,
    cfs_should_preempt, nice_to_weight,
};
use super::process::{PROCESS_TABLE, Process};
use super::types::{BlockReason, Pid, ProcessContext, ProcessId, ProcessPriority, ProcessState};

// === E2: unsafe 集中化 — 裸子模块 ===
//
// FFI 调用与每 CPU 指针解引用无法在 safe Rust 中表达,
// 在此集中封装.
pub(crate) mod raw {
    /// 更新每 CPU 的 `current_process_ptr`, 供汇编入口路径使用.
    ///
    /// # Safety
    /// - `ptr` 必须是合法的 `*const Process` 或 0
    #[inline(always)]
    pub unsafe fn update_current_process_ptr(ptr: u64) {
        unsafe {
            // SAFETY: update_current_process_ptr 由调度器汇编路径提供, 供中断/上下文切换调用
            unsafe extern "C" {
                fn update_current_process_ptr(ptr: u64);
            }
            update_current_process_ptr(ptr);
        }
    }
}

use raw::update_current_process_ptr;

macro_rules! klog_sched_warn {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_warn, $($arg)*)
    };
}

const RT_PRIORITY_MAX: u8 = 99;
const RT_TIME_SLICE: u64 = 5;
const RT_FIFO_WATCHDOG: u64 = 500;

/// kswapd softirq 唤醒周期 (ticks). 100 ticks @ 1kHz timer = 100ms.
/// B3 完整实现: 周期触发 kswapd, 软中断上下文回收不活跃页面.
const KSWAPD_TICK_INTERVAL: u64 = 100;

pub struct PwidQuota {
    pub pwm: u64,
    pub used: bool,
    pub max_runtime: u64,
    pub period: u64,
    pub consumed: u64,
    pub next_reset: u64,
}

impl PwidQuota {
    const fn new() -> Self {
        Self {
            pwm: 0,
            used: false,
            max_runtime: 0,
            period: 0,
            consumed: 0,
            next_reset: 0,
        }
    }
}

use crate::framework::constants::limits::{MAX_LIMITS, MAX_QUOTAS};

pub struct PwidLimit {
    pub pwm: u64,
    pub used: bool,
    pub max_procs: u32,
    pub current: u32,
}

pub static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

/// 获取当前全局 tick 计数 (供 `tick_query` 注册回调使用).
#[inline]
pub fn get_tick() -> u64 {
    TICK_COUNT.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedPolicy {
    Normal = 0,
    Fifo = 1,
    Rr = 2,
    Idle = 3,
    Deadline = 4,
}

impl SchedPolicy {
    #[expect(
        clippy::match_same_arms,
        reason = "match_same_arms: match arm 重复是为可读性/调试断点; 当前优先 expect"
    )]
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::Fifo,
            2 => Self::Rr,
            3 => Self::Idle,
            4 => Self::Deadline,
            _ => Self::Normal,
        }
    }
}

pub struct RtTaskInfo {
    pub pid: Pid,
    pub rt_priority: u8,
    pub policy: SchedPolicy,
    pub time_slice_remaining: u64,
}

struct PerCpuSched {
    rt_queue: Mutex<VecDeque<RtTaskInfo>>,
    cfs_rq: Mutex<CfsRunQueue>,
    dl_rq: Mutex<DlRunQueue>,
    current: AtomicU32,
    need_reschedule: AtomicBool,
    rt_running: AtomicBool,
    dl_running: AtomicBool,
    fifo_watchdog: AtomicU64,
}

// 所有字段 (Mutex<VecDeque<Pid>>, Mutex<CfsRunQueue>, Mutex<DlRunQueue>, Atomic*) 自动实现 Send + Sync.

static PER_CPU_SCHED: [OnceLock<PerCpuSched>; crate::framework::config::MAX_CPUS] =
    [const { OnceLock::new() }; crate::framework::config::MAX_CPUS];

pub fn init_per_cpu_sched(cpu_id: u32) {
    let idx = (cpu_id as usize) % crate::framework::config::MAX_CPUS;
    PER_CPU_SCHED[idx].get_or_init(|slot| {
        // SAFETY: OnceLock::get_or_init 保证闭包仅执行一次, slot 未初始化.
        unsafe {
            slot.write(PerCpuSched {
                rt_queue: Mutex::new(VecDeque::new()),
                cfs_rq: Mutex::new(CfsRunQueue::new()),
                dl_rq: Mutex::new(DlRunQueue::new()),
                current: AtomicU32::new(0),
                need_reschedule: AtomicBool::new(false),
                rt_running: AtomicBool::new(false),
                dl_running: AtomicBool::new(false),
                fifo_watchdog: AtomicU64::new(0),
            });
        }
    });
}

#[inline]
fn per_cpu() -> &'static PerCpuSched {
    let cpu = crate::framework::smp::get_current_cpu();
    let idx = (cpu as usize) % crate::framework::config::MAX_CPUS;
    // 确保已初始化 (幂等)
    init_per_cpu_sched(cpu);
    // SAFETY: init_per_cpu_sched 后 OnceLock 已初始化, get_or_init 返回 &'static 引用.
    PER_CPU_SCHED[idx].get_or_init(|_| unreachable!())
}

#[inline]
fn per_cpu_for(cpu_id: u32) -> &'static PerCpuSched {
    let idx = (cpu_id as usize) % crate::framework::config::MAX_CPUS;
    // 确保已初始化 (幂等)
    init_per_cpu_sched(cpu_id);
    // SAFETY: init_per_cpu_sched 后 OnceLock 已初始化.
    PER_CPU_SCHED[idx].get_or_init(|_| unreachable!())
}

pub struct Scheduler {
    quotas: Mutex<[PwidQuota; MAX_QUOTAS]>,
    limits: Mutex<[PwidLimit; MAX_LIMITS]>,
    initialized: AtomicBool,
}

// All fields (Mutex<[PwidQuota; N]>, Mutex<[PwidLimit; N]>, AtomicBool) auto-implement Send + Sync.

impl Scheduler {
    pub const fn new() -> Self {
        const QUOTA_ZERO: PwidQuota = PwidQuota::new();
        const LIMIT_ZERO: PwidLimit = PwidLimit {
            pwm: 0,
            used: false,
            max_procs: 0,
            current: 0,
        };
        Self {
            quotas: Mutex::new([QUOTA_ZERO; MAX_QUOTAS]),
            limits: Mutex::new([LIMIT_ZERO; MAX_LIMITS]),
            initialized: AtomicBool::new(false),
        }
    }

    pub fn init(&self) {
        init_per_cpu_sched(0);

        self.initialized.store(true, Ordering::SeqCst);

        let init_pid = self.create_process("init", None, 0);
        if let Some(pid) = init_pid {
            PROCESS_TABLE.with_process(pid, |proc| {
                let _ = proc.set_state_safe(ProcessState::Running);
                proc.set_priority(ProcessPriority::Normal);
            });
            self.set_current(pid);

            if let Some(process_ptr) = PROCESS_TABLE.get(pid) {
                // SAFETY: process_ptr is valid from PROCESS_TABLE.get()
                unsafe {
                    update_current_process_ptr(process_ptr as u64);
                }
            }
        }

        if per_cpu().need_reschedule.swap(false, Ordering::SeqCst) {
            self.schedule();
        }
    }

    #[expect(
        clippy::ptr_as_ptr,
        reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
    )]
    pub fn create_process(&self, name: &str, parent: Option<Pid>, pwm: u64) -> Option<Pid> {
        let pid = PROCESS_TABLE.allocate_pid()?;

        // L4: per-PWM proc count limit
        if pwm != 0 {
            let mut limits = self.limits.lock();
            for l in limits.iter_mut() {
                if l.used && l.pwm == pwm {
                    if l.max_procs > 0 && l.current >= l.max_procs {
                        return None;
                    }
                    l.current += 1;
                    break;
                }
            }
        }

        let parent_id = parent.map(ProcessId);
        let process = alloc::boxed::Box::new(Process::new(pid, name, parent_id));
        process.set_pwm(pwm);

        let process_ptr = alloc::boxed::Box::into_raw(process);

        if !PROCESS_TABLE.insert(process_ptr) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                alloc::alloc::dealloc(
                    process_ptr as *mut u8,
                    alloc::alloc::Layout::new::<Process>(),
                );
            };
            return None;
        }

        // 初始化进程组 ID (POSIX: 新进程默认自成一组, pgid = pid)
        crate::framework::proc::proc_init_pgid(pid);

        Some(pid)
    }

    pub fn add(&self, pid: Pid) {
        self.cfs_enqueue(pid);
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// 设置 nice 值并更新进程的 CFS 权重.
    pub fn set_nice(&self, pid: Pid, nice: i8) {
        PROCESS_TABLE.with_process(pid, |proc| {
            let clamped = nice.clamp(-20, 19);
            let w = nice_to_weight(clamped);
            proc.nice.store(clamped as u32, Ordering::Release);
            proc.cfs_weight.store(w, Ordering::Release);
        });
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// 将进程入队 CFS 运行队列.
    fn cfs_enqueue(&self, pid: Pid) {
        let per_cpu = per_cpu();
        let vr = PROCESS_TABLE.with_process(pid, |p| {
            let _ = p.set_state_safe(ProcessState::Ready);
            let v = p.cfs_vruntime.load(Ordering::Acquire);
            let w = p.cfs_weight.load(Ordering::Acquire);
            p.cfs_on_rq.store(true, Ordering::Release);
            (v, w)
        });

        if let Some((vruntime, weight)) = vr {
            per_cpu.cfs_rq.lock().enqueue(pid, vruntime, weight);
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// 为进程设置 `SCHED_DEADLINE` 参数.
    pub fn set_deadline_params(&self, pid: Pid, params: DeadlineParams) -> bool {
        if !params.is_valid() {
            return false;
        }
        let util = params.utilization_pct();
        if util > DL_MAX_UTILIZATION_PCT {
            return false;
        }
        PROCESS_TABLE.with_process(pid, |proc| {
            proc.set_sched_policy(SchedPolicy::Deadline);
            proc.dl_runtime.store(params.runtime, Ordering::Release);
            proc.dl_deadline.store(params.deadline, Ordering::Release);
            proc.dl_period.store(params.period, Ordering::Release);
            let now = TICK_COUNT.load(Ordering::Acquire);
            proc.dl_abs.store(now + params.deadline, Ordering::Release);
            proc.dl_remaining.store(params.runtime, Ordering::Release);
        });
        true
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn add_rt_task(&self, pid: Pid, rt_priority: u8, policy: SchedPolicy) {
        let priority = rt_priority.min(RT_PRIORITY_MAX);

        let mut rt_queue = per_cpu().rt_queue.lock();
        let mut inserted = false;

        for i in 0..rt_queue.len() {
            if rt_queue[i].rt_priority < priority {
                rt_queue.insert(
                    i,
                    RtTaskInfo {
                        pid,
                        rt_priority: priority,
                        policy,
                        time_slice_remaining: RT_TIME_SLICE,
                    },
                );
                inserted = true;
                break;
            }
        }

        if !inserted {
            rt_queue.push_back(RtTaskInfo {
                pid,
                rt_priority: priority,
                policy,
                time_slice_remaining: RT_TIME_SLICE,
            });
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// 选取一个 deadline 任务 (EDF —— 绝对 deadline 最早者优先).
    fn pick_deadline_task(&self) -> Option<Pid> {
        let per_cpu = per_cpu();
        let mut dl_rq = per_cpu.dl_rq.lock();
        if dl_rq.is_empty() {
            per_cpu.dl_running.store(false, Ordering::SeqCst);
            return None;
        }
        if let Some((pid, dl_abs)) = dl_rq.pick_next() {
            let alive = PROCESS_TABLE
                .with_process(pid, |p| {
                    p.get_state() != ProcessState::Zombie
                        && p.get_sched_policy() == SchedPolicy::Deadline
                })
                .unwrap_or(false);
            if alive {
                per_cpu.dl_running.store(true, Ordering::SeqCst);
                Some(pid)
            } else {
                // pick_next() 已从树中移除任务, 但
                // 保留 nr_running (与 CfsRunQueue 相同).
                // reinsert() 把它放回去, 不修改计数器.
                dl_rq.reinsert(pid, dl_abs);
                per_cpu.dl_running.store(false, Ordering::SeqCst);
                None
            }
        } else {
            per_cpu.dl_running.store(false, Ordering::SeqCst);
            None
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// 选取一个 CFS 任务 (vruntime 最小者).
    fn pick_cfs_task(&self) -> Option<Pid> {
        let per_cpu = per_cpu();
        let mut cfs_rq = per_cpu.cfs_rq.lock();
        if cfs_rq.is_empty() {
            return None;
        }
        match cfs_rq.pick_next() {
            Some((pid, vr)) => {
                let schedulable = PROCESS_TABLE
                    .with_process(pid, |p| {
                        let state = p.get_state();
                        let policy = p.get_sched_policy();
                        state != ProcessState::Blocked
                            && state != ProcessState::Zombie
                            && policy == SchedPolicy::Normal
                    })
                    .unwrap_or(false);
                if schedulable {
                    PROCESS_TABLE.with_process(pid, |p| {
                        p.cfs_on_rq.store(false, Ordering::Release);
                    });
                    Some(pid)
                } else {
                    // 任务已被 pick_next() 从树中移除, 但不可调度
                    // (阻塞/僵尸/策略错). 重新插入以避免静默丢失.
                    cfs_rq.update_curr(pid, vr);
                    None
                }
            }
            None => None,
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
    )]
    #[expect(
        clippy::ptr_as_ptr,
        reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
    )]
    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    pub fn schedule(&self) -> Option<Pid> {
        let saved_flags = crate::arch!(interrupt_disable()) as u64;

        let per_cpu = per_cpu();
        let current_pid = per_cpu.current.load(Ordering::SeqCst);

        let mut next_pid = self.pick_deadline_task();

        // 2. RT (FIFO/RR) —— 从 MLFQ 保留
        if next_pid.is_none() {
            per_cpu.dl_running.store(false, Ordering::SeqCst);

            let mut rt_queue = per_cpu.rt_queue.lock();

            while !rt_queue.is_empty() {
                let rt_task = if let Some(task) = rt_queue.pop_front() {
                    task
                } else {
                    klog_sched_warn!("[SCHEDULER] RT queue race condition detected");
                    break;
                };
                let rt_pid = rt_task.pid;

                let alive = PROCESS_TABLE
                    .with_process(rt_pid, |p| p.get_state() != ProcessState::Zombie)
                    .unwrap_or(false);

                if !alive {
                    continue;
                }

                match rt_task.policy {
                    SchedPolicy::Fifo => {
                        next_pid = Some(rt_pid);
                        per_cpu.rt_running.store(true, Ordering::SeqCst);
                        per_cpu
                            .fifo_watchdog
                            .store(RT_FIFO_WATCHDOG, Ordering::SeqCst);
                        break;
                    }
                    SchedPolicy::Rr => {
                        next_pid = Some(rt_pid);
                        per_cpu.rt_running.store(true, Ordering::SeqCst);
                        let mut updated_rt = rt_task;
                        updated_rt.time_slice_remaining = RT_TIME_SLICE;
                        rt_queue.push_back(updated_rt);
                        break;
                    }
                    _ => {
                        self.cfs_enqueue(rt_pid);
                        per_cpu.rt_running.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }

            if next_pid.is_none() {
                per_cpu.rt_running.store(false, Ordering::SeqCst);
            }
        }

        // 3. CFS (vruntime 最小) —— 取代 MLFQ 用于 SCHED_NORMAL
        if next_pid.is_none() {
            next_pid = self.pick_cfs_task();
        }

        // 4. 本地无候选时进行负载均衡
        if next_pid.is_none() && crate::framework::smp::is_enabled() {
            self.load_balance();
            next_pid = self.pick_cfs_task();
        }

        let next = if let Some(pid) = next_pid {
            pid
        } else {
            if saved_flags & 0x200 != 0 {
                crate::arch!(interrupt_enable());
            }
            return None;
        };

        if next == current_pid {
            if saved_flags & 0x200 != 0 {
                crate::arch!(interrupt_enable());
            }
            return Some(next);
        }

        let prev_ptr = if current_pid != 0 {
            PROCESS_TABLE.get(current_pid)
        } else {
            None
        };

        let next_ptr = PROCESS_TABLE.get(next);

        if next_ptr.is_none() {
            if saved_flags & 0x200 != 0 {
                crate::arch!(interrupt_enable());
            }
            return None;
        }

        PROCESS_TABLE.with_process(next, |proc| {
            let _ = proc.set_state_safe(ProcessState::Running);
            let next_kernel_stack = proc.kernel_stack.load(Ordering::SeqCst);
            if next_kernel_stack != 0 {
                crate::framework::cpu::arch::set_kernel_stack(next_kernel_stack);
            }
        });

        per_cpu.current.store(next, Ordering::SeqCst);

        super::scheduler_ex::SCHEDULER_EX
            .current
            .store(u64::from(next), Ordering::SeqCst);

        if let Some(next_ptr_raw) = next_ptr {
            // SAFETY: next_ptr_raw is valid from PROCESS_TABLE.get()
            unsafe {
                update_current_process_ptr(next_ptr_raw as u64);
            }
        }

        if let Some(user_proc) = super::user_proc::USER_PROC_MANAGER.get(next) {
            super::user_proc::USER_PROC_MANAGER.set_current(Some(user_proc));
            // 更新 per-CPU 用户页表 CR3, 使 syscall/中断返回用户态时
            // 从 [gs:USER_PML4_OFF] 读取到正确的进程用户页表.
            // SAFETY: user_proc 来自 PROCESS_TABLE, 通过 process() 访问权威 Process;
            // 当前在调度器持锁上下文, 独占访问 per-CPU 数据.
            unsafe {
                let user_cr3 = (*user_proc).process().cr3.load(Ordering::SeqCst);
                #[cfg(target_arch = "x86_64")]
                crate::framework::arch::gdt::gdt_set_user_cr3(user_cr3);
                let _ = user_cr3;
            }
        }

        // 重新入队上一个任务
        if let Some(ref _prev_proc) = prev_ptr {
            let was_dl = per_cpu.dl_running.load(Ordering::SeqCst);
            let was_rt = per_cpu.rt_running.load(Ordering::SeqCst);

            if was_dl {
                let dl_info =
                    PROCESS_TABLE.with_process(current_pid, |p| p.dl_abs.load(Ordering::Acquire));
                if let Some(dl_abs) = dl_info {
                    // pick_next() 保留了 nr_running (与 CFS 相同).
                    // reinsert() 把任务放回树, 不修改计数器
                    // —— 任务在最初入队时已计入.
                    per_cpu.dl_rq.lock().reinsert(current_pid, dl_abs);
                }
            } else if was_rt {
                let rt_info = PROCESS_TABLE
                    .with_process(current_pid, |p| (p.get_rt_priority(), p.get_sched_policy()));
                if let Some((rt_priority, policy)) = rt_info {
                    if policy != SchedPolicy::Fifo {
                        let mut rt_queue = per_cpu.rt_queue.lock();
                        let mut inserted = false;
                        for i in 0..rt_queue.len() {
                            if rt_queue[i].rt_priority < rt_priority {
                                rt_queue.insert(
                                    i,
                                    RtTaskInfo {
                                        pid: current_pid,
                                        rt_priority,
                                        policy,
                                        time_slice_remaining: RT_TIME_SLICE,
                                    },
                                );
                                inserted = true;
                                break;
                            }
                        }
                        if !inserted {
                            rt_queue.push_back(RtTaskInfo {
                                pid: current_pid,
                                rt_priority,
                                policy,
                                time_slice_remaining: RT_TIME_SLICE,
                            });
                        }
                    }
                }
            } else {
                let (vr, _wt, _nice) = PROCESS_TABLE
                    .with_process(current_pid, |p| {
                        (
                            p.cfs_vruntime.load(Ordering::Acquire),
                            p.cfs_weight.load(Ordering::Acquire),
                            p.nice.load(Ordering::Acquire) as i8,
                        )
                    })
                    .unwrap_or((0, NICE0_WEIGHT, 0i8));
                per_cpu.cfs_rq.lock().update_curr(current_pid, vr);
                PROCESS_TABLE.with_process(current_pid, |p| {
                    p.cfs_on_rq.store(true, Ordering::Release);
                });
            }
            PROCESS_TABLE.with_process(current_pid, |p| {
                let _ = p.set_state_safe(ProcessState::Ready);
            });
        }

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let prev_ctx_ptr = prev_ptr.map_or(core::ptr::null_mut(), |p| unsafe {
            &raw mut (*p).context as *mut Mutex<ProcessContext>
        });

        let next_ctx_ptr = next_ptr.map_or(core::ptr::null(), |p| {
            // SAFETY: next_ptr 来自 PROCESS_TABLE.get(), 它返回指向活动
            // Process 的合法指针. 这里只读取 context 字段地址.
            unsafe { &raw const (*p).context as *const Mutex<ProcessContext> }
        });

        if !prev_ctx_ptr.is_null() {
            // 关键修复 (B05-55): 不能持 MutexGuard 调 context_switch.
            // process_switch_asm 切换后永不返回 (iretq 到 next 用户态), Guard 的
            // Drop 不执行 → prev/next 的 context 锁永久泄漏 → next 进程运行后
            // p.context.lock() (如 proc_save_user_regs) 自旋死锁.
            // 改用 get_mut_unchecked 裸访问: 单核 + process_switch_asm 开头 cli
            // 保证切换期间无并发访问.
            // SAFETY: prev/next_ptr 均派生自 PROCESS_TABLE 中活动的 Process 条目;
            // 单核 + cli 排他, 详见 get_mut_unchecked 文档.
            unsafe {
                let prev_ctx = core::ptr::addr_of_mut!(*((*prev_ctx_ptr).get_mut_unchecked()));
                let next_ctx = core::ptr::addr_of!(*((*next_ctx_ptr).get_mut_unchecked()));
                crate::arch!(context_switch(
                    prev_ctx as *mut u8,
                    next_ctx as *const u8
                ));
            }
        }

        crate::framework::sync::rcu::rcu_note_quiescent_state();

        Some(next)
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn current(&self) -> Option<Pid> {
        let pid = per_cpu().current.load(Ordering::SeqCst);
        if pid == 0 { None } else { Some(pid) }
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn get_current_process(&self) -> Option<*mut Process> {
        let pid = per_cpu().current.load(Ordering::SeqCst);
        if pid == 0 {
            None
        } else {
            PROCESS_TABLE.get(pid)
        }
    }

    pub fn block(&self, reason: BlockReason) {
        let per_cpu = per_cpu();
        if let Some(pid) = self.current() {
            PROCESS_TABLE.with_process(pid, |proc| {
                let _ = proc.set_state_safe(ProcessState::Blocked);
                proc.block_reason.store(reason as u32, Ordering::SeqCst);
            });
            per_cpu.need_reschedule.store(true, Ordering::SeqCst);
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn unblock(&self, pid: Pid) {
        let sched_policy = PROCESS_TABLE
            .with_process(pid, |proc| {
                let state = proc.get_state();
                if state != ProcessState::Blocked && state != ProcessState::Frozen {
                    return None;
                }
                let _ = proc.set_state_safe(ProcessState::Ready);
                let policy = proc.get_sched_policy();
                if policy == SchedPolicy::Normal {
                    proc.cfs_on_rq.store(true, Ordering::Release);
                    let vr = proc.cfs_vruntime.load(Ordering::Acquire);
                    let w = proc.cfs_weight.load(Ordering::Acquire);
                    Some((policy, vr, w))
                } else if policy == SchedPolicy::Deadline {
                    let dl_abs = proc.dl_abs.load(Ordering::Acquire);
                    let runtime = proc.dl_runtime.load(Ordering::Acquire);
                    let period = proc.dl_period.load(Ordering::Acquire);
                    Some((
                        policy,
                        dl_abs,
                        if period > 0 {
                            (runtime * 100) / period
                        } else {
                            0
                        },
                    ))
                } else {
                    Some((policy, 0, 0))
                }
            })
            .flatten();

        match sched_policy {
            Some((SchedPolicy::Normal, vr, weight)) => {
                per_cpu().cfs_rq.lock().enqueue(pid, vr, weight);
            }
            Some((SchedPolicy::Fifo | SchedPolicy::Rr, _, _)) => {
                let (prio, pol) = PROCESS_TABLE
                    .with_process(pid, |p| (p.get_rt_priority(), p.get_sched_policy()))
                    .unwrap_or((0, SchedPolicy::Normal));

                let mut rt_q = per_cpu().rt_queue.lock();
                let mut inserted = false;
                for i in 0..rt_q.len() {
                    if rt_q[i].rt_priority < prio {
                        rt_q.insert(
                            i,
                            RtTaskInfo {
                                pid,
                                rt_priority: prio,
                                policy: pol,
                                time_slice_remaining: RT_TIME_SLICE,
                            },
                        );
                        inserted = true;
                        break;
                    }
                }
                if !inserted {
                    rt_q.push_back(RtTaskInfo {
                        pid,
                        rt_priority: prio,
                        policy: pol,
                        time_slice_remaining: RT_TIME_SLICE,
                    });
                }
            }
            Some((SchedPolicy::Deadline, dl_abs, util)) => {
                per_cpu().dl_rq.lock().enqueue(pid, dl_abs, util);
            }
            _ => {}
        }
    }

    pub fn exit(&self, exit_code: u32) {
        let per_cpu = per_cpu();
        if let Some(pid) = self.current() {
            // 会话 leader 退出时释放控制终端
            crate::framework::proc::session_leader_exit(pid);

            let parent_pid_opt = PROCESS_TABLE.with_process(pid, |proc| {
                let pwm = proc.get_pwm();
                proc.exit_code.store(exit_code, Ordering::SeqCst);
                let _ = proc.set_state_safe(ProcessState::Zombie);
                self.dec_limit(pwm);

                // D2: 进程退出时从 cgroup 移除
                let cg_id = proc.cgroup_id.load(core::sync::atomic::Ordering::Acquire);
                if crate::framework::proc::cgroup_is_initialized() {
                    let sub = crate::framework::proc::cgroup_subsystem();
                    if let Some(cg) = sub.find(cg_id) {
                        cg.detach_proc(pid);
                    }
                }

                proc.parent.map(|p| p.0)
            });

            if let Some(parent_pid) = parent_pid_opt.flatten() {
                self.unblock(parent_pid);
            }

            PROCESS_TABLE.with_process(pid, |proc| {
                let children: alloc::vec::Vec<Pid> =
                    proc.children.lock().iter().map(|c| c.0).collect();
                for child_pid in children {
                    PROCESS_TABLE.with_process_mut(child_pid, |child| {
                        let state = child.get_state();
                        if state == ProcessState::Zombie {
                            let _ = child.set_state_safe(ProcessState::Terminated);
                        } else {
                            child.parent = Some(ProcessId(1));
                        }
                    });
                    if PROCESS_TABLE
                        .with_process(child_pid, |c| c.get_state() == ProcessState::Terminated)
                        .unwrap_or(false)
                    {
                        PROCESS_TABLE.remove_and_free(child_pid);
                    }
                }
                proc.children.lock().clear();
            });
        }

        per_cpu.need_reschedule.store(true, Ordering::SeqCst);

        if self.schedule().is_none() {
            crate::arch!(outb(0xf4, (exit_code as u8).wrapping_shl(1) | 1));
            loop {
                crate::arch!(halt());
            }
        }
    }

    pub fn yield_current(&self) {
        per_cpu().need_reschedule.store(true, Ordering::SeqCst);
        self.schedule();
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn set_need_reschedule(&self) {
        per_cpu().need_reschedule.store(true, Ordering::SeqCst);
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn should_reschedule(&self) -> bool {
        per_cpu().need_reschedule.swap(false, Ordering::SeqCst)
    }

    pub fn add_to_run_queue(&self, pid: Pid) {
        // I-35: 重定向到 cfs_enqueue. 历史 MLFQ queues[0] 路径曾导致新进程
        // 永远不会被 pick_cfs_task 选中 (调度器只读 cfs_rq, 不读 queues[]).
        // 现在 add / add_to_run_queue 等价, 都走 vruntime 红黑树.
        self.cfs_enqueue(pid);
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn set_current(&self, pid: Pid) {
        per_cpu().current.store(pid, Ordering::SeqCst);

        if let Some(process_ptr) = PROCESS_TABLE.get(pid) {
            // SAFETY: process_ptr is valid from PROCESS_TABLE.get()
            unsafe {
                update_current_process_ptr(process_ptr as u64);
            }
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn has_runnable(&self) -> bool {
        let per_cpu = per_cpu();
        if !per_cpu.dl_rq.lock().is_empty() {
            return true;
        }
        if !per_cpu.rt_queue.lock().is_empty() {
            return true;
        }
        if !per_cpu.cfs_rq.lock().is_empty() {
            return true;
        }
        false
    }

    /// 便捷: 检查是否存在任意可运行任务.
    pub fn has_any_runnable(&self) -> bool {
        self.has_runnable()
    }

    #[expect(
        clippy::too_many_lines,
        reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
    )]
    pub fn tick(&self, cpu_id: usize) {
        // SMP: 禁用中断保护整个 tick 临界区
        // 防止非中断上下文的 schedule() 调用与 timer ISR 的 tick() 并发修改 per-CPU 状态
        let flags = crate::framework::sync::disable_interrupts();

        let new_tick = TICK_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        let per_cpu = per_cpu_for(cpu_id as u32);

        crate::framework::barrier::RECOVERY_MANAGER
            .lock()
            .tick(new_tick);

        if crate::framework::barrier::check_and_clear_bsr_escalation() {
            crate::framework::barrier::reset::config::set_reset_in_progress(true);
            crate::framework::barrier::reset::config::set_current_layer(
                crate::framework::barrier::reset::config::RecoveryLayer::Layer2,
            );
            crate::framework::barrier::reset::bsr::freeze_all_domains();
            crate::framework::barrier::reset::bsr::rollback_to_init();
            crate::framework::barrier::reset::bsr::reset_devices();
            crate::framework::barrier::reset::bsr::unfreeze_all_domains();
            crate::framework::barrier::reset::bsr::clear_panic_state();
        }

        crate::framework::proc::oomd::OOMD.tick();
        crate::framework::proc::SCHEDULER_EX.tick_accounting();

        // Periodic kswapd wakeup — 每 100 ticks 唤醒一次内存回收
        // (B3 完整实现: kswapd 走 softirq 路径, 由 scheduler tick 周期驱动)
        if new_tick.is_multiple_of(KSWAPD_TICK_INTERVAL) {
            crate::framework::mm::kswapd_wakeup();
        }

        // 周期性 CFS 提升 —— 防止 vruntime 饥饿
        if new_tick.is_multiple_of(CFS_BOOST_INTERVAL_TICKS) {
            let mut cfs_rq = per_cpu.cfs_rq.lock();
            cfs_rq.boost_all_vruntime();
        }

        let current_pid = per_cpu.current.load(Ordering::SeqCst);
        if current_pid != 0 {
            // RT FIFO watchdog
            let is_rt = per_cpu.rt_running.load(Ordering::SeqCst);
            if is_rt && per_cpu.fifo_watchdog.load(Ordering::SeqCst) > 0 {
                let remaining = per_cpu.fifo_watchdog.fetch_sub(1, Ordering::SeqCst);
                if remaining <= 1 {
                    per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                    per_cpu.rt_running.store(false, Ordering::SeqCst);
                }
            }

            // Tick 计数: 按策略的时间跟踪
            let is_dl = per_cpu.dl_running.load(Ordering::SeqCst);

            if is_dl {
                let (expired, should_replenish) = PROCESS_TABLE
                    .with_process(current_pid, |p| {
                        let old_rem = p.dl_remaining.fetch_sub(1, Ordering::SeqCst);
                        let rem = old_rem - 1;
                        let expired = rem == 0;
                        let deadline = p.dl_deadline.load(Ordering::Acquire);
                        let dl_abs = p.dl_abs.load(Ordering::Acquire);
                        let now = TICK_COUNT.load(Ordering::Acquire);
                        let should_replenish =
                            now >= dl_abs || (expired && now + deadline > dl_abs);
                        (expired, should_replenish)
                    })
                    .unwrap_or((true, false));

                if should_replenish {
                    PROCESS_TABLE.with_process(current_pid, |p| {
                        let runtime = p.dl_runtime.load(Ordering::Acquire);
                        let deadline = p.dl_deadline.load(Ordering::Acquire);
                        let now = TICK_COUNT.load(Ordering::Acquire);
                        p.dl_remaining.store(runtime, Ordering::Release);
                        p.dl_abs.store(now + deadline, Ordering::Release);
                    });
                }

                if expired || should_replenish {
                    per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                }
            } else if is_rt {
                // RT FIFO watchdog
                let policy = PROCESS_TABLE
                    .with_process(current_pid, super::process::Process::get_sched_policy)
                    .unwrap_or(SchedPolicy::Normal);

                if policy == SchedPolicy::Fifo {
                    let old_watchdog = per_cpu.fifo_watchdog.fetch_sub(1, Ordering::SeqCst);
                    if old_watchdog - 1 == 0 {
                        per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                        crate::klog_crit!(
                            Kernel,
                            "[SCHEDULER] RT-FIFO watchdog triggered for pid={}",
                            current_pid
                        );
                    }
                }
            } else if current_pid != 0 {
                // CFS —— 基于 vruntime 的抢占.

                // 重要: 不要在此把运行中任务重新插入树.
                // 运行中任务保持在树外, 仅在停止运行时
                // (schedule 的重新入队路径) 才会被重新入队.
                let (should_preempt, should_yield) = {
                    let cfs_rq = per_cpu.cfs_rq.lock();
                    let vr = PROCESS_TABLE
                        .with_process(current_pid, |p| {
                            let old_vr = p.cfs_vruntime.load(Ordering::Acquire);
                            let weight = p.cfs_weight.load(Ordering::Acquire);
                            let delta = calc_vruntime_delta(weight);
                            // L2 修复: 使用 saturating_add 防止 vruntime 溢出
                            let new_vr = old_vr.saturating_add(delta);
                            p.cfs_vruntime.store(new_vr, Ordering::Release);
                            let sum = p.cfs_sum_exec_runtime.load(Ordering::Acquire);
                            // L2 修复: 使用 saturating_add 防止 sum_exec_runtime 溢出
                            p.cfs_sum_exec_runtime
                                .store(sum.saturating_add(1), Ordering::Release);
                            new_vr
                        })
                        .unwrap_or(0);

                    let should_preempt = cfs_rq.nr_running > 0
                        && cfs_should_preempt(
                            vr,
                            cfs_rq.min_vruntime.load(Ordering::Acquire),
                            PROCESS_TABLE
                                .with_process(current_pid, |p| p.cfs_weight.load(Ordering::Acquire))
                                .unwrap_or(NICE0_WEIGHT),
                        );

                    let should_yield =
                        vr > cfs_rq.min_vruntime.load(Ordering::Acquire) + TARGET_LATENCY_TICKS;

                    (should_preempt, should_yield)
                };

                if should_preempt || should_yield {
                    per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                }
            }
        }

        // 睡眠唤醒扫描
        {
            let current_ticks = new_tick;
            let mut to_wake: [Pid; 8] = [0; 8];
            let mut wake_count = 0;
            for pid in 1..=255 {
                if wake_count >= 8 {
                    break;
                }
                if pid == self.current().unwrap_or(0) {
                    continue;
                }
                PROCESS_TABLE.with_process(pid, |proc| {
                    let state = proc.get_state();
                    if state == ProcessState::Blocked {
                        let reason = proc.block_reason.load(Ordering::Relaxed);
                        if reason == BlockReason::Sleeping as u32 {
                            let until = proc.sleep_until.load(Ordering::SeqCst);
                            if until > 0 && current_ticks >= until {
                                to_wake[wake_count] = pid;
                                wake_count += 1;
                            }
                        }
                    }
                });
            }
            for i in 0..wake_count {
                self.unblock(to_wake[i]);
            }
        }

        // Zombie cleanup
        let socks_clean_interval: u64 = 1000;
        if new_tick.is_multiple_of(socks_clean_interval) {
            let mut to_reap: [Pid; 16] = [0; 16];
            let mut reap_count = 0;
            for pid in 1..=255 {
                if reap_count >= 16 {
                    break;
                }
                if let Some(_proc) = PROCESS_TABLE.get(pid) {
                    let is_zombie = PROCESS_TABLE
                        .with_process(pid, |p| {
                            if p.get_state() == ProcessState::Zombie {
                                let parent_alive = p.parent.map_or(true, |ppid| {
                                    PROCESS_TABLE
                                        .with_process(ppid.0, |pp| {
                                            let s = pp.get_state();
                                            s != ProcessState::Zombie
                                                && s != ProcessState::Terminated
                                        })
                                        .unwrap_or(false)
                                });
                                !parent_alive || p.parent == Some(ProcessId(1))
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if is_zombie {
                        to_reap[reap_count] = pid;
                        reap_count += 1;
                    }
                }
            }
            for i in 0..reap_count {
                PROCESS_TABLE.with_process(to_reap[i], |p| {
                    let _ = p.set_state_safe(ProcessState::Terminated);
                });
                PROCESS_TABLE.remove_and_free(to_reap[i]);
            }
        }

        // 周期性负载均衡
        if new_tick.is_multiple_of(64) {
            let local_load =
                self.total_runnable_for(crate::framework::smp::get_current_cpu());
            if local_load < 2 {
                self.load_balance();
            }
        }

        if per_cpu.need_reschedule.swap(false, Ordering::SeqCst) {
            self.schedule();
        }

        crate::framework::sync::restore_interrupts(&flags);
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn set_sched_policy(&self, pid: Pid, policy: SchedPolicy, rt_priority: u8) -> bool {
        PROCESS_TABLE
            .with_process(pid, |proc| {
                proc.set_sched_policy(policy);
                proc.set_rt_priority(rt_priority.min(RT_PRIORITY_MAX));
            })
            .is_some()
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    pub fn get_rt_count(&self) -> usize {
        per_cpu().rt_queue.lock().len()
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// C2: 检查目标 CPU 是否在进程的 allowed cpuset 中
    ///
    /// 调度器选 CPU / 负载均衡迁移时调用, 约束进程 CPU 亲和性.
    /// 单核系统: 始终返回 true.
    pub fn is_cpu_allowed(&self, pid: Pid, cpu_id: u32) -> bool {
        if pid == 0 {
            return true;
        }
        let cpu_count = crate::framework::smp::get_cpu_count();
        if cpu_count <= 1 || cpu_id >= cpu_count {
            return true;
        }
        if (cpu_id as usize) >= 64 {
            return true; // QueenX 当前 cpuset 是 64-bit
        }
        let allowed = PROCESS_TABLE
            .with_process(pid, |p| p.cpuset_allowed.load(Ordering::Acquire))
            .unwrap_or(u64::MAX);
        (allowed >> cpu_id) & 1 == 1
    }

    /// C2: 为进程选择最合适的 CPU
    ///
    /// 策略: 在 allowed cpuset 中选 load 最低的 CPU.
    /// 单核: 直接返回当前 CPU.
    /// 找不到 allowed CPU: 返回 `hint_cpu` (退化路径, 调度器仍可工作).
    pub fn select_cpu_for(&self, pid: Pid, hint_cpu: u32) -> u32 {
        let cpu_count = crate::framework::smp::get_cpu_count();
        if cpu_count <= 1 {
            return hint_cpu.min(cpu_count.saturating_sub(1));
        }

        // 优先尝试 hint_cpu (通常为当前 CPU, 缓存亲和)
        if self.is_cpu_allowed(pid, hint_cpu) {
            return hint_cpu;
        }

        // 在 allowed cpuset 中选 load 最低的 CPU
        let mut best_cpu = hint_cpu;
        let mut best_load: u64 = u64::MAX;
        for cpu in 0..cpu_count {
            if !self.is_cpu_allowed(pid, cpu) {
                continue;
            }
            let sched = per_cpu_for(cpu);
            let load = sched.cfs_rq.lock().total_weight.load(Ordering::Acquire);
            if load < best_load {
                best_load = load;
                best_cpu = cpu;
            }
        }
        best_cpu
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    fn total_runnable_for(&self, cpu_id: u32) -> usize {
        let sched = per_cpu_for(cpu_id);
        let mut count = sched.cfs_rq.lock().nr_running as usize;
        count += sched.dl_rq.lock().nr_running as usize;
        count += sched.rt_queue.lock().len();
        // 统计运行中的任务
        if sched.current.load(Ordering::SeqCst) != 0 {
            count += 1;
        }
        count
    }

    pub fn load_balance(&self) {
        let cpu_count = crate::framework::smp::get_cpu_count();
        if cpu_count <= 1 {
            return;
        }

        let this_cpu = crate::framework::smp::get_current_cpu();
        let local_weight = {
            let sched = per_cpu_for(this_cpu);
            sched.cfs_rq.lock().total_weight.load(Ordering::Acquire)
        };

        let mut max_weight: u64 = 0;
        let mut busiest_cpu: u32 = this_cpu;

        for cpu in 0..cpu_count {
            if cpu == this_cpu {
                continue;
            }
            let w = {
                let sched = per_cpu_for(cpu);
                sched.cfs_rq.lock().total_weight.load(Ordering::Acquire)
            };
            if w > max_weight {
                max_weight = w;
                busiest_cpu = cpu;
            }
        }

        // 检查最忙 CPU 是否显著更忙 (按权重)
        if max_weight < local_weight.saturating_add(LOAD_BALANCE_THRESHOLD) {
            return;
        }

        // 从最忙 CPU 偷取任务
        let mut tasks_to_migrate: [Pid; 4] = [0; 4];
        let mut count = 0;
        {
            let mut src_rq = per_cpu_for(busiest_cpu).cfs_rq.lock();
            for _ in 0..4 {
                match src_rq.pick_next() {
                    Some((pid, vr)) => {
                        let weight = PROCESS_TABLE
                            .with_process(pid, |p| p.cfs_weight.load(Ordering::Acquire))
                            .unwrap_or(NICE0_WEIGHT);
                        src_rq.dequeue(pid, vr, weight);
                        tasks_to_migrate[count] = pid;
                        count += 1;
                    }
                    None => break,
                }
            }
        }

        let mut dst_rq = per_cpu_for(this_cpu).cfs_rq.lock();
        for i in 0..count {
            let pid = tasks_to_migrate[i];
            // C2: 亲和性检查 — 目标 CPU (this_cpu) 必须在进程 allowed 集合中,
            // 否则跳过该进程, 留给后续在 allowed CPU 上调度
            if !self.is_cpu_allowed(pid, this_cpu) {
                // 放回源队列 (避免丢失)
                let vr = PROCESS_TABLE
                    .with_process(pid, |p| p.cfs_vruntime.load(Ordering::Acquire))
                    .unwrap_or(0);
                let weight = PROCESS_TABLE
                    .with_process(pid, |p| p.cfs_weight.load(Ordering::Acquire))
                    .unwrap_or(NICE0_WEIGHT);
                drop(dst_rq);
                per_cpu_for(busiest_cpu)
                    .cfs_rq
                    .lock()
                    .enqueue(pid, vr, weight);
                dst_rq = per_cpu_for(this_cpu).cfs_rq.lock();
                continue;
            }
            let (vr, weight) = PROCESS_TABLE
                .with_process(pid, |p| {
                    (
                        p.cfs_vruntime.load(Ordering::Acquire),
                        p.cfs_weight.load(Ordering::Acquire),
                    )
                })
                .unwrap_or((0, NICE0_WEIGHT));
            dst_rq.enqueue(pid, vr, weight);
        }
    }

    /// 为 PWM 设置 CPU 配额. 调用方必须持有 `SYSTEM_CAP_QUOTA_ADMIN`.
    pub fn set_quota(&self, pwm: u64, max_runtime: u64, period: u64) {
        let mut quotas = self.quotas.lock();
        let now = TICK_COUNT.load(Ordering::SeqCst);
        for q in quotas.iter_mut() {
            if q.used && q.pwm == pwm {
                q.max_runtime = max_runtime;
                q.period = period;
                q.consumed = 0;
                q.next_reset = now + period;
                return;
            }
        }
        for q in quotas.iter_mut() {
            if !q.used {
                q.used = true;
                q.pwm = pwm;
                q.max_runtime = max_runtime;
                q.period = period;
                q.consumed = 0;
                q.next_reset = now + period;
                return;
            }
        }
    }

    /// Remove CPU quota for a PWM
    pub fn remove_quota(&self, pwm: u64) {
        let mut quotas = self.quotas.lock();
        for q in quotas.iter_mut() {
            if q.used && q.pwm == pwm {
                q.used = false;
                q.pwm = 0;
                return;
            }
        }
    }

    /// 设置 PWM 的进程数上限.
    pub fn set_limit(&self, pwm: u64, max_procs: u32) {
        let mut limits = self.limits.lock();
        for l in limits.iter_mut() {
            if l.used && l.pwm == pwm {
                l.max_procs = max_procs;
                return;
            }
        }
        for l in limits.iter_mut() {
            if !l.used {
                l.used = true;
                l.pwm = pwm;
                l.max_procs = max_procs;
                l.current = 0;
                return;
            }
        }
    }

    /// 进程退出时递减进程计数 (由 `exit()` 调用)
    fn dec_limit(&self, pwm: u64) {
        if pwm == 0 {
            return;
        }
        let mut limits = self.limits.lock();
        for l in limits.iter_mut() {
            if l.used && l.pwm == pwm && l.current > 0 {
                l.current -= 1;
                return;
            }
        }
    }
}

pub static SCHEDULER: Scheduler = Scheduler::new();

pub static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);

pub fn init() {
    SCHEDULER.init();
    SCHEDULER_READY.store(true, Ordering::Release);
}
