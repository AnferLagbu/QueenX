#[cfg(not(feature = "kernel_test"))]
use core::sync::atomic::{AtomicU32, Ordering};

#[cfg(not(feature = "kernel_test"))]
use crate::framework::driver::DriverError;
use crate::framework::driver::{DeviceType, Driver, DriverResult};
#[cfg(test)]
use crate::framework::mm::KERNEL_BASE;
#[cfg(not(feature = "kernel_test"))]
use crate::framework::mm::PhysAddr;
use crate::framework::mm::virt_to_phys;
#[cfg(not(feature = "kernel_test"))]
use crate::framework::sync::IrqSpinLock as Mutex;
#[cfg(not(feature = "kernel_test"))]
use crate::framework::userptr::{UserReadPtr, UserWritePtr};
#[cfg(not(feature = "kernel_test"))]
use crate::klog_debug;
#[cfg(not(feature = "kernel_test"))]
use crate::klog_err;
#[cfg(not(feature = "kernel_test"))]
use crate::klog_info;
#[cfg(not(feature = "kernel_test"))]
use crate::klog_warn;
#[cfg(not(feature = "kernel_test"))]
use alloc::boxed::Box;
use alloc::vec::Vec;
#[cfg(not(feature = "kernel_test"))]
// 网络性能统计: 接收包计数
static POLL_COUNT: AtomicU32 = AtomicU32::new(0);

// ============================================================================
// B04-19: 描述符结构 + 常量已从 services 上移至 framework::driver::net::dma_ring.
// 反向依赖解除 (框架不再 use services).
// ============================================================================

pub use crate::framework::driver::net::dma_ring::{
    E1000_RX_BUFFER_SIZE, E1000_RX_RING_SIZE, E1000_RXD_ERR_CE, E1000_RXD_ERR_RXE,
    E1000_RXD_ERR_SE, E1000_RXD_ERR_SEQ, E1000_RXD_STAT_DD, E1000_TX_RING_SIZE,
    E1000_TXD_CMD_EOP, E1000_TXD_CMD_IFCS, E1000_TXD_CMD_RS, E1000_TXD_STAT_DD, E1000RxDesc,
    E1000TxDesc,
};

// B04-AUDIT-005 #4 v2 修复 (2026-08-24): E1000Driver 整体上移 framework.
// E1000Io + 寄存器常量 + E1000Driver 业务逻辑 + E1000Device 全部在 framework 层.
// services 仅保留描述符状态/命令常量 + re-export shim (services/driver/net/e1000.rs).
// 严格 framekernel 单向数据流: framework 不依赖 services.
// E1000Driver 在 kernel_test 构建下仍被使用 (E1000Device.driver 字段/访问器).
use crate::framework::driver::net::e1000_io::E1000Driver;
// kernel_test 构建下 E1000Io / E1000_ICR_* / E1000_RDT 仅被
// #[cfg(not(feature = "kernel_test"))] 门控的 probe/handle_interrupt 使用,
// 与使用点对齐, 避免 kernel_test 构建产生 unused_imports warning.
#[cfg(not(feature = "kernel_test"))]
use crate::framework::driver::net::e1000_io::{
    E1000_ICR_LSC, E1000_ICR_RXDMT0, E1000_ICR_RXO, E1000_ICR_RXT0, E1000_RDT, E1000Io,
};

// ============================================================================
// 虚拟地址 → 物理地址转换
// ============================================================================
//
// 复用 mm::virt_to_phys (基于 KERNEL_BASE 常量, 自动适配架构:
// - x86_64: KERNEL_BASE=0xFFFF800000000000, 减去得物理地址
// - aarch64: KERNEL_BASE=0 (恒等映射), VA==PA, 减 0 无变化)
//
// 本文件不重复定义 virt_to_phys, 避免 I-53 架构互斥 cfg 检查失败.

// ============================================================================
// DMA 描述符环安全包装 (framework 层, 封装 unsafe 指针操作)
// ============================================================================

/// TX 描述符环安全包装
///
/// 封装 E1000 TX DMA 描述符环的 unsafe 指针操作, 提供安全公共 API。
/// 内部管理描述符内存分配、物理地址转换、DD 状态检查。
pub struct TxRing {
    ptr: *mut E1000TxDesc,
    phys: u64,
    count: usize,
    tail: usize,
}

