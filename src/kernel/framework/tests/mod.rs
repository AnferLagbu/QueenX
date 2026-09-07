use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

use crate::kernel::framework::sync::once_lock::OnceLock;
// E-03 (2026-09-06): feature 语义拆分 — 纯逻辑测试模块在 host-test 下同样编译
// (同源双编译, 供 host-tests 引用内核真实源码). 硬件路径门控保持 kernel_test.
// 语义: any(kernel_test, host-test) = 纯逻辑测试辅助; kernel_test = 硬件路径切换.
#[cfg(any(feature = "kernel_test", feature = "host-test"))]
pub mod arch;
#[cfg(feature = "kernel_test")]
pub mod driver;
// E-03: host 不可编译（依赖 idt/types.rs::InterruptFrame::new_test_frame 与
// idt/statistics.rs::DetailedStatistics::reset, 二者 cfg(any(test, kernel_test))
// 门控在 host-test 下关闭），保持 kernel_test
#[cfg(feature = "kernel_test")]
pub mod idt;
#[cfg(feature = "kernel_test")]
pub mod net;
// E-03: host 不可编译（依赖 barrier::reset::{bbr,bsr,audit,parallel}::tests,
// 其 cfg(feature = "kernel_test") 门控在 host-test 下关闭），保持 kernel_test
#[cfg(feature = "kernel_test")]
pub mod reset;
#[cfg(any(feature = "kernel_test", feature = "host-test"))]
pub mod sched;
#[cfg(any(feature = "kernel_test", feature = "host-test"))]
pub mod string;
#[cfg(any(feature = "kernel_test", feature = "host-test"))]
pub mod sync;
#[cfg(any(feature = "kernel_test", feature = "host-test"))]
pub mod sys;
pub mod test_barrier;
pub mod test_barrier_ext;
pub mod test_config;
pub mod test_devfs;
#[cfg(target_arch = "x86_64")]
pub mod test_hvfs;
#[cfg(target_arch = "x86_64")]
pub mod test_hvfs_ext;
pub mod test_ipc;
pub mod test_mm;
pub mod test_new_features;
pub mod test_pi_mutex;
pub mod test_proc;
pub mod test_pwm;
pub mod test_smp;
pub mod test_uds;
pub mod test_vfs;

pub type TestFn = fn() -> TestResult;

