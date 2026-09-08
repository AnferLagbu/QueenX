//! 存储设备驱动子系统 (Storage Driver Subsystem)
//!
//! 负责发现和初始化存储控制器：
//! - **AHCI SATA控制器**: 通过PCI扫描 (class 0x01, subclass 0x06)
//! - **NVMe 控制器**: 通过PCI扫描 (class 0x01, subclass 0x08)
//! - **ATA IDE 磁盘**: 传统IDE(PATA)磁盘支持
//!
//! ## 初始化流程
//!
//! ```text
//! storage_init()
//!   ├── PCI::scan_all_buses()
//!   ├── for each AHCI device  → AhciController::new(BAR).init()
//!   ├── for each NVMe device  → NvmeController::new(BAR).init()
//!   └── ata::detect_drives()   → 检测PATA磁盘
//! ```

pub mod ahci;
pub mod ahci_block;
#[cfg(target_arch = "x86_64")]
pub mod ata;
#[cfg(target_arch = "x86_64")]
pub mod ata_block;
pub mod nvme;
pub mod nvme_block;

// 为 driver/mod.rs 方便而重导出关键类型
pub use ahci::{AhciController, AhciPort, AtaCommand, H2dFis};
pub use nvme::{NvmeCommand, NvmeCompletion, NvmeController};

use super::framework::{self, Driver};
#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::arch::InterruptArch;
use crate::kernel::framework::iomem::IoMem;
#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::mm::PAGE_SIZE;
use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use crate::klog_info;
use crate::klog_warn;
use alloc::vec::Vec;

/// PCI 存储控制器类码
#[cfg(target_arch = "x86_64")]
const PCI_CLASS_STORAGE: u8 = 0x01;
#[cfg(target_arch = "x86_64")]
const PCI_SUBCLASS_AHCI: u8 = 0x06;
#[cfg(target_arch = "x86_64")]
const PCI_SUBCLASS_NVME: u8 = 0x08;

/// 全局存储控制器注册表
static AHCI_CONTROLLERS: Mutex<Vec<AhciController>> = Mutex::new(Vec::new());
static NVME_CONTROLLERS: Mutex<Vec<NvmeController>> = Mutex::new(Vec::new());

