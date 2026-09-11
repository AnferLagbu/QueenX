//! USB 子系统 (USB Subsystem)
//!
//! 提供完整的USB支持：
//! - **USB核心**: 设备枚举和管理
//! - **xHCI驱动**: USB 3.0主机控制器
//! - **HID类**: 人机接口设备（键盘、鼠标）
//! - **大容量存储**: USB存储设备
//!
//! ## 架构
//!
//! ```text
//! USB Subsystem
//! ├── usb_core.rs    # 核心框架
//! ├── xhci.rs        # xHCI控制器
//! ├── hid.rs         # HID类驱动
//! └── mass_storage.rs # 大容量存储
//! ```

use alloc::vec::Vec;

pub mod enumerate;
pub mod hid;
pub mod mass_storage;
pub mod ring;
pub mod usb_core;
pub mod xhci;

// 导出常用类型
pub use usb_core::{
    ConfigurationDescriptor, DeviceClass, DeviceDescriptor, DeviceState, EndpointDescriptor,
    HostController, InterfaceDescriptor, Urb, UsbCore, UsbDevice, UsbSpeed,
};

pub use xhci::XhciController;

// ============================================================================
// xHCI PCI 发现 (USB-1.2)
// ============================================================================
//
// PCI class code 0x0C (Serial Bus 串行总线), subclass 0x03 (USB), prog_if 0x30 (xHCI)
// 来源: PCI Code and ID Assignment Specification §6
//
// 注: 不强制依赖 ACPI MCFG / 物理 MMIO base, 直接从 PciDevice.bars[0] 读取
//      xHCI 控制器的 BAR0 (MMIO 32-bit 或 64-bit).

/// PCI class code 串行总线控制器
const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;
/// PCI subclass code USB 控制器
const PCI_SUBCLASS_USB: u8 = 0x03;
/// PCI 编程接口 xHCI (USB 3.0)
const PCI_PROGIF_XHCI: u8 = 0x30;

/// 默认 xHCI MMIO 映射大小 (xHCI 规范要求至少 256 字节; 现代控制器通常 64 KiB).
///
/// 真实 BAR size 由 PciBar.size 提供, 但 `find_by_class` 返回的 PciDevice.bar
/// 可能 size=0 (某些固件未配置), 此 fallback 用于该情况.
const XHCI_DEFAULT_MMIO_SIZE: usize = 0x10000; // 64 KiB

/// 发现系统中的所有 xHCI 控制器 (USB-1.2: TRACK-558BA7 消除).
///
/// 扫描 PCI 总线, 过滤 `class=0x0C/subclass=0x03/prog_if=0x30` 的设备,
/// 为每个设备分配 `IoMem` 并实例化 `XhciController` (未初始化).
///
/// # 错误
///
/// - 返回 `Ok(Vec<XhciController>)`, 即使找不到控制器 (Vec 为空) 也不算错误.
/// # Errors
/// 为控制器分配 MMIO 或创建 `IoMem` 失败时返回 Err。
// 有意窄化: 用户内存代理, 指针/长度上下文保证
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "保留 Option/Result<()> 包装便于 API 兼容性 (调用方可能 match 或 .unwrap); 移除包装需同步修改调用点, 风险大"
)]
pub fn discover_xhci_controllers() -> framework::Result<Vec<XhciController>> {
    use crate::kernel::framework::iomem::IoMem;
    use crate::kernel::framework::mm::PhysAddr;

    // 1. 扫描所有 Serial Bus 设备
    let serial_bus_devs = crate::kernel::framework::pci::find_by_class(PCI_CLASS_SERIAL_BUS);
    let mut controllers = Vec::new();

    for dev in &serial_bus_devs {
        // 2. 过滤: subclass=0x03 (USB), prog_if=0x30 (xHCI)
        if !is_xhci_device(dev) {
            continue;
        }

        // 3. 读取 BAR0 (MMIO 基地址 + size)
        let (bar_base, bar_size) = match dev.bars.first() {
            Some(bar) if bar.bar_type != crate::kernel::framework::pci::BarType::None => (
                bar.base_addr,
                if bar.size > 0 {
                    bar.size
                } else {
                    XHCI_DEFAULT_MMIO_SIZE as u64
                },
            ),
            _ => {
                // 无 BAR0 跳过 (设备未配置, 通常是 BIOS 尚未枚举的设备)
                continue;
            }
        };

        // 4. 创建 IoMem — 经 from_pci_bar 安全包装 (PCI BAR 由枚举保证有效区域)
        let mmio_size = bar_size.min(XHCI_DEFAULT_MMIO_SIZE as u64) as usize;
        let Ok(iomem) = IoMem::from_pci_bar(PhysAddr(bar_base), mmio_size, "xhci-pci") else {
            continue; // MMIO 映射失败, 跳过该设备
        };

        // 5. 实例化 XhciController (未初始化, 调用方需 init_hardware)
        //    with_bar 携带 BAR0 地址/大小, 用于初始化日志.
        controllers.push(XhciController::new(iomem).with_bar(bar_base, mmio_size as u64));
    }

    Ok(controllers)
}