pub enum TestResult {
    Pass,
    Fail(&'static str),
    Skip(&'static str),
}

#[derive(Copy, Clone)]
pub struct TestCase {
    pub module: &'static str,
    pub name: &'static str,
    pub func: TestFn,
}

const MAX_TESTS: usize = 256;

fn noop_test() -> TestResult {
    TestResult::Pass
}

const NOOP_CASE: TestCase = TestCase {
    module: "",
    name: "",
    func: noop_test,
};

struct TestRegistry {
    count: usize,
    cases: [TestCase; MAX_TESTS],
}

impl TestRegistry {
    const fn new() -> Self {
        Self {
            count: 0,
            cases: [NOOP_CASE; MAX_TESTS],
        }
    }

    fn register(&mut self, module: &'static str, name: &'static str, func: TestFn) {
        if self.count < MAX_TESTS {
            self.cases[self.count] = TestCase { module, name, func };
            self.count += 1;
        }
    }
}

pub struct TestRunner {
    registry: IrqSpinLock<TestRegistry>,
    pub passed: AtomicU32,
    pub failed: AtomicU32,
    pub skipped: AtomicU32,
}

impl TestRunner {
    pub const fn new() -> Self {
        Self {
            registry: IrqSpinLock::new(TestRegistry::new()),
            passed: AtomicU32::new(0),
            failed: AtomicU32::new(0),
            skipped: AtomicU32::new(0),
        }
    }

    pub fn register(&self, module: &'static str, name: &'static str, func: TestFn) {
        self.registry.lock().register(module, name, func);
    }

    pub fn run_all(&self) {
        let reg = self.registry.lock();
        let total = reg.count;

        Self::serial_print(b"\n========================================\n");
        Self::serial_print(b"  QueenX Test Suite\n  ");
        Self::serial_print_num(total as u64);
        Self::serial_print(b" test cases registered\n");
        Self::serial_print(b"========================================\n\n");

        // B03-15 修复: interrupt_disable() 返回 usize 保存旧 flags,
        // 测试循环后调 interrupt_restore(flags) 恢复原状态。
        // 之前循环结束后无 re-enable, 测试结束中断永久全关。
        let saved_flags = crate::arch!(interrupt_disable());

        for i in 0..total {
            let tc = reg.cases[i];
            let module = tc.module;
            let name = tc.name;
            let func = tc.func;

            Self::serial_print(b"[");
            Self::serial_print_num((i + 1) as u64);
            Self::serial_print(b"/");
            Self::serial_print_num(total as u64);
            Self::serial_print(b"] ");
            Self::serial_print(module.as_bytes());
            Self::serial_print(b"::");
            Self::serial_print(name.as_bytes());
            Self::serial_print(b"...");

            let result = func();

            match result {
                TestResult::Pass => {
                    self.passed.fetch_add(1, Ordering::Relaxed);
                    Self::serial_print(b"PASS\n");
                }
                TestResult::Fail(msg) => {
                    self.failed.fetch_add(1, Ordering::Relaxed);
                    Self::serial_print(b"FAIL: ");
                    Self::serial_print(msg.as_bytes());
                    Self::serial_print(b"\n");
                }
                TestResult::Skip(reason) => {
                    self.skipped.fetch_add(1, Ordering::Relaxed);
                    Self::serial_print(b"SKIP: ");
                    Self::serial_print(reason.as_bytes());
                    Self::serial_print(b"\n");
                }
            }
        }

        drop(reg);

        let p = self.passed.load(Ordering::Relaxed);
        let f = self.failed.load(Ordering::Relaxed);
        let s = self.skipped.load(Ordering::Relaxed);

        Self::serial_print(b"\n========================================\n");
        if f > 0 {
            Self::serial_print(b"  RESULT: ");
            Self::serial_print_num(u64::from(p));
            Self::serial_print(b" passed, ");
            Self::serial_print_num(u64::from(f));
            Self::serial_print(b" FAILED, ");
            Self::serial_print_num(u64::from(s));
            Self::serial_print(b" skipped\n");
        } else {
            Self::serial_print(b"  RESULT: ALL ");
            Self::serial_print_num(u64::from(p));
            Self::serial_print(b" TESTS PASSED (");
            Self::serial_print_num(u64::from(s));
            Self::serial_print(b" skipped)\n");
        }
        Self::serial_print(b"========================================\n");

        // B03-15: 恢复中断状态 (而非保持 disable 状态)。
        // SAFETY: saved_flags 由 interrupt_save() 获取, 与 interrupt_disable()
        // 配套使用恢复原状态。
        crate::arch!(interrupt_restore(saved_flags));
    }

    fn serial_print(s: &[u8]) {
        #[cfg(target_arch = "x86_64")]
        serial_print(s);
        #[cfg(target_arch = "aarch64")]
        for &b in s {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                crate::kernel::framework::arch::aarch64::uart::putc(b);
            }
        }
    }

    fn serial_print_num(n: u64) {
        #[cfg(target_arch = "x86_64")]
        serial_print_num(n);
        #[cfg(target_arch = "aarch64")]
        {
            if n == 0 {
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe {
                    crate::kernel::framework::arch::aarch64::uart::putc(b'0');
                }
                return;
            }
            let mut buf = [0u8; 20];
            let mut pos = 0usize;
            let mut val = n;
            while val > 0 {
                buf[pos] = (val % 10) as u8 + b'0';
                pos += 1;
                val /= 10;
            }
            for i in (0..pos).rev() {
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe {
                    crate::kernel::framework::arch::aarch64::uart::putc(buf[i]);
                }
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
unsafe fn port_inb(port: u16) -> u8 {
    crate::arch!(inb(port))
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe fn port_outb(port: u16, value: u8) {
    crate::arch!(outb(port, value));
}

#[cfg(target_arch = "x86_64")]
pub fn serial_print(s: &[u8]) {
    const COM1: u16 = 0x3F8;
    for &b in s {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            while (port_inb(COM1 + 5) & 0x20) == 0 {
                core::hint::spin_loop();
            }
            port_outb(COM1, b);
        }
        if b == b'\n' {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                port_outb(COM1, b'\r');
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
pub fn serial_print_num(mut n: u64) {
    if n == 0 {
        serial_print(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut pos = 0usize;
    while n > 0 {
        buf[pos] = (n % 10) as u8 + b'0';
        pos += 1;
        n /= 10;
    }
    for i in (0..pos).rev() {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            const COM1: u16 = 0x3F8;
            while (port_inb(COM1 + 5) & 0x20) == 0 {
                core::hint::spin_loop();
            }
            port_outb(COM1, buf[i]);
        }
    }
}

static TEST_RUNNER: OnceLock<TestRunner> = OnceLock::new();

pub fn runner() -> &'static TestRunner {
    TEST_RUNNER.get_or_init(|slot| {
        slot.write(TestRunner::new());
    })
}

#[macro_export]
macro_rules! check {
    ($cond:expr_2021, $msg:literal $(,)?) => {
        if !($cond) {
            return $crate::kernel::framework::tests::TestResult::Fail($msg);
        }
    };
}

#[macro_export]
macro_rules! assert_eq_test {
    ($left:expr_2021, $right:expr_2021, $msg:literal $(,)?) => {
        let l = $left;
        let r = $right;
        if l != r {
            return $crate::kernel::framework::tests::TestResult::Fail($msg);
        }
    };
}

#[macro_export]
macro_rules! skip_test {
    ($reason:literal $(,)?) => {
        return $crate::kernel::framework::tests::TestResult::Skip($reason);
    };
}

#[macro_export]
macro_rules! register_tests_inner {
    ($r:ident: $($mod:literal: { $($name:literal: $func:ident),* $(,)? }),* $(,)?) => {
        $(
            $(
                $r.register($mod, $name, $func);
            )*
        )*
    };
}

pub use {assert_eq_test, check, skip_test};

#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
pub fn test_runner_init() {
    crate::klog_boot_info!("[TEST] === QueenX Test Framework ===");

    // FS 全局单例初始化 — 测试模式下需主动 init, 否则后续
    // 调用 global() 会 panic (e.g. devfs::global() called before init_global()).
    // init_global 是幂等的 (OnceCell::get_or_init), 多次调用安全.
    // 注: ramfs::init_global 需要 mount_point 参数, 在 mount 测试内显式调用.
    crate::kernel::services::fs::devfs::init_global();
    crate::kernel::services::fs::procfs::init_global();

    test_barrier::register_barrier_tests();
    test_barrier_ext::register_barrier_ext_tests();
    test_config::register_config_tests();
    #[cfg(target_arch = "x86_64")]
    {
        test_hvfs::register_hvfs_tests();
        test_hvfs_ext::register_hvfs_ext_tests();
    }
    test_pwm::register_pwm_tests();
    test_mm::register_mm_tests();
    test_vfs::register_vfs_tests();
    test_ipc::register_ipc_tests();
    test_uds::register_uds_tests();
    test_pi_mutex::register_pi_mutex_tests();
    test_devfs::register_devfs_tests();
    test_proc::register_proc_tests();
    test_new_features::register_new_tests();
    test_smp::register_smp_tests();

    // E-03 (2026-09-06): feature 语义拆分 — 纯逻辑测试注册 (host-test 下同样编译,
    // 供 host-tests 同源引用; kernel_test 下行为与改造前完全一致).
    #[cfg(any(feature = "kernel_test", feature = "host-test"))]
    {
        #[cfg(target_arch = "x86_64")]
        {
            arch::register_tests();
            sys::register_tests();
        }
        string::register_tests();
        sched::register_tests();
        sync::register_tests();
    }

    // E-03 (2026-09-06): 硬件路径测试注册 — 依赖裸机硬件 (驱动/网络/定时器/中断等),
    // 仅 kernel_test (QEMU 裸机测试) 生效; host-test 下不编译.
    // 注: idt/reset 因依赖 kernel_test 门控的框架测试辅助 (见上), 注册同样留在本块.
    #[cfg(feature = "kernel_test")]
    {
        #[cfg(target_arch = "x86_64")]
        {
            driver::register_tests();
            idt::register_tests();
        }
        net::register_tests();
        reset::register_tests();
        #[cfg(target_arch = "x86_64")]
        {
            crate::kernel::framework::timer::pit::register_pit_tests();
            crate::kernel::framework::timer::calibration::register_timer_calibration_tests();
        }
        crate::kernel::framework::timer::tick::register_timer_tick_tests();
        #[cfg(target_arch = "x86_64")]
        crate::kernel::framework::timer::irq::register_timer_irq_tests();
        crate::kernel::framework::timer::sleep::register_timer_sleep_tests();
        crate::kernel::framework::timer::hrtimer::register_hrtimer_tests();
        crate::kernel::framework::proc::signal::register_signal_tests();
        crate::kernel::framework::config::memory::register_aslr_tests();
        crate::kernel::framework::fs::initramfs::register_initramfs_tests();
        crate::kernel::framework::syscall::futex::register_futex_tests();
        crate::kernel::framework::mm::pcache::register_pcache_tests();
        crate::kernel::framework::mm::swap::register_swap_tests();
        crate::kernel::framework::pci::msi::register_msi_tests();
        crate::kernel::framework::syscall::epoll::register_epoll_tests();
        crate::kernel::framework::syscall::eventfd::register_eventfd_tests();
        crate::kernel::framework::syscall::signalfd::register_signalfd_tests();
        crate::kernel::framework::syscall::timerfd::register_timerfd_tests();
        crate::kernel::framework::syscall::sendfile::register_sendfile_tests();
    }

    let r = runner();
    let count = r.registry.lock().count;
    crate::klog_boot_info!("[TEST] Registered {} test cases", count);

    // 诊断: test 运行前检查页表
    {
        let read_u64 = |phys: u64, idx: usize| -> u64 {
            let va = phys + crate::kernel::framework::mm::KERNEL_BASE + idx as u64 * 8;
            // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
            unsafe { core::ptr::read_volatile(va as *const u64) }
        };
        let pd24 = read_u64(0x109000, 24);
        let pd63 = read_u64(0x109000, 63);
        crate::klog_boot_info!(
            "[PAGETABLE] before run_all: pd[24]=0x{:016X} pd[63]=0x{:016X}",
            pd24,
            pd63
        );
    }

    r.run_all();

    let p = r.passed.load(Ordering::Relaxed);
    let f = r.failed.load(Ordering::Relaxed);
    if f == 0 {
        crate::klog_boot_info!(
            "[TEST] ALL TESTS PASSED ({}/{})",
            p,
            p + r.skipped.load(Ordering::Relaxed)
        );
    } else {
        crate::klog_boot_info!("[TEST] COMPLETE: {} passed, {} FAILED", p, f);
    }
}

pub fn qemu_exit(success: bool) -> ! {
    #[cfg(target_arch = "x86_64")]
    {
        let exit_code = if success { 0x10 } else { 0x11 };
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            use core::arch::asm;
            asm!(
                "out dx, al",
                in("dx") 0xf4u16,
                in("al") exit_code as u8,
                options(nomem, nostack)
            );
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = success;
    }
    loop {
        crate::arch!(halt());
    }
}
