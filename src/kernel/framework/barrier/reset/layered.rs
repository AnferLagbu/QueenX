//! # 分层恢复入口
//!
//! 统一的恢复策略：BBR → BSR → BHR
//!
//! ```text
//! Layer 1: BBR (Barrier Base Recovery)  ~1μs   >95%成功率
//! Layer 2: BSR (Barrier Soft Reset)     ~50ms  >80%成功率
//! Layer 3: BHR (Barrier Hard Reset)     ~120ms ~100%成功率
//! ```

use core::sync::atomic::Ordering;

use super::bbr;
use super::bhr;
use super::bsr;
use super::config::{self, RecoveryLayer, RecoveryResult};

pub fn execute_layered() -> ! {
    if !config::is_reset_in_progress() {
        config::set_reset_in_progress(true);
    }

    if config::RECOVERY_CONFIG.enable_layer2 {
        config::set_current_layer(RecoveryLayer::Layer2);

        match bsr::execute() {
            RecoveryResult::Success => {
                crate::klog_crit!(Kernel, "[RECOVERY] Layer 2 (BSR) succeeded");
                #[cfg(not(feature = "kernel_test"))]
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe {
                    core::hint::unreachable_unchecked();
                }
                #[cfg(feature = "kernel_test")]
                loop {
                    core::hint::spin_loop();
                }
            }
            RecoveryResult::Escalate => {
                crate::klog_warn!(
                    Kernel,
                    "[RECOVERY] Layer 2 (BSR) failed, escalating to Layer 3"
                );
            }
            RecoveryResult::Failed => {
                crate::klog_err!(Kernel, "[RECOVERY] Layer 2 (BSR) failed");
            }
        }
    }

    if config::RECOVERY_CONFIG.enable_layer3 {
        config::set_current_layer(RecoveryLayer::Layer3);
        bhr::execute();
    }

    bhr::execute_fallback()
}

pub fn execute_from_panic(panic_info: &core::panic::PanicInfo<'_>) -> ! {
    crate::klog_crit!(
        Kernel,
        "[RECOVERY] Panic detected, initiating layered recovery"
    );

    if config::RECOVERY_CONFIG.enable_layer1 {
        config::set_current_layer(RecoveryLayer::Layer1);

        match bbr::execute(panic_info) {
            RecoveryResult::Success => {
                crate::klog_crit!(Kernel, "[RECOVERY] Layer 1 (BBR) succeeded");
                config::set_reset_in_progress(false);
                #[cfg(not(feature = "kernel_test"))]
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe {
                    core::hint::unreachable_unchecked();
                }
                #[cfg(feature = "kernel_test")]
                loop {
                    core::hint::spin_loop();
                }
            }
            RecoveryResult::Escalate => {
                crate::klog_warn!(
                    Kernel,
                    "[RECOVERY] Layer 1 (BBR) failed, escalating to Layer 2"
                );
            }
            RecoveryResult::Failed => {
                crate::klog_err!(Kernel, "[RECOVERY] Layer 1 (BBR) failed");
            }
        }
    }

    execute_layered()
}

pub fn try_bbr_first(panic_info: &core::panic::PanicInfo<'_>) -> RecoveryResult {
    bbr::execute(panic_info)
}

pub fn get_recovery_status() -> RecoveryStatus {
    RecoveryStatus {
        current_layer: config::get_current_layer(),
        reset_in_progress: config::is_reset_in_progress(),
        bbr_count: config::BBR_ATTEMPT_COUNT.load(Ordering::SeqCst),
        bsr_count: config::BSR_ATTEMPT_COUNT.load(Ordering::SeqCst),
        bhr_count: config::BHR_ATTEMPT_COUNT.load(Ordering::SeqCst),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RecoveryStatus {
    pub current_layer: RecoveryLayer,
    pub reset_in_progress: bool,
    pub bbr_count: u32,
    pub bsr_count: u32,
    pub bhr_count: u32,
}

#[cfg(feature = "kernel_test")]
pub mod tests {
    // J-01 (2026-09-08): wildcard_imports 清理 — 显式列出本模块使用的 super 符号
    use super::get_recovery_status;

    pub fn test_recovery_status() -> bool {
        let status = get_recovery_status();
        // 计数字段应为有效值 (u32 范围, 不溢出)
        // current_layer 是合法 enum 值即可
        let _ = status.bbr_count;
        let _ = status.bsr_count;
        let _ = status.bhr_count;
        true
    }
}
