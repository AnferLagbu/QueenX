use super::check;
use crate::framework::credo::capability;
use crate::framework::credo::engine;
use crate::framework::credo::sha256;
use crate::framework::credo::types::{
    AuditAction, AuditEntry, AuditResult, CapBits, CapDomain, GrantRecord, PwmEntry, PwmFlags,
    PwmId,
};
use crate::framework::tests::{TestResult, runner};
use crate::register_tests_inner;

fn test_sha256_vectors() -> TestResult {
    let hash = sha256::sha256(b"");
    check!(hash[0] == 0xe3, "SHA-256('') byte 0 mismatch");
    check!(hash[1] == 0xb0, "SHA-256('') byte 1 mismatch");
    check!(hash[2] == 0xc4, "SHA-256('') byte 2 mismatch");

    let hash2 = sha256::sha256(b"abc");
    check!(hash2[0] == 0xba, "SHA-256('abc') byte 0 mismatch");
    check!(hash2[1] == 0x78, "SHA-256('abc') byte 1 mismatch");
    TestResult::Pass
}

fn test_pwm_id_newtype() -> TestResult {
    let id = PwmId(42);
    check!(id.is_valid(), "non-zero PwmId should be valid");
    check!(id.as_u64() == 42, "as_u64 mismatch");

    let zero = PwmId::ZERO;
    check!(!zero.is_valid(), "zero PwmId should be invalid");
    check!(zero.as_u64() == 0, "ZERO as_u64 mismatch");

    // 注意: 历史上此处使用 PwmId::TEST 验证 is_valid, 但 TEST 常量已被
    // 移除 (P0-I-29d): 硬编码的"魔法值"权限字会绕过访问控制。
    // 现在改用任意非零 PwmId 即可验证 is_valid 语义。
    let arbitrary = PwmId(0xDEAD_BEEF_CAFE_F00D);
    check!(arbitrary.is_valid(), "non-zero PwmId should be valid");
    check!(
        arbitrary.as_u64() == 0xDEAD_BEEF_CAFE_F00D,
        "arbitrary as_u64 mismatch"
    );
    TestResult::Pass
}

fn test_cap_domain_newtype() -> TestResult {
    let fs = CapDomain::FS;
    check!(fs.as_u16() == 1, "FS domain should be 1");
    check!(fs.as_usize() == 1, "FS domain usize should be 1");

    let from_raw: CapDomain = 2u16.into();
    check!(from_raw == CapDomain::NET, "u16->CapDomain should be NET");

    let sys = CapDomain::SYSTEM;
    check!(sys.as_usize() == 0, "SYSTEM domain usize should be 0");
    TestResult::Pass
}

fn test_cap_bits_newtype() -> TestResult {
    let none = CapBits::NONE;
    check!(none.as_u64() == 0, "NONE should be 0");

    let all = CapBits::ALL;
    check!(all.as_u64() == u64::MAX, "ALL should be u64::MAX");

    let read = CapBits(capability::FS_CAP_READ);
    let write = CapBits(capability::FS_CAP_WRITE);
    let rw = read | write;
    check!(rw.contains(read), "rw should contain read");
    check!(rw.contains(write), "rw should contain write");
    check!(!read.contains(write), "read should not contain write");

    let mut caps = CapBits::NONE;
    caps |= read;
    check!(caps.contains(read), "after |= should contain read");
    caps &= !read;
    check!(!caps.contains(read), "after &= ! should not contain read");
    TestResult::Pass
}

fn test_pwmentry_caps() -> TestResult {
    let entry = PwmEntry::new();
    check!(!entry.is_valid(), "new entry should not be valid");

    entry.pwm.store(123, core::sync::atomic::Ordering::Release);
    check!(entry.is_valid(), "entry with pwm should be valid");
    check!(entry.get_pwm() == PwmId(123), "get_pwm mismatch");

    let fs_caps = entry.load_caps(CapDomain::FS);
    check!(fs_caps == CapBits::NONE, "new entry fs caps should be NONE");

    entry.fetch_or_caps(
        CapDomain::FS,
        CapBits(capability::FS_CAP_READ | capability::FS_CAP_WRITE),
    );
    let after = entry.load_caps(CapDomain::FS);
    check!(
        after.contains(CapBits(capability::FS_CAP_READ)),
        "should have FS_READ"
    );
    check!(
        after.contains(CapBits(capability::FS_CAP_WRITE)),
        "should have FS_WRITE"
    );
    check!(
        !after.contains(CapBits(capability::FS_CAP_EXECUTE)),
        "should not have FS_EXEC"
    );

    check!(
        entry.has_capability(CapDomain::FS, CapBits(capability::FS_CAP_READ)),
        "has_capability should be true"
    );
    check!(
        !entry.has_capability(CapDomain::FS, CapBits(capability::FS_CAP_DELETE)),
        "has_capability DELETE should be false"
    );
    TestResult::Pass
}

