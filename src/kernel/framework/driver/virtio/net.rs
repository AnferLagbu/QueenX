//! `VirtIO` 网络设备驱动 (设备 ID 1)
//!
//! 实现 `VirtIO` 网络设备规范.
//! 使用 2 个 split virtqueue:
//!   - 队列 0: 接收 (RX) — 设备写入收到的包
//!   - 队列 1: 发送 (TX) — 驱动写入要发送的包
//!
//! 配置空间 (偏移 0x100):
//!   - 0x00: mac`[6]` (MAC 地址, 当 `VIRTIO_NET_F_MAC` 被设置时)
//!   - 0x06: status (u16, 当 `VIRTIO_NET_F_STATUS` 被设置时)

use super::queue::{VQ_SIZE, VirtQueue};
use super::{VIRTIO_ID_NET, VirtioMmioDevice};
use crate::kernel::framework::fs::KernelError;
use crate::kernel::framework::mm::{KERNEL_BASE, PAGE_SIZE};
use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use crate::kernel::framework::userptr::{UserReadPtr, UserWritePtr};
use crate::klog_err;
use crate::klog_error;
use crate::klog_info;
use crate::klog_warn;
use alloc::boxed::Box;
// ── Feature bits ──

const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const VIRTIO_NET_F_STATUS: u64 = 1 << 16;

// ── VirtIO Net 头 (固定 12 字节) ──
// 旧版与 v1 头在 Linux/QEMU 中完全相同:
// flags(1) + gso_type(1) + hdr_len(2) + gso_size(2) + csum_start(2) + csum_offset(2) + num_buffers(2) = 12  // 字段布局

/// `virtio_net_hdr` — 每个 TX/RX 包前的头.
/// 旧版与 `VERSION_1` 都使用相同的 12 字节布局.
/// `num_buffers` 仅在协商 `VIRTIO_NET_F_MRG_RXBUF` 时才有意义.
#[repr(C)]
struct VirtioNetHdr {
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    gso_size: u16,
    csum_start: u16,
    csum_offset: u16,
    num_buffers: u16,
}

/// `virtio_net_hdr` 大小 (12 字节).
const NET_HDR_SIZE: usize = core::mem::size_of::<VirtioNetHdr>();

// ── 配置空间偏移 (相对于 0x100) ──

const NET_CONFIG_MAC: usize = 0x00; // 6 字节
const NET_CONFIG_STATUS: usize = 0x06; // 2 字节

// ── RX 缓冲常量 ──

const RX_BUFFER_SIZE: usize = 2048;

/// 带 virtqueue 的 virtio-net 设备.
pub struct VirtioNet {
    /// MMIO 设备传输引用.
    pub device: VirtioMmioDevice,
    /// RX virtqueue (队列 0).
    pub rx_vq: VirtQueue,
    /// TX virtqueue (队列 1).
    pub tx_vq: VirtQueue,
    /// 配置空间中的 MAC 地址.
    pub mac: [u8; 6],
    /// 配置空间中的链路状态 (1 = up).
    pub link_up: bool,
    /// RX 缓冲区内存 (从 PMM 分配).
    rx_buffers: [*mut u8; VQ_SIZE as usize],
    /// RX 缓冲区物理地址.
    rx_buffers_phys: [u64; VQ_SIZE as usize],
    /// 头大小: 10 (现代 `VERSION_1`) 或 12 (旧版).
    pub hdr_size: usize,
    /// 统计.
    pub tx_count: u64,
    pub rx_count: u64,
    /// TX DMA 缓冲区 (12 字节头 + 2048 字节帧)
    tx_dma_buf: [u8; NET_HDR_SIZE + 2048],
}

// SAFETY: IoMem 是 Send+Sync; VirtQueue 是 Send+Sync; DMA 缓冲区经 PMM 分配
// 采用单一所有者 &mut self 访问, 防止同一设备并发 I/O.
// SAFETY: VirtualNet 是 Send+Sync 因为其所有字段都是 Send+Sync.
unsafe impl Send for VirtioNet {}
unsafe impl Sync for VirtioNet {}

