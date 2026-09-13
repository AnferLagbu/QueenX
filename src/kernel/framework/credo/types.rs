#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯类型定义和常量。
//! Credo v1 类型定义 — framework 层权威定义
//!
//! ## 归属记录
//!
//! 纯数据定义 (PWM 类型/能力矩阵/身份条目/审计类型), 0 unsafe.
//! T6-7 曾于 2026-06-16 提取到 services; 第二十五批 (DECISION-K 项 5 credo
//! 判据: 持有类型归 framework — framework/proc/process.rs 进程表机制持有
//! PwmContext) 反转归位本文件. services/credo/types.rs 为 re-export 壳.
//!
//! Credo 权限模型的核心数据结构.
//! 域身份 (DID) + 能力矩阵 + 身份条目 + 审计类型.
//! Credo: 密码决定身份 | 无预设特权 | 能力来自授予

use core::sync::atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64};

pub const MAX_PWM_ENTRIES: usize = 256;
pub const PWM_NOTE_LEN: usize = 64;
pub const PWM_HASH_LEN: usize = 48;
pub const PWM_SALT_LEN: usize = 16;
pub const PWM_DIGEST_LEN: usize = 32;
pub const MAX_GRANT_RECORDS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PwmId(pub u64);

impl PwmId {
    pub const ZERO: Self = Self(0);

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for PwmId {
    fn default() -> Self {
        Self::ZERO
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct DomainId(pub u64);

impl DomainId {
    pub const ZERO: Self = Self(0);
    pub const KERNEL: Self = Self(1);
    pub const ROOT: Self = Self(1000);
    pub const NOBODY: Self = Self(65534);

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn from_uid(uid: u32) -> Self {
        Self(u64::from(uid))
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn to_uid(&self) -> u32 {
        self.0 as u32
    }
}

impl Default for DomainId {
    fn default() -> Self {
        Self::ZERO
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DomainFlags: u32 {
        const NONE       = 0;
        const NO_FORK    = 1 << 0;
        const NO_EXEC    = 1 << 1;
        const NO_NET     = 1 << 2;
        const NO_DEVICE  = 1 << 3;
        const SANDBOX    = 1 << 4;
        const READONLY   = 1 << 5;
        const TEMP       = 1 << 6;
        const SYSTEM     = 1 << 7;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CapDomain(pub u16);

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

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn as_usize(&self) -> usize {
        (self.0 as usize) % 16
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

impl From<u16> for CapDomain {
    fn from(v: u16) -> Self {
        Self(v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CapBits(pub u64);

impl CapBits {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(0xFFFFFFFFFFFFFFFF);

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "DECISION-043 pedantic 兜底: 当前批量 expect 兑底; 后续可逐处手工重构 (改 .cast() / let-else / 命名等)"
    )]
    pub fn as_u64(&self) -> u64 {
        self.0
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

impl core::ops::BitOrAssign for CapBits {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl core::ops::BitAndAssign for CapBits {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl core::ops::Not for CapBits {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

bitflags::bitflags! {
    #[repr(transparent)]
    #[derive(Clone, Copy, Debug)]
    pub struct PwmFlags: u16 {
        const NONE       = 0;
        const DISABLED   = 1 << 0;
        const MODIFIED   = 1 << 3;
        const LOCKED     = 1 << 4;
    }
}

impl Default for PwmFlags {
    fn default() -> Self {
        Self::NONE
    }
}

#[repr(C)]
pub struct PwmEntry {
    pub pwm: AtomicU64,
    pub posix_uid: AtomicU32,
    pub posix_gid: AtomicU32,
    pub creator_pwm: AtomicU64,
    pub privilege_level: AtomicU8,
    pub flags: AtomicU16,
    pub caps: [AtomicU64; 16],
    /// 备注 (T4-1 全 Atomic 化: u8 数组 → `AtomicU8` 数组, 支持 &self 写入)
    pub note: [AtomicU8; PWM_NOTE_LEN],
    /// 密码哈希 (T4-1 全 Atomic 化: u8 数组 → `AtomicU8` 数组, 支持 &self 写入)
    pub password_hash: [AtomicU8; PWM_HASH_LEN],
    pub created_time: AtomicU64,
    pub expires_at: AtomicU64,
    pub lockout_until: AtomicU64,
    pub failed_attempts: AtomicU32,
    pub last_login_time: AtomicU64,
}

impl Default for PwmEntry {
    fn default() -> Self {
        Self {
            pwm: AtomicU64::new(0),
            posix_uid: AtomicU32::new(0),
            posix_gid: AtomicU32::new(0),
            creator_pwm: AtomicU64::new(0),
            privilege_level: AtomicU8::new(0xFF),
            flags: AtomicU16::new(0),
            caps: [0; 16].map(AtomicU64::new),
            note: [const { AtomicU8::new(0) }; PWM_NOTE_LEN],
            password_hash: [const { AtomicU8::new(0) }; PWM_HASH_LEN],
            created_time: AtomicU64::new(0),
            expires_at: AtomicU64::new(0),
            lockout_until: AtomicU64::new(0),
            failed_attempts: AtomicU32::new(0),
            last_login_time: AtomicU64::new(0),
        }
    }
}

impl PwmEntry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_valid(&self) -> bool {
        self.pwm.load(core::sync::atomic::Ordering::Acquire) != 0
    }

    pub fn get_pwm(&self) -> PwmId {
        PwmId(self.pwm.load(core::sync::atomic::Ordering::Acquire))
    }

    pub fn get_creator_pwm(&self) -> PwmId {
        PwmId(self.creator_pwm.load(core::sync::atomic::Ordering::Acquire))
    }

    pub fn get_flags(&self) -> PwmFlags {
        PwmFlags::from_bits_truncate(self.flags.load(core::sync::atomic::Ordering::Acquire))
    }

    pub fn set_flags(&self, flags: PwmFlags) {
        self.flags
            .store(flags.bits(), core::sync::atomic::Ordering::Release);
    }

    pub fn add_flags(&self, flags: PwmFlags) {
        let current = self.get_flags();
        self.set_flags(current | flags);
    }

    pub fn remove_flags(&self, flags: PwmFlags) {
        let current = self.get_flags();
        self.set_flags(current & !flags);
    }

    pub fn has_flag(&self, flag: PwmFlags) -> bool {
        self.get_flags().contains(flag)
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    /// T4-1: 全 Atomic 化后此 API 行为变化.
    /// 原因: [`AtomicU8`; N] 不能直接借用为 &[u8], 返回 owned &str 需要内部静态缓冲.
    /// 当前实现: 返回静态空串占位. 推荐使用 `note_bytes()` 复制 + 自行转换.
    /// 兼容保留: 签名不变, 行为退化为"返回空串 (新值前为 None)". 调用方应迁移.
    pub fn get_note_str(&self) -> &'static str {
        // T4-1: 静态生命周期 &str, 仅占位 (此 API 已废弃, 推荐 note_bytes/note_equals)
        ""
    }

    /// T4-1: 复制 note 到 owned 数组 (替代 `get_note_str` 的 &str 返回)
    pub fn note_bytes(&self) -> [u8; PWM_NOTE_LEN] {
        let mut buf = [0u8; PWM_NOTE_LEN];
        for (i, slot) in self.note.iter().enumerate() {
            buf[i] = slot.load(core::sync::atomic::Ordering::Acquire);
        }
        buf
    }

    /// T4-1: 全 Atomic 化后, `set_note` 改用原子字节写入, 接受 &self.
    pub fn set_note(&self, note: &str) {
        let bytes = note.as_bytes();
        let len = bytes.len().min(PWM_NOTE_LEN - 1);
        for i in 0..len {
            self.note[i].store(bytes[i], core::sync::atomic::Ordering::Release);
        }
        self.note[len].store(0, core::sync::atomic::Ordering::Release);
    }

    /// T4-1: 比较备注是否相等 (避免 &str 生命周期问题)
    pub fn note_equals(&self, other: &str) -> bool {
        let buf = self.note_bytes();
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        raw::bytes_to_str(&buf[..len]) == other
    }

    pub fn load_caps(&self, domain: CapDomain) -> CapBits {
        let idx = domain.as_usize();
        CapBits(self.caps[idx].load(core::sync::atomic::Ordering::Acquire))
    }

    pub fn store_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = domain.as_usize();
        self.caps[idx].store(caps.0, core::sync::atomic::Ordering::Release);
    }

    pub fn fetch_or_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = domain.as_usize();
        self.caps[idx].fetch_or(caps.0, core::sync::atomic::Ordering::AcqRel);
    }

    pub fn fetch_and_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = domain.as_usize();
        self.caps[idx].fetch_and(caps.0, core::sync::atomic::Ordering::AcqRel);
    }

    pub fn has_capability(&self, domain: CapDomain, required: CapBits) -> bool {
        let current = self.load_caps(domain);
        current.contains(required)
    }

    pub fn get_uid(&self) -> u32 {
        self.posix_uid.load(core::sync::atomic::Ordering::Acquire)
    }

    pub fn get_gid(&self) -> u32 {
        self.posix_gid.load(core::sync::atomic::Ordering::Acquire)
    }

    pub fn set_uid(&self, uid: u32) {
        self.posix_uid
            .store(uid, core::sync::atomic::Ordering::Release);
    }

    pub fn set_gid(&self, gid: u32) {
        self.posix_gid
            .store(gid, core::sync::atomic::Ordering::Release);
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct PwmContext {
    pub current_entry: *const PwmEntry,
    pub session_pwm: PwmId,
    pub cached_uid: u32,
    pub cached_gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub saved_euid: u32,
    pub saved_egid: u32,
    pub active_domain_id: DomainId,
    pub elevation_granted_pwm: PwmId,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct GrantRecord {
    pub grantor_pwm: PwmId,
    pub grantee_pwm: PwmId,
    pub domain: CapDomain,
    pub caps: CapBits,
    pub granted_at: u64,
}

impl GrantRecord {
    pub const EMPTY: Self = Self {
        grantor_pwm: PwmId::ZERO,
        grantee_pwm: PwmId::ZERO,
        domain: CapDomain(0),
        caps: CapBits::NONE,
        granted_at: 0,
    };

    pub fn is_empty(&self) -> bool {
        self.grantor_pwm == PwmId::ZERO
    }
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwmError {
    Ok = 0,
    NotFound = -1,
    Disabled = -2,
    PasswordIncorrect = -3,
    PermissionDenied = -4,
    TableFull = -5,
    AlreadyExists = -6,
    InsufficientPrivilege = -7,
    NotAuthorized = -8,
    NotCreator = -9,
    WouldBreakFloor = -10,
    PrivilegeOverflow = -11,
    TokenUsed = -12,
    NoFirstToken = -13,
    InvalidPassword = -14,
}

impl PwmError {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default)]
pub enum AuditAction {
    #[default]
    Login = 1,
    Logout = 2,
    Create = 3,
    Delete = 4,
    Modify = 5,
    PasswordChange = 8,
    Grant = 10,
    Revoke = 11,
    TransferCreator = 12,
    FirstTokenGrant = 13,
}

impl AuditAction {
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default)]
pub enum AuditResult {
    #[default]
    Success = 0,
    Failure = 1,
    Denied = 2,
}

impl AuditResult {
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub pwm: PwmId,
    pub action: AuditAction,
    pub result: AuditResult,
    pub target_pwm: PwmId,
    pub details: u64,
}

// ============================================================================
// 特权子模块 (Framekernel raw): 集中不安全转换
// ============================================================================

pub(crate) mod raw {
    /// 字节切片 → &str (safe 版本)
    /// 合法来源: `set_note()` 写入的字符串已经是合法 UTF-8。
    /// T6-7: 从 `from_utf8_unchecked` 改为 `from_utf8`, 消除唯一 unsafe,
    /// 使 types.rs 可迁移到 services 层.
    pub fn bytes_to_str(bytes: &[u8]) -> &str {
        core::str::from_utf8(bytes).unwrap_or("")
    }
}

// ============================================================================
// 单元测试 — REVAL-5 T4-2: CapabilityMatrix 路径契约
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    #[test]
    fn pwm_entry_default_is_invalid() {
        let e = PwmEntry::default();
        // pwm == 0 表示未分配, is_valid() 应返 false
        assert!(!e.is_valid());
        assert_eq!(e.get_pwm(), PwmId(0));
    }

    #[test]
    fn pwm_entry_valid_after_pwm_set() {
        let e = PwmEntry::default();
        e.pwm.store(0xABCD, Ordering::Release);
        assert!(e.is_valid());
        assert_eq!(e.get_pwm(), PwmId(0xABCD));
    }

    #[test]
    fn pwm_entry_caps_load_store() {
        let e = PwmEntry::default();
        // 初始 0
        assert_eq!(e.load_caps(CapDomain(0)), CapBits::NONE);
        // 写入域 0
        e.store_caps(CapDomain(0), CapBits(0xFF));
        assert_eq!(e.load_caps(CapDomain(0)), CapBits(0xFF));
        // 其他域仍为 0 (隔离性)
        assert_eq!(e.load_caps(CapDomain(1)), CapBits::NONE);
    }

    #[test]
    fn pwm_entry_caps_fetch_or() {
        let e = PwmEntry::default();
        e.store_caps(CapDomain(0), CapBits(0b1100));
        e.fetch_or_caps(CapDomain(0), CapBits(0b0011));
        // OR 合并: 0b1100 | 0b0011 = 0b1111
        assert_eq!(e.load_caps(CapDomain(0)), CapBits(0b1111));
    }

    #[test]
    fn pwm_entry_caps_fetch_and() {
        let e = PwmEntry::default();
        e.store_caps(CapDomain(0), CapBits(0b1111));
        e.fetch_and_caps(CapDomain(0), CapBits(0b1010));
        // AND 屏蔽: 0b1111 & 0b1010 = 0b1010
        assert_eq!(e.load_caps(CapDomain(0)), CapBits(0b1010));
    }

    #[test]
    fn pwm_entry_has_capability_subset() {
        let e = PwmEntry::default();
        e.store_caps(CapDomain(0), CapBits(0b1111));
        // 子集检查: 拥有的位 ⊇ required
        assert!(e.has_capability(CapDomain(0), CapBits(0b1000)));
        assert!(e.has_capability(CapDomain(0), CapBits(0b0100)));
        assert!(e.has_capability(CapDomain(0), CapBits(0b1100)));
        // 超出拥有的位 → false
        assert!(!e.has_capability(CapDomain(0), CapBits(0b10000)));
    }

    #[test]
    fn pwm_entry_uid_gid_round_trip() {
        let e = PwmEntry::default();
        assert_eq!(e.get_uid(), 0);
        assert_eq!(e.get_gid(), 0);
        e.set_uid(1000);
        e.set_gid(100);
        assert_eq!(e.get_uid(), 1000);
        assert_eq!(e.get_gid(), 100);
    }

    #[test]
    fn pwm_entry_flags_lifecycle() {
        let e = PwmEntry::default();
        assert!(!e.has_flag(PwmFlags::DISABLED));
        e.add_flags(PwmFlags::DISABLED);
        assert!(e.has_flag(PwmFlags::DISABLED));
        e.remove_flags(PwmFlags::DISABLED);
        assert!(!e.has_flag(PwmFlags::DISABLED));
    }

    #[test]
    fn pwm_entry_set_note_round_trip() {
        let e = PwmEntry::default();
        e.set_note("admin");
        // T4-1: 用 note_equals 比较, 避免 &str 生命周期问题
        assert!(e.note_equals("admin"));
        assert!(!e.note_equals("root"));
    }

    #[test]
    fn pwm_context_default_fields() {
        let c = PwmContext::default();
        assert!(c.current_entry.is_null());
        assert_eq!(c.cached_uid, 0);
        assert_eq!(c.active_domain_id, DomainId(0));
    }
}
