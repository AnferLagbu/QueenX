#![deny(unsafe_code)]
//! Core Dump — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
//! - 委托 framework/proc/coredump 实际完成 core dump 生成
//! - 提供查询接口: `coredump_allowed` / `coredump_limit`

use crate::framework::proc::coredump as fw;

/// 检查当前进程是否允许生成 core dump (`RLIMIT_CORE` > 0)
pub fn coredump_allowed() -> bool {
    fw::coredump_allowed()
}

/// 获取当前进程的 core dump 大小限制
pub fn coredump_limit() -> u64 {
    fw::coredump_limit()
}
