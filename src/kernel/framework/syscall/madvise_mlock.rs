//! madvise / mlock / mincore — framework 层 re-export
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 实现在 `framework::proc::madvise_mlock` (机制项, 由 services 迁回), 本文件
//! re-export 其全部符号 (MADV_*/MCL_* 常量 + sys_* 入口) 保持 framework/syscall
//! 侧调用方兼容. 消除旧的 `services::mm::madvise_mlock` 反向依赖。

pub use crate::kernel::framework::proc::madvise_mlock::*;