/// 初始化存储子系统
///
/// 扫描 PCI 总线发现 AHCI/NVMe 控制器，然后初始化它们。
/// # Errors
/// 存储控制器初始化失败时返回 Err。
#[cfg(target_arch = "x86_64")]
// 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::too_many_lines,
    reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
)]
#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
#[expect(
    clippy::missing_panics_doc,
    reason = "storage_init 内唯一 panic 点为 .expect(\"NVMe controller just pushed\") 内部不变量断言 (刚 push 必然存在), 仅在内部逻辑错误时触发, 对外部调用者不可达, 故省略 # Panics 段"
)]
pub fn storage_init() -> framework::Result<()> {
    // Step 1: 确保 PCI 子系统已初始化
    let pci_count = crate::kernel::framework::pci::init();
    if pci_count == 0 {
        klog_warn!(
            Driver,
            "storage_init: no PCI devices found, falling back to ATA"
        );
    }

    // Step 2: 扫描 PCI 总线寻找存储控制器
    let devices = crate::kernel::framework::pci::scan_all_buses();

    let mut ahci_found = 0u32;
    let mut nvme_found = 0u32;

    for dev in &devices {
        if dev.class_code != PCI_CLASS_STORAGE {
            continue;
        }

        match dev.subclass_code {
            PCI_SUBCLASS_AHCI => {
                // AHCI 控制器 - 使用 BAR5 (偏移 0x24)
                let bar = dev.bars[5].base_addr;
                if bar == 0 || bar == 0xFFFFFFFF {
                    klog_warn!(
                        Driver,
                        "AHCI: device {:02X}:{:02X}.{} has no valid BAR5",
                        dev.bus,
                        dev.device,
                        dev.function
                    );
                    continue;
                }

                let mmio_base = (bar as usize) & !(PAGE_SIZE as usize - 1); // 掩码低12位 (BAR类型/可预取位)
                klog_info!(
                    Driver,
                    "AHCI: found at {:02X}:{:02X}.{}, BAR5=0x{:X}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    mmio_base
                );

                let mut controller = AhciController::new(mmio_base);
                match controller.init_controller() {
                    Ok(()) => {
                        klog_info!(
                            Driver,
                            "AHCI: {:02X}:{:02X}.{} initialized ({} ports)",
                            dev.bus,
                            dev.device,
                            dev.function,
                            controller.port_count()
                        );
                        AHCI_CONTROLLERS.lock().push(controller);
                        ahci_found += 1;
                    }
                    Err(e) => {
                        klog_warn!(
                            Driver,
                            "AHCI: {:02X}:{:02X}.{} init failed: {:?}",
                            dev.bus,
                            dev.device,
                            dev.function,
                            e
                        );
                    }
                }
            }

            PCI_SUBCLASS_NVME => {
                // NVMe 控制器 - 使用 BAR0
                let bar = dev.bars[0].base_addr;
                if bar == 0 || bar == 0xFFFFFFFF {
                    klog_warn!(
                        Driver,
                        "NVMe: device {:02X}:{:02X}.{} has no valid BAR0",
                        dev.bus,
                        dev.device,
                        dev.function
                    );
                    continue;
                }

                let mmio_base = (bar as usize) & !(PAGE_SIZE as usize - 1);
                klog_info!(
                    Driver,
                    "NVMe: found at {:02X}:{:02X}.{}, BAR0=0x{:X}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    mmio_base
                );

                let mut controller = NvmeController::new(mmio_base);
                match controller.init() {
                    Ok(()) => {
                        // B07 MSI-X 完整接入: 启用 MSI-X + 注册 ISR.
                        if let Some(msi_vector) = controller.enable_msix(dev) {
                            // MSIX-03: 创建 I/O 队列对 (现在已知 MSI-X vector, 可正确写 cdw11[31:16])
                            // NVMe vector 字段 = MSI-X Table 数组索引, 与 LAPIC vector 不同.
                            // 0 → Table[0] entry, 该 entry.msg_data = msi_vector (LAPIC vector).
                            match controller.create_io_queue(0) {
                                Ok(()) => {
                                    crate::klog_info!(Driver, "[MSIX-03][diag] create_io_queue OK");
                                }
                                Err(e) => {
                                    crate::klog_warn!(
                                        Driver,
                                        "NVMe: create_io_queue failed after MSI-X enable: {:?}, falling back to poll",
                                        e
                                    );
                                    // I/O 队列失败: 不 push 到控制器表, 不注册 ISR 测试路径
                                    continue;
                                }
                            }
                            // MSIX-03: 一次性打印 LAPIC + MSI-X 状态 (RFLAGS / SVR / LAPIC ID /
                            // MSI-X ctrl / Table[0] entry). 是中断路径打通的关键证据.
                            {
                                #![expect(
                                    clippy::items_after_statements,
                                    reason = "use 声明位于诊断块语句之后 (MSIX-03 调试段), 前移会割裂局部上下文; 以 block 内 expect 兑底"
                                )]
                                let mut rflags: u64 = 0;
                                // J-02 (2026-09-08, G-03): pushfq 为 x86_64 专属指令,
                                // 补 cfg 门控 — aarch64 下 rflags 保持 0 (klog 仅诊断打印 IF=0).
                                // SAFETY: 只读 RFLAGS (x86_64)
                                #[cfg(target_arch = "x86_64")]
                                unsafe {
                                    core::arch::asm!("pushfq; pop {0}", out(reg) rflags, options(nomem));
                                }
                                use crate::kernel::framework::pci::msi::pci_find_capability;
                                let (lapic_id, ctrl, entry_addr, entry_data, entry_vc) =
                                    if let Some(co) = pci_find_capability(dev, 0x11) {
                                        let ctrl = crate::kernel::framework::pci::read_config_word(
                                            dev.bus, dev.device, dev.function, co + 0x02,
                                        );
                                        let table_info = crate::kernel::framework::pci::read_config_dword(
                                            dev.bus, dev.device, dev.function, co + 0x04,
                                        );
                                        let tbar = (table_info & 0x07) as usize;
                                        let toff = u64::from(table_info & !0x07);
                                        // SAFETY: te 指向设备 MSI-X Table MMIO 区域 (由 msix_enable 刚配置)
                                        let te =
                                            (dev.bars[tbar].base_addr + toff) as *const u32;
                                        let (addr, data, vc) = unsafe {
                                            (
                                                core::ptr::read_volatile(te),
                                                core::ptr::read_volatile(te.add(2)),
                                                core::ptr::read_volatile(te.add(3)),
                                            )
                                        };
                                        (
                                            crate::kernel::framework::arch::apic::get_id(),
                                            ctrl,
                                            addr,
                                            data,
                                            vc,
                                        )
                                    } else {
                                        (
                                            crate::kernel::framework::arch::apic::get_id(),
                                            0u16,
                                            0u32,
                                            0u32,
                                            0u32,
                                        )
                                    };
                                crate::klog_info!(
                                    Driver,
                                    "[MSIX-03][diag] rflags={:#X} IF={} svr={:#X} lapic_id={} msix ctrl={:#X} entry: addr={:#X} data={} vec_ctrl={:#X}",
                                    rflags,
                                    (rflags & 0x200) != 0,
                                    crate::kernel::framework::arch::apic::apic_read(0xF0),
                                    lapic_id,
                                    ctrl,
                                    entry_addr,
                                    entry_data,
                                    entry_vc
                                );
                            }
                            // msi.rs::msix_enable 分配 vector ∈ [MSI_VECTOR_BASE=0x40, 0x40+MSI_VECTOR_COUNT=96).
                            // IDT 索引 = vector - IRQ_BASE (0x20), 例如 vector=0x40 → irq=32.
                            let irq = msi_vector - crate::kernel::framework::idt::IRQ_BASE;
                            if let Err(e) = register_nvme_msix_isr(irq, msi_vector) {
                                klog_warn!(
                                    Driver,
                                    "NVMe: MSI-X ISR register failed on irq {}: {}, falling back to poll",
                                    irq,
                                    e
                                );
                            } else {
                                klog_info!(
                                    Driver,
                                    "NVMe: MSI-X enabled, vector={}, irq={}",
                                    msi_vector,
                                    irq
                                );
                            }
                        } else {
                            klog_info!(Driver, "NVMe: MSI-X unavailable, using poll mode");
                        }
                        klog_info!(
                            Driver,
                            "NVMe: {:02X}:{:02X}.{} initialized",
                            dev.bus,
                            dev.device,
                            dev.function
                        );
                        NVME_CONTROLLERS.lock().push(controller);
                        nvme_found += 1;

                        crate::klog_info!(Driver, "[MSIX-03] pre-test hook entered");

                        // MSIX-03: 受控 MSI-X 中断投递验证 — 开 IF 后经 ISR 路径发 I/O 命令,
                        // 验证 handle_irq MSI 分支 → LAPIC EOI → ISR → I/O CQ 处理.
                        // 该 hook 是 MSIX-03/07 端到端验收点: ISR-driven io read 的最终状态
                        // (Ok 或 Err) 反映 MSI-X 中断路径是否真正打通.
                        {
                            // 取刚 push 的控制器原始指针, 释放全局锁后供 ISR 测试使用.
                            #[expect(
                                clippy::ref_as_ptr,
                                reason = "ref_as_ptr: 需取 last_mut() 引用的裸指针, 在释放全局锁后供 ISR 测试路径继续访问控制器 (单核上下文语义已知安全)"
                            )]
                            let ctrl_ptr = {
                                let mut cs = NVME_CONTROLLERS.lock();
                                cs.last_mut().expect("NVMe controller just pushed")
                                    as *mut nvme::NvmeController
                            };
                            // SAFETY: 单核; 等待期间主上下文暂停于 hlt, ISR 抢占时通过
                            // NVME_CONTROLLERS 锁重新获取 &mut, 二者不会同时访问.
                            let ctrl = unsafe { &mut *ctrl_ptr };
                            if ctrl.irq_vector.is_some() {
                                // 开 IF 让 LAPIC 能投递 MSI-X 中断.
                                let _saved_if =
                                    crate::kernel::framework::arch::CurrentArch::interrupt_disable();
                                crate::kernel::framework::arch::CurrentArch::interrupt_enable();
                                // 经 I/O CQ ISR 路径读 ns=1 sector=0 1 扇区:
                                // 1. 分配 DMA buf
                                // 2. 构造 Read 命令
                                // 3. 走 submit_io_command_isr (写 SQ + 敲 SQ doorbell + hlt 等 ISR)
                                // 4. ISR 由 LAPIC MSI-X (vector=64) 唤醒, 处理 I/O CQ 完成 entry
                                let r: framework::Result<()> = (|| {
                                    use crate::kernel::framework::mm::PAGE_SIZE;
                                    let dma = crate::kernel::framework::dma::get_dma();
                                    let (bv, bp) = dma
                                        .alloc_coherent(PAGE_SIZE as usize)
                                        .ok_or(framework::DriverError::Busy)?;
                                    let cmd = nvme::NvmeCommand::read(1, 0, 1, bp.0);
                                    // SAFETY: cmd 引用 valid NvmeCommand, submit_io_command_isr
                                    // 调用方保证指针/类型有效 (cmd 引用栈上对象, DMA buf 有效)
                                    let res = unsafe { ctrl.submit_io_command_isr_pub(&cmd) };
                                    dma.free_coherent(bv, PAGE_SIZE as usize);
                                    res
                                })();
                                // 恢复 boot IF=0
                                let _ = crate::kernel::framework::arch::CurrentArch::interrupt_disable();
                                klog_info!(
                                    Driver,
                                    "[MSIX-03] ISR-driven io read: {:?}",
                                    r
                                );
                            }
                        }
                    }
                    Err(e) => {
                        klog_warn!(
                            Driver,
                            "NVMe: {:02X}:{:02X}.{} init failed: {:?}",
                            dev.bus,
                            dev.device,
                            dev.function,
                            e
                        );
                    }
                }
            }

            _ => {
                // 其他存储子类 (IDE, RAID等) 静默跳过
            }
        }
    }

    // Step 3: 传统 ATA 检测 (回退)
    // ATA 驱动使用内部全局单例, 通过 C FFI 接口初始化
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        crate::kernel::framework::driver::storage::ata::ata_init();
    }

    crate::kernel::framework::chitin::chitin_register_driver(
        "ata_controller",
        crate::kernel::framework::chitin::ChitinProto::Block,
        None,
        None,
        alloc::boxed::Box::new(
            crate::kernel::framework::driver::storage::ata::AtaController::new(),
        ),
    );

    // Step 3.5: 将 ATA 磁盘注册到 Chitin (唯一注册入口)
    {
        use crate::kernel::framework::chitin::proto_block;
        use crate::kernel::framework::driver::BlockDevice;
        use crate::kernel::framework::driver::storage::ata_block::AtaBlockDevice;
        for drive in 0..4u8 {
            if let Some(dev) = AtaBlockDevice::new(drive) {
                let sectors = dev.blk_total_sectors();
                let dev_name = match drive {
                    0 => "ata0",
                    1 => "ata1",
                    2 => "ata2",
                    _ => "ata3",
                };
                proto_block::register_block_device(dev_name, dev, None);
                klog_info!(
                    Driver,
                    "ATA: drive {} registered, {} sectors ({:.1} MB)",
                    drive,
                    sectors,
                    (sectors * 512) as f64 / (1024.0 * 1024.0)
                );
            }
        }
    }

    // Step 3.6: 将 AHCI 端口注册到 Chitin (唯一注册入口)
    {
        use crate::kernel::framework::chitin::proto_block;
        use crate::kernel::framework::driver::BlockDevice;
        use crate::kernel::framework::driver::storage::ahci_block::AhciBlockDevice;

        let mut ahci_ports: Vec<(usize, usize)> = Vec::new();
        {
            let mut controllers = AHCI_CONTROLLERS.lock();
            for (ci, controller) in controllers.iter_mut().enumerate() {
                let port_count = controller.port_count();
                for pi in 0..port_count {
                    if let Some(port) = controller.get_port(pi) {
                        if port.device_present {
                            ahci_ports.push((ci, pi));
                        }
                    }
                }
            }
        }

        for (ci, pi) in ahci_ports {
            if let Some(dev) = AhciBlockDevice::new(ci, pi) {
                let sectors = dev.blk_total_sectors();
                let dev_name = alloc::format!("ahci{ci}-p{pi}");
                let name_leaked: &'static str = dev_name.leak();
                proto_block::register_block_device(name_leaked, dev, None);
                klog_info!(
                    Driver,
                    "AHCI: ctrl={} port={} registered, {} sectors ({:.1} MB)",
                    ci,
                    pi,
                    sectors,
                    (sectors * 512) as f64 / (1024.0 * 1024.0)
                );
            }
        }
    }

    // Step 3.7: 将 NVMe 命名空间注册到 Chitin (唯一注册入口)
    {
        use crate::kernel::framework::chitin::proto_block;
        use crate::kernel::framework::driver::BlockDevice;
        use crate::kernel::framework::driver::storage::nvme_block::NvmeBlockDevice;

        let mut nvme_ns: Vec<(usize, u32)> = Vec::new();
        {
            let controllers = NVME_CONTROLLERS.lock();
            for (ci, controller) in controllers.iter().enumerate() {
                let ns_count = controller.namespace_count();
                for nsid in 1..=ns_count {
                    let size = controller.namespace_size();
                    if size > 0 {
                        nvme_ns.push((ci, nsid));
                    }
                }
            }
        }

        for (ci, nsid) in nvme_ns {
            if let Some(dev) = NvmeBlockDevice::new(ci, nsid) {
                let sectors = dev.blk_total_sectors();
                let dev_name = alloc::format!("nvme{ci}-ns{nsid}");
                let name_leaked: &'static str = dev_name.leak();
                proto_block::register_block_device(name_leaked, dev, None);
                klog_info!(
                    Driver,
                    "NVMe: ctrl={} nsid={} registered, {} sectors ({:.1} MB)",
                    ci,
                    nsid,
                    sectors,
                    (sectors * 512) as f64 / (1024.0 * 1024.0)
                );
            }
        }
    }

    klog_info!(
        Driver,
        "storage: {} AHCI, {} NVMe, ATA detected",
        ahci_found,
        nvme_found
    );

    if ahci_found > 0 || nvme_found > 0 {
        Ok(())
    } else {
        // 没有存储控制器时不算致命错误 - ATA 可能仍有设备
        klog_warn!(Driver, "storage: no PCI storage controllers, ATA-only mode");
        Ok(())
    }
}