/// 判断 `PciDevice` 是否为 xHCI 控制器.
fn is_xhci_device(dev: &crate::kernel::framework::pci::PciDevice) -> bool {
    dev.class_code == PCI_CLASS_SERIAL_BUS
        && dev.subclass_code == PCI_SUBCLASS_USB
        && dev.prog_if == PCI_PROGIF_XHCI
}

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化USB子系统
/// # Errors
/// 扫描 xHCI 控制器失败时返回 Err。
pub fn usb_init() -> framework::Result<()> {
    // USB-1.2: TRACK-558BA7 消除 — 扫描 PCI 总线查找 xHCI 控制器
    let controllers = discover_xhci_controllers()?;
    crate::klog_info!(
        Driver,
        "[USB] discovered {} xHCI controller(s)",
        controllers.len()
    );

    // USB-1.2 续: 初始化找到的控制器
    // 注: 每个控制器的 init_hardware 会触发 reset + start (USB-1.1).
    //      当前阶段返回的控制器未持久化 (Phase E 引入 chitin 注册); 此处仅触发 init.
    for mut ctrl in controllers {
        // SAFETY: `iomem` 由 `discover_xhci_controllers` 通过 PCI BAR 创建, IoMem 边界已保证.
        match unsafe { init_xhci_controller(&mut ctrl) } {
            Ok(()) => {
                crate::klog_info!(
                    Driver,
                    "[USB] xHCI controller initialized: BAR0=0x{:X}",
                    ctrl.bar_base
                );
            }
            Err(e) => {
                crate::klog_drv_warn!("[USB] xHCI init failed: {:?}, skipping", e);
                continue;
            }
        }

        // USB-1.6: TRACK-832FCE 消除 — 枚举 USB 设备
        // 当前为骨架: 通过 mock 数据演示枚举流程. 真实硬件应使用 Control Transfer TRB 序列.
        enumerate_connected_devices(&mut ctrl);
    }

    Ok(())
}

/// 初始化单个 xHCI 控制器 (薄包装, 保留扩展点).
///
/// # Safety
///
/// 调用方必须保证 `ctrl` 由 `discover_xhci_controllers` 创建 (`IoMem` 边界已保证).
unsafe fn init_xhci_controller(ctrl: &mut XhciController) -> framework::Result<()> {
    ctrl.init_hardware()
        .map_err(|_| super::framework::DriverError::HardwareError)?;
    Ok(())
}

/// 枚举已初始化 xHCI 控制器上连接的所有 USB 设备 (USB-1.6).
///
/// 当前为**软件骨架**: 假设每个端口都有 HID Keyboard 设备, 调用 `enumerate::enumerate_new_device`.
/// 真实硬件应通过 Control Transfer TRB 序列发送 `GET_DESCRIPTOR` / `SET_ADDRESS` / `SET_CONFIGURATION`.
///
/// # 限制
///
/// - 不处理多 Configuration / 多 Interface 设备
/// - 不读取真实 Descriptor (mock 数据)
/// - 不等待设备稳定 (USB 规范要求 100ms 等待, 但 xHCI 已通过 port reset 处理)
fn enumerate_connected_devices(_ctrl: &mut XhciController) {
    use super::usb::enumerate;
    use super::usb::usb_core::UsbSpeed;

    // Phase E 集成: 此处应从 ctrl 读取 num_ports + port_has_device(),
    //               对每个连接的端口调用 enumerate_new_device.
    // 当前骨架: 仅示例 1 个端口的枚举流程 (不依赖真实硬件).
    let _ = enumerate::enumerate_new_device(1, UsbSpeed::Full, || {
        // 模拟地址分配: 返回下一个可用地址 (Phase E 由 XhciController 提供)
        Ok(1)
    });
}

use super::framework;