impl TxRing {
    /// 分配并初始化 TX 描述符环
    #[cfg(not(feature = "kernel_test"))]
    #[expect(
        clippy::ptr_as_ptr,
        reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
    )]
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
    )]
    pub fn alloc(count: usize) -> Option<Self> {
        // B04-17: count == 0 时 kmalloc_align(0, 16) 返回非 null ZERO_SIZE_PTR,
        // 后续 (*desc_ptr.add(i)) 解引用越过分配边界 → 任意内存读/写。
        if count == 0 {
            return None;
        }
        let size = core::mem::size_of::<E1000TxDesc>() * count;
        // SAFETY: count > 0 ⇒ size > 0, kmalloc_align 分配 16 字节对齐的 size 字节。
        let ptr = unsafe { kmalloc_align(size as u64, 16) };
        if ptr.is_null() {
            return None;
        }
        let desc_ptr = ptr as *mut E1000TxDesc;
        for i in 0..count {
            // SAFETY: desc_ptr 由 kmalloc_align 分配, 大小为 size;
            // i < count 保证索引在分配范围内。
            unsafe {
                (*desc_ptr.add(i)).addr = 0;
                (*desc_ptr.add(i)).length = 0;
                (*desc_ptr.add(i)).cmd = 0;
                (*desc_ptr.add(i)).status = E1000_TXD_STAT_DD;
            }
        }
        Some(Self {
            ptr: desc_ptr,
            phys: virt_to_phys(ptr as u64),
            count,
            tail: 0,
        })
    }

    /// TX 描述符环物理地址 (用于硬件 TDBAL/TDBAH)
    pub fn phys_addr(&self) -> u64 {
        self.phys
    }

    /// TX 描述符环字节长度 (用于硬件 TDLEN)
    pub fn len_bytes(&self) -> usize {
        self.count * core::mem::size_of::<E1000TxDesc>()
    }

    /// 当前 tail 索引
    pub fn tail(&self) -> usize {
        self.tail
    }

    /// 准备一个描述符用于发送 (物理地址版本)
    ///
    /// 设置 buffer 物理地址、长度、命令字, 清除 DD 状态。
    /// 调用方需先确认当前 tail 位置的描述符已完成 (DD=1)。
    pub fn prepare(&mut self, buf_phys: u64, buf_len: u16) {
        // SAFETY: tail 在 0..count 范围内; ptr 由 kmalloc_align 分配且大小足够。
        let desc = unsafe { &mut *self.ptr.add(self.tail) };
        desc.addr = buf_phys;
        desc.length = buf_len;
        desc.cmd = E1000_TXD_CMD_EOP | E1000_TXD_CMD_IFCS | E1000_TXD_CMD_RS;
        desc.status = 0;
    }

    /// 准备一个描述符用于发送 (虚拟地址版本, 内部转换物理地址)
    pub fn prepare_from_virt(&mut self, buf_virt: u64, buf_len: u16) {
        let buf_phys = virt_to_phys(buf_virt);
        self.prepare(buf_phys, buf_len);
    }

    /// 检查指定索引的描述符是否完成 (DD bit)
    pub fn is_done(&self, idx: usize) -> bool {
        // SAFETY: idx 在 0..count 范围内; ptr 已分配。
        let desc = unsafe { &*self.ptr.add(idx) };
        desc.status & E1000_TXD_STAT_DD != 0
    }

    /// 推进 tail 指针到下一个描述符
    pub fn advance_tail(&mut self) {
        self.tail = (self.tail + 1) % self.count;
    }
}

/// RX 描述符环安全包装
///
/// 封装 E1000 RX DMA 描述符环的 unsafe 指针操作, 提供安全公共 API。
/// 内部管理描述符内存分配、接收缓冲区分配、物理地址转换、DD 状态检查。
pub struct RxRing {
    ptr: *mut E1000RxDesc,
    phys: u64,
    count: usize,
    bufs: Vec<*mut u8>,
    tail: usize,
    buf_size: usize,
}