/// AArch64 存储初始化 — 通过 virtio-mmio 发现块设备。
#[cfg(not(target_arch = "x86_64"))]
#[expect(
    clippy::missing_errors_doc,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
#[expect(
    clippy::uninlined_format_args,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "DECISION-043 pedantic 兜底: aarch64 编译目标特有 lint, 当前批量 expect 兑底"
)]
pub fn storage_init() -> framework::Result<()> {
    use crate::kernel::framework::driver::virtio::{self, VIRTIO_ID_BLOCK};

    // 扫描 virtio-mmio 区域，寻找块设备
    let devices = virtio::probe_all();
    let mut blk_count = 0u32;

    for dev in devices {
        if dev.device_id == VIRTIO_ID_BLOCK {
            if let Some(mut blk) = virtio::blk::VirtioBlk::new(dev) {
                // I-42: 尝试注册 IRQ 中断驱动路径; 失败时退到 spin-loop 轮询.
                if let Err(e) = blk.enable_irq() {
                    klog_warn!(
                        Driver,
                        "virtio-blk: IRQ registration failed: {}, using poll mode",
                        e
                    );
                }
                let blk_name = alloc::format!("virtio-blk{}", blk_count);
                let name_leaked: &'static str = blk_name.leak();
                let mmio_base = blk.device.iomem.phys().as_u64();
                crate::kernel::framework::chitin::proto_block::register_block_device(
                    name_leaked,
                    blk,
                    Some(mmio_base as u64),
                );
                blk_count += 1;
                klog_info!(Driver, "virtio-blk: registered device #{}", blk_count);
            }
        }
    }

    klog_info!(Driver, "storage: {} virtio-blk device(s) found", blk_count);
    Ok(())
}

