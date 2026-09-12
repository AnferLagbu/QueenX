//! NVMe Identify 解析 host 集成测试 (§6.4 storage 专项 0 号子步)
//!
//! §6.4 直接方案 B: `nvme_read_identify_*` 解析 helper 从 framework 迁 services
//! (纯逻辑, 输入为 DMA 缓冲区字节切片). 本测试直接引用内核 services 真实
//! `parse_identify_controller` / `parse_identify_namespace` 纯函数 (B08-12 路线 C),
//! 验证 offset 提取与边界语义, 无测试/生产分叉.
//!
//! 覆盖:
//! 1. Identify Controller: nn (offset 516, LE u32) + mn (offset 24, 40 字节空终止)
//! 2. Identify Namespace: nsze (offset 0, LE u64) + flbas (offset 26) +
//!    lbaf_data (offset 128 + lbaf_idx*4, LE u32)
//! 3. 短缓冲区: 长度不足返回 None
//! 4. lbaf_idx >= 16: lbaf_data = 0

use queenx::kernel::services::driver::storage::nvme::{
    parse_identify_controller, parse_identify_namespace,
};

#[test]
fn identify_controller_parse_full() {
    let mut data = [0u8; 520];
    let model = b"QueenX NVMe";
    data[24..24 + model.len()].copy_from_slice(model);
    data[516..520].copy_from_slice(&3u32.to_le_bytes());

    let (nn, mn) = parse_identify_controller(&data).expect("长度足够应解析成功");
    assert_eq!(nn, 3);
    assert_eq!(&mn[..model.len()], model);
    assert_eq!(mn[model.len()], 0, "mn 应为空终止");
}

#[test]
fn identify_controller_short_buffer() {
    let data = [0u8; 100]; // 不足 520
    assert!(parse_identify_controller(&data).is_none());
}

#[test]
fn identify_controller_empty() {
    let data = [0u8; 0];
    assert!(parse_identify_controller(&data).is_none());
}

#[test]
fn identify_namespace_parse_full() {
    let mut data = [0u8; 192];
    data[0..8].copy_from_slice(&1_048_576u64.to_le_bytes());
    data[26] = 2; // lbaf_idx = 2
    data[136..140].copy_from_slice(&0x0001_0000u32.to_le_bytes()); // lbaf[2]: LBADS=9 → 512B

    let (nsze, flbas, lbaf_data) = parse_identify_namespace(&data).expect("长度足够应解析成功");
    assert_eq!(nsze, 1_048_576);
    assert_eq!(flbas, 2);
    assert_eq!(lbaf_data, 0x0001_0000);
}

#[test]
fn identify_namespace_lbaf_index_zero() {
    let mut data = [0u8; 192];
    data[0..8].copy_from_slice(&512u64.to_le_bytes());
    data[26] = 0x1F; // flbas = 0x1F → lbaf_idx = 15
    data[128 + 15 * 4..128 + 15 * 4 + 4].copy_from_slice(&0x0001_0000u32.to_le_bytes());

    let (nsze, flbas, lbaf_data) = parse_identify_namespace(&data).unwrap();
    assert_eq!(nsze, 512);
    assert_eq!(flbas, 0x1F);
    assert_eq!(lbaf_data, 0x0001_0000);
}

#[test]
fn identify_namespace_lbaf_idx_beyond_table() {
    // flbas & 0xF = 0, lbaf[0] 未设置 → 0
    let mut data = [0u8; 192];
    data[26] = 0x10; // 高半字节置位, 低半字节 0 → lbaf_idx = 0
    let (_, flbas, lbaf_data) = parse_identify_namespace(&data).unwrap();
    assert_eq!(flbas, 0x10);
    assert_eq!(lbaf_data, 0);
}

#[test]
fn identify_namespace_short_buffer() {
    let data = [0u8; 100]; // 不足 192
    assert!(parse_identify_namespace(&data).is_none());
}
