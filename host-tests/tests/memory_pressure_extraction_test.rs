//! Memory Pressure 归属契约测试 (P1-I-01 D9 → DECISION-O ② 反转)
//!
//! 契约随 DECISION-O ② (2026-09-13) 反转：MemoryPressure 类型/压力状态/
//! `update_pressure` 包装归 framework (机制持有, OOMD 是调度器 tick 直接驱动
//! 的机制组件); 分级阈值/算法留 services (策略), 经注册注入。
//!
//! 静态契约:
//! 1. MemoryPressure 枚举 / update_pressure / current_pressure 必在
//!    framework/mm/pressure.rs (机制权威)
//! 2. framework/mm/mod.rs 必声明 `pub mod pressure`
//! 3. framework/proc/oomd.rs 必引用 framework::mm::pressure, 禁止引用 services
//! 4. services/mm/memory_pressure.rs 必 deny unsafe_code, 保留阈值/分级算法/
//!    set_thresholds, 并 re-export framework API (services→framework 合法方向)
//! 5. services/mm/mod.rs init 必注册分级策略 (register_pressure_classifier)
//! 6. 4 级状态机变体 + 双重阈值 (绝对值 + 百分比) 契约保持
//! 7. services 文件不含 klog_ffi (避免 unsafe 边界)

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().to_path_buf()
}

fn framework_pressure_rs() -> String {
    let path = format!(
        "{}/../src/kernel/framework/mm/pressure.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read framework/mm/pressure.rs")
}

fn services_memory_pressure_rs() -> String {
    let path = format!(
        "{}/../src/kernel/services/mm/memory_pressure.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read services/mm/memory_pressure.rs")
}

fn framework_mm_mod_rs() -> String {
    let p = repo_root().join("src/kernel/framework/mm/mod.rs");
    fs::read_to_string(&p).expect("read framework/mm/mod.rs")
}

fn framework_oomd_rs() -> String {
    let p = repo_root().join("src/kernel/framework/proc/oomd.rs");
    fs::read_to_string(&p).expect("read framework/proc/oomd.rs")
}

fn services_mm_mod_rs() -> String {
    let path = format!(
        "{}/../src/kernel/services/mm/mod.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read services/mm/mod.rs")
}

/// 提取 `src` 中 `sig` 起始的顶层函数体 (至首个行首 `}` 结束)
fn fn_at_root<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src
        .find(sig)
        .unwrap_or_else(|| panic!("未找到函数签名: {sig}"));
    let rest = &src[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |i| i + 2);
    &rest[..end]
}

#[test]
fn memory_pressure_mechanism_in_framework() {
    // DECISION-O ② 验收: 类型/状态/update_pressure 包装必在 framework (机制权威)
    let src = framework_pressure_rs();
    assert!(
        src.contains("pub enum MemoryPressure"),
        "DECISION-O ②: MemoryPressure 枚举必在 framework/mm/pressure.rs"
    );
    assert!(
        src.contains("pub fn update_pressure"),
        "DECISION-O ②: update_pressure 包装必在 framework"
    );
    assert!(
        src.contains("pub fn current_pressure") && src.contains("pub fn previous_pressure"),
        "DECISION-O ②: 压力状态读取原语必在 framework"
    );
    assert!(
        src.contains("pub fn register_pressure_classifier"),
        "DECISION-O ②: 分级策略注册口必在 framework"
    );
}

#[test]
fn framework_mm_mod_declares_pressure() {
    // DECISION-O ② 验收: framework/mm/mod.rs 必声明 pressure 模块
    let src = framework_mm_mod_rs();
    assert!(
        src.contains("pub mod pressure"),
        "DECISION-O ②: framework/mm/mod.rs 必声明 pub mod pressure"
    );
}