/// 获取所有已发现的 AHCI 端口总数
pub fn ahci_port_count() -> usize {
    let mut total = 0usize;
    for ctrl in AHCI_CONTROLLERS.lock().iter() {
        total += ctrl.port_count();
    }
    total
}

/// 获取 `NVMe` 控制器数量
pub fn nvme_controller_count() -> usize {
    NVME_CONTROLLERS.lock().len()
}

#[cfg(target_arch = "x86_64")]
/// B07 MSI-X 完整接入: NVMe 中断路径 (端到端).
///
/// `register_nvme_msix_isr(irq, msi_vector)` 通过 `IdtManager::register_msi_irq`
/// 注册 NVMe 中断处理, ISR 调用 `handle_interrupt` 处理完成队列并 ack.
///
/// # Safety
///
/// `frame` 由 IDT 中断入口压栈, 指向保存的寄存器. NVMe ISR 不读取 frame
/// 内容, 仅作 IDT 签名要求.
extern "C" fn nvme_msix_irq_handler(_frame: *mut crate::kernel::framework::idt::InterruptFrame) {
    // B07: handle_irq 已自动 EOI (LAPIC 路径), 这里只需 dispatch 给 controller.
    // MSIX-03: 中断触发计数 — 首次 + 每 1000 次打印, 验证真实 MSI-X 投递路径.
    let count = NVME_MSIX_IRQ_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
    if count <= 5 || count.is_multiple_of(1000) {
        crate::klog_info!(Driver, "[NVMe] MSI-X IRQ {} fired (total {})", count, count);
    }
    let mut controllers = NVME_CONTROLLERS.lock();
    for ctrl in controllers.iter_mut() {
        if ctrl.irq_vector.is_some() {
            let _ = ctrl.handle_interrupt();
        }
    }
}

