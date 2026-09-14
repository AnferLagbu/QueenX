//! Syscall 子系统初始化 — framework TCB
//!
//! ## 职责
//!
//! 这是 services 层与 `kernel::syscall::syscall_init` 之间的**唯一** unsafe 边界。
//! services 层 0 unsafe。

use crate::framework::syscall;

/// 初始化 syscall 子系统 (仅 framework 层: MSR/STAR/LSTAR 配置)
///
/// # Safety
///
/// 启动阶段单线程调用, 内部仅输出日志。
pub fn syscall_init() {
    // SAFETY: klog_write 是 C-ABI 日志函数; 启动阶段单线程
    unsafe { syscall::syscall_init() }
}