#[test]
fn framework_oomd_uses_framework_pressure() {
    // DECISION-O ② 验收: OOMD 引用 framework::mm::pressure, 禁止 services 引用
    let src = framework_oomd_rs();
    assert!(
        src.contains("framework::mm::pressure::{MemoryPressure, update_pressure}"),
        "DECISION-O ②: oomd.rs 必从 framework::mm::pressure 引入"
    );
    assert!(
        !src.contains("kernel::services::"),
        "DECISION-O ②: framework/proc/oomd.rs 禁止引用 services (反向依赖清零)"
    );
}

/// C1 (T6 治理): OOMD Emergency 宽限期后必须**真实**发送 SIGKILL 至 RSS 最大进程
///
/// 回归保护: 治理前仅 `terminated_count += 1` + 打日志 (未真正 kill);
/// 若后续任何改动退回到"仅计数", 本用例失败.
#[test]
fn oomd_emergency_actually_sends_sigkill() {
    let src = framework_oomd_rs();
    assert!(
        src.contains("do_signal_send(victim, super::SIGKILL)"),
        "C1: OOMD Emergency 必须真实发送 SIGKILL (不可仅计数)"
    );
    assert!(
        src.contains("count_present_user_pages"),
        "C1: OOMD 必须经页表用户页计数选择 RSS 最大进程"
    );
    assert!(
        !src.contains("SIGKILL 发送至最大 RSS 进程待实现"),
        "C1: 待实现标记必须随治理移除"
    );
}

/// 顺序不变量: `do_signal_send` 不得在 `process_for_each` 闭包内调用
///
/// `do_signal_send` 内部会再次获取进程表锁, 而 `process_for_each` 已在闭包
/// 存续期间持有该锁 — 闭包内调用将自锁死. 该路径运行在 scheduler tick
/// (中断上下文, 关中断), 一旦自锁无法恢复, 属系统级死锁.
#[test]
fn oomd_sigkill_sent_outside_process_table_iteration() {
    let src = framework_oomd_rs();
    let start = src
        .find("process_for_each(")
        .expect("C1: OOMD 必须遍历进程表选择 victim");
    let end = src[start..]
        .find("});")
        .map(|i| start + i)
        .expect("C1: process_for_each 调用必须以 }); 结束");

    let iter_body = &src[start..end];
    assert!(
        !iter_body.contains("do_signal_send"),
        "C1: do_signal_send 不得在 process_for_each 闭包内调用 (会自锁进程表)"
    );
    assert!(
        src[end..].contains("do_signal_send"),
        "C1: do_signal_send 必须在 process_for_each 迭代结束之后调用"
    );
}

#[test]
fn memory_pressure_services_keeps_policy() {
    // DECISION-O ② 验收: 阈值/分级算法/set_thresholds 必留 services (策略)
    let src = services_memory_pressure_rs();
    assert!(
        src.contains("pub fn set_thresholds"),
        "DECISION-O ②: set_thresholds 必在 services"
    );
    assert!(
        src.contains("fn classify_pressure"),
        "DECISION-O ②: 分级算法 classify_pressure 必在 services"
    );
    assert!(
        src.contains("pub use crate::framework::mm::pressure::"),
        "DECISION-O ②: services 必经 re-export 保持 API 兼容 (services→framework 合法方向)"
    );
}

#[test]
fn memory_pressure_services_denies_unsafe() {
    // P1-I-01 D9 验收 (保持): services 文件必 deny unsafe_code
    let src = services_memory_pressure_rs();
    assert!(
        src.contains("#![deny(unsafe_code)]"),
        "DECISION-O ②: services/mm/memory_pressure.rs 必 #![deny(unsafe_code)]"
    );
}

#[test]
fn services_mm_init_registers_classifier() {
    // DECISION-O ② 验收: services::mm::init 必注册分级策略 (机制留注册口)
    let src = services_mm_mod_rs();
    assert!(
        src.contains("pub mod memory_pressure"),
        "DECISION-O ②: services/mm/mod.rs 必 pub mod memory_pressure"
    );
    assert!(
        src.contains("memory_pressure::register_pressure_classifier()"),
        "DECISION-O ②: services::mm::init 必注册分级策略"
    );
}

