//! ATA/IDE 磁盘驱动 (Rust 安全重写)
//!
//! 提供对 ATA (Advanced Technology Attachment) 硬盘的底层控制：
//! - **控制器检测**: Primary/Secondary 通道
//! - **驱动器检测**: Master/Slave 设备
//! - **扇区读写**: LBA28 寻址模式
//! - **设备识别**: IDENTIFY 命令
//!
//! ## 硬件架构
//!
//! ```text
//! ATA 子系统
//! ├── Primary Channel (IO: 0x1F0-0x1F7, Ctrl: 0x3F6)
//! │   ├── Master (Drive 0)
//! │   └── Slave  (Drive 1)
//! └── Secondary Channel (IO: 0x170-0x177, Ctrl: 0x376)
//!     ├── Master (Drive 2)
//!     └── Slave  (Drive 3)
//! ```
//!
//! # Safety
//! 此模块直接操作硬件端口，必须在特权级执行。

#[cfg(target_arch = "x86_64")]
use super::framework::Driver;
use super::framework::{DeviceInfo, DeviceType, DriverError, Result};
use crate::framework::ioport::IoPort;
use crate::framework::sync::IrqSpinLock as Mutex;
use alloc::boxed::Box;
// ============================================================================
// ATA 硬件常量定义
// ============================================================================

/// Primary 通道 I/O 基地址
pub const ATA_PRIMARY_IO: u16 = 0x1F0;
/// Primary 通道控制寄存器基址
pub const ATA_PRIMARY_CTRL: u16 = 0x3F6;
/// Secondary 通道 I/O 基址
pub const ATA_SECONDARY_IO: u16 = 0x170;
/// Secondary 通道控制寄存器基址
pub const ATA_SECONDARY_CTRL: u16 = 0x376;

/// I/O 寄存器偏移量
const ATA_DATA: u16 = 0; // 数据寄存器 (16位)
const ATA_ERROR: u16 = 1; // 错误寄存器
const ATA_SECTOR_COUNT: u16 = 2; // 扇区计数
const ATA_SECTOR_NUM: u16 = 3; // 扇区号 (LBA 0-7)
const ATA_CYLINDER_LOW: u16 = 4; // 柱面低字节 (LBA 8-15)
const ATA_CYLINDER_HIGH: u16 = 5; // 柱面高字节 (LBA 16-23)
const ATA_DRIVE_HEAD: u16 = 6; // 驱动器/磁头选择
const ATA_STATUS: u16 = 7; // 状态寄存器
const ATA_COMMAND: u16 = 7; // 命令寄存器

/// 控制寄存器偏移量
const ATA_CTRL_ALT_STATUS: u8 = 0; // 替代状态 (控制寄存器块偏移 0)

/// 状态寄存器标志位
const ATA_STATUS_BSY: u8 = 0x80; // Busy
const ATA_STATUS_DRDY: u8 = 0x40; // Drive Ready
const ATA_STATUS_DF: u8 = 0x20; // Device Fault
const ATA_STATUS_DRQ: u8 = 0x08; // Data Request
const ATA_STATUS_ERR: u8 = 0x01; // Error

/// ATA 命令集
const ATA_CMD_IDENTIFY: u8 = 0xEC; // IDENTIFY DEVICE
const ATA_CMD_READ_SECTORS: u8 = 0x20; // READ SECTORS
const ATA_CMD_WRITE_SECTORS: u8 = 0x30; // WRITE SECTORS
const ATA_CMD_FLUSH_CACHE: u8 = 0xE7; // FLUSH CACHE

/// 超时值 (循环次数)
const ATA_TIMEOUT: u32 = 100000;

/// 成功和错误码
const ATA_SUCCESS: i32 = 0;
const ATA_ERR: i32 = -1;
const ATA_NO_DISK: i32 = -3;

/// 每个扇区的字数 (256 × 16bit = 512 bytes)
pub const WORDS_PER_SECTOR: usize = 256;

/// 最大支持设备数量 (2通道 × 2驱动器)
pub const MAX_ATA_DEVICES: usize = 4;

// ============================================================================
// 设备状态结构体
// ============================================================================

/// ATA 驱动器状态
#[derive(Debug, Clone, Copy)]
pub struct AtaDevice {
    /// 是否存在
    pub present: bool,
    /// 是否为主驱动器 (Master)
    pub is_master: bool,
    /// 所属通道 (0=Primary, 1=Secondary)
    pub channel: u8,
}