impl RxRing {
    /// 分配并初始化 RX 描述符环及接收缓冲区
    #[cfg(not(feature = "kernel_test"))]
    #[expect(
        clippy::ptr_as_ptr,
        reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
    )]
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
    )]
    pub fn alloc(count: usize, buf_size: usize) -> Option<Self> {
        // B04-17: count == 0 或 buf_size == 0 时 kmalloc_align 返回 ZERO_SIZE_PTR,
        // 后续解引用越过分配边界 → 任意内存读/写。
        if count == 0 || buf_size == 0 {
            return None;
        }
        let size = core::mem::size_of::<E1000RxDesc>() * count;
        // SAFETY: count > 0 ⇒ size > 0, kmalloc_align 分配 16 字节对齐的 size 字节。
        let ptr = unsafe { kmalloc_align(size as u64, 16) };
        if ptr.is_null() {
            return None;
        }
        let desc_ptr = ptr as *mut E1000RxDesc;
        let mut bufs = Vec::new();
        for i in 0..count {
            // SAFETY: kmalloc_align 分配 RX 缓冲区, 16 字节对齐。
            let buf_ptr = unsafe { kmalloc_align(buf_size as u64, 16) };
            if buf_ptr.is_null() {
                return None;
            }
            bufs.push(buf_ptr as *mut u8);
            let buf_phys = virt_to_phys(buf_ptr as u64);
            // SAFETY: desc_ptr 已分配; i < count。
            unsafe {
                (*desc_ptr.add(i)).addr = buf_phys;
                (*desc_ptr.add(i)).length = 0;
                (*desc_ptr.add(i)).status = 0;
            }
        }
        Some(Self {
            ptr: desc_ptr,
            phys: virt_to_phys(ptr as u64),
            count,
            bufs,
            tail: 0,
            buf_size,
        })
    }

    /// RX 描述符环物理地址 (用于硬件 RDBAL/RDBAH)
    pub fn phys_addr(&self) -> u64 {
        self.phys
    }

    /// RX 描述符环字节长度 (用于硬件 RDLEN)
    pub fn len_bytes(&self) -> usize {
        self.count * core::mem::size_of::<E1000RxDesc>()
    }

    /// 当前 tail 索引
    pub fn tail(&self) -> usize {
        self.tail
    }

    /// 环大小 (描述符个数)
    pub fn count(&self) -> usize {
        self.count
    }

    /// 检查指定索引的描述符是否包含就绪数据包 (DD bit)
    pub fn is_done(&self, idx: usize) -> bool {
        // SAFETY: idx 在 0..count 范围内; ptr 已分配。
        let desc = unsafe { &*self.ptr.add(idx) };
        desc.status & E1000_RXD_STAT_DD != 0
    }

    /// 检查描述符是否有接收错误
    pub fn has_errors(&self, idx: usize) -> bool {
        // SAFETY: idx 在 0..count 范围内; ptr 已分配。
        let desc = unsafe { &*self.ptr.add(idx) };
        desc.errors & (E1000_RXD_ERR_CE | E1000_RXD_ERR_SE | E1000_RXD_ERR_SEQ | E1000_RXD_ERR_RXE)
            != 0
    }

    /// 获取描述符的接收长度
    pub fn packet_length(&self, idx: usize) -> usize {
        // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
        let desc = unsafe { &*self.ptr.add(idx) };
        desc.length as usize
    }

    /// 获取描述符的错误码
    pub fn errors(&self, idx: usize) -> u8 {
        // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
        let desc = unsafe { &*self.ptr.add(idx) };
        desc.errors
    }

    /// 从指定索引的缓冲区复制数据到调用方缓冲区
    pub fn copy_packet(&self, idx: usize, buf: &mut [u8]) -> usize {
        let len = self.packet_length(idx).min(buf.len()).min(self.buf_size);
        if !self.bufs[idx].is_null() && len > 0 {
            // SAFETY: bufs[idx] 由 kmalloc_align 分配, 大小为 buf_size;
            // buf 由调用方保证有效; len <= buf_size && len <= buf.len()。
            unsafe {
                core::ptr::copy_nonoverlapping(self.bufs[idx], buf.as_mut_ptr(), len);
            }
        }
        len
    }

    /// 清除指定索引的 DD 状态位
    pub fn clear_status(&mut self, idx: usize) {
        // SAFETY: idx 在 0..count 范围内; ptr 已分配。
        let desc = unsafe { &mut *self.ptr.add(idx) };
        desc.status = 0;
    }

    /// 推进 tail 指针到下一个描述符
    pub fn advance_tail(&mut self) {
        self.tail = (self.tail + 1) % self.count;
    }
}

// ============================================================================
// E1000 设备 (framework 层: DMA 管理 + FFI + 安全驱动逻辑委托)
// ============================================================================

pub struct E1000Device {
    pub bus: u8,
    pub device: u8,
    pub func: u8,
    #[cfg(not(feature = "kernel_test"))]
    mmio_phys: u64,
    /// 安全驱动逻辑 (services 层, 0 unsafe)
    driver: Option<E1000Driver>,
    /// TX 描述符环 (安全包装, 封装 unsafe 指针操作)
    #[cfg(not(feature = "kernel_test"))]
    tx_ring: Option<TxRing>,
    tx_count: u64,
    /// RX 描述符环 (安全包装, 封装 unsafe 指针操作)
    #[cfg(not(feature = "kernel_test"))]
    rx_ring: Option<RxRing>,
    rx_count: u64,
    isr_count: u64,
    link_change_count: u64,
    info: crate::framework::driver::DeviceInfo,
}

impl Default for E1000Device {
    fn default() -> Self {
        Self {
            bus: 0,
            device: 0,
            func: 0,
            #[cfg(not(feature = "kernel_test"))]
            mmio_phys: 0,
            driver: None,
            #[cfg(not(feature = "kernel_test"))]
            tx_ring: None,
            tx_count: 0,
            #[cfg(not(feature = "kernel_test"))]
            rx_ring: None,
            rx_count: 0,
            isr_count: 0,
            link_change_count: 0,
            info: crate::framework::driver::DeviceInfo::new(
                "Intel E1000",
                DeviceType::Network,
            ),
        }
    }
}

/// `E1000Device` 辅助方法 (安全驱动访问)
impl E1000Device {
    /// 获取安全驱动引用 (driver 已初始化时可用)
    fn driver_ref(&self) -> &E1000Driver {
        self.driver.as_ref().expect("e1000: driver 未初始化")
    }

    /// 获取安全驱动可变引用 (driver 已初始化时可用)
    fn driver_mut(&mut self) -> &mut E1000Driver {
        self.driver.as_mut().expect("e1000: driver 未初始化")
    }
}

