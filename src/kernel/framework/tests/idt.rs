use crate::framework::idt::DetailedStatistics;
use crate::framework::idt::handlers::{
    AccessType, DefaultHandler, DivisionByZeroHandler, ExceptionCategory, ExceptionHandler,
    ExceptionStatisticsCollector, FaultCause, Mode, PageFaultHandler, PanicInfo, RecoveryAction,
    Severity, create_handler,
};
use crate::framework::idt::{
    CpuFeatures, is_null_or_invalid, is_valid_kernel_address, is_valid_user_address,
};
use crate::framework::idt::{
    ErrorFlags, GDT_KERNEL_CODE, IDT_ENTRIES, IDT_TYPE_INTERRUPT, IRQ_BASE, IdtEntry, IdtPtr,
    InterruptFrame, InterruptStatistics, get_exception_name, get_irq_name,
};
use crate::framework::mm::{KERNEL_BASE, KERNEL_TEXT_BASE};
use crate::framework::tests::{TestResult, assert_eq_test, check, runner};
use crate::register_tests_inner;
use core::sync::atomic::Ordering;

fn interrupt_frame_size() -> TestResult {
    assert_eq_test!(
        core::mem::size_of::<InterruptFrame>(),
        176,
        "InterruptFrame size"
    );
    TestResult::Pass
}

fn user_mode_detection() -> TestResult {
    let kernel_frame = InterruptFrame::new_test_frame(14, KERNEL_TEXT_BASE, 0x08);
    check!(
        !kernel_frame.is_user_mode(),
        "kernel CS should be kernel mode"
    );
    let user_frame = InterruptFrame::new_test_frame(14, 0x400000, 0x23);
    check!(user_frame.is_user_mode(), "user CS should be user mode");
    TestResult::Pass
}

fn idt_entry_creation() -> TestResult {
    let entry = IdtEntry::new(0xDEADBEEFCAFEBABE, GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT);
    assert_eq_test!(entry.offset_low, 0xBABE, "offset_low");
    assert_eq_test!(entry.selector, GDT_KERNEL_CODE, "selector");
    check!(entry.is_present(), "should be present");
    assert_eq_test!(
        entry.handler_address(),
        0xDEADBEEFCAFEBABE,
        "handler address"
    );
    TestResult::Pass
}

fn idt_ptr_creation() -> TestResult {
    let base_addr = 0xFFFF800000001000u64;
    let ptr = IdtPtr::new(base_addr);
    assert_eq_test!(ptr.base, base_addr, "base address");
    assert_eq_test!(ptr.limit, (IDT_ENTRIES * 16 - 1) as u16, "limit");
    TestResult::Pass
}

fn statistics_recording() -> TestResult {
    let stats = InterruptStatistics::new();
    stats.record_exception(0);
    stats.record_exception(14);
    stats.record_irq(0);
    assert_eq_test!(stats.get_count(0), 1, "exception 0 count");
    assert_eq_test!(stats.get_count(14), 1, "exception 14 count");
    assert_eq_test!(stats.get_count(IRQ_BASE), 1, "IRQ 0 count");
    assert_eq_test!(stats.get_count(100), 0, "invalid vector count");
    TestResult::Pass
}

fn error_flags() -> TestResult {
    let flags = ErrorFlags::PRESENT | ErrorFlags::WRITE | ErrorFlags::USER;
    check!(flags.contains(ErrorFlags::PRESENT), "PRESENT flag");
    check!(flags.contains(ErrorFlags::WRITE), "WRITE flag");
    check!(flags.contains(ErrorFlags::USER), "USER flag");
    check!(
        !flags.contains(ErrorFlags::RESERVED),
        "RESERVED flag absent"
    );
    TestResult::Pass
}

fn exception_names() -> TestResult {
    assert_eq_test!(get_exception_name(0), "Division By Zero", "exception 0");
    assert_eq_test!(get_exception_name(14), "Page Fault", "exception 14");
    assert_eq_test!(get_exception_name(99), "Unknown", "exception 99");
    TestResult::Pass
}

fn irq_names() -> TestResult {
    assert_eq_test!(get_irq_name(0), "Timer", "IRQ 0");
    assert_eq_test!(get_irq_name(1), "Keyboard", "IRQ 1");
    assert_eq_test!(get_irq_name(20), "Unknown", "IRQ 20");
    TestResult::Pass
}

fn recovery_action_variants() -> TestResult {
    assert_eq_test!(
        RecoveryAction::Recovered,
        RecoveryAction::Recovered,
        "Recovered eq"
    );
    check!(
        RecoveryAction::TerminateProcess(1) != RecoveryAction::Recovered,
        "TerminateProcess neq"
    );
    TestResult::Pass
}

fn severity_ordering() -> TestResult {
    check!(Severity::Info < Severity::Warning, "Info < Warning");
    check!(Severity::Warning < Severity::Error, "Warning < Error");
    check!(Severity::Error < Severity::Fatal, "Error < Fatal");
    check!(
        Severity::Fatal < Severity::Catastrophic,
        "Fatal < Catastrophic"
    );
    TestResult::Pass
}