impl Default for AtaDevice {
    fn default() -> Self {
        Self {
            present: false,
            is_master: true,
            channel: 0,
        }
    }
}

/// ATA 控制器状态
pub struct AtaController {
    /// Primary 通道是否存在
    pub primary_present: bool,
    /// Secondary 通道是否存在
    pub secondary_present: bool,
    /// 驱动器列表 [Primary-Master, Primary-Slave, Secondary-Master, Secondary-Slave]
    pub devices: [AtaDevice; MAX_ATA_DEVICES],
    /// 设备信息 (用于 Driver trait)
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
    /// Primary I/O 端口 (0x1F0-0x1F7)
    primary_io: IoPort,
    /// Primary 控制端口 (0x3F6)
    primary_ctrl: IoPort,
    /// Secondary I/O 端口 (0x170-0x177)
    secondary_io: IoPort,
    /// Secondary 控制端口 (0x176)
    secondary_ctrl: IoPort,
}

// ============================================================================
// 底层辅助函数
// ============================================================================

/// 获取指定设备的 I/O 基地址
pub fn get_io_base(drive: u8) -> u16 {
    if drive < 2 {
        ATA_PRIMARY_IO
    } else {
        ATA_SECONDARY_IO
    }
}

/// 获取指定设备的控制寄存器基地址
pub fn get_ctrl_base(drive: u8) -> u16 {
    if drive < 2 {
        ATA_PRIMARY_CTRL
    } else {
        ATA_SECONDARY_CTRL
    }
}

/// ATA 延时函数 (读取状态寄存器 4 次)
fn ata_delay(ctrl: &IoPort) {
    for _ in 0..4 {
        let _ = ctrl.read_u8(u16::from(ATA_CTRL_ALT_STATUS));
    }
}

/// 读取备用状态寄存器 (不清除中断)
#[inline]
fn read_alt_status(ctrl: &IoPort) -> u8 {
    ctrl.read_u8(u16::from(ATA_CTRL_ALT_STATUS))
}

/// 等待 BSY 位清除
///
/// # Returns
/// * `Ok(())` - BSY 已清除
/// * `Err(DriverError::Timeout)` - 超时
fn wait_bsy(io: &IoPort, ctrl: &IoPort) -> Result<()> {
    let mut timeout = ATA_TIMEOUT;

    while timeout > 0 {
        let status = io.read_u8(ATA_STATUS as u16);
        if status & ATA_STATUS_BSY == 0 {
            return Ok(());
        }
        ata_delay(ctrl);
        timeout -= 1;
    }

    Err(DriverError::Timeout)
}

/// 等待 DRQ 位设置且 BSY 清除
fn wait_drq(io: &IoPort, ctrl: &IoPort) -> Result<()> {
    let mut timeout = ATA_TIMEOUT;

    while timeout > 0 {
        let status = io.read_u8(ATA_STATUS as u16);

        if status & ATA_STATUS_DF != 0 {
            return Err(DriverError::HardwareError);
        }

        if status & ATA_STATUS_ERR != 0 {
            let _ = io.read_u8(ATA_ERROR as u16);
            return Err(DriverError::HardwareError);
        }

        if status & (ATA_STATUS_DRQ | ATA_STATUS_BSY) == ATA_STATUS_DRQ {
            return Ok(());
        }
        ata_delay(ctrl);
        timeout -= 1;
    }

    Err(DriverError::Timeout)
}

/// 选择驱动器
fn select_drive(io: &IoPort, ctrl: &IoPort, slave: bool) -> Result<()> {
    io.write_u8(ATA_DRIVE_HEAD as u16, 0xA0 | (u8::from(slave) << 4));
    ata_delay(ctrl);

    wait_bsy(io, ctrl)
}