/// E1000 EEPROM 读取 (按 feature 切换).
///
/// - 默认 (`e1000-real-hw` 关闭): QEMU 仿真器对 EERD 寄存器的写操作会触发内部
///   mutex 死锁, 因此 `eeprom_read` 立即返回 `0xFFFF`, 由 [`read_mac_address`]
///   填入 QEMU 默认 MAC `52:54:00:12:34:56`.
/// - 启用 `e1000-real-hw` feature: 通过 EERD.START 触发读, 轮询 EERD.DONE
///   位 (带 100k 次 `spin_loop` 超时), 返回 (val >> 16) & 0xFFFF.
///
/// QEMU 兼容路径: 跳过 EERD MMIO 访问, 直接返回哨兵值。
#[cfg(all(not(feature = "kernel_test"), not(feature = "e1000-real-hw")))]
fn eeprom_read(io: &E1000Io, addr: u8) -> u16 {
    let _ = io;
    let _ = addr;
    0xFFFF
}

/// 真实硬件 EERD 读取路径. 由 `e1000-real-hw` feature 启用.
#[cfg(all(not(feature = "kernel_test"), feature = "e1000-real-hw"))]
fn eeprom_read(io: &E1000Io, addr: u8) -> u16 {
    io.eeprom_read(addr)
}

/// 读取 MAC 地址 (按 feature 切换).
///
/// - 默认: 跳过所有 MMIO 读取, 使用 QEMU 默认 MAC.
/// - `e1000-real-hw` 启用: 通过 EERD 读取 3 个 16 位 EEPROM 字 (word 0..=2),
///   拼成 6 字节 MAC. 真实 NIC 的 MAC 即在 EEPROM 偏移 0 处开始.
#[cfg(not(feature = "kernel_test"))]
fn read_mac_address(io: &E1000Io) -> [u8; 6] {
    #[cfg(not(feature = "e1000-real-hw"))]
    {
        let _ = eeprom_read(io, 0);
        [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]
    }
    #[cfg(feature = "e1000-real-hw")]
    {
        let lo = eeprom_read(io, 0);
        let mid = eeprom_read(io, 1);
        let hi = eeprom_read(io, 2);
        let mac = [
            (lo & 0xFF) as u8,
            ((lo >> 8) & 0xFF) as u8,
            (mid & 0xFF) as u8,
            ((mid >> 8) & 0xFF) as u8,
            (hi & 0xFF) as u8,
            ((hi >> 8) & 0xFF) as u8,
        ];
        klog_info!(Net, "e1000: MAC from EEPROM {:02x?}", mac);
        mac
    }
}

// ============================================================================
// DMA 描述符环设置 (framework 层, unsafe)
// ============================================================================

#[cfg(not(feature = "kernel_test"))]
// 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
#[expect(clippy::cast_possible_truncation)]
fn setup_descriptor_rings(dev: &mut E1000Device) -> DriverResult<()> {
    let tx_ring = TxRing::alloc(E1000_TX_RING_SIZE).ok_or_else(|| {
        klog_err!(Net, "e1000: TX ring alloc failed");
        DriverError::HardwareError
    })?;

    klog_debug!(
        Net,
        "e1000: TX ring phys=0x{:x} len={}",
        tx_ring.phys_addr(),
        tx_ring.len_bytes()
    );
    dev.driver_ref().set_tx_base(tx_ring.phys_addr());
    dev.driver_ref().set_tx_len(tx_ring.len_bytes() as u32);
    dev.driver_ref().set_tx_head(0);
    dev.driver_ref().set_tx_tail(0);
    dev.tx_ring = Some(tx_ring);

    let rx_ring = RxRing::alloc(E1000_RX_RING_SIZE, E1000_RX_BUFFER_SIZE).ok_or_else(|| {
        klog_err!(Net, "e1000: RX ring alloc failed");
        DriverError::HardwareError
    })?;

    klog_debug!(
        Net,
        "e1000: RX ring phys=0x{:x} len={}",
        rx_ring.phys_addr(),
        rx_ring.len_bytes()
    );
    dev.driver_ref().set_rx_base(rx_ring.phys_addr());
    dev.driver_ref().set_rx_len(rx_ring.len_bytes() as u32);
    dev.driver_ref().set_rx_head(0);
    dev.driver_ref()
        .set_rx_tail((E1000_RX_RING_SIZE - 1) as u32);
    dev.rx_ring = Some(rx_ring);

    Ok(())
}

// ============================================================================
// Driver trait 实现
// ============================================================================