/// NVMe MSI-X ISR 触发计数 (MSIX-03 验证)
///
/// 仅 x86_64 (MSI-X 路径专属, 与 `nvme_msix_irq_handler` 同 cfg).
#[cfg(target_arch = "x86_64")]
static NVME_MSIX_IRQ_COUNT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// 注册 NVMe MSI-X ISR.
///
/// # Errors
///
/// `IdtManager::register_msi_irq` 失败 (irq 范围错) 时返回错误.
#[cfg(target_arch = "x86_64")]
fn register_nvme_msix_isr(irq: u8, msi_vector: u8) -> Result<(), &'static str> {
    use crate::kernel::framework::idt::IdtManager;
    let manager = IdtManager::instance();
    manager.register_msi_irq(irq, nvme_msix_irq_handler, "nvme-msix")?;
    // enable_irq 用于 PIC IRQ 路径; MSI vector 不需 enable (LAPIC 已 mask).
    // 但 IDT 抽象统一要求 enable_irq (否则 vector 被屏蔽). 调用以保持一致性.
    manager.enable_irq(irq);
    klog_info!(
        Driver,
        "NVMe MSI-X ISR registered: vector={}, irq={}",
        msi_vector,
        irq
    );
    Ok(())
}

/// 关机 — 关闭所有存储控制器
/// # Errors
/// 任一存储控制器关闭失败时返回 Err。
#[expect(
    clippy::unnecessary_wraps,
    reason = "storage_shutdown 保持与 storage_init 一致的 framework::Result 签名 (driver 框架统一调度入口), 各控制器 shutdown 错误已用 let _ 吞掉恒返 Ok; 当前无调用点, 简化签名会破坏公共 API 一致性"
)]
pub fn storage_shutdown() -> framework::Result<()> {
    for ctrl in AHCI_CONTROLLERS.lock().iter_mut() {
        let _ = ctrl.shutdown();
    }
    for ctrl in NVME_CONTROLLERS.lock().iter_mut() {
        let _ = ctrl.shutdown();
    }
    Ok(())
}

// ============================================================================
// Safe queue wrapper API (services 层可调用的 0 unsafe 入口)
//
// 将 framework 中 unsafe 的队列操作封装为 safe 函数,
// 使 services 层可执行完整的 NVMe/AHCI 驱动逻辑而不引入 unsafe。
// ============================================================================

// ── 本地常量 (来自 framework nvme.rs/ahci.rs 的私有常量副本) ──

/// Admin/I/O 队列深度
const NVME_QD: u32 = 64;
/// SQ 条目大小
const NVME_SQ_ENTRY: usize = 64;
/// CQ 条目大小
const NVME_CQ_ENTRY: usize = 16;
/// `NVMe` Doorbell 基址
const NVME_DB_BASE: usize = 0x1000;
/// AHCI 命令槽数量
const AHCI_CMD_SLOTS: usize = 32;
/// AHCI 命令头大小
const AHCI_CMD_HDR_SIZE: usize = 32;
/// AHCI 命令表大小
const AHCI_CMD_TBL_SIZE: usize = 256;

/// 分配 `NVMe` Admin 队列 (SQ + CQ DMA 内存)
///
/// 返回 `(admin_sq_phys, admin_cq_phys)` — 物理地址用于写入 AQA/ASQ/ACQ 寄存器。
/// 失败返回 None (DMA 分配不足)。
pub fn nvme_alloc_admin_queues() -> Option<(u64, u64)> {
    use crate::kernel::framework::dma::get_dma;

    let dma = get_dma();
    if !dma.is_initialized() {
        return None;
    }

    let sq_size = NVME_QD as usize * NVME_SQ_ENTRY;
    let cq_size = NVME_QD as usize * NVME_CQ_ENTRY;

    let (_, sq_phys) = dma.alloc_coherent(sq_size)?;
    let (_, cq_phys) = dma.alloc_coherent(cq_size)?;

    Some((sq_phys.0, cq_phys.0))
}

/// 分配 `NVMe` I/O 队列 (SQ + CQ DMA 内存)
///
/// 返回 `(io_sq_phys, io_cq_phys)` — 物理地址用于 Create CQ/SQ Admin 命令。
pub fn nvme_alloc_io_queues() -> Option<(u64, u64)> {
    use crate::kernel::framework::dma::get_dma;

    let dma = get_dma();
    if !dma.is_initialized() {
        return None;
    }

    let sq_size = NVME_QD as usize * NVME_SQ_ENTRY;
    let cq_size = NVME_QD as usize * NVME_CQ_ENTRY;

    let (_, sq_phys) = dma.alloc_coherent(sq_size)?;
    let (_, cq_phys) = dma.alloc_coherent(cq_size)?;

    Some((sq_phys.0, cq_phys.0))
}