fn test_pwmentry_flags() -> TestResult {
    let entry = PwmEntry::new();
    check!(
        !entry.has_flag(PwmFlags::DISABLED),
        "new entry should not be disabled"
    );

    // 2026-06-29 修复: 必须设置非零 pwm 才能测试 DISABLED 标志.
    // engine::check(0, ...) 走 bootstrap 路径直接返回 true, 绕过 flag 检查.
    entry.pwm.store(124, core::sync::atomic::Ordering::Release);
    check!(entry.is_valid(), "entry with pwm=124 should be valid");

    entry.add_flags(PwmFlags::DISABLED);
    check!(
        entry.has_flag(PwmFlags::DISABLED),
        "should be disabled after add"
    );

    check!(
        !engine::check(
            entry.pwm.load(core::sync::atomic::Ordering::Acquire),
            CapDomain::FS,
            CapBits::ALL
        ),
        "disabled entry should fail check"
    );

    entry.remove_flags(PwmFlags::DISABLED);
    check!(
        !entry.has_flag(PwmFlags::DISABLED),
        "should not be disabled after remove"
    );
    TestResult::Pass
}

fn test_grant_record() -> TestResult {
    let rec = GrantRecord {
        grantor_pwm: PwmId(1),
        grantee_pwm: PwmId(2),
        domain: CapDomain::FS,
        caps: CapBits(capability::FS_CAP_READ),
        granted_at: 100,
    };
    check!(!rec.is_empty(), "filled record should not be empty");

    let empty = GrantRecord::EMPTY;
    check!(empty.is_empty(), "EMPTY record should be empty");
    check!(
        empty.grantor_pwm == PwmId::ZERO,
        "EMPTY grantor should be ZERO"
    );
    TestResult::Pass
}

fn test_audit_entry() -> TestResult {
    let entry = AuditEntry {
        timestamp: 1000,
        pwm: PwmId(42),
        action: AuditAction::Create,
        result: AuditResult::Success,
        target_pwm: PwmId(0),
        details: 0,
    };
    check!(entry.pwm.as_u64() == 42, "audit pwm mismatch");
    check!(entry.action.as_u32() == 3, "Create action should be 3");
    check!(entry.result.as_u32() == 0, "Success result should be 0");
    TestResult::Pass
}

fn test_pwmentry_note() -> TestResult {
    // T4-1: 全 Atomic 化后 set_note 接受 &self, 验证用 note_equals
    let entry = PwmEntry::new();
    entry.set_note("test-identity");
    check!(entry.note_equals("test-identity"), "note mismatch");
    check!(!entry.note_equals("other"), "note should not match other");
    TestResult::Pass
}

fn test_viable_floor() -> TestResult {
    check!(
        capability::VIABLE_FLOOR[CapDomain::FS.as_usize()] != 0,
        "FS viable floor should be non-zero"
    );
    check!(
        capability::VIABLE_FLOOR[CapDomain::PROC.as_usize()] != 0,
        "PROC viable floor should be non-zero"
    );
    check!(
        capability::VIABLE_FLOOR[CapDomain::SYSTEM.as_usize()] == 0,
        "SYSTEM viable floor should be zero"
    );
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn test_pwmentry_cow_bp() -> TestResult {
    use crate::services::fs::nestfs::bp::NestBlockPointer;
    use crate::services::fs::nestfs::dmu::NestDmuObject;

    let mut obj = NestDmuObject::new_file(1, 0);
    let bp = NestBlockPointer::null();
    obj.cow_bp(bp, 5);
    check!(obj.birth_txg == 5, "birth txg should be 5 after cow_bp");
    TestResult::Pass
}

pub fn register_pwm_tests() {
    let r = runner();
    register_tests_inner! { r:
        "pwm::sha256": {
            "known_vectors": test_sha256_vectors,
        },
        "pwm::types": {
            "pwm_id_newtype": test_pwm_id_newtype,
            "cap_domain_newtype": test_cap_domain_newtype,
            "cap_bits_newtype": test_cap_bits_newtype,
        },
        "pwm::entry": {
            "caps": test_pwmentry_caps,
            "flags": test_pwmentry_flags,
            "note": test_pwmentry_note,
        },
        "pwm::grant_record": {
            "basic": test_grant_record,
        },
        "pwm::audit": {
            "entry": test_audit_entry,
        },
        "pwm::capability": {
            "viable_floor": test_viable_floor,
        },
    }

    #[cfg(target_arch = "x86_64")]
    r.register("pwm::dmu", "cow_bp", test_pwmentry_cow_bp);
}
