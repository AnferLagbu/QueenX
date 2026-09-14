//! ATA `BlockDevice` 适配层
//!
//! 将 ATA C FFI (`ata_read_sector`/`ata_write_sector`/`ata_disk_present`)
//! 包装为 `BlockDevice` trait 实现，使 `NestFS` 可以通过统一的 `BlockDevice`
//! 注册表访问 ATA 磁盘，与 virtio-blk/AHCI/NVMe 统一接口。
//!
//! 仅用于 `x86_64`; aarch64 上此模块会被编译排除。

use crate::kernel::framework::driver::BlockDevice;

/// ATA 磁盘的 `BlockDevice` 适配器。
///
/// 内部通过 C FFI 调用 ATA 控制器读写扇区。
/// 启动时通过二分探测确定磁盘容量。
pub struct AtaBlockDevice {
    /// ATA 驱动器编号 (0=Primary Master, 1=Primary Slave, etc.)
    drive: u8,
    /// 缓存的磁盘容量 (512字节扇区数)，启动时二分探测获得
    total_sectors_cache: u64,
}

impl AtaBlockDevice {
    /// 创建 ATA `BlockDevice` 适配器并探测磁盘容量。
    ///
    /// 返回 None 如果指定的驱动器上没有磁盘。
    pub fn new(drive: u8) -> Option<Self> {
        // FFI declarations
        // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
        unsafe extern "C" {
            fn ata_disk_present(disk: u8) -> i32;
            fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
        }

        // 检查磁盘是否存在
        // SAFETY: extern 函数的参数/返回值类型与 C ABI 声明一致; 调用方保证指针有效
        let present = unsafe { ata_disk_present(drive) };
        if present == 0 {
            return None;
        }

        // 二分查找总扇区数 (与 NestFS probe_disk_size 同样的方法)
        let mut lo: u32 = 0;
        let mut hi: u32 = 0xFFFF;
        let mut buf = [0u8; 512];
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            if unsafe { ata_read_sector(drive, mid, buf.as_mut_ptr()) } >= 0 {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let detected = if lo > 0 { u64::from(lo) * 512 } else { 0 };
        if detected == 0 {
            return None;
        }

        Some(Self {
            drive,
            total_sectors_cache: detected / 512,
        })
    }
}

// SAFETY: AtaBlockDevice 包装 C FFI 调用; ATA 控制器访问由 ata.rs 中的全局
// ATA_DEVICE Mutex 串行化. BlockDevice trait 方法使用内部锁或原子操作保证跨 CPU 安全.
unsafe impl Send for AtaBlockDevice {}
unsafe impl Sync for AtaBlockDevice {}

impl BlockDevice for AtaBlockDevice {
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32 {
        if buf.len() < 512 || sector > u64::from(u32::MAX) {
            return -1;
        }
        // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
        #[expect(
            clippy::items_after_statements,
            reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
        )]
        unsafe extern "C" {
            fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
        }
        // SAFETY: `as_mut_ptr` 是有效的 C ABI 函数指针; 参数列表与声明一致
        unsafe { ata_read_sector(self.drive, sector as u32, buf.as_mut_ptr()) }
    }

    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32 {
        if buf.len() < 512 || sector > u64::from(u32::MAX) {
            return -1;
        }
        // SAFETY: C ABI 互操作, 函数签名与外部 C 代码约定一致; 调用方保证指针有效
        #[expect(
            clippy::items_after_statements,
            reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
        )]
        unsafe extern "C" {
            fn ata_write_sector(disk: u8, sector: u32, buf: *const u8) -> i32;
        }
        // SAFETY: `as_ptr` 是有效的 C ABI 函数指针; 参数列表与声明一致
        unsafe { ata_write_sector(self.drive, sector as u32, buf.as_ptr()) }
    }

    fn blk_is_present(&self) -> bool {
        // SAFETY: C ABI 互操作, 函数签名与外部 C 代码约定一致
        unsafe extern "C" {
            fn ata_disk_present(disk: u8) -> i32;
        }
        // SAFETY: extern 函数的参数/返回值类型与 C ABI 声明一致; 调用方保证指针有效
        unsafe { ata_disk_present(self.drive) != 0 }
    }

    fn blk_total_sectors(&self) -> u64 {
        self.total_sectors_cache
    }
}