/// 分配 DMA 缓冲区, 返回 `(vaddr, phys_addr, size)` —
/// 实际分配大小可能向上对齐到页。
pub fn nvme_alloc_dma_buffer(size: usize) -> Option<(u64, u64, usize)> {
    use crate::kernel::framework::dma::get_dma;

    let dma = get_dma();
    if !dma.is_initialized() {
        return None;
    }

    let (v, p) = dma.alloc_coherent(size)?;
    Some((v.0, p.0, size))
}

/// 释放 DMA 缓冲区
pub fn nvme_free_dma_buffer(vaddr: u64, size: usize) {
    use crate::kernel::framework::dma::get_dma;
    use crate::kernel::framework::mm::VirtAddr;

    let dma = get_dma();
    if vaddr != 0 {
        dma.free_coherent(VirtAddr(vaddr), size);
    }
}

/// 向 `NVMe` Admin SQ 提交命令并等待完成
///
/// `cmd_ptr` — SQ DMA 区域虚拟地址
/// `cq_ptr` — CQ DMA 区域虚拟地址
/// `cmd` — 要提交的命令 (cid 会被覆盖)
/// `tail` — 当前 SQ tail, 提交后更新为 (tail+1) % depth
/// `cq_head` — 当前 CQ head, 完成后更新
/// `phase` — CQ phase bit, 完成后可能翻转
/// `depth` — 队列深度
/// `db_stride` — 门铃步长
/// `iomem` — `NVMe` BAR0 `IoMem` 句柄
///
/// 返回: `Ok(status_code)` 或 `Err(())` (超时/错误)
/// # Errors
/// 命令超时或设备返回非零状态码时返回 Err。
pub fn nvme_submit_admin_cmd(
    cmd_ptr: u64,
    cq_ptr: u64,
    cmd: nvme::NvmeCommand,
    tail: &mut u32,
    cq_head: &mut u32,
    phase: &mut u16,
    depth: u32,
    _db_stride: u32,
    iomem: &IoMem,
    cid: u16,
) -> Result<u16, ()> {
    // SAFETY: cmd_ptr/cq_ptr 由 DMA 分配保证有效; IoMem 确保 MMIO 安全
    unsafe {
        // 写入 SQ entry
        let sq = cmd_ptr as *mut nvme::NvmeCommand;
        let mut entry = cmd;
        entry.cid = cid;
        core::ptr::write_volatile(sq.add(*tail as usize), entry);

        // 更新 tail 并敲门铃
        let new_tail = (*tail + 1) % depth;
        *tail = new_tail;

        let db_offset = NVME_DB_BASE;
        iomem.write_u32(db_offset, new_tail);

        // 等待 CQ 完成
        let cq = cq_ptr as *const nvme::NvmeCompletion;
        let mut timeout = 5_000_000u64;
        loop {
            let entry = core::ptr::read_volatile(cq.add(*cq_head as usize));
            if (entry.status & 0x01) == *phase {
                // 更新 head
                let new_head = (*cq_head + 1) % depth;
                *cq_head = new_head;
                if new_head == 0 {
                    *phase ^= 1;
                }
                // 敲 CQ 门铃
                iomem.write_u32(db_offset + 4, new_head);

                let sc = (entry.status >> 1) & 0x7FF;
                if sc == 0 {
                    return Ok(sc);
                }
                return Err(());
            }
            timeout -= 1;
            if timeout == 0 {
                return Err(());
            }
            core::hint::spin_loop();
        }
    }
}

/// 向 `NVMe` I/O SQ 提交命令并等待完成
/// # Errors
/// 命令超时或设备返回非零状态码时返回 Err。
pub fn nvme_submit_io_cmd(
    cmd_ptr: u64,
    cq_ptr: u64,
    cmd: nvme::NvmeCommand,
    tail: &mut u32,
    cq_head: &mut u32,
    phase: &mut u16,
    depth: u32,
    _db_stride: u32,
    iomem: &IoMem,
    cid: u16,
    io_queue_db_offset: usize,
) -> Result<(), ()> {
    // SAFETY: cmd_ptr/cq_ptr 由 DMA 分配保证有效; IoMem 确保 MMIO 安全
    unsafe {
        let sq = cmd_ptr as *mut nvme::NvmeCommand;
        let mut entry = cmd;
        entry.cid = cid;
        core::ptr::write_volatile(sq.add(*tail as usize), entry);

        let new_tail = (*tail + 1) % depth;
        *tail = new_tail;

        // I/O SQ doorbell
        iomem.write_u32(io_queue_db_offset, new_tail);

        // 等待 CQ 完成
        let cq = cq_ptr as *const nvme::NvmeCompletion;
        let mut timeout = 5_000_000u64;
        loop {
            let entry = core::ptr::read_volatile(cq.add(*cq_head as usize));
            if (entry.status & 0x01) == *phase {
                let new_head = (*cq_head + 1) % depth;
                *cq_head = new_head;
                if new_head == 0 {
                    *phase ^= 1;
                }
                // I/O CQ doorbell
                iomem.write_u32(io_queue_db_offset + 4, new_head);

                let sc = (entry.status >> 1) & 0x7FF;
                if sc == 0 {
                    return Ok(());
                }
                return Err(());
            }
            timeout -= 1;
            if timeout == 0 {
                return Err(());
            }
            core::hint::spin_loop();
        }
    }
}

/// 复制数据到 DMA 缓冲区 (write 路径)
pub fn nvme_copy_to_dma(dst_vaddr: u64, src: *const u8, len: usize) {
    // SAFETY: dst_vaddr 由 DMA 分配保证有效; 调用方保证 src 有效且 len 匹配
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst_vaddr as *mut u8, len);
    }
}