impl VirtioNet {
    /// 创建并初始化 virtio-net 驱动实例.
    ///
    /// 调用者必须保证 `device` 的 `device_id` == `VIRTIO_ID_NET`.
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    #[expect(
        clippy::too_many_lines,
        reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
    )]
    pub fn new(device: VirtioMmioDevice) -> Option<Self> {
        if device.device_id != VIRTIO_ID_NET {
            return None;
        }

        klog_info!(
            Driver,
            "virtio-net: initializing at {:#x}",
            device.iomem.phys().as_u64()
        );

        // 读取设备版本以判断传统/现代模式
        let version = device.read32(super::VERSION);
        let is_legacy = version == 1; // 1=过渡模式 (transitional), 2=仅现代模式 (modern-only)
        klog_info!(
            Driver,
            "virtio-net: version={}, {} mode",
            version,
            if is_legacy { "legacy" } else { "modern" }
        );

        // Feature negotiation
        let negotiated_v1: bool;
        let hdr_size: usize;
        {
            use super::STATUS;
            use super::STATUS_ACKNOWLEDGE;
            use super::STATUS_DRIVER;
            use super::STATUS_FEATURES_OK;

            // 重置 + ACKNOWLEDGE + DRIVER
            device.write32(STATUS, 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            device.write32(STATUS, STATUS_ACKNOWLEDGE);
            device.write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

            // 读取设备特性
            device.write32(super::DEVICE_FEATURES_SEL, 1);
            let dev_feat_hi = device.read32(super::DEVICE_FEATURES);
            device.write32(super::DEVICE_FEATURES_SEL, 0);
            let dev_feat_lo = device.read32(super::DEVICE_FEATURES);
            let dev_features = u64::from(dev_feat_hi) << 32 | u64::from(dev_feat_lo);
            klog_info!(Driver, "virtio-net: dev_features={:#018x}", dev_features);

            let has_v1 = (dev_features & super::VIRTIO_F_VERSION_1) != 0;

            if !is_legacy || has_v1 {
                // 现代模式: 协商 VIRTIO_F_VERSION_1 + VIRTIO_NET_F_MAC + VIRTIO_NET_F_STATUS
                negotiated_v1 = true;
                hdr_size = NET_HDR_SIZE;
                let feat = super::VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC | VIRTIO_NET_F_STATUS;
                device.write32(super::DRIVER_FEATURES_SEL, 1);
                device.write32(super::DRIVER_FEATURES, (feat >> 32) as u32);
                device.write32(super::DRIVER_FEATURES_SEL, 0);
                device.write32(super::DRIVER_FEATURES, (feat & 0xFFFF_FFFF) as u32);
                klog_info!(
                    Driver,
                    "virtio-net: negotiating modern features (VERSION_1 + MAC + STATUS)"
                );
            } else {
                // 传统模式: VIRTIO_NET_F_MAC + VIRTIO_NET_F_STATUS (如果设备支持)
                negotiated_v1 = false;
                hdr_size = NET_HDR_SIZE;
                let mut feat = VIRTIO_NET_F_MAC;
                if dev_features & VIRTIO_NET_F_STATUS != 0 {
                    feat |= VIRTIO_NET_F_STATUS;
                }
                device.write32(super::DRIVER_FEATURES_SEL, 1);
                device.write32(super::DRIVER_FEATURES, 0);
                device.write32(super::DRIVER_FEATURES_SEL, 0);
                device.write32(super::DRIVER_FEATURES, feat as u32);
                klog_info!(Driver, "virtio-net: negotiating legacy features");
            }

            // FEATURES_OK
            device.write32(
                STATUS,
                STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK,
            );
            let status = device.read32(STATUS);
            if status & STATUS_FEATURES_OK == 0 {
                klog_warn!(Driver, "virtio-net: FEATURES_OK rejected");
                return None;
            }
        }

        // 从配置空间读取 MAC 地址
        let mut mac: [u8; 6] = [0; 6];
        for i in 0..6 {
            mac[i] = (device.read_config32(NET_CONFIG_MAC + (i & !3)) >> ((i & 3) * 8)) as u8;
        }

        klog_info!(
            Driver,
            "virtio-net: MAC={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5]
        );

        // 读取链路状态
        let link_status = device.read_config32(NET_CONFIG_STATUS) as u16;
        let link_up = (link_status & 1) != 0;
        klog_info!(
            Driver,
            "virtio-net: link {}",
            if link_up { "up" } else { "down" }
        );

        // 队列布局: 现代=顺序, 传统=页对齐 used ring
        let vq_legacy = !negotiated_v1;

        klog_info!(
            Driver,
            "virtio-net: allocating RX vq (legacy={})",
            vq_legacy
        );

        // 分配 RX virtqueue (队列 0)
        let rx_vq = VirtQueue::new(vq_legacy)?;
        klog_info!(Driver, "virtio-net: RX vq allocated, setting up...");
        if vq_legacy {
            if device.setup_vq_legacy(0, &rx_vq).is_err() {
                klog_err!(Driver, "virtio-net: failed to setup RX vq (legacy)");
                return None;
            }
        } else if device.setup_vq(0, &rx_vq).is_err() {
            klog_err!(Driver, "virtio-net: failed to setup RX vq (modern)");
            return None;
        }
        klog_info!(Driver, "virtio-net: RX vq ready");

        // 分配 TX virtqueue (队列 1)
        klog_info!(Driver, "virtio-net: allocating TX vq");
        let tx_vq = VirtQueue::new(vq_legacy)?;
        klog_info!(Driver, "virtio-net: TX vq allocated, setting up...");
        if vq_legacy {
            if device.setup_vq_legacy(1, &tx_vq).is_err() {
                klog_err!(Driver, "virtio-net: failed to setup TX vq (legacy)");
                return None;
            }
        } else if device.setup_vq(1, &tx_vq).is_err() {
            klog_err!(Driver, "virtio-net: failed to setup TX vq (modern)");
            return None;
        }

        // 设置 DRIVER_OK — 队列配置完成后设备进入 live
        device.set_driver_ok();

        let mut net = Self {
            device,
            rx_vq,
            tx_vq,
            mac,
            link_up,
            rx_buffers: [core::ptr::null_mut(); VQ_SIZE as usize],
            rx_buffers_phys: [0; VQ_SIZE as usize],
            hdr_size,
            tx_count: 0,
            rx_count: 0,
            tx_dma_buf: [0u8; NET_HDR_SIZE + 2048],
        };

        // 用空缓冲区预填 RX 队列
        net.refill_rx();

        Some(net)
    }

    /// 用空缓冲区填充 RX virtqueue, 供设备写入.
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    fn refill_rx(&mut self) {
        for i in 0..VQ_SIZE as usize {
            if !self.rx_buffers[i].is_null() {
                continue; // 已填充
            }

            // 为该 RX 槽位分配缓冲区
            let pages = RX_BUFFER_SIZE.div_ceil(PAGE_SIZE as usize);
            // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
            #[expect(
                clippy::items_after_statements,
                reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
            )]
            unsafe extern "C" {
                fn pmm_alloc_pages(count: u64) -> *mut u8;
            }
            // SAFETY: extern 函数的参数/返回值类型与 C ABI 声明一致; 调用方保证指针有效
            let buf = unsafe { pmm_alloc_pages(pages as u64) };
            if buf.is_null() {
                klog_warn!(Driver, "virtio-net: failed to alloc RX buffer {}", i);
                return;
            }

            let buf_phys = buf as u64;
            let buf_virt = (buf_phys + KERNEL_BASE) as *mut u8;
            self.rx_buffers[i] = buf_virt;
            self.rx_buffers_phys[i] = buf_phys;

            // 准备描述符: 设备写入该缓冲区
            let desc_idx = self
                .rx_vq
                .prepare_desc(buf_phys, RX_BUFFER_SIZE as u32, true);
            self.rx_vq.submit(desc_idx);
        }

        // 通知设备开始使用这些缓冲区
        self.rx_vq.commit_and_kick();
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(0);
    }

    #[expect(
        clippy::unreadable_literal,
        reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
    )]
    /// 通过 TX virtqueue 发送一个包.
    ///
    /// `data` 指向包缓冲区 (物理连续).
    /// `len` 为包长度.
    /// # Errors
    /// 包长度为 0 或超过 65535 时返回 Err。
    pub fn send_packet(&mut self, data_phys: u64, len: u32) -> Result<(), ()> {
        if len == 0 || len > 65535 {
            return Err(());
        }

        // 准备单个 TX 描述符 (设备从该缓冲区读)
        let desc_idx = self.tx_vq.prepare_desc(data_phys, len, false);

        // 提交并通知
        self.tx_vq.submit(desc_idx);
        self.tx_vq.commit_and_kick();

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(1);

        // 轮询等待 TX 完成, 通过 MMIO 读强制 VM exit
        for _ in 0..100000 {
            if let Some((_id, _len)) = self.tx_vq.pop_used() {
                self.tx_vq.reclaim_desc(desc_idx);
                self.tx_count += 1;
                return Ok(());
            }
            self.device.read32(super::INTERRUPT_STATUS);
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }

        klog_warn!(
            Driver,
            "virtio-net: TX timeout (desc_idx={}, data={:#x}, len={})",
            desc_idx,
            data_phys,
            len
        );
        Err(())
    }

    /// 轮询 RX 队列处理收到的包.
    /// 返回已处理的包数.
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    pub fn poll_rx(&mut self) -> usize {
        let mut processed = 0;

        loop {
            let result = self.rx_vq.pop_used();
            if result.is_none() {
                break;
            }

            if let Some((desc_idx, len)) = result {
                let buf_idx = desc_idx as usize;

                if buf_idx < VQ_SIZE as usize && !self.rx_buffers[buf_idx].is_null() {
                    if len > self.hdr_size as u32 && len <= RX_BUFFER_SIZE as u32 {
                        self.rx_count += 1;
                        processed += 1;

                        self.rx_vq.reclaim_desc(desc_idx);
                        let new_desc = self.rx_vq.prepare_desc(
                            self.rx_buffers_phys[buf_idx],
                            RX_BUFFER_SIZE as u32,
                            true,
                        );
                        self.rx_vq.submit(new_desc);
                    } else {
                        klog_error!(
                            "[VirtIO-Net] RX invalid packet length: len={}, hdr_size={}, buf_size={}",
                            len,
                            self.hdr_size,
                            RX_BUFFER_SIZE
                        );
                    }
                }
            }
        }

        // 如果回收了缓冲区则通知设备
        if processed > 0 {
            self.rx_vq.commit_and_kick();
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            self.device.notify(0);
        }

        processed
    }

    /// 尝试接收一个包到提供的缓冲区.
    /// 成功返回 Some(len), 无包则返回 None.
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    pub fn try_receive(&mut self, buffer: &mut [u8]) -> Option<usize> {
        let result = self.rx_vq.pop_used()?;
        let (desc_idx, len) = result;
        let buf_idx = desc_idx as usize;

        if buf_idx >= VQ_SIZE as usize || self.rx_buffers[buf_idx].is_null() {
            self.rx_vq.reclaim_desc(desc_idx);
            return None;
        }

        if len <= self.hdr_size as u32 || len > RX_BUFFER_SIZE as u32 {
            self.rx_vq.reclaim_desc(desc_idx);
            let new_desc =
                self.rx_vq
                    .prepare_desc(self.rx_buffers_phys[buf_idx], RX_BUFFER_SIZE as u32, true);
            self.rx_vq.submit(new_desc);
            self.rx_vq.commit_and_kick();
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            self.device.notify(0);
            return None;
        }

        let data_len = (len - self.hdr_size as u32) as usize;
        let copy_len = data_len.min(buffer.len());

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let src = self.rx_buffers[buf_idx].add(self.hdr_size);
            core::ptr::copy_nonoverlapping(src, buffer.as_mut_ptr(), copy_len);
        }

        self.rx_count += 1;

        self.rx_vq.reclaim_desc(desc_idx);
        let new_desc =
            self.rx_vq
                .prepare_desc(self.rx_buffers_phys[buf_idx], RX_BUFFER_SIZE as u32, true);
        self.rx_vq.submit(new_desc);
        self.rx_vq.commit_and_kick();
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(0);

        Some(copy_len)
    }

    /// 处理 RX 回收用的已使用描述符, 不解析数据.
    /// 用于中断驱动轮询.
    pub fn handle_interrupt(&self) {
        // 读取并清除中断状态
        self.device.write32(
            super::INTERRUPT_ACK,
            self.device.read32(super::INTERRUPT_STATUS),
        );
    }
}

