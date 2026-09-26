//! xHCI 主机控制器驱动 (xHCI Host Controller Driver)
//!
//! 实现USB 3.0 xHCI (eXtensible Host Controller Interface) 规范：
//! - **USB 3.0支持**: 5 Gbps `SuperSpeed`
//! - **USB 2.0兼容**: 支持高速、全速、低速设备
//! - **多端口**: 支持多达256个端口
//! - **DMA传输**: 高效的内存传输
//!
//! ## 硬件规格
//!
//! ```text
//! xHCI Registers:
//! ├── CAPLENGTH (0x00): 能力寄存器长度
//! ├── HCSPARAMS1 (0x04): 结构参数1
//! ├── HCSPARAMS2 (0x08): 结构参数2
//! ├── HCSPARAMS3 (0x0C): 结构参数3
//! ├── HCCPARAMS1 (0x10): 能力参数1
//! ├── DBOFF (0x14): 门铃寄存器偏移
//! ├── RTSOFF (0x18): 运行时寄存器偏移
//! └── Operational Registers:
//!     ├── USBCMD (0x00): USB命令寄存器
//!     ├── USBSTS (0x04): USB状态寄存器
//!     ├── PAGESIZE (0x08): 页大小
//!     └── PORTSC (0x400+): 端口状态和控制
//! ```
//!
//! # Safety
//! xHCI驱动涉及复杂的DMA操作和MMIO寄存器访问。

use super::framework::{DeviceInfo, DeviceType, Driver, DriverError, Result};
use super::usb_core::{HostController, Urb, UsbSpeed};
use crate::framework::iomem::IoMem;
use crate::framework::mm::{PhysAddr, VirtAddr};
use alloc::vec;
use alloc::vec::Vec;
use core::ptr;

// ============================================================================
// xHCI 寄存器定义
// ============================================================================

/// xHCI 能力寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct XhciCapabilityRegisters {
    /// 能力寄存器长度
    pub cap_length: u8,
    /// 保留
    pub reserved: u8,
    /// xHCI版本号
    pub hci_version: u16,
    /// 结构参数1
    pub hcs_params1: u32,
    /// 结构参数2
    pub hcs_params2: u32,
    /// 结构参数3
    pub hcs_params3: u32,
    /// 能力参数1
    pub hcc_params1: u32,
    /// 数据库偏移
    pub db_off: u32,
    /// 运行时寄存器偏移
    pub rts_off: u32,
    /// 能力参数2
    pub hcc_params2: u32,
}

/// xHCI 操作寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct XhciOperationalRegisters {
    /// USB命令寄存器
    pub usb_cmd: u32,
    /// USB状态寄存器
    pub usb_sts: u32,
    /// 页大小
    pub page_size: u32,
    /// 保留
    pub reserved1: [u32; 2],
    /// 设备通知控制
    pub dn_ctrl: u32,
    /// 命令环控制
    pub cr_ctrl: u64,
    /// 保留
    pub reserved2: [u32; 4],
    /// 设备上下文基地址数组指针
    pub dcbaap: u64,
    /// 配置参数
    pub config: u32,
}

/// 端口状态和控制寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct XhciPortRegister {
    /// 端口状态和控制
    pub portsc: u32,
    /// 端口电源管理状态和控制
    pub portpmsc: u32,
    /// 端口链路信息
    pub portli: u32,
    /// 保留
    pub reserved: u32,
}

// ============================================================================
// xHCI 命令和状态位
// ============================================================================

/// xHCI USB 命令寄存器 (USBCMD) 位定义 — xHCI 规范 §5.4.1
///
/// 当前使用的位:
/// - `RUN_STOP`    (bit 0): 运行/停止
/// - `HC_RESET`    (bit 1): 控制器复位
/// - `INTR_ENABLE` (bit 2): 中断使能
///
/// 规范定义的全部位 (未实现部分供参考):
/// - `HOST_SYSTEM_ERROR_ENABLE` (bit 3)
/// - `DRIVER_DEBUG` (bit 4)
/// - `LIGHT_HC_RESET` (bit 5)
/// - `CONTROLLER_SAVE_STATE` (bit 6)
/// - `CONTROLLER_RESTORE_STATE` (bit 7)
/// - `ENABLE_U3` (bit 8)
/// - `ENABLE_S0IX` (bit 9)
/// - `WRAP_EVENT_CHECKING` (bit 10)
/// - `STROBE_DEBUG` (bit 11)
/// - `PARK_MODE`_{ENABLE,SELECT} (bits 12-14)
/// - `EVENT_RING_SEGMENT_TABLE_SIZE_MODE` (bit 15)
/// - `CONFIGURE_ENDPOINT_MAX_EXIT_LATENCY_TOO_LARGE` (bit 16)
mod usb_cmd {
    pub const RUN_STOP: u32 = 1 << 0;
    pub const HC_RESET: u32 = 1 << 1;
    pub const INTR_ENABLE: u32 = 1 << 2;
}

/// xHCI USB 状态寄存器 (USBSTS) 位定义 — xHCI 规范 §5.4.2
///
/// 当前使用的位:
/// - `HC_HALTED`        (bit 0): 控制器已停止
/// - `HC_RESET_COMPLETE` (bit 1): 复位完成
///
/// 规范定义的全部位 (未实现部分供参考):
/// - `EVENT_RING_NOT_EMPTY` (bit 2), `INTR_PENDING` (bit 3),
/// - `HOST_SYSTEM_ERROR` (bit 4), `EVENT_COUNTER_OVERFLOW` (bit 5),
/// - `PORT_CHANGE_DETECT` (bit 6), `SAVE_RESTORE_COMPLETE` (bit 7),
/// - `RESTORE_ERROR` (bit 8), `CONTROLLER_NOT_READY` (bit 11),
/// - `HOST_CONTROLLER_ERROR` (bit 12)
mod usb_sts {
    pub const HC_HALTED: u32 = 1 << 0;
    pub const HC_RESET_COMPLETE: u32 = 1 << 1;
}

