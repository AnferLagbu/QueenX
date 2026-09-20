//! SMP (对称多处理) 支持
//!
//! 多处理器初始化、IPI 与每 CPU 状态管理.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

static SMP_ENABLED: AtomicBool = AtomicBool::new(false);
static CPU_COUNT: AtomicU32 = AtomicU32::new(1);
static BSP_ID: AtomicU32 = AtomicU32::new(0);

static CPU_APIC_IDS: [AtomicU32; crate::framework::config::MAX_CPUS] =
    [const { AtomicU32::new(0xFFFF) }; crate::framework::config::MAX_CPUS];

static CPU_ONLINE: [AtomicBool; crate::framework::config::MAX_CPUS] =
    [const { AtomicBool::new(false) }; crate::framework::config::MAX_CPUS];

// 全局 TLB 失效代: 单调递增, 由页表修改批次在其末尾发布.
static TLB_GEN: AtomicU64 = AtomicU64::new(0);

// 每核"已追平代": 本核最后一次全量 TLB 失效时观察到的 TLB_GEN 值.
static CPU_TLB_GEN: [AtomicU64; crate::framework::config::MAX_CPUS] =
    [const { AtomicU64::new(0) }; crate::framework::config::MAX_CPUS];

/// `tlb_gen_publish_and_shoot` 被调用的次数, 即真实的跨核 TLB 失效批次次数.
///
/// 仅用于可观测性/测试断言 (Makefile 依据打印行统计), 不参与任何控制逻辑.
static TLB_SHOOTDOWN_COUNT: AtomicU64 = AtomicU64::new(0);

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
    CPU_TLB_GEN[count as usize].store(TLB_GEN.load(Ordering::Acquire), Ordering::Release);
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

/// 读取当前全局 TLB 失效代.
///
/// 供接收侧在 flush **之前**调用: 先读到当代, 待全量失效完成后再以
/// [`tlb_gen_set_self`] 声明已追平该代; 次序不可颠倒, 反序会"假追平".
pub fn tlb_gen_now() -> u64 {
    TLB_GEN.load(Ordering::Acquire)
}

/// 发布新代并返回新代值 (单调递增).
///
/// 页表修改批次在其末尾调用; 调用方随后须通知全部在线核失效.
pub fn tlb_gen_publish() -> u64 {
    TLB_GEN.fetch_add(1, Ordering::AcqRel) + 1
}

/// 声明本核已追平代 `g`.
///
/// 接收侧完成全量 TLB 失效后调用, `g` 须为 flush **之前**读到的代;
/// 核索引越界时直接返回 (不得 panic), 越界核不参与 [`tlb_gen_min_online`] 统计.
pub fn tlb_gen_set_self(g: u64) {
    let cpu = get_current_cpu() as usize;
    if cpu >= crate::framework::config::MAX_CPUS {
        return;
    }
    CPU_TLB_GEN[cpu].store(g, Ordering::Release);
}

/// 返回全部**在线**核已追平代的最小值.
///
/// 仅统计 `CPU_ONLINE[i] == true` 的槽位: 离线槽位不得参与取 min, 否则最小值会
/// 永久卡在旧值. 若一个在线槽位都没有, 返回 `u64::MAX` (fail-closed: 宁可永不
/// 判定"可释放", 也绝不误判为可释放).
pub fn tlb_gen_min_online() -> u64 {
    let cpu_count = get_cpu_count();
    let mut min_gen = u64::MAX;
    for i in 0..cpu_count {
        if CPU_ONLINE[i as usize].load(Ordering::Acquire) {
            let g = CPU_TLB_GEN[i as usize].load(Ordering::Acquire);
            if g < min_gen {
                min_gen = g;
            }
        }
    }
    min_gen
}

/// 发布新代并向全部在线核 (含本核) 发送 `0xFD` 定向 IPI, 返回新代值.
///
/// 发布必须先于发 IPI: 对端收到 IPI 即会读代, 反序会读到旧代. IPI 集合含本核,
/// 使发送侧与接收侧走完全相同的"读代 → 全量 flush → 声明"路径. 本函数**不等待**:
/// 不引入任何自旋或 ack, 并发发布各自得到不同代, 无需互斥.
pub fn tlb_gen_publish_and_shoot() -> u64 {
    let g = tlb_gen_publish();
    let cpu_count = get_cpu_count();
    let mut targets = 0u64;
    for i in 0..cpu_count {
        if CPU_ONLINE[i as usize].load(Ordering::Acquire) {
            send_tlb_invalidate_ipi(get_apic_id(i) as u8);
            targets += 1;
        }
    }
    // SIMPLIFIED: 每次 shootdown 都打印一行日志 (无采样/无阈值); 高频 unmap 负载下串口输出
    // 量线性增长, 可能拖慢该路径; 当 shootdown 成为热路径或日志刷屏时, 改为首次/每 N 次采样打印.
    let count = TLB_SHOOTDOWN_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    crate::klog_info!(Kernel, "[SMP] TLB shootdown #{} gen={} targets={}", count, g, targets);
    g
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