// ============================================================================
// Global instance + FFI
// ============================================================================

static VIRTIO_NET_DEVICE: Mutex<Option<Box<VirtioNet>>> = Mutex::new(None);

pub fn take_device() -> Option<Box<VirtioNet>> {
    VIRTIO_NET_DEVICE.lock().take()
}

/// C FFI: 探测 virtio-net 设备. 成功返回 0, 失败返回 -1.
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// 设备已完成初始化, `DESC_POOL` 包含有效描述符.
pub unsafe extern "C" fn virtio_net_probe() -> i32 {
    probe()
}

#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::borrow_as_ptr,
    reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
)]
/// 探测 virtio-net 设备并创建全局实例.
///
/// 成功返回 0, 失败返回 -1.
pub fn probe() -> i32 {
    let devices = super::probe_all();
    for dev in devices {
        if dev.device_id == VIRTIO_ID_NET {
            match VirtioNet::new(dev) {
                Some(net) => {
                    let mut boxed = Box::new(net);

                    // 注册到几丁质框架 (非所有权指针, 内存由 VIRTIO_NET_DEVICE 管理)
                    let raw_ptr: *mut VirtioNet = &mut *boxed;
                    let _id = crate::kernel::framework::chitin::chitin_register(
                        "virtio_net",
                        crate::kernel::framework::chitin::ChitinProto::Net,
                        Some(boxed.device.iomem.phys().as_u64()),
                        None,
                        raw_ptr as *mut u8,
                    );
                    *VIRTIO_NET_DEVICE.lock() = Some(boxed);
                    klog_info!(Driver, "virtio-net: probed successfully");
                    return 0;
                }
                None => {
                    klog_warn!(Driver, "virtio-net: found device but init failed");
                }
            }
        }
    }
    klog_info!(Driver, "virtio-net: no device found");
    -1
}