#[test]
fn memory_pressure_uses_4_level_state_machine() {
    // P1-I-01 D9 验收 (保持): 策略核心是 4 级状态机
    let src = framework_pressure_rs();
    // 必 4 个级别
    for variant in ["Normal", "Warning", "Critical", "Emergency"] {
        assert!(
            src.contains(&format!("    {} =", variant)),
            "DECISION-O ②: MemoryPressure 必含 {} 变体",
            variant
        );
    }
}

#[test]
fn memory_pressure_services_uses_double_threshold() {
    // P1-I-01 D9 验收 (保持): 双重阈值 (绝对值 + 百分比)
    let src = services_memory_pressure_rs();
    // 三个阈值常量
    assert!(
        src.contains("FREE_PAGES_THRESHOLD_WARNING"),
        "DECISION-O ②: 必含 warning 阈值"
    );
    assert!(
        src.contains("FREE_PAGES_THRESHOLD_CRITICAL"),
        "DECISION-O ②: 必含 critical 阈值"
    );
    assert!(
        src.contains("FREE_PAGES_THRESHOLD_EMERGENCY"),
        "DECISION-O ②: 必含 emergency 阈值"
    );
    // 阈值守卫: warning > critical > emergency
    assert!(
        src.contains("warning > critical && critical > emergency"),
        "DECISION-O ②: set_thresholds 必验证 warn > crit > emer 顺序"
    );
}

#[test]
fn memory_pressure_services_no_klog_ffi() {
    // P1-I-01 D9 验收 (保持): services 文件不含 klog_ffi (避免 unsafe 边界)
    let src = services_memory_pressure_rs();
    assert!(
        !src.contains("klog_ffi!"),
        "DECISION-O ②: services 文件不应含 klog_ffi! (触发 unsafe)"
    );
    assert!(
        !src.contains("crate::klog_ffi"),
        "DECISION-O ②: services 文件不应含 crate::klog_ffi"
    );
}

/// 丙批审查 A1 门槛: 用户页表遍历必须与 map/unmap 同持 `VMM_LOCK`.
///
/// 背景: `count_present_user_pages` 原为**无锁**遍历, 而 `unmap_page_in_table`
/// 在解除最后一个表项后会递归释放变空的中间页表 (`get_pmm().free_page`) —
/// 两者交错即踩野指针. 修复后遍历全程持 `VMM_LOCK`, 使该释放无法与遍历交错.
///
/// 页表并发行为需裸机 SMP 才能复现, host 无页表物理内存, 故以源码结构门槛
/// 收口 (与 B1/B2 同体例); 计数正确性由 QEMU 用例
/// `mm::vmm::count_present_user_pages` (基线增量恒等) 行为验证.
#[test]
fn user_page_count_traversal_holds_vmm_lock() {
    for (file, sig) in [
        (
            "src/kernel/framework/mm/vmm_x86_64.rs",
            "pub fn count_present_user_pages(",
        ),
        (
            "src/kernel/framework/mm/vmm_aarch64.rs",
            "pub fn count_present_user_pages(",
        ),
    ] {
        let src = fs::read_to_string(repo_root().join(file))
            .unwrap_or_else(|e| panic!("read {file}: {e}"));
        let body = fn_at_root(&src, sig);
        // 剔除注释行后再判定, 避免"注释里写着 acquire_lock()"造成假通过
        // (负向验证: 把判据行改成注释后, 本用例必须变红).
        let code: String = body
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        let acquire = code
            .find("acquire_lock()")
            .unwrap_or_else(|| panic!("A1: {file} 遍历必须获取 VMM_LOCK"));
        let release = code
            .find("release_lock(")
            .unwrap_or_else(|| panic!("A1: {file} 遍历必须释放 VMM_LOCK"));
        assert!(
            acquire < release,
            "A1: {file} 必须先获取 VMM_LOCK 再释放 (次序不得颠倒)"
        );
        assert!(
            !body.contains("不取 VMM 锁"),
            "A1: {file} 文档不得再声明无锁遍历 (锁契约已收紧)"
        );
    }
}