/// 检测驱动器是否存在
#[cfg(target_arch = "x86_64")]
fn detect_drive(io: &IoPort, ctrl: &IoPort, slave: bool) -> bool {
    // 选择驱动器
    if select_drive(io, ctrl, slave).is_err() {
        return false;
    }

    let status = io.read_u8(ATA_STATUS as u16);
    if status & ATA_STATUS_DRDY == 0 {
        return false;
    }

    // 设置参数为 0 (用于 IDENTIFY)
    io.write_u8(ATA_SECTOR_COUNT as u16, 0);
    io.write_u8(ATA_SECTOR_NUM as u16, 0);
    io.write_u8(ATA_CYLINDER_LOW as u16, 0);
    io.write_u8(ATA_CYLINDER_HIGH as u16, 0);

    // 发送 IDENTIFY 命令
    io.write_u8(ATA_COMMAND as u16, ATA_CMD_IDENTIFY);
    ata_delay(ctrl);

    // 检查状态
    let status = io.read_u8(ATA_STATUS as u16);

    // 如果状态为 0，说明没有设备
    if status == 0 {
        return false;
    }

    // 等待 BSY 清除
    if wait_bsy(io, ctrl).is_err() {
        return false;
    }

    // 检查错误
    let status = io.read_u8(ATA_STATUS as u16);
    if status & ATA_STATUS_ERR != 0 {
        return false;
    }

    // 等待 DRQ
    if wait_drq(io, ctrl).is_err() {
        return false;
    }

    // 读取 IDENTIFY 数据 (丢弃)
    for _ in 0..WORDS_PER_SECTOR {
        let _ = io.read_u16(ATA_DATA as u16);
    }

    true
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

// SAFETY: 单核内核, ATA PIO 模式操作无并发
unsafe impl Send for AtaController {}
unsafe impl Sync for AtaController {}

#[cfg(target_arch = "x86_64")]
impl Driver for AtaController {
    fn name(&self) -> &'static str {
        "ATA/IDE Controller"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn init(&mut self) -> Result<()> {
        self.primary_present = false;
        self.secondary_present = false;

        for device in &mut self.devices {
            *device = AtaDevice::default();
        }

        // === 检测 Primary 通道 ===
        // Software Reset
        self.primary_ctrl.write_u8(0, 0x04);
        ata_delay(&self.primary_ctrl);
        self.primary_ctrl.write_u8(0, 0x00);

        // ATA 规范 9.2: 轮询备用状态等待 BSY 清除 (400ns minimum)
        for _ in 0..1000 {
            if read_alt_status(&self.primary_ctrl) & ATA_STATUS_BSY == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // 写入签名值进行检测
        self.primary_io.write_u8(ATA_SECTOR_COUNT as u16, 0x55);
        self.primary_io.write_u8(ATA_SECTOR_NUM as u16, 0xAA);

        // 读取并验证
        let count = self.primary_io.read_u8(ATA_SECTOR_COUNT as u16);
        let num = self.primary_io.read_u8(ATA_SECTOR_NUM as u16);

        if count == 0x55 && num == 0xAA {
            self.primary_present = true;

            // 检测 Master
            if detect_drive(&self.primary_io, &self.primary_ctrl, false) {
                self.devices[0].present = true;
                self.devices[0].is_master = true;
                self.devices[0].channel = 0;
            }

            // 检测 Slave
            if detect_drive(&self.primary_io, &self.primary_ctrl, true) {
                self.devices[1].present = true;
                self.devices[1].is_master = false;
                self.devices[1].channel = 0;
            }
        }

        // === 检测 Secondary 通道 ===
        self.secondary_ctrl.write_u8(0, 0x04);
        ata_delay(&self.secondary_ctrl);
        self.secondary_ctrl.write_u8(0, 0x00);

        // ATA 规范 9.2: 轮询备用状态等待 BSY 清除
        for _ in 0..1000 {
            if read_alt_status(&self.secondary_ctrl) & ATA_STATUS_BSY == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        self.secondary_io.write_u8(ATA_SECTOR_COUNT as u16, 0x55);
        self.secondary_io.write_u8(ATA_SECTOR_NUM as u16, 0xAA);

        let count = self.secondary_io.read_u8(ATA_SECTOR_COUNT as u16);
        let num = self.secondary_io.read_u8(ATA_SECTOR_NUM as u16);

        if count == 0x55 && num == 0xAA {
            self.secondary_present = true;

            if detect_drive(&self.secondary_io, &self.secondary_ctrl, false) {
                self.devices[2].present = true;
                self.devices[2].is_master = true;
                self.devices[2].channel = 1;
            }

            if detect_drive(&self.secondary_io, &self.secondary_ctrl, true) {
                self.devices[3].present = true;
                self.devices[3].is_master = false;
                self.devices[3].channel = 1;
            }
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized && (self.primary_present || self.secondary_present)
    }

    #[expect(
        clippy::no_effect_underscore_binding,
        reason = "no_effect_underscore_binding: let _ = expr 用于类型推导/副作用; 当前优先 expect"
    )]
    fn status(&self) -> &'static str {
        if !self.initialized {
            "Not initialized"
        } else if !self.is_ready() {
            "No drives detected"
        } else {
            let _present_count = 0;
            for dev in &self.devices {
                if dev.present {
                    // present_count += 1;  // 可用于统计
                }
            }
            "Ready"
        }
    }
}

// ============================================================================
// 公共 API
// ============================================================================

impl AtaController {
    /// 创建新的 ATA 控制器实例
    /// # Panics
    /// 主/次 ATA I/O 或控制端口初始化失败时 panic。
    pub fn new() -> Self {
        // SAFETY: ATA I/O 端口地址由 PC 规范确定 (Primary: 0x1F0/0x3F6, Secondary: 0x170/0x176),
        // 不与其他 IoPort 实例重叠.
        let primary_io =
            unsafe { IoPort::new(ATA_PRIMARY_IO, 8, "ata-pio") }.expect("ata-pio port init failed");
        let primary_ctrl = unsafe { IoPort::new(ATA_PRIMARY_CTRL, 2, "ata-pctrl") }
            .expect("ata-pctrl port init failed");
        let secondary_io = unsafe { IoPort::new(ATA_SECONDARY_IO, 8, "ata-sio") }
            .expect("ata-sio port init failed");
        let secondary_ctrl = unsafe { IoPort::new(ATA_SECONDARY_CTRL, 2, "ata-sctrl") }
            .expect("ata-sctrl port init failed");
        Self {
            primary_present: false,
            secondary_present: false,
            devices: [AtaDevice::default(); MAX_ATA_DEVICES],
            info: DeviceInfo::new("ata_controller", DeviceType::Block),
            initialized: false,
            primary_io,
            primary_ctrl,
            secondary_io,
            secondary_ctrl,
        }
    }

    /// 检查指定驱动器是否存在
    ///
    /// # Arguments
    /// * `drive` - 驱动器编号 (0-3)
    pub fn disk_present(&self, drive: u8) -> bool {
        if (drive as usize) >= MAX_ATA_DEVICES {
            return false;
        }
        self.devices[drive as usize].present
    }

    /// 获取指定驱动器的 I/O 和控制端口引用
    fn ports_for_drive(&self, drive: u8) -> (&IoPort, &IoPort) {
        if drive < 2 {
            (&self.primary_io, &self.primary_ctrl)
        } else {
            (&self.secondary_io, &self.secondary_ctrl)
        }
    }

    /// 读取单个扇区
    ///
    /// # Arguments
    /// * `drive` - 驱动器编号 (0-3)
    /// * `lba` - 逻辑块地址 (28-bit LBA)
    /// * `buffer` - 输出缓冲区 (至少 512 字节)
    ///
    /// # Returns
    /// * `Ok(())` - 读取成功
    /// * `Err(DriverError)` - 错误
    /// # Errors
    /// 驱动器不存在、驱动器选择失败或设备忙/未就绪时返回 Err。
    #[cfg(target_arch = "x86_64")]
    pub fn read_sector(&self, drive: u8, lba: u32, buffer: &mut [u8; 512]) -> Result<()> {
        if !self.disk_present(drive) {
            return Err(DriverError::DeviceNotFound);
        }

        let (io, ctrl) = self.ports_for_drive(drive);
        let slave = (drive & 0x01) != 0;

        select_drive(io, ctrl, slave)?;

        // 设置 LBA 地址
        io.write_u8(ATA_SECTOR_COUNT as u16, 1); // 1 个扇区
        io.write_u8(ATA_SECTOR_NUM as u16, (lba & 0xFF) as u8); // LBA 0-7
        io.write_u8(ATA_CYLINDER_LOW as u16, ((lba >> 8) & 0xFF) as u8); // LBA 8-15
        io.write_u8(ATA_CYLINDER_HIGH as u16, ((lba >> 16) & 0xFF) as u8); // LBA 16-23
        io.write_u8(
            ATA_DRIVE_HEAD as u16,
            0xE0 | (u8::from(slave) << 4) | (((lba >> 24) & 0x0F) as u8),
        );
        ata_delay(ctrl);

        // 发送读命令
        io.write_u8(ATA_COMMAND as u16, ATA_CMD_READ_SECTORS);
        ata_delay(ctrl);

        wait_bsy(io, ctrl)?;
        wait_drq(io, ctrl)?;

        // 读取数据 (512 字节 = 256 个 16-bit 字)
        for i in 0..WORDS_PER_SECTOR {
            let word = io.read_u16(ATA_DATA as u16);
            buffer[i * 2] = (word & 0xFF) as u8;
            buffer[i * 2 + 1] = ((word >> 8) & 0xFF) as u8;
        }

        Ok(())
    }

    /// 写入单个扇区
    ///
    /// # Arguments
    /// * `drive` - 驱动器编号 (0-3)
    /// * `lba` - 逻辑块地址
    /// * `buffer` - 输入缓冲区 (512 字节)
    /// # Errors
    /// 驱动器不存在、驱动器选择失败或设备忙/未就绪时返回 Err。
    #[cfg(target_arch = "x86_64")]
    pub fn write_sector(&self, drive: u8, lba: u32, buffer: &[u8; 512]) -> Result<()> {
        if !self.disk_present(drive) {
            return Err(DriverError::DeviceNotFound);
        }

        let (io, ctrl) = self.ports_for_drive(drive);
        let slave = (drive & 0x01) != 0;

        select_drive(io, ctrl, slave)?;

        // 设置 LBA 地址
        io.write_u8(ATA_SECTOR_COUNT as u16, 1);
        io.write_u8(ATA_SECTOR_NUM as u16, (lba & 0xFF) as u8);
        io.write_u8(ATA_CYLINDER_LOW as u16, ((lba >> 8) & 0xFF) as u8);
        io.write_u8(ATA_CYLINDER_HIGH as u16, ((lba >> 16) & 0xFF) as u8);
        io.write_u8(
            ATA_DRIVE_HEAD as u16,
            0xE0 | (u8::from(slave) << 4) | (((lba >> 24) & 0x0F) as u8),
        );
        ata_delay(ctrl);

        // 发送写命令
        io.write_u8(ATA_COMMAND as u16, ATA_CMD_WRITE_SECTORS);
        ata_delay(ctrl);

        wait_bsy(io, ctrl)?;
        wait_drq(io, ctrl)?;

        // 写入数据
        for i in 0..WORDS_PER_SECTOR {
            let word = (u16::from(buffer[i * 2 + 1]) << 8) | u16::from(buffer[i * 2]);
            io.write_u16(ATA_DATA as u16, word);
        }

        // 刷新缓存
        io.write_u8(ATA_COMMAND as u16, ATA_CMD_FLUSH_CACHE);
        ata_delay(ctrl);

        wait_bsy(io, ctrl)?;

        Ok(())
    }

    /// 读取多个扇区
    /// # Errors
    /// 缓冲区过小或底层单扇区读取失败时返回 Err。
    #[cfg(target_arch = "x86_64")]
    pub fn read_sectors(
        &self,
        drive: u8,
        lba: u32,
        count: u32,
        buffer: &mut [u8],
    ) -> Result<usize> {
        let required_size = (count as usize) * 512;
        if buffer.len() < required_size {
            return Err(DriverError::BufferTooSmall);
        }

        for i in 0..count {
            let offset = (i as usize) * 512;
            let mut sector_buf = [0u8; 512];

            self.read_sector(drive, lba + i, &mut sector_buf)?;

            buffer[offset..offset + 512].copy_from_slice(&sector_buf);
        }

        Ok(count as usize)
    }

    /// 写入多个扇区
    /// # Errors
    /// 缓冲区过小或底层单扇区写入失败时返回 Err。
    #[cfg(target_arch = "x86_64")]
    pub fn write_sectors(&self, drive: u8, lba: u32, count: u32, buffer: &[u8]) -> Result<usize> {
        let required_size = (count as usize) * 512;
        if buffer.len() < required_size {
            return Err(DriverError::BufferTooSmall);
        }

        for i in 0..count {
            let offset = (i as usize) * 512;
            let mut sector_buf = [0u8; 512];
            sector_buf.copy_from_slice(&buffer[offset..offset + 512]);

            self.write_sector(drive, lba + i, &sector_buf)?;
        }

        Ok(count as usize)
    }

    /// 获取已检测到的驱动器数量
    pub fn detected_device_count(&self) -> usize {
        self.devices.iter().filter(|d| d.present).count()
    }

    /// 获取控制器详细信息
    pub fn get_info(&self) -> &DeviceInfo {
        &self.info
    }
}

// ============================================================================
// FFI 兼容接口 (C 函数签名)
// ============================================================================

/// 全局 ATA 控制器实例 (无 unsafe, Mutex 保护)
static ATA_DEVICE: Mutex<Option<Box<AtaController>>> = Mutex::new(None);

/// 初始化 ATA 子系统 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[cfg(target_arch = "x86_64")]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::borrow_as_ptr,
    reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
)]
pub extern "C" fn ata_init() {
    let mut controller = Box::new(AtaController::new());
    let _ = controller.init();

    // 注册到几丁质框架 (非所有权指针)
    let raw_ptr: *mut AtaController = &mut *controller;
    let _id = crate::framework::chitin::chitin_register(
        "ata_controller",
        crate::framework::chitin::ChitinProto::Block,
        Some(0x1F0), // Primary IO
        Some(14),    // IRQ 14
        raw_ptr as *mut u8,
    );

    *ATA_DEVICE.lock() = Some(controller);
}