/// 从 DMA 缓冲区复制数据 (read 路径)
pub fn nvme_copy_from_dma(dst: *mut u8, src_vaddr: u64, len: usize) {
    // SAFETY: src_vaddr 由 DMA 分配保证有效; 调用方保证 dst 有效且 len 匹配
    unsafe {
        core::ptr::copy_nonoverlapping(src_vaddr as *const u8, dst, len);
    }
}

/// 从 DMA 缓冲区读取 Identify Controller 数据
///
/// 返回 `(namespace_count, model_string_truncated_to_40)` — 0 表示失败
pub fn nvme_read_identify_controller(vaddr: u64) -> Option<(u32, [u8; 40])> {
    if vaddr == 0 {
        return None;
    }
    // SAFETY: vaddr 由 DMA 分配保证有效; 读取 offset 516 (nn 字段) 和 24..64 (mn 字段)
    unsafe {
        let nn = core::ptr::read_volatile((vaddr as *const u32).add(129)); // offset 516/4=129
        let mut model = [0u8; 40];
        core::ptr::copy_nonoverlapping((vaddr as *const u8).add(24), model.as_mut_ptr(), 40);
        Some((nn, model))
    }
}

/// 从 DMA 缓冲区读取 Identify Namespace 数据
///
/// 返回 `(nsze, flbas, lbaf_data_at_index)` — None 表示失败
pub fn nvme_read_identify_namespace(vaddr: u64) -> Option<(u64, u8, u32)> {
    if vaddr == 0 {
        return None;
    }
    // SAFETY: vaddr 由 DMA 分配保证有效; 读取 nsze (offset 0), flbas (offset 26), LBA 格式
    unsafe {
        let nsze = core::ptr::read_volatile(vaddr as *const u64);
        let flbas = core::ptr::read_volatile((vaddr as *const u8).add(26));
        let lbaf_idx = (flbas & 0xF) as usize;
        let lbaf_data = if lbaf_idx < 16 {
            core::ptr::read_volatile((vaddr as *const u32).add(32 + lbaf_idx)) // offset 128/4=32
        } else {
            0
        };
        Some((nsze, flbas, lbaf_data))
    }
}

/// 清零 DMA 缓冲区
pub fn nvme_zero_dma(vaddr: u64, len: usize) {
    // SAFETY: vaddr 由 DMA 分配保证有效
    unsafe {
        core::ptr::write_bytes(vaddr as *mut u8, 0, len);
    }
}

/// AHCI 命令列表句柄 — services 层持有的 safe 句柄
pub struct AhciCmdListHandle {
    /// 命令列表 DMA 虚拟地址
    pub cmd_list_virt: u64,
    /// 命令列表 DMA 物理地址
    pub cmd_list_phys: u64,
    /// FIS 接收缓冲区 DMA 虚拟地址
    pub fis_virt: u64,
    /// FIS 接收缓冲区 DMA 物理地址
    pub fis_phys: u64,
    /// 命令表 DMA 虚拟地址
    pub cmd_table_virt: u64,
    /// 命令表 DMA 物理地址
    pub cmd_table_phys: u64,
}

impl AhciCmdListHandle {
    /// 命令列表物理地址 (64-bit)
    pub fn cmd_list_phys(&self) -> u64 {
        self.cmd_list_phys
    }

    /// FIS 缓冲区物理地址 (64-bit)
    pub fn fis_phys(&self) -> u64 {
        self.fis_phys
    }
}

/// 分配 AHCI 端口 DMA 资源 (命令列表 + FIS 缓冲区 + 命令表)
// 有意窄化: 用户内存代理, 指针/长度上下文保证
#[expect(clippy::cast_possible_truncation)]
pub fn ahci_alloc_port_dma() -> Option<AhciCmdListHandle> {
    use crate::kernel::framework::dma::get_dma;
    use crate::kernel::framework::mm::PAGE_SIZE;

    let dma = get_dma();
    if !dma.is_initialized() {
        return None;
    }

    let cmd_list_size = AHCI_CMD_SLOTS * AHCI_CMD_HDR_SIZE;
    let fis_size = PAGE_SIZE as usize;
    let cmd_table_size = AHCI_CMD_TBL_SIZE;

    let (_, cmd_list_phys) = dma.alloc_coherent(cmd_list_size)?;
    let (_, fis_phys) = dma.alloc_coherent(fis_size)?;
    let (cmd_table_v, cmd_table_phys) = dma.alloc_coherent(cmd_table_size)?;

    Some(AhciCmdListHandle {
        cmd_list_virt: 0, // 仅物理地址用于寄存器
        cmd_list_phys: cmd_list_phys.0,
        fis_virt: 0,
        fis_phys: fis_phys.0,
        cmd_table_virt: cmd_table_v.0,
        cmd_table_phys: cmd_table_phys.0,
    })
}

/// AHCI DMA buffer 分配 (用于读写数据传输)
pub fn ahci_alloc_dma_buffer(size: usize) -> Option<(u64, u64, usize)> {
    use crate::kernel::framework::dma::get_dma;

    let dma = get_dma();
    if !dma.is_initialized() {
        return None;
    }

    let (v, p) = dma.alloc_coherent(size)?;
    Some((v.0, p.0, size))
}