/// xHCI 端口状态与控制寄存器 (PORTSC) 位定义 — xHCI 规范 §5.4.8
///
/// 当前使用的位:
/// - `CURRENT_CONNECT_STATUS` (bit 0): 设备已连接
/// - `PORT_ENABLED`           (bit 1): 端口已使能
/// - `PORT_RESET`             (bit 4): 端口复位
/// - `PORT_POWER`             (bit 9): 端口供电
///
/// 规范定义的其余位 (未实现部分供参考):
/// - `PORT_LINK_STATE` `[5:8]`, `PORT_SPEED` `[10:13]`, `PORT_INDICATOR` `[14:15]`,
/// - `CONNECT_STATUS_CHANGE` (bit 16), `PORT_ENABLED_DISABLED_CHANGE` (bit 17),
/// - `OVER_CURRENT_CHANGE` (bit 19), `RESET_CHANGE` (bit 21),
/// - `WAKE_ON`_{`CONNECT,DISCONNECT,OVER_CURRENT`} (bits 20-22),
/// - `DEVICE_REMOVABLE` (bit 23), `PORT_LINK_STATE_STROBE` (bit 26),
/// - `PORT_TEST` `[28:31]`
mod portsc {
    pub const CURRENT_CONNECT_STATUS: u32 = 1 << 0;
    pub const PORT_RESET: u32 = 1 << 4;
}

// ============================================================================
// xHCI 传输描述符 (TRB)
// ============================================================================

/// TRB类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TrbType {
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Isoch = 5,
    Link = 6,
    EventData = 7,
    NoOp = 8,
    EnableSlot = 9,
    DisableSlot = 10,
    AddressDevice = 11,
    ConfigureEndpoint = 12,
    EvaluateContext = 13,
    ResetEndpoint = 14,
    StopEndpoint = 15,
    SetTrDequeuePointer = 16,
    ResetDevice = 17,
    ForceEvent = 18,
    NegotiateBandwidth = 19,
    SetLatencyToleranceValue = 20,
    GetPortBandwidth = 21,
    ForceHeader = 22,
    NoOpCommand = 23,
    TransferEvent = 32,
    CommandCompletionEvent = 33,
    PortStatusChangeEvent = 34,
    BandwidthRequestEvent = 35,
    DoorbellEvent = 36,
    HostControllerEvent = 37,
    DeviceNotificationEvent = 38,
    MfindexWrapEvent = 39,
}

/// 传输描述符 (TRB) - 16字节
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    pub fn new(parameter: u64, status: u32, control: u32) -> Self {
        Self {
            parameter,
            status,
            control,
        }
    }

    #[expect(
        clippy::match_same_arms,
        reason = "match_same_arms: match arm 重复是为可读性/调试断点; 当前优先 expect"
    )]
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn trb_type(&self) -> TrbType {
        let ty = (self.control >> 10) & 0x3F;
        match ty {
            1 => TrbType::Normal,
            2 => TrbType::SetupStage,
            3 => TrbType::DataStage,
            4 => TrbType::StatusStage,
            5 => TrbType::Isoch,
            6 => TrbType::Link,
            7 => TrbType::EventData,
            8 => TrbType::NoOp,
            9 => TrbType::EnableSlot,
            10 => TrbType::DisableSlot,
            11 => TrbType::AddressDevice,
            12 => TrbType::ConfigureEndpoint,
            13 => TrbType::EvaluateContext,
            14 => TrbType::ResetEndpoint,
            15 => TrbType::StopEndpoint,
            16 => TrbType::SetTrDequeuePointer,
            17 => TrbType::ResetDevice,
            18 => TrbType::ForceEvent,
            19 => TrbType::NegotiateBandwidth,
            20 => TrbType::SetLatencyToleranceValue,
            21 => TrbType::GetPortBandwidth,
            22 => TrbType::ForceHeader,
            23 => TrbType::NoOpCommand,
            32 => TrbType::TransferEvent,
            33 => TrbType::CommandCompletionEvent,
            34 => TrbType::PortStatusChangeEvent,
            35 => TrbType::BandwidthRequestEvent,
            36 => TrbType::DoorbellEvent,
            37 => TrbType::HostControllerEvent,
            38 => TrbType::DeviceNotificationEvent,
            39 => TrbType::MfindexWrapEvent,
            _ => TrbType::Normal,
        }
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn cycle_bit(&self) -> bool {
        self.control & 1 != 0
    }
}

// ============================================================================
// xHCI 主机控制器
// ============================================================================
// xHCI 寄存器操作常量 (USB-1.1)
// ============================================================================
//
// 这些常量集中定义 xHCI 控制器初始化/复位/启动序列所用的常量,
// 供 `init_hardware` / `reset_controller` / `start_controller` 共用.
//
// 注意: 端口状态 (PORTSC) 等运行时寄存器由各使用点定义.

