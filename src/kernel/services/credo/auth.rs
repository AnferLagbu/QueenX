#![deny(unsafe_code)]
//! Credo 认证策略 — PWM 登录/登出/创建/删除/验证/授权
//!
//! 从 framework/syscall/mod.rs 迁移的策略代码:
//! - auth_login_syscall: PWM 登录
//! - auth_logout_syscall: PWM 登出
//! - auth_create_syscall: PWM 创建身份
//! - auth_delete_syscall: PWM 删除身份
//! - auth_info_syscall: 查询身份信息
//! - auth_changepw_syscall: 修改密码
//! - auth_verify_syscall: 验证密码
//! - auth_create_first_syscall: 创建首个身份
//! - auth_grant_syscall: 授权
//! - auth_revoke_syscall: 撤销授权
//! - auth_check_cap_syscall: 检查能力
//! - auth_get_caps_syscall: 获取能力
//! - pwm_get_syscall: 获取当前 PWM
//! - pwm_set_syscall: 设置当前 PWM
//!
//! ## 框内核边界
//! - 100% safe Rust
//! - 通过 framework::credo 公开 API 访问
//! - 无 unsafe, 无裸指针

use crate::framework::syscall::Errno;

/// `auth_login(password`, note) 策略
pub fn auth_login_syscall(password_ptr: u64, note_ptr: u64) -> i64 {
    crate::framework::credo::pwm_login(note_ptr as *const u8, password_ptr as *const u8)
}

/// `auth_logout()` 策略
pub fn auth_logout_syscall() -> i64 {
    crate::framework::credo::pwm_logout();
    0
}

/// `auth_create(password`, note, level) 策略
pub fn auth_create_syscall(password_ptr: u64, note_ptr: u64, _level: u8) -> i64 {
    let creator = crate::framework::credo::pwm_get_current();
    crate::framework::credo::pwm_create(
        password_ptr as *const u8,
        note_ptr as *const u8,
        creator,
    )
}

/// `auth_delete(target)` 策略
pub fn auth_delete_syscall(target: u64) -> i64 {
    i64::from(crate::framework::credo::pwm_delete(target))
}

/// `auth_info(target)` 策略
pub fn auth_info_syscall(target: u64) -> i64 {
    i64::from(crate::framework::credo::pwm_get_privilege_level(
        target,
    ))
}

/// `auth_changepw(old_pw`, `new_pw`) 策略
pub fn auth_changepw_syscall(old_pw_ptr: u64, new_pw_ptr: u64) -> i64 {
    let pwm = crate::framework::credo::pwm_get_current();
    i64::from(crate::framework::credo::pwm_change_password(
        pwm,
        old_pw_ptr as *const u8,
        new_pw_ptr as *const u8,
    ))
}

/// `auth_verify(password)` 策略
pub fn auth_verify_syscall(password_ptr: u64) -> i64 {
    let pwm = crate::framework::credo::pwm_get_current();
    i64::from(crate::framework::credo::pwm_verify_password(
        pwm,
        password_ptr as *const u8,
    ))
}

/// `auth_create_first(password)` 策略
pub fn auth_create_first_syscall(password_ptr: u64) -> i64 {
    if password_ptr == 0 {
        return Errno::EINVAL.as_ret();
    }
    crate::framework::credo::pwm_create_first_identity(password_ptr as *const u8)
}

/// `auth_grant(grantor`, grantee, domain, caps) 策略
pub fn auth_grant_syscall(grantor: u64, grantee: u64, domain: u16, caps: u64) -> i64 {
    i64::from(crate::framework::credo::pwm_grant(
        grantor, grantee, domain, caps,
    ))
}

/// `auth_revoke(revoker`, target, domain, caps) 策略
pub fn auth_revoke_syscall(revoker: u64, target: u64, domain: u16, caps: u64) -> i64 {
    i64::from(crate::framework::credo::pwm_revoke(
        revoker, target, domain, caps,
    ))
}

/// `auth_check_cap(pwm`, domain, required) 策略
pub fn auth_check_cap_syscall(pwm: u64, domain: u16, required: u64) -> i64 {
    i64::from(crate::framework::credo::pwm_has_capability(
        pwm, domain, required,
    ))
}

/// `auth_get_caps(pwm`, domain) 策略
pub fn auth_get_caps_syscall(pwm: u64, domain: u16) -> i64 {
    crate::framework::credo::pwm_get_capability_raw(pwm, domain) as i64
}

/// `pwm_get()` 策略
pub fn pwm_get_syscall() -> i64 {
    crate::framework::credo::pwm_get_current() as i64
}

/// `pwm_set(pwm)` 策略
///
/// B07-05 (DECISION-078): 设置进程 PWM 属身份安全关键操作, 调用方必须持有
/// SYSTEM 域 `SET_PWM` 能力位 (PWM 0/bootstrap 经 `engine::check` 恒全权).
/// 原实现无任何校验, 任意进程可把自身 PWM 改为 root, 绕过全部 UID/GID 检查
/// (任意提权漏洞 P0-07).
pub fn pwm_set_syscall(pwm: u64) -> i64 {
    use crate::services::credo::capability::{
        CAP_DOMAIN_SYSTEM, SYSTEM_CAP_SET_PWM,
    };
    let current = crate::framework::credo::pwm_get_current();
    if !crate::framework::credo::pwm_has_capability(
        current,
        CAP_DOMAIN_SYSTEM,
        SYSTEM_CAP_SET_PWM,
    ) {
        return Errno::EPERM.as_ret();
    }
    let pid = crate::framework::proc::process_get_current_pid();
    i64::from(crate::framework::proc::proc_set_pwm(pid, pwm))
}
