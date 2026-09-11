//! ConfigSummary 启动期编码为紧凑字节序列 — services 层策略实现
//!
//! ## 迁移记录
//!
//! 策略代码于 2026-06-17 从 framework::config::boot_image 迁移至此。
//! framework 层仅保留 re-export 保持调用方兼容。

use crate::kernel::framework::config::get_config_summary;
use crate::kernel::framework::sync::IrqSpinLock;

const ENCODED_LEN: usize = 64;
const HEADER_MAGIC: u32 = 0xC0FFEE01;
const TAIL_MAGIC: [u8; 8] = [0xEE, 0xFF, 0xC0, 0x01, 0x00, 0x00, 0x00, 0x00];

/// 全局 "启动镜像" —— 由 `init()` 一次性填充.
pub static BOOT_IMAGE: IrqSpinLock<[u8; ENCODED_LEN]> = IrqSpinLock::new([0u8; ENCODED_LEN]);

#[inline]
fn pack_u32(buf: &mut [u8], offset: usize, v: u32) {
    if offset + 4 > buf.len() {
        return;
    }
    buf[offset] = (v & 0xFF) as u8;
    buf[offset + 1] = ((v >> 8) & 0xFF) as u8;
    buf[offset + 2] = ((v >> 16) & 0xFF) as u8;
    buf[offset + 3] = ((v >> 24) & 0xFF) as u8;
}

#[inline]
fn pack_u64(buf: &mut [u8], offset: usize, v: u64) {
    if offset + 8 > buf.len() {
        return;
    }
    pack_u32(buf, offset, v as u32);
    pack_u32(buf, offset + 4, (v >> 32) as u32);
}

#[inline]
fn pack_u8(buf: &mut [u8], offset: usize, v: u8) {
    if offset < buf.len() {
        buf[offset] = v;
    }
}

#[inline]
fn pack_u16(buf: &mut [u8], offset: usize, v: u16) {
    if offset + 2 > buf.len() {
        return;
    }
    buf[offset] = (v & 0xFF) as u8;
    buf[offset + 1] = ((v >> 8) & 0xFF) as u8;
}

/// 将当前 `ConfigSummary` 编码到全局 `boot_image` 缓冲区.
///
/// **调用语义**: 在 `config::init()` 末尾调用一次, 单线程上下文.
pub fn encode_boot_image() {
    let s = get_config_summary();
    let caps = s.capabilities;

    let mut guard = BOOT_IMAGE.lock();
    // 2026-09-11 实测: &mut *guard 触发 explicit_auto_deref, 需豁免
    #[allow(clippy::explicit_auto_deref)]
    let buf: &mut [u8; ENCODED_LEN] = &mut *guard;
    buf.fill(0);
    pack_u32(buf, 0, HEADER_MAGIC);
    pack_u32(buf, 4, 0);
    pack_u32(buf, 8, 0);
    pack_u32(buf, 12, s.max_cpus as u32);
    pack_u32(buf, 16, s.max_irqs as u32);
    pack_u32(buf, 20, s.max_processes as u32);
    pack_u32(buf, 24, s.max_threads as u32);
    pack_u32(buf, 28, s.actual_cpus);
    pack_u32(buf, 32, s.page_size as u32);
    pack_u32(buf, 36, s.kaslr_offset as u32);
    pack_u32(buf, 40, (s.kaslr_offset >> 32) as u32);

    let mut flags = 0u8;
    if caps.smp {
        flags |= 1 << 0;
    }
    if caps.kaslr {
        flags |= 1 << 1;
    }
    if caps.preempt {
        flags |= 1 << 2;
    }
    if caps.kpti {
        flags |= 1 << 3;
    }
    if caps.barrier {
        flags |= 1 << 4;
    }
    pack_u8(buf, 44, flags);
    pack_u8(buf, 45, 0);

    let mut feature_id: u16 = 0;
    if caps.smp {
        feature_id |= 1 << 0;
    }
    if caps.preempt {
        feature_id |= 1 << 1;
    }
    if caps.kaslr {
        feature_id |= 1 << 2;
    }
    if caps.kpti {
        feature_id |= 1 << 3;
    }
    if caps.barrier {
        feature_id |= 1 << 4;
    }
    pack_u16(buf, 46, feature_id);

    let mut xor = 0u64;
    for i in 0..48 {
        xor ^= u64::from(buf[i]) << ((i & 7) * 8);
    }
    pack_u64(buf, 48, xor);

    for (i, &b) in TAIL_MAGIC.iter().enumerate() {
        pack_u8(buf, 56 + i, b);
    }
}

/// Read a snapshot of the `boot_image` (供测试/调试).
pub fn read_boot_image() -> [u8; ENCODED_LEN] {
    *BOOT_IMAGE.lock()
}

/// 编码后的镜像占用字节数.
pub const fn encoded_len() -> usize {
    ENCODED_LEN
}