fn exception_categories() -> TestResult {
    let handler = DivisionByZeroHandler;
    assert_eq_test!(
        handler.category(),
        ExceptionCategory::Arithmetic,
        "category"
    );
    assert_eq_test!(handler.name(), "Division By Zero", "name");
    TestResult::Pass
}

fn page_fault_analysis() -> TestResult {
    let analysis = PageFaultHandler::analyze_error_code(0x02);
    check!(!analysis.present, "not present");
    assert_eq_test!(analysis.access_type, AccessType::Write, "access type");
    assert_eq_test!(analysis.mode, Mode::Kernel, "mode");
    assert_eq_test!(analysis.cause, FaultCause::PageNotPresent, "cause");
    TestResult::Pass
}

fn default_handler() -> TestResult {
    let handler = DefaultHandler::new(7);
    assert_eq_test!(handler.name(), "No Coprocessor", "name");
    assert_eq_test!(handler.severity(), Severity::Warning, "severity");
    TestResult::Pass
}

fn factory_pattern() -> TestResult {
    let handler0 = create_handler(0);
    let handler13 = create_handler(13);
    let handler99 = create_handler(99);
    assert_eq_test!(handler0.name(), "Division By Zero", "handler 0");
    assert_eq_test!(handler13.name(), "General Protection Fault", "handler 13");
    check!(
        handler99.category() == ExceptionCategory::Unknown,
        "handler 99 should be unknown category"
    );
    TestResult::Pass
}

fn statistics_collector() -> TestResult {
    let collector = ExceptionStatisticsCollector::new();
    let handler = DivisionByZeroHandler;
    let action = RecoveryAction::TerminateProcess(42);
    collector.record(&handler, &action);
    assert_eq_test!(
        collector.total_exceptions.load(Ordering::Relaxed),
        1,
        "total exceptions"
    );
    assert_eq_test!(
        collector.process_terminations.load(Ordering::Relaxed),
        1,
        "terminations"
    );
    assert_eq_test!(
        collector.by_category[0].load(Ordering::Relaxed),
        1,
        "arithmetic category"
    );
    TestResult::Pass
}

fn panic_info_creation() -> TestResult {
    let info = PanicInfo::new("Test panic", 14, 0xDEADBEEF);
    assert_eq_test!(info.vector, 14, "vector");
    assert_eq_test!(info.rip, 0xDEADBEEF, "rip");
    assert_eq_test!(info.reason, "Test panic", "reason");
    TestResult::Pass
}

fn detailed_stats_init() -> TestResult {
    let stats = DetailedStatistics::new();
    assert_eq_test!(stats.total_count.load(Ordering::Relaxed), 0, "total init");
    assert_eq_test!(
        stats.nested_interrupts.load(Ordering::Relaxed),
        0,
        "nested init"
    );
    TestResult::Pass
}

fn detailed_stats_record_exception() -> TestResult {
    let stats = DetailedStatistics::new();
    let frame = InterruptFrame::new_test_frame(14, 0x400000, 0x23);
    stats.record_exception(14, &frame);
    assert_eq_test!(
        stats.total_count.load(Ordering::Relaxed),
        1,
        "total after exception"
    );
    assert_eq_test!(stats.get_vector_count(14), 1, "vector 14 count");
    assert_eq_test!(
        stats.user_mode_interrupts.load(Ordering::Relaxed),
        1,
        "user mode count"
    );
    assert_eq_test!(
        stats.kernel_mode_interrupts.load(Ordering::Relaxed),
        0,
        "kernel mode count"
    );
    TestResult::Pass
}

fn detailed_stats_record_irq() -> TestResult {
    let stats = DetailedStatistics::new();
    stats.record_irq(1);
    stats.record_irq(0);
    stats.record_irq(1);
    assert_eq_test!(
        stats.total_count.load(Ordering::Relaxed),
        3,
        "total after IRQs"
    );
    assert_eq_test!(
        stats.irq_counts[1].load(Ordering::Relaxed),
        2,
        "IRQ 1 count"
    );
    assert_eq_test!(
        stats.irq_counts[0].load(Ordering::Relaxed),
        1,
        "IRQ 0 count"
    );
    TestResult::Pass
}

fn detailed_stats_nested() -> TestResult {
    let stats = DetailedStatistics::new();
    stats.record_nested(1);
    stats.record_nested(2);
    stats.record_nested(3);
    stats.record_nested(2);
    assert_eq_test!(
        stats.nested_interrupts.load(Ordering::Relaxed),
        4,
        "nested count"
    );
    assert_eq_test!(
        stats.max_nesting_depth.load(Ordering::Relaxed),
        3,
        "max depth"
    );
    TestResult::Pass
}

fn detailed_stats_reset() -> TestResult {
    let stats = DetailedStatistics::new();
    stats.record_exception(0, &InterruptFrame::new_test_frame(0, 0x1000, 0x08));
    stats.record_irq(5);
    stats.record_nested(1);
    stats.reset();
    assert_eq_test!(
        stats.total_count.load(Ordering::Relaxed),
        0,
        "total after reset"
    );
    assert_eq_test!(stats.get_vector_count(0), 0, "vector 0 after reset");
    assert_eq_test!(
        stats.irq_counts[5].load(Ordering::Relaxed),
        0,
        "IRQ 5 after reset"
    );
    TestResult::Pass
}