// 复位/启动等待超时 (与 dp.rs POLL_TRAINING_ITERS / hdmi/ddc.rs AUX_TIMEOUT_ITERS 对齐)
/// xHCI 复位等待超时 (单次迭代 ~1-2 µs, `1_000_000` ≈ 1-2 s)
const HC_RESET_TIMEOUT_ITERS: usize = 1_000_000;
/// xHCI 启动等待超时 (同上, 适配控制器冷启动)
const HC_START_TIMEOUT_ITERS: usize = 1_000_000;
/// 单次迭代 `spin_loop` 次数 (与 DP/HDMI 一致, 提供 ~1-2 µs 延时)
const HC_POLL_DELAY_ITERS: usize = 1;

// ============================================================================

/// xHCI 主机控制器驱动
pub struct XhciController {
    /// MMIO 句柄 (safe access proxy)
    iomem: Option<IoMem>,
    /// BAR0 MMIO 基地址 (从 PCI BAR 提取, 用于日志显示)
    pub bar_base: u64,
    /// BAR0 MMIO 大小
    pub bar_size: u64,
    /// 能力寄存器指针
    cap_regs: *const XhciCapabilityRegisters,
    /// 操作寄存器指针
    op_regs: *mut XhciOperationalRegisters,
    /// 端口寄存器数组指针
    port_regs: *mut XhciPortRegister,
    /// 端口数量
    num_ports: usize,
    /// 插槽数量
    num_slots: usize,
    /// 设备信息
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
    /// 下次分配的 URB ID (单调递增, USB-1.3).
    next_urb_id: u32,
    /// 待处理 URB 列表 (URB ID → caller-provided URB ID, USB-1.3).
    pending_urbs: Vec<(u32, u32)>,
    /// 已分配设备地址位图 (USB-1.4).
    address_bitmap: [u8; 32],
    /// 下一个待分配地址扫描起点 (USB-1.4)
    next_address_hint: u8,
    /// Command Ring 虚拟地址
    cmd_ring_virt: VirtAddr,
    /// Command Ring 物理地址
    cmd_ring_phys: PhysAddr,
    /// Command Ring 当前尾指针索引
    cmd_ring_tail: u32,
    /// Command Ring 当前 phase bit
    cmd_ring_phase: u8,
    /// Command Ring 大小 (TRB 数量, 必须是 2 的幂)
    cmd_ring_size: u32,
}

impl XhciController {
    /// `创建新的xHCI控制器实例`
    pub fn new(iomem: IoMem) -> Self {
        Self {
            iomem: Some(iomem),
            bar_base: 0,
            bar_size: 0,
            cap_regs: ptr::null(),
            op_regs: ptr::null_mut(),
            port_regs: ptr::null_mut(),
            num_ports: 0,
            num_slots: 0,
            info: DeviceInfo::new("xhci", DeviceType::Bus),
            initialized: false,
            next_urb_id: 1,
            pending_urbs: Vec::new(),
            address_bitmap: [0u8; 32],
            next_address_hint: 1,
            cmd_ring_virt: VirtAddr(0),
            cmd_ring_phys: PhysAddr(0),
            cmd_ring_tail: 0,
            cmd_ring_phase: 1,
            cmd_ring_size: 256, // 默认 256 个 TRB
        }
    }