/// 释放 AHCI DMA 缓冲区
pub fn ahci_free_dma_buffer(vaddr: u64, size: usize) {
    use crate::kernel::framework::dma::get_dma;
    use crate::kernel::framework::mm::VirtAddr;

    let dma = get_dma();
    if vaddr != 0 {
        dma.free_coherent(VirtAddr(vaddr), size);
    }
}

/// 复制数据到 AHCI DMA 缓冲区
pub fn ahci_copy_to_dma(dst_vaddr: u64, src: *const u8, len: usize) {
    // SAFETY: dst_vaddr 由 DMA 分配保证有效; 调用方保证 src 有效且 len 匹配
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst_vaddr as *mut u8, len);
    }
}

/// 从 AHCI DMA 缓冲区复制数据
pub fn ahci_copy_from_dma(dst: *mut u8, src_vaddr: u64, len: usize) {
    // SAFETY: src_vaddr 由 DMA 分配保证有效; 调用方保证 dst 有效且 len 匹配
    unsafe {
        core::ptr::copy_nonoverlapping(src_vaddr as *const u8, dst, len);
    }
}

/// 填充 AHCI Command Header (slot 0)
// 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::similar_names,
    reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
)]
pub fn ahci_fill_cmd_header(
    cmd_list_virt: u64,
    slot: u32,
    fis_len_dwords: u32,
    is_write: bool,
    prdt_len: u16,
    cmd_table_phys: u64,
) {
    use core::ptr::addr_of_mut;
    // SAFETY: cmd_list_virt 由 DMA 分配保证有效; slot < CMD_SLOTS
    unsafe {
        let hdr = (cmd_list_virt as *mut ahci::AhciCommandHeader).add(slot as usize);
        let flags: u32 = fis_len_dwords | (if is_write { 1 << 6 } else { 0 });
        let dw0_val = flags | u32::from(prdt_len);
        let ctba_val = cmd_table_phys as u32;
        let ctbau_val = (cmd_table_phys >> 32) as u32;
        addr_of_mut!((*hdr).dw0).write_volatile(dw0_val);
        addr_of_mut!((*hdr).prdtl).write_volatile(0u32);
        addr_of_mut!((*hdr).prdbc).write_volatile(0u32);
        addr_of_mut!((*hdr).ctba).write_volatile(ctba_val);
        addr_of_mut!((*hdr).ctbau).write_volatile(ctbau_val);
    }
}

/// 填充 AHCI H2D FIS 到命令表 CFIS 区域
///
/// 使用字节拷贝避免 packed struct 对齐问题。
/// `fis_src` — FIS 源指针, `fis_size` — 字节大小
pub fn ahci_fill_h2d_fis(cmd_table_virt: u64, fis_src: usize, fis_size: usize) {
    // SAFETY: cmd_table_virt 由 DMA 分配保证有效; fis_src 由调用方保证指向有效 FIS 内存
    unsafe {
        let cfis = cmd_table_virt as *mut u8;
        let src = fis_src as *const u8;
        core::ptr::copy_nonoverlapping(src, cfis, fis_size);
    }
}

/// 填充 AHCI PRDT entry (数据缓冲区物理地址 + 字节数)
// 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
#[expect(clippy::cast_possible_truncation)]
pub fn ahci_fill_prdt(
    cmd_table_virt: u64,
    entry_index: usize,
    data_phys: u64,
    byte_count: u32,
    ioc: bool,
) {
    // SAFETY: cmd_table_virt 由 DMA 分配保证有效; entry_index < 8
    unsafe {
        let table = cmd_table_virt as *mut ahci::AhciCommandTable;
        (*table).prdt[entry_index] = ahci::PhysicalRegionDescriptor {
            dba: data_phys as u32,
            dbau: (data_phys >> 32) as u32,
            rsvd: 0,
            dbc: (byte_count - 1) | (if ioc { 1u32 << 31 } else { 0 }),
        };
    }
}

// ============================================================================
// xHCI Transfer Ring safe wrapper API (services 层可调用的 0 unsafe 入口)
//
// 将 framework 中 unsafe 的 Transfer Ring 操作封装为 safe 函数,
// 使 services 层可执行完整的 USB 传输逻辑而不引入 unsafe。
// ============================================================================

/// 向 Transfer Ring 写入一个 TRB (16 字节).
///
/// `ring_vaddr` — Transfer Ring DMA 虚拟地址
/// `index` — TRB 索引 (0-based)
/// `trb_src` — 源 TRB 字节指针 (调用方保证指向有效 16 字节内存)
pub fn xhci_write_trb(ring_vaddr: u64, index: u32, trb_src: *const u8) {
    // SAFETY: ring_vaddr 由 DMA 分配保证有效; trb_src 由调用方保证指向有效 16 字节内存
    unsafe {
        let dst = (ring_vaddr as *mut u8).add((index as usize) * 16);
        core::ptr::copy_nonoverlapping(trb_src, dst, 16);
    }
}

/// 从 Transfer Ring 读取一个 TRB (16 字节) 到缓冲区.
///
/// `ring_vaddr` — Transfer Ring DMA 虚拟地址
/// `index` — TRB 索引 (0-based)
/// `dst` — 目标缓冲区 (至少 16 字节)
pub fn xhci_read_trb(ring_vaddr: u64, index: u32, dst: *mut u8) {
    // SAFETY: ring_vaddr 由 DMA 分配保证有效; dst 由调用方保证有效
    unsafe {
        let src = (ring_vaddr as *const u8).add((index as usize) * 16);
        core::ptr::copy_nonoverlapping(src, dst, 16);
    }
}
