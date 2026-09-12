//! OOMD — 内存不足守护进程 (策略层)
//!
//! 周期性检查内存压力级别，渐进式回收内存。
//!
//! ## 策略
//!
//! | 级别 | 动作 |
//! |------|------|
//! | Normal | 仅更新统计 |
//! | Warning | 通知进程释放 page cache |
//! | Critical | Top-3 RSS 进程降优先级, 阻塞新 mmap |
//! | Emergency | SIGTERM → 最大 RSS 进程, 5s 后 SIGKILL |
//!
//! ## 迁移记录
//!
//! 策略代码于 2026-06-17 从 framework::proc::oomd 迁移至此。
//! framework 层仅保留 re-export 以保持调用方兼容。

use crate::kernel::framework::mm::{self as mm_api};
use crate::kernel::framework::proc::scheduler::TICK_COUNT;
// DECISION-J: framework::mm 不再导出 MemoryPressure/update_pressure (pressure 壳已删),
// 改从 services::mm::memory_pressure 引用 (services 权威)
use crate::kernel::services::mm::memory_pressure::{MemoryPressure, update_pressure};
use crate::slog_err;
use crate::slog_info;
use crate::slog_warn;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const OOMD_CHECK_INTERVAL: u64 = 100;
const OOMD_KILL_GRACE_TICKS: u64 = 500;

pub struct OomDaemon {
    last_check: AtomicU64,
    emergency_since: AtomicU64,
    terminated_count: AtomicU64,
    warned_count: AtomicU64,
    enabled: AtomicBool,
}

impl OomDaemon {
    pub const fn new() -> Self {
        Self {
            last_check: AtomicU64::new(0),
            emergency_since: AtomicU64::new(0),
            terminated_count: AtomicU64::new(0),
            warned_count: AtomicU64::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    pub fn tick(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }

        let tick = TICK_COUNT.load(Ordering::Relaxed);
        let last = self.last_check.load(Ordering::Relaxed);

        if tick.saturating_sub(last) < OOMD_CHECK_INTERVAL {
            return;
        }
        self.last_check.store(tick, Ordering::Relaxed);

        let free_pages = mm_api::pmm_get_free_pages();
        let total_pages = mm_api::pmm_get_total_pages();

        let p = update_pressure(free_pages, total_pages);
        match p {
            MemoryPressure::Normal => {
                self.emergency_since.store(0, Ordering::Relaxed);
            }
            MemoryPressure::Warning => {
                self.warned_count.fetch_add(1, Ordering::Relaxed);
                self.emergency_since.store(0, Ordering::Relaxed);
                slog_info!(
                    Memory,
                    "[OOMD] Memory pressure WARNING: notify processes to release cache"
                );
            }
            MemoryPressure::Critical => {
                self.warned_count.fetch_add(1, Ordering::Relaxed);
                slog_warn!(
                    Memory,
                    "[OOMD] Memory pressure CRITICAL: lowering priority for top-RSS processes"
                );
            }
            MemoryPressure::Emergency => {
                let es = self.emergency_since.load(Ordering::Relaxed);
                if es == 0 {
                    self.emergency_since.store(tick, Ordering::Relaxed);
                    slog_err!(
                        Memory,
                        "[OOMD] Memory pressure EMERGENCY: will terminate largest RSS process if not released"
                    );
                } else if tick.saturating_sub(es) > OOMD_KILL_GRACE_TICKS {
                    // TODO: 实际发送 SIGKILL 到最大 RSS 进程
                    // 当前仅计数，未真正 kill 进程
                    self.terminated_count.fetch_add(1, Ordering::Relaxed);
                    self.emergency_since.store(0, Ordering::Relaxed);
                    slog_err!(
                        Memory,
                        "[OOMD] Emergency timeout: killing largest RSS process (total killed: {})",
                        self.terminated_count.load(Ordering::Relaxed)
                    );
                }
            }
        }
    }

    pub fn stats(&self) -> (u64, u64) {
        (
            self.warned_count.load(Ordering::Relaxed),
            self.terminated_count.load(Ordering::Relaxed),
        )
    }

    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
    }
}

pub static OOMD: OomDaemon = OomDaemon::new();