    #[expect(
        clippy::return_self_not_must_use,
        reason = "return_self_not_must_use: 返回 Self 是 builder/fluent API; 当前优先 expect"
    )]
    /// 构造时附加 BAR0 信息 (用于日志/调试)
    pub fn with_bar(mut self, bar_base: u64, bar_size: u64) -> Self {
        self.bar_base = bar_base;
        self.bar_size = bar_size;
        self
    }

    /// 获取设备信息
    pub fn get_info(&self) -> &DeviceInfo {
        &self.info
    }

    /// 初始化控制器 (USB-1.1).
    ///
    /// 完整初始化流程 (xHCI 规范 §4.3):
    /// 1. 解析能力寄存器, 提取 `num_slots` / `num_ports`
    /// 2. 计算操作寄存器基地址 (`cap_length` 偏移)
    /// 3. 计算端口寄存器基地址 (`op_base` + 0x400)
    /// 4. 调用 `reset_controller` 复位 xHCI
    /// 5. 调用 `start_controller` 启动 xHCI
    ///
    /// # Safety (USB-1.1)
    ///
    /// 此方法使用 raw pointer 访问 MMIO 寄存器 (`*const / *mut` 强转).
    /// 调用方必须保证:
    /// - `self.iomem` 字段在调用前已通过 `new()` 设置, 且映射大小 ≥ PAGESIZE (4 KiB)
    /// - 硬件控制器物理 MMIO 已通过 ACPI/PCI BAR 正确映射
    /// - 调用时独占访问 (无并发 reset/start)
    ///
    /// # 错误
    ///
    /// - `DriverError::NotInitialized` - iomem 未设置
    /// - `DriverError::Timeout` - 复位或启动超时
    /// # Errors
    /// iomem 未设置或控制器复位/启动超时时返回 Err。
    pub fn init_hardware(&mut self) -> Result<()> {
        // SAFETY: 调用方保证 iomem 已映射且控制器独占访问.
        unsafe {
            let iomem = self.iomem.as_ref().ok_or(DriverError::NotInitialized)?;
            let base = iomem.virt_ptr() as usize;

            // 设置能力寄存器指针
            self.cap_regs = base as *const XhciCapabilityRegisters;

            // 读取能力寄存器
            let cap = &*self.cap_regs;

            // 计算操作寄存器地址
            let op_base = base + cap.cap_length as usize;
            self.op_regs = op_base as *mut XhciOperationalRegisters;

            // 解析结构参数
            self.num_slots = (cap.hcs_params1 & 0xFF) as usize;
            self.num_ports = ((cap.hcs_params1 >> 24) & 0xFF) as usize;

            // 计算端口寄存器地址
            self.port_regs = (op_base + 0x400) as *mut XhciPortRegister;

            // 复位控制器
            self.reset_controller()?;

            // 启动控制器
            self.start_controller()?;
        }

        Ok(())
    }

    /// 复位控制器 (USB-1.1).
    ///
    /// 设置 USBCMD 寄存器的 `HC_RESET` 位, 等待 USBSTS 的 `HC_RESET_COMPLETE`
    /// 位被硬件置 1. 超时 `HC_RESET_TIMEOUT_ITERS` (~1-2 s) 返回 `Timeout`.
    ///
    /// # Safety (USB-1.1)
    ///
    /// 调用方必须保证:
    /// - `self.op_regs` 已通过 `init_hardware` 设置
    /// - 独占访问 USBCMD / USBSTS 寄存器
    /// # Errors
    /// 复位超时时返回 Err。
    pub fn reset_controller(&mut self) -> Result<()> {
        // SAFETY: 调用方保证 op_regs 有效且独占访问.
        unsafe {
            let op = &mut *self.op_regs;

            // 设置复位位
            op.usb_cmd |= usb_cmd::HC_RESET;

            // 等待复位完成 (HC_RESET_TIMEOUT_ITERS)
            let mut iters = 0usize;
            loop {
                if op.usb_sts & usb_sts::HC_RESET_COMPLETE != 0 {
                    break;
                }
                if iters > HC_RESET_TIMEOUT_ITERS {
                    return Err(DriverError::Timeout);
                }
                for _ in 0..HC_POLL_DELAY_ITERS {
                    core::hint::spin_loop();
                }
                iters += HC_POLL_DELAY_ITERS;
            }
        }

        Ok(())
    }

    /// 启动控制器 (USB-1.1).
    ///
    /// 设置 USBCMD 寄存器的 `RUN_STOP` 和 `INTR_ENABLE` 位, 等待 USBSTS 的
    /// `HC_HALTED` 位被硬件清 0 (即控制器退出 halt 状态). 超时返回 `Timeout`.
    ///
    /// # Safety (USB-1.1)
    ///
    /// 调用方必须保证:
    /// - `self.op_regs` 已通过 `init_hardware` 设置
    /// - 独占访问 USBCMD / USBSTS 寄存器
    /// # Errors
    /// 启动超时时返回 Err。
    pub fn start_controller(&mut self) -> Result<()> {
        // SAFETY: 调用方保证 op_regs 有效且独占访问.
        unsafe {
            let op = &mut *self.op_regs;

            // 设置运行位和中断使能
            op.usb_cmd |= usb_cmd::RUN_STOP | usb_cmd::INTR_ENABLE;

            // 等待控制器就绪 (HC_HALTED == 0)
            let mut iters = 0usize;
            loop {
                if op.usb_sts & usb_sts::HC_HALTED == 0 {
                    break;
                }
                if iters > HC_START_TIMEOUT_ITERS {
                    return Err(DriverError::Timeout);
                }
                for _ in 0..HC_POLL_DELAY_ITERS {
                    core::hint::spin_loop();
                }
                iters += HC_POLL_DELAY_ITERS;
            }
        }

        Ok(())
    }

    /// 获取端口寄存器
    fn get_port_reg(&self, port: usize) -> Option<&XhciPortRegister> {
        if port >= self.num_ports {
            return None;
        }

        // SAFETY: `self` 由调用方保证为有效指针; 只读访问
        unsafe { Some(&*self.port_regs.add(port)) }
    }

    /// 获取端口寄存器 (可变)
    fn get_port_reg_mut(&mut self, port: usize) -> Option<&mut XhciPortRegister> {
        if port >= self.num_ports {
            return None;
        }

        // SAFETY: `self` 由调用方保证为有效指针; 只读访问
        unsafe { Some(&mut *self.port_regs.add(port)) }
    }

    /// 初始化 Command Ring
    ///
    /// 分配 DMA 内存并配置 Command Ring，写入 CRCR 寄存器。
    /// xHCI 规范 §5.6.1: Command Ring Control Register (CRCR)
    ///
    /// # Safety
    ///
    /// 调用方必须确保：
    /// - 控制器已复位且尚未启动
    /// - `op_regs` 有效
    /// # Errors
    /// DMA 内存分配失败时返回 Err。
    pub fn init_command_ring(&mut self) -> Result<()> {
        use crate::framework::dma::get_dma;

        let dma = get_dma();
        let ring_size = self.cmd_ring_size as usize;
        let ring_bytes = ring_size * core::mem::size_of::<Trb>();

        // 分配 Command Ring DMA 内存
        let (virt, phys) = dma.alloc_coherent(ring_bytes).ok_or(DriverError::Busy)?;

        self.cmd_ring_virt = virt;
        self.cmd_ring_phys = phys;
        self.cmd_ring_tail = 0;
        self.cmd_ring_phase = 1;

        // 清零 Command Ring (已由 alloc_coherent 清零)

        // 写入 CRCR 寄存器
        // SAFETY: op_regs 由 init_hardware 设置，有效且独占访问
        unsafe {
            let op = &mut *self.op_regs;
            // CRCR = Ring Physical Address | Command Ring Running (bit 0)
            op.cr_ctrl = phys.0 | 1;
        }

        crate::klog_ffi!(
            klog_ffi_info,
            "[xHCI] Command Ring initialized: phys=0x{:x}, size={}",
            phys.0,
            ring_size
        );

        Ok(())
    }

    /// 提交 Command Ring 命令
    ///
    /// 将 TRB 写入 Command Ring 并更新尾指针。
    /// xHCI 规范 §4.5.1: Command Ring
    ///
    /// # Safety
    ///
    /// 调用方必须确保：
    /// - Command Ring 已初始化
    /// - 无并发访问 Command Ring
    /// # Errors
    /// Command Ring 未初始化时返回 Err。
    pub unsafe fn submit_command(&mut self, trb: Trb) -> Result<u32> {
        if self.cmd_ring_virt.0 == 0 {
            return Err(DriverError::NotInitialized);
        }

        // 获取命令槽位索引
        let slot = self.cmd_ring_tail;

        // 写入 TRB 到 Command Ring
        // SAFETY: ring_ptr 指向有效的 DMA 内存，slot < cmd_ring_size
        let ring_ptr = unsafe { (self.cmd_ring_virt.0 as *mut Trb).add(slot as usize) };
        // 设置 phase bit
        let mut trb_with_phase = trb;
        if self.cmd_ring_phase != 0 {
            trb_with_phase.control |= 1; // Cycle bit
        } else {
            trb_with_phase.control &= !1;
        }
        // SAFETY: ring_ptr 指向有效的 DMA 内存，slot < cmd_ring_size
        unsafe { core::ptr::write_volatile(ring_ptr, trb_with_phase) };

        // 更新尾指针
        self.cmd_ring_tail = (self.cmd_ring_tail + 1) % self.cmd_ring_size;
        if self.cmd_ring_tail == 0 {
            self.cmd_ring_phase ^= 1;
        }

        // 更新 CRCR 寄存器的 Ring Consumer Cycle State
        // SAFETY: op_regs 有效
        unsafe {
            let op = &mut *self.op_regs;
            let new_tail_phys = self.cmd_ring_phys.0 + u64::from(self.cmd_ring_tail) * 16;
            op.cr_ctrl = new_tail_phys | u64::from(self.cmd_ring_phase);
        }

        // Doorbell 寄存器 0 = 触发 Command Ring
        if let Some(mmio) = self.iomem.as_ref() {
            // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
            let cap = unsafe { &*self.cap_regs };
            // SAFETY: mmio 由 IoMem 抽象提供, virt_ptr() 返回有效的虚拟地址
            let doorbell_base = unsafe { mmio.virt_ptr() } as usize + cap.db_off as usize;
            // SAFETY: doorbell 地址有效
            unsafe {
                core::ptr::write_volatile(doorbell_base as *mut u32, 0);
            }
        }

        Ok(slot)
    }

    #[expect(
        clippy::unused_self,
        reason = "保留 &self 签名以便调用点统一用法, 不依赖 self 字段时可改关联函数"
    )]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "保留 Option/Result<()> 包装便于 API 兼容性 (调用方可能 match 或 .unwrap); 移除包装需同步修改调用点, 风险大"
    )]
    /// 等待 Command Completion Event
    ///
    /// 轮询 Event Ring 等待命令完成。
    /// xHCI 规范 §4.6.1: Command Completion Event
    /// # Errors
    /// 命令完成等待超时时返回 Err。
    pub fn wait_command_completion(&mut self) -> Result<()> {
        // Event Ring 处理待实现 (登记分册 9 B09-10)
        // 当前简化实现: 短暂等待后返回
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// 发送 Stop Endpoint Command
    ///
    /// xHCI 规范 §4.6.6: Stop Endpoint Command
    /// # Errors
    /// 控制器未初始化或命令提交/完成等待失败时返回 Err。
    pub fn send_stop_endpoint(&mut self, slot_id: u8, ep_id: u8) -> Result<()> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }

        // 构造 Stop Endpoint Command TRB
        // TRB Type = 10 (Stop Endpoint)
        // Bits [15:8] = Endpoint ID
        // Bits [7:0] = Slot ID
        let trb = Trb::new(
            0,
            (u32::from(ep_id) << 8) | u32::from(slot_id),
            (TrbType::StopEndpoint as u32) << 10,
        );

        // SAFETY: 提交命令到 Command Ring
        unsafe {
            self.submit_command(trb)?;
        }

        // 等待命令完成
        self.wait_command_completion()?;

        crate::klog_ffi!(
            klog_ffi_info,
            "[xHCI] Stop Endpoint completed: slot={}, ep={}",
            slot_id,
            ep_id
        );

        Ok(())
    }

    /// 发送 Reset Endpoint Command
    ///
    /// xHCI 规范 §4.6.7: Reset Endpoint Command
    /// # Errors
    /// 控制器未初始化或命令提交/完成等待失败时返回 Err。
    pub fn send_reset_endpoint(&mut self, slot_id: u8, ep_id: u8) -> Result<()> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }

        // 构造 Reset Endpoint Command TRB
        // TRB 类型 = 14 (复位端点)
        // Bits [15:8] = Endpoint ID
        // Bits [7:0] = Slot ID
        let trb = Trb::new(
            0,
            (u32::from(ep_id) << 8) | u32::from(slot_id),
            (TrbType::ResetEndpoint as u32) << 10,
        );

        // SAFETY: 提交命令到 Command Ring
        unsafe {
            self.submit_command(trb)?;
        }

        // 等待命令完成
        self.wait_command_completion()?;

        crate::klog_ffi!(
            klog_ffi_info,
            "[xHCI] Reset Endpoint completed: slot={}, ep={}",
            slot_id,
            ep_id
        );

        Ok(())
    }

    /// 端点错误恢复
    ///
    /// 当 USB 传输发生错误时, 重置指定端点以恢复功能。
    /// xHCI 规范 §4.6.6 + §4.6.7: Stop Endpoint → Reset Endpoint
    /// # Errors
    /// 停止或重置端点失败时返回 Err。
    pub fn recover_endpoint(&mut self, slot_id: u8, ep_id: u8) -> Result<()> {
        // 1. 停止端点
        self.stop_endpoint(slot_id, ep_id)?;
        // 2. 重置端点
        self.reset_endpoint(slot_id, ep_id)?;
        Ok(())
    }

    /// 停止指定端点
    ///
    /// 发送 Stop Endpoint Command 停止端点的传输。
    /// xHCI 规范 §4.6.6: Stop Endpoint Command
    /// # Errors
    /// 停止端点命令失败时返回 Err。
    pub fn stop_endpoint(&mut self, slot_id: u8, ep_id: u8) -> Result<()> {
        self.send_stop_endpoint(slot_id, ep_id)
    }

    /// 重置端点状态
    ///
    /// 发送 Reset Endpoint Command 重置端点状态。
    /// xHCI 规范 §4.6.7: Reset Endpoint Command
    /// # Errors
    /// 重置端点命令失败时返回 Err。
    pub fn reset_endpoint(&mut self, slot_id: u8, ep_id: u8) -> Result<()> {
        self.send_reset_endpoint(slot_id, ep_id)
    }
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for XhciController {
    fn name(&self) -> &'static str {
        "xHCI Controller"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Bus
    }

    fn init(&mut self) -> Result<()> {
        self.init_hardware()
            .map_err(|_| DriverError::HardwareError)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let op = &mut *self.op_regs;
            op.usb_cmd &= !usb_cmd::RUN_STOP;
        }

        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn status(&self) -> &'static str {
        if self.initialized {
            "xHCI running"
        } else {
            "xHCI stopped"
        }
    }
}