/// 检查磁盘是否存在 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn ata_disk_present(drive: u8) -> i32 {
    let guard = ATA_DEVICE.lock();
    (*guard)
        .as_ref()
        .map_or(0, |controller| i32::from(controller.disk_present(drive)))
}

/// 读取扇区 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[cfg(target_arch = "x86_64")]
pub extern "C" fn ata_read_sector(drive: u8, lba: u32, buffer: *mut u8) -> i32 {
    if buffer.is_null() {
        return ATA_ERR;
    }

    (*ATA_DEVICE.lock())
        .as_ref()
        .map_or(ATA_NO_DISK, |controller| {
            let mut buf = [0u8; 512];
            match controller.read_sector(drive, lba, &mut buf) {
                Ok(()) => {
                    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                    unsafe {
                        core::ptr::copy_nonoverlapping(buf.as_ptr(), buffer, 512);
                    }
                    ATA_SUCCESS
                }
                Err(_) => ATA_ERR,
            }
        })
}

/// 写入扇区 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[cfg(target_arch = "x86_64")]
pub extern "C" fn ata_write_sector(drive: u8, lba: u32, buffer: *const u8) -> i32 {
    if buffer.is_null() {
        return ATA_ERR;
    }

    (*ATA_DEVICE.lock())
        .as_ref()
        .map_or(ATA_NO_DISK, |controller| {
            let mut buf = [0u8; 512];
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                core::ptr::copy_nonoverlapping(buffer, buf.as_mut_ptr(), 512);
            }

            match controller.write_sector(drive, lba, &buf) {
                Ok(()) => ATA_SUCCESS,
                Err(_) => ATA_ERR,
            }
        })
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(ATA_PRIMARY_IO, 0x1F0);
        assert_eq!(ATA_SECONDARY_IO, 0x170);
        assert_eq!(WORDS_PER_SECTOR, 256);
        assert_eq!(MAX_ATA_DEVICES, 4);
    }

    #[test]
    fn test_device_default_state() {
        let device = AtaDevice::default();
        assert!(!device.present);
        assert!(device.is_master);
        assert_eq!(device.channel, 0);
    }

    #[test]
    fn test_controller_creation() {
        let controller = AtaController::new();
        assert!(!controller.primary_present);
        assert!(!controller.secondary_present);
        assert_eq!(controller.detected_device_count(), 0);
        assert!(!controller.is_ready());
    }

    // UT-06 (2026-09-24): test_driver_trait_impl 已迁出 — controller.init() 走端口 I/O,
    // host 下 SIGSEGV; 等价断言迁至 framework/tests/driver.rs::ata_driver_trait
    // (register_ata_tests 注册), 在 kernel_test (QEMU 裸机) 执行.

    #[test]
    fn test_io_base_calculation() {
        assert_eq!(get_io_base(0), ATA_PRIMARY_IO);
        assert_eq!(get_io_base(1), ATA_PRIMARY_IO);
        assert_eq!(get_io_base(2), ATA_SECONDARY_IO);
        assert_eq!(get_io_base(3), ATA_SECONDARY_IO);

        assert_eq!(get_ctrl_base(0), ATA_PRIMARY_CTRL);
        assert_eq!(get_ctrl_base(3), ATA_SECONDARY_CTRL);
    }

    #[test]
    fn test_disk_present_bounds() {
        let controller = AtaController::new();

        // 未初始化时所有设备都不存在
        assert!(!controller.disk_present(0));
        assert!(!controller.disk_present(3));

        // 超出范围应该返回 false
        assert!(!controller.disk_present(4));
        assert!(!controller.disk_present(255));
    }

    #[test]
    fn test_error_codes() {
        let err = DriverError::DeviceNotFound;
        assert_eq!(err.to_string(), "Device not found");
        assert_ne!(err, DriverError::Timeout);
    }
}
