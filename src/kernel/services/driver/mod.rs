#![deny(unsafe_code)]
//! 设备驱动 — 网卡/存储/显示/输入 (services 层)
//!
//! ## 当前状态: ⏳ 5/6 未迁移 (Phase 2.1 在途)
//!
//! 已迁移 (演示级 safe API, 0 unsafe):
//! - [net/e1000.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/net/e1000.rs) — E1000 网卡 (Phase 2.1.1)
//! - [virtio/transport.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/virtio/transport.rs) — VirtIO MMIO Transport (Phase 2.1.2/2.1.3 共享底层)
//!
//! 未迁移 (实际实现仍在 `kernel/driver/`):
//! - [kernel/driver/char/](file:///home/anfer/Code/QueenX/src/kernel/driver/char/) — 字符设备 (vga/serial/pl011) → Phase 2.1.5
//! - [kernel/driver/display/](file:///home/anfer/Code/QueenX/src/kernel/driver/display/) — 显示 (framebuffer/HDMI/DP) → Phase 2.1.5
//! - [kernel/driver/storage/](file:///home/anfer/Code/QueenX/src/kernel/driver/storage/) — 存储 (ATA/AHCI/NVMe) → Phase 2.1.3/2.1.4
//! - [kernel/driver/usb/](file:///home/anfer/Code/QueenX/src/kernel/driver/usb/) — USB (xHCI 控制器) → Phase 2.1.6
//!
//! ## 迁移路径
//!
//! 1. 所有 MMIO 走 `framework::iomem::IoMem`
//! 2. 所有 PIO 走 `framework::ioport::IoPort`
//! 3. 所有 DMA 走 `framework::dma_buf::DmaStream`
//! 4. 中断处理走 `framework::irqline::IrqLine`
//! 5. 在 services/driver/ 暴露纯 safe 驱动 API
//!
//! ## Phase 2.1 进度 (2026-06-04)
//!
//! | 子任务 | 驱动 | 状态 | safe wrapper |
//! |--------|------|------|--------------|
//! | 2.1.1  | E1000 网卡 | ✅ | `net/e1000.rs` (138 行) |
//! | 2.1.2  | VirtIO-Net | 🟡 (transport 通用层已就绪) | `virtio/transport.rs` |
//! | 2.1.3  | NVMe 存储 | ⏳ | 待 `storage/nvme.rs` |
//! | 2.1.4  | AHCI/ATA  | ⏳ | 待 `storage/ahci.rs` |
//! | 2.1.5  | VGA/串口/Framebuffer | ⏳ | 待 `char/vga.rs` 等 |
//! | 2.1.6  | USB/XHCI | ⏳ | 待 `usb/xhci.rs` |
//!
//! 进度: 1/6 → 2/6 (transport 为后续 2.1.2/2.1.3 共享底层, 视为部分完成)

pub mod acpi;
pub mod char;
pub mod firmware;
/// T24: E1000 网卡驱动 (services 层安全逻辑)
pub mod net;
/// D5: 电源管理安全封装
pub mod power;
pub mod storage;
pub mod usb;
pub mod virtio;

/// 显示子系统 (DDC + HDMI) 安全封装
pub mod display;
/// D10: kexec 安全封装
pub mod kexec;
/// D11: UEFI 安全封装
pub mod uefi;

// ============================================================================
// T-04: 中断处理决策策略
// ============================================================================

use crate::framework::idt::irq_trait::{
    IrqContext, IrqDecision, SoftirqContext, register_irq_decision,
};

/// 驱动层中断处理决策策略
///
/// - 共享 IRQ: 按注册顺序选择 handler (先注册先服务)
/// - Softirq: 按固定优先级 (High > Timer > `NetRx` > `NetTx` > Block > Tasklet > Sched > Kswapd)
/// - ksoftirqd: 超过 10 次循环后唤醒
pub struct DriverIrqDecision;

impl IrqDecision for DriverIrqDecision {
    fn select_handler_index(&self, _ctx: IrqContext) -> usize {
        // 先注册先服务
        0
    }

    fn softirq_priority_mask(&self, ctx: SoftirqContext) -> u64 {
        // 返回最高优先级位
        if ctx.pending_mask == 0 {
            0
        } else {
            1u64 << ctx.pending_mask.ilog2()
        }
    }

    fn should_wake_ksoftirqd(&self, loop_count: u32) -> bool {
        loop_count > 10
    }
}

/// `services::driver` 初始化 — 注册策略到 framework
pub fn init() {
    // T-04: 注册驱动层中断处理决策策略
    static POLICY: DriverIrqDecision = DriverIrqDecision;
    let _ = register_irq_decision(&POLICY);
}