// ============================================================================
// HostController Trait 实现
// ============================================================================

impl HostController for XhciController {
    fn supported_speeds(&self) -> Vec<UsbSpeed> {
        vec![
            UsbSpeed::Super,
            UsbSpeed::High,
            UsbSpeed::Full,
            UsbSpeed::Low,
        ]
    }

    fn num_ports(&self) -> usize {
        self.num_ports
    }

    fn port_has_device(&self, port: usize) -> bool {
        self.get_port_reg(port).map_or(false, |port_reg| {
            port_reg.portsc & portsc::CURRENT_CONNECT_STATUS != 0
        })
    }

    fn reset_port(&mut self, port: usize) -> Result<()> {
        let port_reg = self
            .get_port_reg_mut(port)
            .ok_or(DriverError::InvalidParameter)?;

        // 设置复位位
        port_reg.portsc |= portsc::PORT_RESET;

        // 等待复位完成
        let mut timeout = 1_000_000;
        while timeout > 0 {
            if port_reg.portsc & portsc::PORT_RESET == 0 {
                break;
            }
            timeout -= 1;
            core::hint::spin_loop();
        }

        if timeout == 0 {
            return Err(DriverError::Timeout);
        }

        Ok(())
    }

    fn get_port_speed(&self, port: usize) -> UsbSpeed {
        self.get_port_reg(port)
            .map_or(UsbSpeed::Unknown, |port_reg| {
                let speed = (port_reg.portsc >> 10) & 0xF;
                match speed {
                    1 => UsbSpeed::Full,
                    2 => UsbSpeed::Low,
                    3 => UsbSpeed::High,
                    4 => UsbSpeed::Super,
                    _ => UsbSpeed::Unknown,
                }
            })
    }

    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    fn submit_urb(&mut self, urb: &Urb) -> Result<()> {
        // USB-1.3: TRACK-688EA7 消除 — URB 提交骨架
        //
        // 提交流程 (xHCI 规范 §4.6.1 + §4.11.3):
        // 1. 检查控制器已初始化
        // 2. 检查 URB 字段合法性 (endpoint in [1, 15], device != 0, buffer_length > 0)
        // 3. 分配 URB ID
        // 4. 构造 TRB (Normal Transfer TRB 或 Setup/Data/Status Stage TRB)
        // 5. 计算 doorbell 寄存器地址 (DBOFF + slot * 4)
        // 6. 写 doorbell 触发控制器处理
        //
        // 注: 此为**骨架实装**, 不含完整 Event Ring / DMA 调度:
        // - TRB 写入由 caller 管理的 Transfer Ring 缓冲 (Phase E 第 4 组 USB-1.5 实装)
        // - Event Ring 处理由中断上半部 + 下半部调度 (Phase E 第 4 组)
        // - 当前阶段仅触发 doorbell, 状态查询通过 URB ID 索引

        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }
        if urb.device == 0 {
            return Err(DriverError::InvalidParameter);
        }
        if urb.endpoint > 15 || urb.endpoint == 0 {
            return Err(DriverError::InvalidParameter);
        }
        if urb.buffer.is_null() || urb.buffer_length == 0 {
            return Err(DriverError::InvalidParameter);
        }

