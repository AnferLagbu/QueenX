//! SMP (对称多处理) 支持
//!
//! 多处理器初始化、IPI 与每 CPU 状态管理.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static SMP_ENABLED: AtomicBool = AtomicBool::new(false);
static CPU_COUNT: AtomicU32 = AtomicU32::new(1);
static BSP_ID: AtomicU32 = AtomicU32::new(0);

static CPU_APIC_IDS: [AtomicU32; crate::framework::config::MAX_CPUS] =
    [const { AtomicU32::new(0xFFFF) }; crate::framework::config::MAX_CPUS];

static CPU_ONLINE: [AtomicBool; crate::framework::config::MAX_CPUS] =
    [const { AtomicBool::new(false) }; crate::framework::config::MAX_CPUS];

#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
pub fn init() {
    let bsp_apic_id = crate::arch!(cpu_id());
    BSP_ID.store(bsp_apic_id, Ordering::Release);

    CPU_APIC_IDS[0].store(bsp_apic_id, Ordering::Release);
    CPU_ONLINE[0].store(true, Ordering::Release);
    CPU_COUNT.store(1, Ordering::Release);

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        crate::framework::klog::klog_info(c"[SMP] BSP initialized".as_ptr() as *const i8);
    }
}

pub fn is_enabled() -> bool {
    SMP_ENABLED.load(Ordering::Acquire)
}

pub fn get_cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Acquire)
}

pub fn get_current_cpu() -> u32 {
    crate::arch!(cpu_id())
}

pub fn register_cpu(apic_id: u32) -> bool {
    let count = CPU_COUNT.fetch_add(1, Ordering::AcqRel);
    if count as usize >= crate::framework::config::MAX_CPUS {
        CPU_COUNT.fetch_sub(1, Ordering::AcqRel);
        return false;
    }

    CPU_APIC_IDS[count as usize].store(apic_id, Ordering::Release);
    CPU_ONLINE[count as usize].store(true, Ordering::Release);
    SMP_ENABLED.store(true, Ordering::Release);
    true
}

pub fn is_cpu_online(cpu_index: u32) -> bool {
    if cpu_index as usize >= crate::framework::config::MAX_CPUS {
        return false;
    }
    CPU_ONLINE[cpu_index as usize].load(Ordering::Acquire)
}

pub fn get_apic_id(cpu_index: u32) -> u32 {
    if cpu_index as usize >= crate::framework::config::MAX_CPUS {
        return 0xFFFF;
    }
    CPU_APIC_IDS[cpu_index as usize].load(Ordering::Acquire)
}

pub fn send_tlb_invalidate_ipi(target_apic_id: u8) {
    crate::arch!(send_ipi(u32::from(target_apic_id), 0xFD));
}

pub fn send_broadcast_ipi(vector: u8) {
    crate::arch!(broadcast_ipi(vector));
}

pub fn broadcast_tlb_invalidate() {
    if is_enabled() {
        send_broadcast_ipi(0xFD);
    }
}

pub fn send_reschedule_ipi(target_apic_id: u8) {
    crate::arch!(send_ipi(u32::from(target_apic_id), 0xFE));
}

pub fn broadcast_reschedule() {
    if is_enabled() {
        send_broadcast_ipi(0xFE);
    }
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn smp_init() {
    init();
}
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn smp_is_enabled() -> bool {
    is_enabled()
}
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn smp_get_cpu_count() -> u32 {
    get_cpu_count()
}
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn smp_get_current_cpu() -> u32 {
    get_current_cpu()
}
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn smp_register_cpu(apic_id: u32) -> bool {
    register_cpu(apic_id)
}
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn smp_send_tlb_invalidate_ipi(target_apic_id: u8) {
    send_tlb_invalidate_ipi(target_apic_id);
}
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn smp_broadcast_tlb_invalidate() {
    broadcast_tlb_invalidate();
}
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn smp_send_reschedule_ipi(target_apic_id: u8) {
    send_reschedule_ipi(target_apic_id);
}