impl Driver for E1000Device {
    fn name(&self) -> &'static str {
        "Intel E1000 Gigabit Ethernet"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Network
    }

    #[cfg(not(feature = "kernel_test"))]
    fn init(&mut self) -> DriverResult<()> {
        // 1. Services 层: 复位硬件并检测链路
        self.driver_mut()
            .reset_and_detect_link()
            .map_err(|_| DriverError::HardwareError)?;

        // 2. Framework 层: 分配 DMA 描述符环并配置基地址
        setup_descriptor_rings(self)?;

        // 3. Services 层: 完成初始化 (TCTL/RCTL/MAC/IPG/IMS)
        self.driver_mut().complete_init();

        Ok(())
    }

    #[cfg(feature = "kernel_test")]
    fn init(&mut self) -> DriverResult<()> {
        self.driver_mut().mark_initialized();
        Ok(())
    }

    fn shutdown(&mut self) -> DriverResult<()> {
        if !self.driver_ref().is_ready() {
            return Ok(());
        }
        self.driver_mut().shutdown();
        Ok(())
    }

    fn is_ready(&self) -> bool {
        // driver 为 None (尚未 init) 是合法状态, 查询就绪应返回 false 而非 panic
        // (原实现经 driver_ref() 的 expect 直接 panic; 2026-09-24 UT-06 实测修复).
        self.driver.as_ref().is_some_and(E1000Driver::is_ready)
    }

    fn status(&self) -> &'static str {
        if self.driver_ref().is_ready() {
            "Link ready"
        } else {
            "Not initialized"
        }
    }
}

// ============================================================================
// E1000Device 方法 (DMA 收发 + 中断处理)
// ============================================================================

impl E1000Device {
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取 MAC 地址
    pub fn mac(&self) -> [u8; 6] {
        self.driver_ref().mac
    }

    /// 通过 PCI 总线探测并初始化 e1000 网卡设备。
    /// # Errors
    /// 未找到匹配的网卡设备或初始化失败时返回 Err。
    #[cfg(not(feature = "kernel_test"))]
    pub fn probe(&mut self) -> DriverResult<()> {
        // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
        unsafe extern "C" {
            fn pci_read_config_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16;
            fn pci_read_config_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32;
            fn pci_write_config_dword(bus: u8, dev: u8, func: u8, offset: u8, val: u32);
        }

        for bus in 0..255u8 {
            // SAFETY: pci_read_config_word 是 C-ABI PCI 配置读取；offset 0x00
            // 读取 vendor_id，是只读寄存器，无副作用。
            let vendor_id = unsafe { pci_read_config_word(bus, 0, 0, 0x00) };
            if vendor_id == 0xFFFF || vendor_id == 0x0000 {
                if bus > 0 {
                    continue;
                }
            }

            for dev_idx in 0..32u8 {
                for func in 0..8u8 {
                    // SAFETY: PCI 配置空间读取 (vendor id / class code)，
                    // 设备不存在返回 0xFFFF/0x0000。
                    let vid = unsafe { pci_read_config_word(bus, dev_idx, func, 0x00) };
                    if vid == 0xFFFF || vid == 0x0000 {
                        if func == 0 {
                            break;
                        }
                        continue;
                    }

                    // SAFETY: device id 寄存器 (offset 0x02)。
                    let _did = unsafe { pci_read_config_word(bus, dev_idx, func, 0x02) };
                    // SAFETY: revision + class code 寄存器 (offset 0x08)。
                    let class_code = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x08) };
                    let base_class = ((class_code >> 24) & 0xFF) as u8;

                    if vid == 0x8086 && base_class == 0x02 {
                        self.bus = bus;
                        self.device = dev_idx;
                        self.func = func;

                        // SAFETY: BAR0 寄存器 (offset 0x10) 读取。
                        let bar0_lo = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x10) };
                        // SAFETY: 写 BAR0 全 1 用于探测 BAR 大小。
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x10, 0xFFFFFFFF) };
                        // SAFETY: 读取 BAR0 size mask。
                        let _bar_size_mask =
                            unsafe { pci_read_config_dword(bus, dev_idx, func, 0x10) };
                        // SAFETY: 恢复原 BAR0 值。
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x10, bar0_lo) };

                        let is_io = (bar0_lo & 0x01) != 0;
                        if is_io {
                            return Err(DriverError::UnsupportedOperation);
                        }

                        self.mmio_phys = u64::from(bar0_lo & 0xFFFFFFF0);

                        // SAFETY: 中断寄存器 (offset 0x3C)。
                        let irq_reg = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x3C) };
                        let irq = (irq_reg & 0xFF) as u8;

                        // SAFETY: 命令寄存器 (offset 0x04)；开启 MMIO + Bus Master。
                        let mut cmd = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x04) };
                        cmd |= 0x06;
                        // SAFETY: 写回命令寄存器。
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x04, cmd) };

                        // 创建安全 MMIO 访问器 (framework::e1000_io 层)
                        // B04-AUDIT-005 #4: 错误类型从 services::KernelError 变为
                        // framework::E1000IoError (避免 framework→services 反向依赖).
                        // 在 framework 边界用 match 自行处理.
                        let io = match E1000Io::new(
                            PhysAddr::new(self.mmio_phys),
                            128 * 1024, // E1000 BAR0 is 128KB
                        ) {
                            Ok(m) => m,
                            Err(_e) => {
                                klog_err!(Net, "e1000: E1000Io::new failed (IoMem 映射)");
                                return Err(DriverError::HardwareError);
                            }
                        };

                        // 读取 MAC 地址 (feature-gated: QEMU 跳过 EERD)
                        let mac = read_mac_address(&io);

                        // 创建安全驱动实例
                        self.driver = Some(E1000Driver::new(io, mac, irq));

                        klog_info!(Net, "e1000: MMIO phys=0x{:x} IRQ={}", self.mmio_phys, irq);

                        return Ok(());
                    }

                    if func == 0 && (vid & 0x8000) == 0 {
                        break;
                    }
                }
            }
        }

        Err(DriverError::DeviceNotFound)
    }

    /// 发送一个网络数据包, 返回实际发送的字节数。
    /// # Errors
    /// 驱动未初始化或底层发送超时时返回 Err。
    #[cfg(not(feature = "kernel_test"))]
    pub fn send_packet(&mut self, data: &[u8]) -> DriverResult<usize> {
        if !self.is_ready() {
            return Err(DriverError::NotInitialized);
        }

        let len = self
            .driver_mut()
            .send_packet(data)
            .map_err(|_| DriverError::Timeout)?;
        self.tx_count += 1;
        Ok(len)
    }

    #[cfg(not(feature = "kernel_test"))]
    pub fn process_rx_packets(&mut self) {
        if !self.is_ready() {
            return;
        }

        let processed = self.driver_mut().process_rx();
        if processed > 0 {
            self.rx_count += u64::from(processed);
            klog_debug!(
                Net,
                "e1000: RX processed {} packets (total={})",
                processed,
                self.rx_count
            );
        }
    }

    #[cfg(not(feature = "kernel_test"))]
    pub fn try_receive(&mut self, buffer: &mut [u8]) -> Option<usize> {
        if !self.is_ready() {
            return None;
        }

        // 网络性能统计: 递增接收计数
        POLL_COUNT.fetch_add(1, Ordering::Relaxed);

        let result = self.driver_mut().receive_packet(buffer);
        if result.is_some() {
            self.rx_count += 1;
        }
        result
    }

    #[cfg(not(feature = "kernel_test"))]
    pub fn handle_interrupt(&mut self) {
        if !self.is_ready() {
            return;
        }

        let icr = self.driver_ref().ack_interrupt();
        if icr == 0 {
            return;
        }

        self.isr_count += 1;

        if self.isr_count <= 5 {
            klog_debug!(Net, "e1000: ISR icr=0x{:x}", icr);
        }

        if icr & E1000_ICR_LSC != 0 {
            // 链路状态变化
            self.link_change_count += 1;
            klog_info!(Net, "e1000: link status change");
        }

        // 处理接收溢出中断
        if icr & E1000_ICR_RXO != 0 {
            klog_warn!(Net, "e1000: RX buffer overflow, clearing");
            // 清除 RDT 以恢复接收
            let rdt = self.driver_ref().read_reg(E1000_RDT);
            self.driver_ref().write_reg(E1000_RDT, rdt);
        }

        if icr & (E1000_ICR_RXT0 | E1000_ICR_RXDMT0) != 0 {
            if self.isr_count <= 5 {
                klog_debug!(Net, "e1000: RX interrupt");
            }
            self.process_rx_packets();
        }
    }

    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        (
            self.isr_count,
            self.rx_count,
            self.tx_count,
            self.link_change_count,
        )
    }

    pub fn get_info(&self) -> &crate::framework::driver::DeviceInfo {
        &self.info
    }
}

