//! 调度器常量 (SCHED_\*) — framework 机制常量
//!
//! ## DECISION-J 归属反转记录 (2026-09-12)
//!
//! 原 SCHED_\* 常量于 T6-9 (2026-06-16) 迁至 `services::config::sched`, 本文件仅
//! re-export。按"机制持有的数据结构/常量归 framework"统一判据反转：`SCHED_*`
//! 被 framework proc 调度机制 (user_proc/scheduler_ex) 直接消费 — 属机制常量,
//! 迁回。`CFS_*` 仅被 services proc/sched_policy 策略消费, 留在 services。
//!
//! services 侧改 CFS_* 本地保留 + re-export SCHED_* 保持 API 兼容。
//! 本文件 0 unsafe (纯常量).

// ============================================================================
// 通用调度器常量
// ============================================================================

/// 调度级别 0 量子 (最高优先级, 实时).
pub const SCHED_LEVEL_0_QUANTUM: u32 = 80;

/// 调度级别 1 量子.
pub const SCHED_LEVEL_1_QUANTUM: u32 = 60;

/// 调度级别 2 量子.
pub const SCHED_LEVEL_2_QUANTUM: u32 = 40;

/// 调度级别 3 量子 (最低优先级, idle).
pub const SCHED_LEVEL_3_QUANTUM: u32 = 20;

/// 调度器提升检查间隔 (tick 数).
pub const SCHED_BOOST_INTERVAL: u64 = 1000;

/// 实时调度器看门狗超时 (tick 数).
pub const SCHED_RT_WATCHDOG_TICKS: u64 = 500;
