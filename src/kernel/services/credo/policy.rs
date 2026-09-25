#![deny(unsafe_code)]
//! 身份与权限 — PWM/能力矩阵/会话 (services 层)
//!
//! ## 框内核中的 PWID 表达
//!
//! ```text
//! framework/credo/                  ← TCB (unsafe 允许)
//!   ├─ atomic_matrix.rs              ← 16×64 AtomicU64 物理存储
//!   ├─ password.rs                   ← SHA-256 + 常数时间比较
//!   └─ persist.rs                    ← 磁盘序列化
//!
//! services/credo/ (本模块)         ← 100% safe Rust
//!   ├─ policy.rs                     ← 能力检查策略 (本文件)
//!   ├─ grants.rs                     ← 委托规则
//!   ├─ sessions.rs                   ← 会话生命周期
//!   └─ audit.rs                      ← 审计日志生成
//! ```
//!
//! ## @SAFE
//! 本文件不含 `unsafe`。所有硬件交互通过 `framework::credo` 的安全 API。
//!
//! CI 由 `tools/check_tcb.sh` 通过 `grep -rP 'unsafe\s*(\{|fn |impl)'` 实际校验,
//! 不用编译期假属性 (历史上曾误用 `//! #![@SAFE]` 注释伪装, 已删除)。

use core::sync::atomic::{AtomicU64, Ordering};

/// 16 个能力域
pub const CAP_DOMAINS: usize = 16;
/// 每域 64 位
pub const CAP_BITS_PER_DOMAIN: u64 = 64;

/// 域 ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CapDomain(pub u8);

impl CapDomain {
    pub const SYSTEM: Self = Self(0);
    pub const FS: Self = Self(1);
    pub const NET: Self = Self(2);
    pub const PROC: Self = Self(3);
    pub const DEVICE: Self = Self(4);
    pub const USER_MGMT: Self = Self(5);
    pub const IPC: Self = Self(6);
    pub const MEM: Self = Self(7);
    pub const TIME: Self = Self(8);
    pub const BARRIER: Self = Self(9);
    pub const SIGNAL: Self = Self(10);
    pub const SHM: Self = Self(11);
    pub const SEM: Self = Self(12);
    pub const MSGQ: Self = Self(13);
    pub const DMA: Self = Self(14);
    pub const RESERVED: Self = Self(15);

    pub fn is_valid(self) -> bool {
        (self.0 as usize) < CAP_DOMAINS
    }
}

/// 能力位
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct CapBits(pub u64);