// ============================================================================
// 全局设备存储 + Chitin FFI 回调
// ============================================================================

#[cfg(not(feature = "kernel_test"))]
static E1000_DEVICE: Mutex<Option<Box<E1000Device>>> = Mutex::new(None);

#[cfg(not(feature = "kernel_test"))]
pub fn take_device() -> Option<Box<E1000Device>> {
    E1000_DEVICE.lock().take()
}

#[cfg(not(feature = "kernel_test"))]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
pub extern "C" fn e1000_net_send(driver_data: *mut u8, data: *const u8, len: u32) -> i32 {
    if driver_data.is_null() || data.is_null() {
        return -1;
    }
    // SAFETY: driver_data 由驱动注册时设置, data 由 Chitin NetOps 契约保证有效。
    let dev = unsafe { &mut *(driver_data as *mut E1000Device) };
    // SAFETY: data 是 NetOps 契约保证的合法用户/内核只读指针。
    let user_data = unsafe { UserReadPtr::new(data, len as usize) };
    match dev.send_packet(user_data.as_slice()) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[cfg(not(feature = "kernel_test"))]
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
pub extern "C" fn e1000_net_recv(driver_data: *mut u8, buf: *mut u8, buf_len: u32) -> i32 {
    if driver_data.is_null() || buf.is_null() {
        return -1;
    }
    // SAFETY: 同上。
    let dev = unsafe { &mut *(driver_data as *mut E1000Device) };
    let mut user_buf = unsafe { UserWritePtr::new(buf, buf_len as usize) };
    dev.try_receive(user_buf.as_mut_slice())
        .map_or(0, |n| n as i32)
}

#[cfg(not(feature = "kernel_test"))]
#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
pub extern "C" fn e1000_net_get_mac(driver_data: *mut u8, mac: *mut [u8; 6]) {
    if driver_data.is_null() {
        return;
    }
    // SAFETY: driver_data 由驱动注册时设置, mac 由 Chitin NetOps 契约保证有效。
    let dev = unsafe { &*(driver_data as *const E1000Device) };
    unsafe {
        *mac = dev.mac();
    }
}

#[cfg(not(feature = "kernel_test"))]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
pub extern "C" fn e1000_net_irq(driver_data: *mut u8) {
    if driver_data.is_null() {
        return;
    }
    // SAFETY: driver_data 由 Chitin NetOps 契约保证有效。
    let dev = unsafe { &mut *(driver_data as *mut E1000Device) };
    dev.handle_interrupt();
}

#[cfg(not(feature = "kernel_test"))]
#[unsafe(no_mangle)]
pub extern "C" fn e1000_irq_entry(_frame: *mut u8) {
    // IRQ 上下文使用 try_lock 避免与主代码路径死锁
    if let Some(mut guard) = E1000_DEVICE.try_lock() {
        if let Some(ref mut dev) = *guard {
            dev.handle_interrupt();
        }
    }
}

#[cfg(not(feature = "kernel_test"))]
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn e1000_probe() -> i32 {
    // aarch64 QEMU virt 无 e1000 NIC, PCI ECAM 访问可能导致 Data Abort.
    // e1000 是 x86_64 专用 NIC, aarch64 直接返回"未找到".
    #[cfg(target_arch = "aarch64")]
    return -1;

    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut need_probe = false;
        {
            let guard = E1000_DEVICE.lock();
            if guard.is_none() {
                need_probe = true;
            }
        }

        if need_probe {
            let mut dev = Box::new(E1000Device::new());
            match dev.probe() {
                Ok(()) => {
                    let raw_ptr: *mut E1000Device = &raw mut *dev;
                    #[expect(
                        clippy::items_after_statements,
                        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
                    )]
                    static E1000_NET_OPS: crate::framework::chitin::NetOps =
                        crate::framework::chitin::NetOps {
                            send: e1000_net_send,
                            try_receive: e1000_net_recv,
                            get_mac: e1000_net_get_mac,
                            handle_irq: Some(e1000_net_irq),
                        };
                    let _id = crate::framework::chitin::chitin_register_with_ops(
                        "e1000",
                        crate::framework::chitin::ChitinProto::Net,
                        Some(dev.mmio_phys),
                        Some(dev.driver_ref().irq),
                        raw_ptr as *mut u8,
                        crate::framework::chitin::ChitinOps::Net(&E1000_NET_OPS),
                    );
                    *E1000_DEVICE.lock() = Some(dev);
                    return 0;
                }
                Err(_) => return -1,
            }
        }

        // 已存在
        match &*E1000_DEVICE.lock() {
            Some(_) => 0,
            None => -1,
        }
    }
}

