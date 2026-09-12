#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! `VirtIO` 设备驱动 — services 层 (Phase 2.1.2 + 2.1.3)
//!
//! 包含 `VirtIO` MMIO Transport 的 services 层安全 API,
//! 为 virtio-blk (Phase 2.1.3) 和 virtio-net (Phase 2.1.2) 提供 100% safe 的设备驱动。
//!
//! ## 模块结构
//!
//! - [transport] — `VirtIO` MMIO Transport 安全代理, 0 unsafe
//! - [blk] — `VirtIO` 块设备安全驱动, 0 unsafe
//! - [net] — `VirtIO` 网络设备安全驱动, 0 unsafe
//!
//! ## 迁移状态
//!
//! - [`transport::VirtioDevice`] — MMIO 读写 + 状态机 + 中断 + 队列配置全 100% safe
//! - [`blk::VirtioBlkDriver`] — 块设备初始化 + 特性协商 + 配置读取全 100% safe
//! - [`net::VirtioNetDriver`] — 网卡初始化 + 特性协商 + MAC/链路读取全 100% safe
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.2/2.1.3 任务

pub mod blk;
pub mod net;
pub mod transport;

/// 初始化 VirtIO 块设备并注册到 Chitin (§6.4 直接方案 B: services 权威)
///
/// 探测 virtio-mmio 区域, 为块设备创建 services `VirtioBlkDriver`,
/// 完成初始化 (`finalize`: vq0 MMIO 配置 + DRIVER_OK) 后经
/// `proto_block::register_block_device` 注册为块设备。
///
/// framework 保留: `VirtioMmioDevice` (MMIO 传输机制) + `queue` (DMA 环机制)。
/// aarch64 (QEMU -M virt) 是 virtio-blk 的主战场; x86_64 走 PCI AHCI/NVMe。
pub fn blk_init() {
    use crate::kernel::framework::chitin::proto_block::register_block_device;
    use crate::kernel::framework::driver::virtio::{
        VIRTIO_MMIO_BASE, VIRTIO_MMIO_MAX_DEVICES, VIRTIO_MMIO_STRIDE,
    };
    use blk::VirtioBlkDriver;
    use transport::{DEVICE_ID_BLOCK, VirtioDevice};

    let mut blk_count = 0u32;
    for i in 0..VIRTIO_MMIO_MAX_DEVICES {
        let base = VIRTIO_MMIO_BASE + u64::from(i) * VIRTIO_MMIO_STRIDE;
        let Some(dev) = VirtioDevice::probe(base) else {
            continue;
        };
        if dev.device_id() != DEVICE_ID_BLOCK {
            continue;
        }
        let Some(mut blk) = VirtioBlkDriver::new(dev) else {
            continue;
        };
        // 完成初始化: vq0 MMIO 配置 + DRIVER_OK (设备进入 live)
        blk.finalize();
        let name = alloc::format!("virtio-blk{blk_count}");
        let name: &'static str = name.leak();
        let mmio_base = blk.device().mmio_base();
        register_block_device(name, blk, Some(mmio_base));
        blk_count += 1;
        crate::slog_info!(
            Driver,
            "virtio-blk: registered device #{} (services 权威)",
            blk_count
        );
    }
    if blk_count > 0 {
        crate::slog_info!(
            Driver,
            "virtio-blk: {} device(s) registered (services 权威)",
            blk_count
        );
    }
}