impl CapBits {
    pub const NONE: Self = Self(0);
    /// 全能力 (所有 64 位)
    pub const ALL: Self = Self(u64::MAX);

    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[expect(
        clippy::return_self_not_must_use,
        reason = "return_self_not_must_use: 返回 Self 是 builder/fluent API; 当前优先 expect"
    )]
    pub fn diff(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl core::ops::BitOr for CapBits {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for CapBits {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl core::ops::Not for CapBits {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

/// 策略结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyResult {
    /// 允许
    Allow,
    /// 拒绝 + 原因
    Deny(DenyReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    /// 域 ID 非法
    InvalidDomain,
    /// PWM 未找到
    UnknownPwm,
    /// 域内无此能力
    NoAuthority,
    /// 试图撤销可行下界
    FloorProtected,
    /// 域被禁用
    Disabled,
}

/// 委托结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantResult {
    Granted,
    NoAuthority,
    InvalidDomain,
    Empty,
}

/// 撤销结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeResult {
    Revoked,
    NoChange,
    InvalidDomain,
    FloorProtected,
}

/// 能力矩阵抽象 (services 调用 framework TCB)
///
/// `framework::credo::AtomicMatrix` 实现此 trait,
/// 通过 lock-free 路径提供能力读写。
pub trait CapabilityMatrix {
    /// 读取某域的能力位
    fn get(&self, domain: CapDomain) -> Option<CapBits>;
    /// 原子设置某域能力
    ///
    /// # Errors
    /// 当设置失败 (如原子操作冲突) 时返回 `Err(())`.
    fn set(&self, domain: CapDomain, bits: CapBits) -> Result<CapBits, ()>;
    /// 比较并交换 (用于 lock-free grant)
    ///
    /// # Errors
    /// 当比较并交换失败 (当前值已与 `current` 不一致) 时, 返回 `Err(实际当前值)`.
    fn compare_exchange(
        &self,
        domain: CapDomain,
        current: CapBits,
        new: CapBits,
    ) -> Result<CapBits, CapBits>;
}

/// 内存实现 (单线程 fallback, 测试用)
pub struct InMemoryMatrix {
    rows: [AtomicU64; CAP_DOMAINS],
}

/// 能力位图快照 (按域) — 用于快照/审计/降级
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapMatrix {
    bits: [u64; CAP_DOMAINS],
}

impl CapMatrix {
    /// 空能力位图 (所有域均为 NONE)
    pub const fn empty() -> Self {
        Self {
            bits: [0u64; CAP_DOMAINS],
        }
    }
    /// 全能力位图
    pub const fn all() -> Self {
        Self {
            bits: [u64::MAX; CAP_DOMAINS],
        }
    }
    /// 从域位图构造
    pub const fn from_bits(bits: [u64; CAP_DOMAINS]) -> Self {
        Self { bits }
    }
    /// 取某域位图
    pub fn get(&self, domain: CapDomain) -> CapBits {
        CapBits(self.bits[domain.0 as usize])
    }
}

impl InMemoryMatrix {
    pub const fn new() -> Self {
        Self {
            rows: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }
}

impl CapabilityMatrix for InMemoryMatrix {
    fn get(&self, domain: CapDomain) -> Option<CapBits> {
        if !domain.is_valid() {
            return None;
        }
        Some(CapBits(
            self.rows[domain.0 as usize].load(Ordering::Acquire),
        ))
    }

    fn set(&self, domain: CapDomain, bits: CapBits) -> Result<CapBits, ()> {
        if !domain.is_valid() {
            return Err(());
        }
        let old = self.rows[domain.0 as usize].swap(bits.0, Ordering::AcqRel);
        Ok(CapBits(old))
    }

    fn compare_exchange(
        &self,
        domain: CapDomain,
        current: CapBits,
        new: CapBits,
    ) -> Result<CapBits, CapBits> {
        if !domain.is_valid() {
            return Err(current);
        }
        self.rows[domain.0 as usize]
            .compare_exchange(current.0, new.0, Ordering::AcqRel, Ordering::Acquire)
            .map(CapBits)
            .map_err(CapBits)
    }
}

/// 可行性下界 (viability floor)
///
/// 系统保留的最低能力:
/// - FS: READ | EXEC
/// - PROC: 派生 (FORK) | 执行 (EXEC)
/// - `USER_MGMT`: LIST
pub const VIABLE_FLOOR: [u64; CAP_DOMAINS] = {
    let mut f = [0u64; CAP_DOMAINS];
    f[CapDomain::FS.0 as usize] = (1 << 0) | (1 << 2); // READ | EXEC
    f[CapDomain::PROC.0 as usize] = (1 << 0) | (1 << 1); // FORK | EXEC
    f[CapDomain::USER_MGMT.0 as usize] = 1 << 0; // LIST
    f
};

/// 策略引擎 (services 层)
pub struct PolicyEngine {
    // 当前实现: 无状态, 仅依赖全局常量 VIABLE_FLOOR
    // 未来可扩展: 域间约束表 / 临时禁令 / 时段限制
}

impl PolicyEngine {
    pub const fn new() -> Self {
        Self {}
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// 检查 (matrix, domain, required) 是否被允许
    pub fn check<M: CapabilityMatrix>(
        &self,
        matrix: &M,
        domain: CapDomain,
        required: CapBits,
    ) -> PolicyResult {
        if !domain.is_valid() {
            return PolicyResult::Deny(DenyReason::InvalidDomain);
        }
        if required.is_empty() {
            return PolicyResult::Allow;
        }
        let Some(owned) = matrix.get(domain) else {
            return PolicyResult::Deny(DenyReason::UnknownPwm);
        };
        if !owned.contains(required) {
            return PolicyResult::Deny(DenyReason::NoAuthority);
        }
        // 不能撤销可行下界 (仅对设有非零下界的域生效; 零下界域不受此约束)
        let floor = CapBits(VIABLE_FLOOR[domain.0 as usize]);
        if !floor.is_empty() && required.contains(floor) {
            return PolicyResult::Deny(DenyReason::FloorProtected);
        }
        PolicyResult::Allow
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// 委托 (使用 CAS 保证并发安全)
    pub fn grant<M: CapabilityMatrix>(
        &self,
        from: &M,
        to: &M,
        domain: CapDomain,
        bits: CapBits,
    ) -> GrantResult {
        if !domain.is_valid() {
            return GrantResult::InvalidDomain;
        }
        if bits.is_empty() {
            return GrantResult::Empty;
        }
        let Some(owned) = from.get(domain) else {
            return GrantResult::NoAuthority;
        };
        if !owned.contains(bits) {
            return GrantResult::NoAuthority;
        }
        // 循环 CAS 重试
        loop {
            let to_current = to.get(domain).unwrap_or(CapBits::NONE);
            let to_new = to_current | bits;
            match to.compare_exchange(domain, to_current, to_new) {
                Ok(_) => return GrantResult::Granted,
                Err(_) => {} // 重试
            }
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// 撤销 (保护可行下界)
    pub fn revoke<M: CapabilityMatrix>(
        &self,
        matrix: &M,
        domain: CapDomain,
        bits: CapBits,
    ) -> RevokeResult {
        if !domain.is_valid() {
            return RevokeResult::InvalidDomain;
        }
        if bits.is_empty() {
            return RevokeResult::NoChange;
        }
        let floor = CapBits(VIABLE_FLOOR[domain.0 as usize]);
        let revocable = bits.diff(floor);
        if revocable.is_empty() {
            return RevokeResult::FloorProtected;
        }
        loop {
            let current = matrix.get(domain).unwrap_or(CapBits::NONE);
            let new = current.diff(revocable);
            match matrix.compare_exchange(domain, current, new) {
                Ok(_) => return RevokeResult::Revoked,
                Err(_) => {}
            }
        }
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_matrix() -> InMemoryMatrix {
        InMemoryMatrix::new()
    }

    #[test]
    fn policy_basic_check() {
        let m = make_matrix();
        m.set(CapDomain::FS, CapBits(0xFF)).unwrap();

        let p = PolicyEngine::new();
        // 0x08 (bit3) 不含 FS 的可行性下界 (READ|EXEC = 0b101), 故仅需矩阵授权即 Allow
        // (原用 0x0F 含下界位, 必被 FloorProtected 拒绝; 2026-09-24 UT-06 修正)
        assert_eq!(
            p.check(&m, CapDomain::FS, CapBits(0x08)),
            PolicyResult::Allow
        );
        assert_eq!(
            p.check(&m, CapDomain::FS, CapBits(0x100)),
            PolicyResult::Deny(DenyReason::NoAuthority)
        );
    }

    #[test]
    fn policy_invalid_domain() {
        let m = make_matrix();
        let p = PolicyEngine::new();
        assert_eq!(
            p.check(&m, CapDomain(20), CapBits(0x01)),
            PolicyResult::Deny(DenyReason::InvalidDomain)
        );
    }

    #[test]
    fn policy_empty_required() {
        let m = make_matrix();
        let p = PolicyEngine::new();
        // 无要求即通过
        assert_eq!(
            p.check(&m, CapDomain::FS, CapBits::NONE),
            PolicyResult::Allow
        );
    }

    #[test]
    fn policy_floor_protection() {
        let m = make_matrix();
        m.set(CapDomain::FS, CapBits(0xFF)).unwrap();
        let p = PolicyEngine::new();
        // FS 底限 = READ | EXEC = 0b0101
        // 试图"撤销" 0b0101 应被 FloorProtected 拒绝
        // 实际语义: check(FS, required=0b0101) — 但 required=owned ⊆ owned 总是 true
        // 我们要求 required 包含 floor bits → 拒绝
        let result = p.check(&m, CapDomain::FS, CapBits(0b0101));
        assert_eq!(result, PolicyResult::Deny(DenyReason::FloorProtected));
    }

    #[test]
    fn policy_zero_floor_domain_allowed() {
        // 回归 (零下界域恒拒): 曾因 floor == 0 使 required.contains(floor) 恒真,
        // 导致 SYSTEM/NET 等 13 个零下界域一律被 FloorProtected 拒绝.
        let m = make_matrix();
        m.set(CapDomain::NET, CapBits(0x03)).unwrap();
        m.set(CapDomain::SYSTEM, CapBits(0x01)).unwrap();
        let p = PolicyEngine::new();
        assert_eq!(
            p.check(&m, CapDomain::NET, CapBits(0x03)),
            PolicyResult::Allow
        );
        assert_eq!(
            p.check(&m, CapDomain::SYSTEM, CapBits(0x01)),
            PolicyResult::Allow
        );
    }

    #[test]
    fn grant_basic() {
        let from = make_matrix();
        let to = make_matrix();
        from.set(CapDomain::FS, CapBits(0b1111)).unwrap();

        let p = PolicyEngine::new();
        let r = p.grant(&from, &to, CapDomain::FS, CapBits(0b1000));
        assert_eq!(r, GrantResult::Granted);
        assert_eq!(to.get(CapDomain::FS), Some(CapBits(0b1000)));
    }

    #[test]
    fn grant_no_authority() {
        let from = make_matrix();
        let to = make_matrix();
        let p = PolicyEngine::new();
        let r = p.grant(&from, &to, CapDomain::FS, CapBits(0b0001));
        assert_eq!(r, GrantResult::NoAuthority);
    }

    #[test]
    fn grant_invalid_domain() {
        let from = make_matrix();
        let to = make_matrix();
        let p = PolicyEngine::new();
        let r = p.grant(&from, &to, CapDomain(20), CapBits(0b0001));
        assert_eq!(r, GrantResult::InvalidDomain);
    }

    #[test]
    fn revoke_basic() {
        let m = make_matrix();
        m.set(CapDomain::FS, CapBits(0b1111)).unwrap();
        let p = PolicyEngine::new();
        let r = p.revoke(&m, CapDomain::FS, CapBits(0b1000));
        assert_eq!(r, RevokeResult::Revoked);
        assert_eq!(m.get(CapDomain::FS), Some(CapBits(0b0111)));
    }

    #[test]
    fn revoke_floor_protected() {
        let m = make_matrix();
        m.set(CapDomain::FS, CapBits(0b1111)).unwrap();
        let p = PolicyEngine::new();
        // 试图撤销 floor (READ|EXEC = 0b0101)
        let r = p.revoke(&m, CapDomain::FS, CapBits(0b0101));
        assert_eq!(r, RevokeResult::FloorProtected);
    }

    #[test]
    fn revoke_invalid_domain() {
        let m = make_matrix();
        let p = PolicyEngine::new();
        let r = p.revoke(&m, CapDomain(20), CapBits(0b0001));
        assert_eq!(r, RevokeResult::InvalidDomain);
    }

    #[test]
    fn revoke_empty() {
        let m = make_matrix();
        let p = PolicyEngine::new();
        let r = p.revoke(&m, CapDomain::FS, CapBits::NONE);
        assert_eq!(r, RevokeResult::NoChange);
    }

    /// 跨域独立性
    #[test]
    fn domains_independent() {
        let m = make_matrix();
        m.set(CapDomain::FS, CapBits::ALL).unwrap();
        m.set(CapDomain::NET, CapBits::ALL).unwrap();
        assert_eq!(m.get(CapDomain::FS), Some(CapBits::ALL));
        assert_eq!(m.get(CapDomain::NET), Some(CapBits::ALL));
        assert_eq!(m.get(CapDomain::PROC), Some(CapBits::NONE));
    }
}