// ============================================================================
// NetOps 桥接 — 供 ChitinNetDevice 使用
// ============================================================================

// 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
pub extern "C" fn virtio_net_send(driver_data: *mut u8, data: *const u8, len: u32) -> i32 {
    if driver_data.is_null() || data.is_null() || len == 0 {
        return KernelError::InvalidArgument.as_i32();
    }
    // SAFETY: driver_data 由 Chitin 注册时设置, data 由 Chitin NetOps 契约保证有效。
    let dev = unsafe { &mut *(driver_data as *mut VirtioNet) };
    let hdr = dev.hdr_size;
    let total = (hdr + len as usize).min(dev.tx_dma_buf.len());
    dev.tx_dma_buf[..hdr].fill(0);
    let user_data = unsafe { UserReadPtr::new(data, len as usize) };
    dev.tx_dma_buf[hdr..total].copy_from_slice(&user_data.as_slice()[..total - hdr]);
    let phys = dev.tx_dma_buf.as_ptr() as u64;
    // I-53: 消除编译时架构互斥.
    // KERNEL_BASE 由 framework::mm 在两个架构上 cfg-gated 定义:
    //   x86_64: 0xFFFF800000000000 (内核高位映射, DMA 走物理低地址需回退)
    //   aarch64: 0 (恒等映射, DMA 物理地址 = 虚拟地址)
    // 同一个表达式 `if phys >= KERNEL_BASE { phys - KERNEL_BASE } else { phys }`
    // 在 aarch64 上退化为 `phys`, 等价于原 aarch64 分支; 不再需要 cfg 互斥.
    // SAFETY: aarch64 上 KERNEL_BASE=0, `phys >= 0` 对 u64 恒真, clippy
    // absurd_extreme_comparisons 可安全抑制 — 语义等价于直接取 phys.
    // 2026-09-11 实测: 仅 aarch64 触发 (x86_64 不触发), 需豁免
    #[allow(clippy::absurd_extreme_comparisons)]
    let dma_phys = if phys >= KERNEL_BASE {
        phys - KERNEL_BASE
    } else {
        phys
    };
    match dev.send_packet(dma_phys, total as u32) {
        Ok(()) => 0,
        Err(()) => KernelError::Io.as_i32(),
    }
}