        // 1. 分配 URB ID
        let urb_id = self.next_urb_id;
        self.next_urb_id = self.next_urb_id.wrapping_add(1);

        // 2. 构造 Normal Transfer TRB (skeleton: 暂不写入 Transfer Ring,
        //    真实硬件应由 caller 维护 Transfer Ring 并填入 DMA 地址).
        //    此处仅记录元数据供 Phase E 第 4 组 Event Ring 处理器查询.
        let _trb = Trb::new(
            urb.buffer as u64,
            (urb.buffer_length as u32) & 0x0001_FFFF, // TRB 状态 (status): 传输长度 (低 17 位)
            (u32::from(urb.endpoint) << 16) | (TrbType::Normal as u32) << 10 | 1, // TRB 控制 (control): 端点 | 类型 | cycle 位
        );

        // 3. 触发 doorbell (DBOFF + slot * 4).
        //    slot ID 默认使用 urb.device (USB 规范: 1-127), xHCI 内部映射到 slot ID.
        let slot_id = urb.device as usize;
        // SAFETY: cap_regs 由 init_hardware 已设置, db_off 字段有效.
        unsafe {
            let cap = &*self.cap_regs;
            let doorbell_base =
                (self.iomem.as_ref().unwrap().virt_ptr() as usize) + cap.db_off as usize;
            let doorbell_addr = doorbell_base + slot_id * 4;
            // SAFETY: doorbell 寄存器已通过 init_hardware 验证, slot_id 在 [1, num_slots).
            //         write_volatile 保证 MMIO 写入不被编译器优化掉.
            core::ptr::write_volatile(doorbell_addr as *mut u32, urb_id);
        }

