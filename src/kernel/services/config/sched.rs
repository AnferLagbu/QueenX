#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯常量 + re-export。
//! 调度器常量 — services 侧 CFS 策略常量 + SCHED re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-12)
//!
//! 原 CFS_*/SCHED_* 常量于 T6-9 (2026-06-16) 迁至此处。按统一判据"机制持有的
//! 数据结构/常量归 framework"反转：`SCHED_*` 被 framework proc 调度机制消费,
//! 迁回 `framework/config/sched.rs` (经其顶层 re-export 显式转发); `CFS_*` 仅被
//! services proc/sched_policy 策略消费, 留在此处。保持 services 侧 API 兼容
//! (services→framework 合法方向)。

pub use crate::kernel::framework::config::{
    SCHED_BOOST_INTERVAL, SCHED_LEVEL_0_QUANTUM, SCHED_LEVEL_1_QUANTUM, SCHED_LEVEL_2_QUANTUM,
    SCHED_LEVEL_3_QUANTUM, SCHED_RT_WATCHDOG_TICKS,
};

// ============================================================================
// CFS (Completely Fair Scheduler) 常量
// ============================================================================

/// CFS 目标延迟 (调度器 tick 数).
pub const CFS_TARGET_LATENCY: u64 = 60;

/// CFS 最小粒度 (调度器 tick 数).
pub const CFS_MIN_GRANULARITY: u64 = 8;

/// CFS 提升检查间隔 (调度器 tick 数).
pub const CFS_BOOST_INTERVAL: u64 = 1000;

/// CFS 默认 nice=0 任务的权重.
pub const CFS_NICE0_WEIGHT: u64 = 1024;

// ============================================================================
// 截止期调度 (EDF + CBS)
// ============================================================================

/// Deadline 调度器最小运行时间 (tick 数).
pub const CFS_DL_MIN_RUNTIME: u64 = 1;

/// Deadline 调度器最小周期 (tick 数).
pub const CFS_DL_MIN_PERIOD: u64 = 10;

/// Deadline 调度器最大利用率 (百分比).
pub const CFS_DL_MAX_UTILIZATION_PCT: u64 = 95;
