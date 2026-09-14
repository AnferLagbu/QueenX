#![deny(unsafe_code)]
//! E1000 网卡驱动 (services 层 re-export shim).
//!
//! ## B04-AUDIT-005 #4 v2 (2026-08-24): E1000Driver 整体上移 framework
//!
//! E1000Driver 业务逻辑 + E1000Io MMIO 访问器 + 寄存器偏移常量全部位于
//! `framework::driver::net::e1000_io`. 本文件仅保留:
//! - 描述符状态/命令常量 (`E1000_TXD_CMD_EOP` 等) — 业务层语义, services 保留
//! - 描述符结构 re-export (从 framework::dma_ring)
//! - E1000Driver re-export (从 framework::e1000_io)
//!
//! 调用方仍可 `use crate::services::driver::net::e1000::*` 兼容.

pub use crate::framework::driver::net::dma_ring::{
    E1000_RX_BUFFER_SIZE, E1000_RX_RING_SIZE, E1000RxDesc, E1000TxDesc,
};
pub use crate::framework::driver::net::e1000_io::E1000Driver;

// ============================================================================
// 描述符状态/命令常量
// ============================================================================

/// TX 描述符命令: End Of Packet
pub const E1000_TXD_CMD_EOP: u8 = 1 << 0;
/// TX 插述符命令: Insert FCS
pub const E1000_TXD_CMD_IFCS: u8 = 1 << 1;
/// TX 描述符命令: Report Status
pub const E1000_TXD_CMD_RS: u8 = 1 << 3;
/// TX 描述符状态: Descriptor Done
pub const E1000_TXD_STAT_DD: u8 = 1 << 0;

/// RX 描述符状态: Descriptor Done
pub const E1000_RXD_STAT_DD: u8 = 1 << 0;
/// RX 描述符错误: CRC Error
pub const E1000_RXD_ERR_CE: u8 = 1 << 0;
/// RX 描述符错误: Symbol Error
pub const E1000_RXD_ERR_SE: u8 = 1 << 1;
/// RX 描述符错误: Sequence Error
pub const E1000_RXD_ERR_SEQ: u8 = 1 << 2;
/// RX 描述符错误: Receive Error
pub const E1000_RXD_ERR_RXE: u8 = 1 << 3;