// 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
pub extern "C" fn virtio_net_recv(driver_data: *mut u8, buf: *mut u8, buf_len: u32) -> i32 {
    if driver_data.is_null() || buf.is_null() {
        return KernelError::InvalidArgument.as_i32();
    }
    // SAFETY: driver_data 由 Chitin 注册时设置, buf 由 Chitin NetOps 契约保证有效。
    let dev = unsafe { &mut *(driver_data as *mut VirtioNet) };
    let mut user_buf = unsafe { UserWritePtr::new(buf, buf_len as usize) };
    dev.try_receive(user_buf.as_mut_slice())
        .map_or(0, |n| n as i32)
}

#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
pub extern "C" fn virtio_net_get_mac(driver_data: *mut u8, mac: *mut [u8; 6]) {
    if driver_data.is_null() {
        return;
    }
    // SAFETY: driver_data 由 Chitin 注册时设置, mac 由 Chitin NetOps 契约保证有效。
    let dev = unsafe { &*(driver_data as *const VirtioNet) };
    unsafe {
        *mac = dev.mac;
    }
}

#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
pub extern "C" fn virtio_net_irq(driver_data: *mut u8) {
    if driver_data.is_null() {
        return;
    }
    // SAFETY: driver_data 由 Chitin 注册时设置。
    let dev = unsafe { &*(driver_data as *const VirtioNet) };
    dev.handle_interrupt();
}