#[cfg(not(feature = "kernel_test"))]
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::ref_as_ptr,
    reason = "ref_as_ptr: &T as *const T 是已知安全 (Rust 2024 可用 &raw const; 当前优先 expect"
)]
pub extern "C" fn get_e1000_instance() -> *mut u8 {
    (*E1000_DEVICE.lock())
        .as_mut()
        .map_or(core::ptr::null_mut(), |dev| dev as *mut _ as *mut u8)
}

#[cfg(not(feature = "kernel_test"))]
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn e1000_dump_regs() {
    #[cfg(feature = "e1000-verbose")]
    {
        let guard = E1000_DEVICE.lock();
        if let Some(ref dev) = *guard {
            let io = dev.driver_ref().io();
            {
                let ctrl = io.read32(0x0000); // E1000_CTRL
                let status = io.read32(0x0008); // E1000_STATUS
                let tctl = io.read32(0x0400); // E1000_TCTL
                let rctl = io.read32(0x0100); // E1000_RCTL
                let icr = io.read32(0x00C0); // E1000_ICR
                let ims = io.read32(0x00D0); // E1000_IMS
                let tdh = io.read32(0x3810); // E1000_TDH
                let tdt = io.read32(0x3818); // E1000_TDT
                let rdh = io.read32(0x2810); // E1000_RDH
                let rdt = io.read32(0x2818); // E1000_RDT
                let rdbal = io.read32(0x2800); // E1000_RDBAL
                let rdbah = io.read32(0x2804); // E1000_RDBAH
                let rdlen = io.read32(0x2808); // E1000_RDLEN
                klog_info!(Net, "=== E1000 Register Dump ===");
                klog_info!(Net, "CTRL=0x{:x} STATUS=0x{:x}", ctrl, status);
                klog_info!(Net, "TCTL=0x{:x} RCTL=0x{:x}", tctl, rctl);
                klog_info!(Net, "ICR=0x{:x} IMS=0x{:x}", icr, ims);
                klog_info!(Net, "TDH={} TDT={}", tdh, tdt);
                let rx_tail = dev.rx_ring.as_ref().map_or(0, |r| r.tail());
                klog_info!(Net, "RDH={} RDT={} rx_tail={}", rdh, rdt, rx_tail);
                klog_info!(
                    Net,
                    "RDBAL=0x{:x} RDBAH=0x{:x} RDLEN={}",
                    rdbal,
                    rdbah,
                    rdlen
                );
                klog_info!(
                    Net,
                    "tx_count={} rx_count={} isr_count={}",
                    dev.tx_count,
                    dev.rx_count,
                    dev.isr_count
                );
            }
        }
    }
    #[cfg(not(feature = "e1000-verbose"))]
    let () = &();
}