fn detailed_stats_recovery_action_tracking() -> TestResult {
    let stats = DetailedStatistics::new();
    stats.record_recovery_action(&RecoveryAction::Recovered);
    stats.record_recovery_action(&RecoveryAction::TerminateProcess(42));
    stats.record_recovery_action(&RecoveryAction::DomainRecovery);
    stats.record_recovery_action(&RecoveryAction::Panic(PanicInfo::new("test", 0, 0)));
    assert_eq_test!(stats.recoveries.load(Ordering::Relaxed), 1, "recoveries");
    assert_eq_test!(
        stats.process_terminations.load(Ordering::Relaxed),
        1,
        "terminations"
    );
    assert_eq_test!(stats.domain_recoveries.load(Ordering::Relaxed), 1, "domain");
    assert_eq_test!(stats.panics.load(Ordering::Relaxed), 1, "panics");
    TestResult::Pass
}

fn detailed_stats_invalid_vector_count() -> TestResult {
    let stats = DetailedStatistics::new();
    assert_eq_test!(stats.get_vector_count(255), 0, "vec 255");
    assert_eq_test!(stats.get_vector_count(100), 0, "vec 100");
    TestResult::Pass
}

fn address_validation() -> TestResult {
    check!(is_null_or_invalid(0), "null is invalid");
    check!(is_null_or_invalid(0xFFF), "0xFFF is invalid");
    check!(!is_null_or_invalid(0x1000), "0x1000 is valid");
    check!(is_valid_user_address(0x400000), "0x400000 is user");
    check!(!is_valid_user_address(KERNEL_BASE), "kernel is not user");
    check!(
        is_valid_kernel_address(KERNEL_TEXT_BASE),
        "kernel addr is valid"
    );
    check!(!is_valid_kernel_address(0x400000), "user is not kernel");
    TestResult::Pass
}

fn cpu_features_no_panic() -> TestResult {
    let features = CpuFeatures::detect();
    // x86_64: detect() 经 CPUID leaf 0/1 真实解析 — 须满足最低硬件不变量
    #[cfg(target_arch = "x86_64")]
    {
        check!(features.max_cpuid_leaf >= 1, "max_cpuid_leaf must be >= 1");
        check!(features.has_apic, "leaf1 EDX bit9 (APIC) must be set");
        // x2APIC 为可选特性 (QEMU 默认 CPU 不暴露); 架构上 x2APIC 蕴含 APIC
        check!(
            !features.has_x2apic || features.has_apic,
            "x2APIC must imply APIC"
        );
    }
    // aarch64: 中断控制器为 GIC, 无 APIC/x2APIC (detect 返回架构中性缺省值)
    #[cfg(target_arch = "aarch64")]
    {
        check!(!features.has_apic, "aarch64 must not report APIC");
        check!(!features.has_x2apic, "aarch64 must not report x2APIC");
        check!(features.max_cpuid_leaf == 0, "aarch64 max_cpuid_leaf default");
    }
    TestResult::Pass
}

pub fn register_idt_types_tests() {
    let r = runner();
    register_tests_inner! { r:
        "idt::types": {
            "frame_size": interrupt_frame_size,
            "user_mode_detection": user_mode_detection,
            "entry_creation": idt_entry_creation,
            "ptr_creation": idt_ptr_creation,
            "statistics_recording": statistics_recording,
            "error_flags": error_flags,
            "exception_names": exception_names,
            "irq_names": irq_names,
        },
    }
}

pub fn register_idt_handlers_tests() {
    let r = runner();
    register_tests_inner! { r:
        "idt::handlers": {
            "recovery_action_variants": recovery_action_variants,
            "severity_ordering": severity_ordering,
            "exception_categories": exception_categories,
            "page_fault_analysis": page_fault_analysis,
            "default_handler": default_handler,
            "factory_pattern": factory_pattern,
            "statistics_collector": statistics_collector,
            "panic_info_creation": panic_info_creation,
        },
    }
}

pub fn register_idt_statistics_tests() {
    let r = runner();
    register_tests_inner! { r:
        "idt::statistics": {
            "init": detailed_stats_init,
            "record_exception": detailed_stats_record_exception,
            "record_irq": detailed_stats_record_irq,
            "nested": detailed_stats_nested,
            "recovery_action_tracking": detailed_stats_recovery_action_tracking,
            "reset": detailed_stats_reset,
            "invalid_vector_count": detailed_stats_invalid_vector_count,
        },
    }
}

pub fn register_idt_safety_tests() {
    let r = runner();
    register_tests_inner! { r:
        "idt::safety": {
            "address_validation": address_validation,
            "cpu_features_no_panic": cpu_features_no_panic,
        },
    }
}

pub fn register_tests() {
    register_idt_types_tests();
    register_idt_handlers_tests();
    register_idt_statistics_tests();
    register_idt_safety_tests();
}
