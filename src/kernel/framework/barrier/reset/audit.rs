//! # 恢复审计日志

use super::config::RecoveryLayer;
use super::config::RecoveryResult;

use crate::kernel::framework::sync::IrqSpinLock;
pub const MAX_AUDIT_ENTRIES: usize = 16;

#[derive(Debug)]
pub struct ResetAuditLog {
    pub entries: [ResetAuditEntry; MAX_AUDIT_ENTRIES],
    pub count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ResetAuditEntry {
    pub tick: u64,
    pub layer: RecoveryLayer,
    pub result: RecoveryResult,
    pub reason: u32,
    pub domain_id: u64,
    pub entries_rolled: usize,
}

impl ResetAuditEntry {
    pub const fn empty() -> Self {
        Self {
            tick: 0,
            layer: RecoveryLayer::Layer1,
            result: RecoveryResult::Success,
            reason: 0,
            domain_id: 0,
            entries_rolled: 0,
        }
    }
}

impl ResetAuditLog {
    pub const fn new() -> Self {
        Self {
            entries: [ResetAuditEntry::empty(); MAX_AUDIT_ENTRIES],
            count: 0,
        }
    }

    pub fn record(
        &mut self,
        tick: u64,
        layer: RecoveryLayer,
        result: RecoveryResult,
        reason: u32,
        domain_id: u64,
        entries_rolled: usize,
    ) {
        if self.count >= MAX_AUDIT_ENTRIES {
            for i in 0..MAX_AUDIT_ENTRIES - 1 {
                self.entries[i] = self.entries[i + 1];
            }
            self.count = MAX_AUDIT_ENTRIES - 1;
        }
        self.entries[self.count] = ResetAuditEntry {
            tick,
            layer,
            result,
            reason,
            domain_id,
            entries_rolled,
        };
        self.count += 1;
    }

    pub fn record_simple(
        &mut self,
        tick: u64,
        layer: RecoveryLayer,
        result: RecoveryResult,
        reason: u32,
    ) {
        self.record(tick, layer, result, reason, 0, 0);
    }

    pub fn last(&self) -> Option<&ResetAuditEntry> {
        if self.count > 0 {
            Some(&self.entries[self.count - 1])
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.count = 0;
    }

    pub fn count_by_layer(&self, layer: RecoveryLayer) -> usize {
        self.entries[..self.count]
            .iter()
            .filter(|e| e.layer == layer)
            .count()
    }

    pub fn count_by_result(&self, result: RecoveryResult) -> usize {
        self.entries[..self.count]
            .iter()
            .filter(|e| e.result == result)
            .count()
    }
}

pub static RESET_AUDIT_LOG: IrqSpinLock<ResetAuditLog> = IrqSpinLock::new(ResetAuditLog::new());

pub fn audit_record(layer: RecoveryLayer, result: RecoveryResult, reason: u32) {
    use super::config::RECOVERY_CONFIG;

    if !RECOVERY_CONFIG.audit_enabled {
        return;
    }
    let tick = crate::kernel::framework::timer::get_ticks();
    let mut log = RESET_AUDIT_LOG.lock();
    log.record_simple(tick, layer, result, reason);
}

pub fn audit_record_domain(
    layer: RecoveryLayer,
    result: RecoveryResult,
    reason: u32,
    domain_id: u64,
    entries_rolled: usize,
) {
    use super::config::RECOVERY_CONFIG;

    if !RECOVERY_CONFIG.audit_enabled {
        return;
    }
    let tick = crate::kernel::framework::timer::get_ticks();
    let mut log = RESET_AUDIT_LOG.lock();
    log.record(tick, layer, result, reason, domain_id, entries_rolled);
}

pub fn audit_get_last() -> Option<ResetAuditEntry> {
    let log = RESET_AUDIT_LOG.lock();
    log.last().copied()
}

pub fn audit_clear() {
    let mut log = RESET_AUDIT_LOG.lock();
    log.clear();
}

#[cfg(feature = "kernel_test")]
pub mod tests {
    // J-01 (2026-09-08): wildcard_imports 清理 — 显式列出本模块使用的 super 符号
    use super::{RecoveryLayer, RecoveryResult, ResetAuditLog};

    pub fn test_audit_log() -> bool {
        let mut log = ResetAuditLog::new();
        log.record_simple(100, RecoveryLayer::Layer1, RecoveryResult::Success, 0);
        log.record_simple(200, RecoveryLayer::Layer2, RecoveryResult::Escalate, 1);
        log.count == 2
    }

    pub fn test_audit_count_by_layer() -> bool {
        let mut log = ResetAuditLog::new();
        log.record_simple(100, RecoveryLayer::Layer1, RecoveryResult::Success, 0);
        log.record_simple(200, RecoveryLayer::Layer2, RecoveryResult::Success, 0);
        log.record_simple(300, RecoveryLayer::Layer1, RecoveryResult::Failed, 1);

        log.count_by_layer(RecoveryLayer::Layer1) == 2
            && log.count_by_layer(RecoveryLayer::Layer2) == 1
    }
}