#[cfg(not(feature = "kernel_test"))]
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn e1000_dump_stats() {
    #[cfg(feature = "e1000-verbose")]
    {
        let guard = E1000_DEVICE.lock();
        if let Some(ref dev) = *guard {
            klog_info!(
                Net,
                "e1000 stats: tx={} rx={} isr={} link_chg={}",
                dev.tx_count,
                dev.rx_count,
                dev.isr_count,
                dev.link_change_count
            );
        }
    }
    let () = &();
}

// SAFETY: 单核内核, E1000 操作序列化在 Mutex 后
#[cfg(not(feature = "kernel_test"))]
// SAFETY: E1000Device 含 MMIO 裸指针, 但所有访问通过自身锁保护, 无锁外可变状态.
unsafe impl Send for E1000Device {}
#[cfg(not(feature = "kernel_test"))]
// SAFETY: 同上, 外部锁保证并发安全.
unsafe impl Sync for E1000Device {}

#[cfg(not(feature = "kernel_test"))]
#[repr(C, align(4096))]
struct AlignedKallocBuf {
    data: [u8; 1048576],
}

#[cfg(not(feature = "kernel_test"))]
static KALLOC_BUF: crate::framework::sync::IrqSpinLock<AlignedKallocBuf> =
    crate::framework::sync::IrqSpinLock::new(AlignedKallocBuf { data: [0; 1048576] });
#[cfg(not(feature = "kernel_test"))]
static KALLOC_OFF: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[cfg(not(feature = "kernel_test"))]
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// `reg` 是 BAR0 区域内的有效 MMIO 寄存器偏移。设备已探测且 MMIO 区域已映射。
// 有意窄化: 用户内存代理, 指针/长度上下文保证
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
pub unsafe extern "C" fn kmalloc_align(size: u64, align: u64) -> *mut u8 {
    let s = size as usize;
    let a = if align == 0 { 1 } else { align as usize };
    let mut buf = KALLOC_BUF.lock();
    let base = buf.data.as_mut_ptr() as usize;
    let current_off = KALLOC_OFF.load(core::sync::atomic::Ordering::Relaxed);
    let current = base + current_off;
    let aligned = (current + a - 1) & !(a - 1);
    let padding = aligned - current;
    if current_off + padding + s > buf.data.len() {
        return core::ptr::null_mut();
    }
    let new_off = current_off + padding;
    KALLOC_OFF.store(new_off, core::sync::atomic::Ordering::Relaxed);
    // SAFETY: new_off 已通过边界检查, 不会越界
    let ptr = unsafe { buf.data.as_mut_ptr().add(new_off) } as *mut u8;
    KALLOC_OFF.store(new_off + s, core::sync::atomic::Ordering::Relaxed);
    ptr
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_creation() {
        let dev = E1000Device::new();
        assert_eq!(dev.bus, 0);
        assert_eq!(dev.device, 0);
        assert!(!dev.is_ready());
        assert_eq!(dev.name(), "Intel E1000 Gigabit Ethernet");
        assert_eq!(dev.device_type(), DeviceType::Network);
    }

    #[test]
    fn test_constants() {
        assert_eq!(E1000_TX_RING_SIZE, 64);
        assert_eq!(E1000_RX_RING_SIZE, 128);
        assert_eq!(E1000_RX_BUFFER_SIZE, 2048);
    }

    #[test]
    fn test_descriptor_sizes() {
        assert_eq!(core::mem::size_of::<E1000TxDesc>(), 16);
        assert_eq!(core::mem::size_of::<E1000RxDesc>(), 16);
    }

    #[test]
    fn test_virt_to_phys_conversion() {
        let high_addr: u64 = KERNEL_BASE;
        assert_eq!(virt_to_phys(high_addr), 0);
        // virt_to_phys 仅对内核空间地址 (>= KERNEL_BASE) 成立;
        // 原用例传低地址 0x12345678 导致下溢 panic (2026-09-24 UT-06 实测修正).
        assert_eq!(virt_to_phys(KERNEL_BASE + 0x12345678), 0x12345678);
    }
}