        // 4. 记录待处理 URB (供 Phase E 第 4 组 Event Ring 处理)
        self.pending_urbs.push((urb_id, urb.id));

        Ok(())
    }

    fn cancel_urb(&mut self, _urb_id: u32) -> Result<()> {
        Err(DriverError::UnsupportedOperation)
    }

    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    fn allocate_address(&mut self) -> Result<u8> {
        // USB-1.4: TRACK-2E0EB0 消除 — 设备地址分配
        //
        // USB 设备地址空间 (USB 2.0 规范 §9.1.2):
        // - 地址 0: 保留给 default address (未配置设备)
        // - 地址 1..=127: 设备地址 (xHCI 兼容, USB 3.0 扩展到 255)
        // - 地址 255: 保留 (广播)
        //
        // 实现策略:
        // 1. 从 next_address_hint 开始扫描 address_bitmap
        // 2. 找到第一个未使用位 (bit=0), 标记为已使用 (bit=1)
        // 3. 返回该地址 (1..=254), 更新 next_address_hint 加速下次分配
        //
        // 注: address_bitmap 仅覆盖 0..=255 槽位, 与 num_slots 字段独立
        //     (num_slots 由 xHCI 控制器硬件决定, 1..=255).

        // 从 hint 开始扫描到 254, 然后回卷到 1
        for offset in 0..254 {
            let addr = self.next_address_hint.wrapping_add(offset as u8);
            // 处理回卷: 跳过地址 0 (保留) 和 255 (保留)
            // u8 wrapping_add 仅产生 0..=255 范围, 上面 match 已穷尽所有情况.
            if addr == 0 || addr == 255 {
                continue;
            }

            let byte_idx = (addr / 8) as usize;
            let bit_idx = addr % 8;
            if byte_idx >= self.address_bitmap.len() {
                continue;
            }
            if self.address_bitmap[byte_idx] & (1 << bit_idx) == 0 {
                // 未使用, 标记
                self.address_bitmap[byte_idx] |= 1 << bit_idx;
                // 更新 hint 为 addr + 1 (下次从这里开始扫描)
                self.next_address_hint = addr.wrapping_add(1);
                if self.next_address_hint == 0 || self.next_address_hint == 255 {
                    self.next_address_hint = 1;
                }
                return Ok(addr);
            }
        }

        // 全部地址已用尽
        Err(DriverError::Busy)
    }

    fn free_address(&mut self, address: u8) {
        // USB-1.4: TRACK-1F75C1 消除 — 设备地址释放
        //
        // 清零 address_bitmap 对应位. 地址 0 和 255 静默忽略 (保留地址).
        if address == 0 || address == 255 {
            return;
        }
        let byte_idx = (address / 8) as usize;
        let bit_idx = address % 8;
        if byte_idx < self.address_bitmap.len() {
            self.address_bitmap[byte_idx] &= !(1 << bit_idx);
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::driver::usb::usb_core;

    #[test]
    fn test_xhci_controller_creation() {
        // UT-06 (2026-09-24): 各用例须用互不重叠的 fake MMIO 区间 —
        // ALIAS_REGISTRY 拒绝区间重叠, host 侧测试并行执行时会互相注册失败.
        // SAFETY: 测试用固定 MMIO 地址, identity-mapped in test environment
        let iomem = unsafe {
            IoMem::new(
                crate::framework::mm::PhysAddr(0xFE000000),
                0x10000,
                "xhci-test",
            )
            .expect("test IoMem")
        };
        let ctrl = XhciController::new(iomem);
        assert_eq!(ctrl.name(), "xHCI Controller");
        assert_eq!(ctrl.device_type(), DeviceType::Bus);
        assert!(!ctrl.is_ready());
    }

    #[test]
    fn test_trb_creation() {
        let trb = Trb::new(0x12345678, 0, 0x12345678);
        // Trb 是 packed 结构: 字段取引用会触发 E0793, 故按值取出再断言.
        assert_eq!({ trb.parameter }, 0x12345678);
        assert_eq!({ trb.status }, 0);
        assert_eq!({ trb.control }, 0x12345678);
    }

    #[test]
    fn test_portsc_bits() {
        assert_eq!(portsc::CURRENT_CONNECT_STATUS, 1);
        assert_eq!(portsc::PORT_RESET, 1 << 4);
    }

    // USB-1.4: 设备地址分配/释放单元测试
    //
    // 注: 地址分配不需要硬件 (仅操作 address_bitmap), 可单测.
    //     submit_urb / cancel_urb 需要真实硬件, 不在此单测.

    /// 单元测试用 XhciController 构造器 (fake MMIO region).
    ///
    /// `base` 须各用例唯一 (见 `test_xhci_controller_creation` 注释).
    ///
    /// SAFETY: fake 物理地址 + identity-map 测试脚手架保证
    /// phys..phys+len 已被映射, ALIAS_REGISTRY 在测试进程下独占.
    fn make_test_ctrl(base: u64) -> XhciController {
        // SAFETY: 测试脚手架, 详见函数 doc.
        let iomem = unsafe {
            IoMem::new(crate::framework::mm::PhysAddr(base), 0x10000, "xhci-test")
                .expect("test IoMem")
        };
        XhciController::new(iomem)
    }

    #[test]
    fn test_address_allocate_returns_first_free_slot() {
        let mut ctrl = make_test_ctrl(0xFE010000);
        // 初始 next_address_hint = 1, 应分配到 1
        assert_eq!(ctrl.allocate_address().unwrap(), 1);
        // 再分配应得 2
        assert_eq!(ctrl.allocate_address().unwrap(), 2);
        // 再分配应得 3
        assert_eq!(ctrl.allocate_address().unwrap(), 3);
    }

    #[test]
    fn test_address_free_then_reallocate() {
        let mut ctrl = make_test_ctrl(0xFE020000);
        let addr1 = ctrl.allocate_address().unwrap();
        ctrl.free_address(addr1);
        // 释放后重新分配, 因 next_address_hint 已更新, 不一定回到 addr1
        let _addr2 = ctrl.allocate_address().unwrap();
        // 验证 addr1 已被释放 (bitmap 对应位为 0)
        // 通过 free_address 幂等性验证: 重复释放同一地址不应 panic
        ctrl.free_address(addr1);
    }

    #[test]
    fn test_address_free_zero_and_255_are_noops() {
        let mut ctrl = make_test_ctrl(0xFE030000);
        // 地址 0 和 255 是保留地址, 静默忽略
        ctrl.free_address(0);
        ctrl.free_address(255);
        // 分配仍应从 1 开始
        assert_eq!(ctrl.allocate_address().unwrap(), 1);
    }

    #[test]
    fn test_address_allocate_exhaustion_returns_busy() {
        let mut ctrl = make_test_ctrl(0xFE040000);
        // 分配所有 254 个地址 (1..=254)
        for _ in 0..254 {
            ctrl.allocate_address().expect("should have free slot");
        }
        // 第 255 次应返回 Busy
        let result = ctrl.allocate_address();
        assert!(matches!(result, Err(DriverError::Busy)));
    }

    #[test]
    fn test_address_reuse_after_free() {
        let mut ctrl = make_test_ctrl(0xFE050000);
        // 分配 1, 2, 3
        let _a = ctrl.allocate_address().unwrap();
        let _b = ctrl.allocate_address().unwrap();
        let c = ctrl.allocate_address().unwrap();
        // 释放 c (值=3)
        ctrl.free_address(c);
        // 继续分配应得 4 (hint 已递增)
        let d = ctrl.allocate_address().unwrap();
        assert_eq!(d, 4);
    }

    // USB-1.3: URB 提交骨架单元测试 (参数校验 + 计数器单调递增)

    #[test]
    fn test_submit_urb_fails_when_not_initialized() {
        let mut ctrl = make_test_ctrl(0xFE060000);
        // 未 init_hardware, initialized=false
        let mut buf = [0u8; 16];
        let urb = Urb {
            id: 1,
            device: 1,
            endpoint: 1,
            setup: None,
            buffer: buf.as_mut_ptr(),
            buffer_length: buf.len(),
            actual_length: 0,
            status: usb_core::UrbStatus::Pending,
            callback: None,
        };
        let result = ctrl.submit_urb(&urb);
        assert!(matches!(result, Err(DriverError::NotInitialized)));
    }

    #[test]
    fn test_submit_urb_fails_with_device_zero() {
        let mut ctrl = make_test_ctrl(0xFE070000);
        ctrl.initialized = true; // 绕过 init_hardware (无硬件环境)
        let mut buf = [0u8; 16];
        let urb = Urb {
            id: 1,
            device: 0, // invalid
            endpoint: 1,
            setup: None,
            buffer: buf.as_mut_ptr(),
            buffer_length: buf.len(),
            actual_length: 0,
            status: usb_core::UrbStatus::Pending,
            callback: None,
        };
        let result = ctrl.submit_urb(&urb);
        assert!(matches!(result, Err(DriverError::InvalidParameter)));
    }

    #[test]
    fn test_submit_urb_fails_with_invalid_endpoint() {
        let mut ctrl = make_test_ctrl(0xFE080000);
        ctrl.initialized = true;
        let mut buf = [0u8; 16];
        let urb = Urb {
            id: 1,
            device: 1,
            endpoint: 16, // invalid (> 15)
            setup: None,
            buffer: buf.as_mut_ptr(),
            buffer_length: buf.len(),
            actual_length: 0,
            status: usb_core::UrbStatus::Pending,
            callback: None,
        };
        let result = ctrl.submit_urb(&urb);
        assert!(matches!(result, Err(DriverError::InvalidParameter)));
    }

    #[test]
    fn test_submit_urb_fails_with_null_buffer() {
        let mut ctrl = make_test_ctrl(0xFE090000);
        ctrl.initialized = true;
        let urb = Urb {
            id: 1,
            device: 1,
            endpoint: 1,
            setup: None,
            buffer: core::ptr::null_mut(), // invalid
            buffer_length: 16,
            actual_length: 0,
            status: usb_core::UrbStatus::Pending,
            callback: None,
        };
        let result = ctrl.submit_urb(&urb);
        assert!(matches!(result, Err(DriverError::InvalidParameter)));
    }
}
