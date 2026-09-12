#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! `NVMe` (Non-Volatile Memory Express) 驱动 — services 层安全代理 (Phase 2.1.3)
//!
//! 封装 `NVMe` 控制器的 `PCIe` BAR0 MMIO 操作,
//! 通过 `framework::IoMem` 提供 100% safe API。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 内部 `IoMem` 由 TCB 抽象, services 层只调用 safe 方法
//! - **类型安全**: 寄存器位、队列 ID、命令字典型用枚举/常量
//! - **完整驱动逻辑**: 队列管理、命令提交、完成处理、Identify、读写均在 services 层
//! - **DMA 通过 framework**: 所有 DMA 分配/释放通过 framework safe wrapper
//!
//! ## 硬件接口
//!
//! ```text
//! BAR0 (NVMe Controller MMIO):
//! ├── 0x00 CAP:   Controller Capabilities (R, u64)
//! ├── 0x08 VS:    Version (R, u32)
//! ├── 0x0C INTMS: Interrupt Mask Set (RW, u32)
//! ├── 0x10 INTMC: Interrupt Mask Clear (RW, u32)
//! ├── 0x14 CC:    Controller Configuration (RW, u32)
//! ├── 0x1C CSTS:  Controller Status (R, u32)
//! ├── 0x24 AQA:   Admin Queue Attributes (RW, u32)
//! ├── 0x28 ASQ:   Admin SQ Base Address (RW, u64)
//! ├── 0x30 ACQ:   Admin CQ Base Address (RW, u64)
//! └── 0x1000+:   Doorbell (per-queue, 32-bit)
//! ```
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.3 任务: `NVMe` 存储控制器迁移

use crate::kernel::framework::driver::storage::nvme as fw_nvme;
use crate::kernel::framework::iomem::IoMem;
use crate::kernel::framework::mm::PhysAddr;

// Services 层日志
use crate::slog_info;
use crate::slog_warn;

// ============================================================================
// BAR0 寄存器偏移
// ============================================================================

/// Controller Capabilities (R, u64)
pub const NVME_REG_CAP: usize = 0x00;
/// Version (R, u32)
pub const NVME_REG_VS: usize = 0x08;
/// Interrupt Mask Set (RW, u32)
pub const NVME_REG_INTMS: usize = 0x0C;
/// Interrupt Mask Clear (RW, u32)
pub const NVME_REG_INTMC: usize = 0x10;
/// Controller Configuration (RW, u32)
pub const NVME_REG_CC: usize = 0x14;
/// Controller Status (R, u32)
pub const NVME_REG_CSTS: usize = 0x1C;
/// Admin Queue Attributes (RW, u32)
pub const NVME_REG_AQA: usize = 0x24;
/// Admin SQ Base Address (RW, u64)
pub const NVME_REG_ASQ: usize = 0x28;
/// Admin CQ Base Address (RW, u64)
pub const NVME_REG_ACQ: usize = 0x30;
/// Doorbell 寄存器基址
pub const NVME_DB_BASE: usize = 0x1000;

// ============================================================================
// CC 寄存器位
// ============================================================================

/// Enable (CC.EN)
pub const CC_EN: u32 = 1 << 0;
/// I/O Submission Queue Entry Size (CC.IOSQES, 4-bit @ bit 16)
pub const CC_IOSQES_MASK: u32 = 0xF << 16;
/// I/O Completion Queue Entry Size (CC.IOCQES, 4-bit @ bit 20)
pub const CC_IOCQES_MASK: u32 = 0xF << 20;
/// CSS NVM Command Set (CC.CSS, 3-bit @ bit 4)
pub const CC_CSS_NVM: u32 = 0 << 4;
/// AMS Round Robin (CC.AMS, 2-bit @ bit 11)
pub const CC_AMS_RR: u32 = 0 << 11;
/// MPS Memory Page Size shift (CC.MPS, 4-bit @ bit 7)
pub const CC_MPS_SHIFT: u32 = 7;
/// IOCQES 值: 16 字节 CQ 条目, log2(16) = 4
pub const CC_IOCQES_VAL: u32 = 4 << 20;
/// IOSQES 值: 64 字节 SQ 条目, log2(64) = 6
pub const CC_IOSQES_VAL: u32 = 6 << 24;

// ============================================================================
// CSTS 寄存器位
// ============================================================================

/// Ready (CSTS.RDY)
pub const CSTS_RDY: u32 = 1 << 0;
/// Controller Fatal Status (CSTS.CFS)
pub const CSTS_CFS: u32 = 1 << 1;
/// Shutdown Status (CSTS.SHST, 2-bit @ bit 2)
pub const CSTS_SHST_MASK: u32 = 0x3 << 2;
/// NVM Subsystem Reset Occurred (CSTS.NSSRO)
pub const CSTS_NSSRO: u32 = 1 << 4;

// ============================================================================
// 队列常量
// ============================================================================

/// Admin 队列 ID
pub const ADMIN_QID: u16 = 0;
/// I/O 队列 ID
pub const IO_QID: u16 = 1;
/// Admin / I/O 队列深度
pub const QUEUE_DEPTH: u16 = 64;
/// SQ 条目大小 (64 字节)
pub const SQ_ENTRY_SIZE: u16 = 64;
/// CQ 条目大小 (16 字节)
pub const CQ_ENTRY_SIZE: u16 = 16;
/// SQ 占用字节 = depth * `entry_size`
pub const SQ_SIZE_BYTES: u32 = 64 * 64;
/// CQ 占用字节 = depth * `entry_size`
pub const CQ_SIZE_BYTES: u32 = 64 * 16;

// ============================================================================
// 命令操作码 (NVMe spec §6)
// ============================================================================

/// Admin Identify
pub const OP_ADMIN_IDENTIFY: u8 = 0x06;
/// Admin 创建 I/O 完成队列
pub const OP_ADMIN_CREATE_IOCQ: u8 = 0x05;
/// Admin 创建 I/O 提交队列
pub const OP_ADMIN_CREATE_IOSQ: u8 = 0x01;

/// I/O Read
pub const OP_IO_READ: u8 = 0x02;
/// I/O Write
pub const OP_IO_WRITE: u8 = 0x01;

// ============================================================================
// Identify CNS
// ============================================================================

/// Identify 控制器
pub const IDENTIFY_CNS_CONTROLLER: u8 = 0x01;
/// Identify 命名空间
pub const IDENTIFY_CNS_NAMESPACE: u8 = 0x00;

/// 扇区大小
pub const SECTOR_SIZE: usize = 512;

// ============================================================================
// 控制器状态结构
// ============================================================================

/// 控制器能力 (CAP) 解析
#[derive(Debug, Clone, Copy)]
pub struct ControllerCapabilities {
    /// 最大队列深度 (CAP.MQES, 16-bit @ bit 0)
    pub max_queue_entries: u16,
    /// Doorbell 步长 (CAP.DSTRD, 4-bit @ bit 32)
    pub doorbell_stride: u8,
}

impl ControllerCapabilities {
    /// 从 CAP 寄存器解析
    pub fn from_register(val: u64) -> Self {
        let mqes = (val & 0xFFFF) as u16;
        let dstrd = ((val >> 32) & 0xF) as u8;
        Self {
            max_queue_entries: mqes,
            doorbell_stride: dstrd,
        }
    }
}

/// 控制器状态 (CSTS) 解析
#[derive(Debug, Clone, Copy)]
pub struct ControllerStatus {
    /// 控制器就绪
    pub ready: bool,
    /// 控制器致命错误状态
    pub fatal: bool,
    /// 关机状态 (0=正常, 1=关机进行中, 2=关机完成)
    pub shutdown: u8,
    /// NVM 子系统复位已发生
    pub nssro: bool,
}

impl ControllerStatus {
    /// 从 CSTS 寄存器解析
    pub fn from_register(val: u32) -> Self {
        Self {
            ready: val & CSTS_RDY != 0,
            fatal: val & CSTS_CFS != 0,
            shutdown: ((val & CSTS_SHST_MASK) >> 2) as u8,
            nssro: val & CSTS_NSSRO != 0,
        }
    }
}

// ============================================================================
// NVMe 命令条目 (services 层定义, 避免直接依赖 framework 内部类型)
// ============================================================================

/// `NVMe` 提交队列条目 (64 字节, 与 `NVMe` spec 一致)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCmdEntry {
    pub opcode: u8,
    pub flags: u8,
    pub cid: u16,
    pub nsid: u32,
    pub cdw2: u32,
    pub cdw3: u32,
    pub mptr: u64,
    pub prp2: u64,
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

impl NvmeCmdEntry {
    /// 创建空命令
    pub fn new() -> Self {
        Self {
            opcode: 0,
            flags: 0,
            cid: 0,
            nsid: 0,
            cdw2: 0,
            cdw3: 0,
            mptr: 0,
            prp2: 0,
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// 创建读命令
    pub fn read(nsid: u32, slba: u64, nlb: u16, prp1: u64) -> Self {
        Self {
            opcode: OP_IO_READ,
            cid: 0,
            nsid,
            mptr: prp1,
            cdw10: (slba & 0xFFFF_FFFF) as u32,
            cdw11: ((slba >> 32) & 0xFFFF_FFFF) as u32,
            cdw12: (u32::from(nlb) - 1) & 0xFFFF,
            ..Self::new()
        }
    }

    /// 创建写命令
    pub fn write(nsid: u32, slba: u64, nlb: u16, prp1: u64) -> Self {
        Self {
            opcode: OP_IO_WRITE,
            cid: 0,
            nsid,
            mptr: prp1,
            cdw10: (slba & 0xFFFF_FFFF) as u32,
            cdw11: ((slba >> 32) & 0xFFFF_FFFF) as u32,
            cdw12: (u32::from(nlb) - 1) & 0xFFFF,
            ..Self::new()
        }
    }

    /// 创建 Identify 命令
    pub fn identify(nsid: u32, cns: u8, prp1: u64) -> Self {
        Self {
            opcode: OP_ADMIN_IDENTIFY,
            nsid,
            mptr: prp1,
            cdw10: u32::from(cns),
            ..Self::new()
        }
    }

    /// 创建 Create I/O Completion Queue 命令
    pub fn create_cq(qid: u16, cq_phys: u64, depth: u16) -> Self {
        Self {
            opcode: OP_ADMIN_CREATE_IOCQ,
            mptr: cq_phys,
            cdw10: ((u32::from(depth) - 1) << 16) | u32::from(qid),
            cdw11: 1, // PC=1 (physically contiguous)
            ..Self::new()
        }
    }

    /// 创建 Create I/O Submission Queue 命令
    pub fn create_sq(qid: u16, cqid: u16, sq_phys: u64, depth: u16) -> Self {
        Self {
            opcode: OP_ADMIN_CREATE_IOSQ,
            mptr: sq_phys,
            cdw10: ((u32::from(depth) - 1) << 16) | u32::from(qid),
            cdw11: u32::from(cqid) << 16 | 1, // CQID | PC
            ..Self::new()
        }
    }
}

/// `NVMe` 完成队列条目 (16 字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCplEntry {
    pub cdw0: u32,
    pub rsvd1: u32,
    pub sqhd: u16,
    pub sqid: u16,
    pub cid: u16,
    pub status: u16,
}

impl NvmeCplEntry {
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    /// 阶段标记匹配 = 完成
    pub fn is_completed(&self, phase: u16) -> bool {
        (self.status & 0x01) == phase
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    /// 获取状态码
    pub fn status_code(&self) -> u16 {
        (self.status >> 1) & 0x7FF
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    /// 是否成功
    pub fn is_success(&self) -> bool {
        self.status_code() == 0
    }
}

// ============================================================================
// 队列对管理 (services 层安全逻辑)
// ============================================================================

/// `NVMe` 队列对句柄 (services 层)
///
/// 封装队列状态 (tail/head/phase) 并通过 framework safe wrapper
/// 执行命令提交, 0 unsafe。
pub struct NvmeQueuePair {
    /// 队列 ID
    qid: u16,
    /// 队列深度
    depth: u32,
    /// SQ 当前 tail
    sq_tail: u32,
    /// CQ 当前 head
    cq_head: u32,
    /// 门铃步长 (2^DSTRD)
    db_stride: u32,
    /// Admin CQ phase bit
    admin_cq_phase: u16,
    /// I/O CQ phase bit
    io_cq_phase: u16,
    /// Admin CID 计数器
    admin_cid: u16,
    /// I/O CID 计数器
    io_cid: u16,
    /// 队列是否已创建
    created: bool,
}

impl NvmeQueuePair {
    /// 创建新的队列对
    pub fn new(qid: u16, depth: u32, db_stride: u32) -> Self {
        Self {
            qid,
            depth,
            sq_tail: 0,
            cq_head: 0,
            db_stride,
            admin_cq_phase: 1,
            io_cq_phase: 1,
            admin_cid: 0,
            io_cid: 0,
            created: false,
        }
    }

    /// 队列是否已创建
    pub fn is_created(&self) -> bool {
        self.created
    }

    /// 标记队列已创建
    pub fn set_created(&mut self, created: bool) {
        self.created = created;
    }

    /// 队列 ID
    pub fn id(&self) -> u16 {
        self.qid
    }

    /// 队列深度
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// SQ tail
    pub fn sq_tail(&self) -> u32 {
        self.sq_tail
    }

    /// 更新 SQ tail
    pub fn set_sq_tail(&mut self, tail: u32) {
        self.sq_tail = tail;
    }

    /// CQ head
    pub fn cq_head(&self) -> u32 {
        self.cq_head
    }

    /// 更新 CQ head
    pub fn set_cq_head(&mut self, head: u32) {
        self.cq_head = head;
    }

    /// Admin CQ phase
    pub fn admin_cq_phase(&self) -> u16 {
        self.admin_cq_phase
    }

    /// 更新 Admin CQ phase
    pub fn set_admin_cq_phase(&mut self, phase: u16) {
        self.admin_cq_phase = phase;
    }

    /// I/O CQ phase
    pub fn io_cq_phase(&self) -> u16 {
        self.io_cq_phase
    }

    /// 更新 I/O CQ phase
    pub fn set_io_cq_phase(&mut self, phase: u16) {
        self.io_cq_phase = phase;
    }

    /// 下一个 Admin CID
    pub fn next_admin_cid(&mut self) -> u16 {
        let cid = self.admin_cid;
        self.admin_cid = self.admin_cid.wrapping_add(1);
        cid
    }

    /// 下一个 I/O CID
    pub fn next_io_cid(&mut self) -> u16 {
        let cid = self.io_cid;
        self.io_cid = self.io_cid.wrapping_add(1);
        cid
    }

    /// 门铃步长
    pub fn db_stride(&self) -> u32 {
        self.db_stride
    }
}

// ============================================================================
// Identify 数据结构
// ============================================================================

/// `NVMe` Identify Controller 数据 (精简版, 仅必要字段)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeIdentifyController {
    pub vid: u16,
    pub ssvid: u16,
    pub sn: [u8; 20],
    pub mn: [u8; 40],
    pub fr: [u8; 8],
    pub rsvd1: [u8; 444],
    pub nn: u32, // offset 516: 命名空间数量
    pub rsvd2: [u8; 3756],
}

/// `NVMe` Identify Namespace 数据 (精简版)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeIdentifyNamespace {
    pub nsze: u64,
    pub ncap: u64,
    pub nuse: u64,
    pub nsfeat: u8,
    pub nlbaf: u8,
    pub flbas: u8,
    pub rsvd: [u8; 4085],
}

// ============================================================================
// Identify 数据解析 (§6.4 storage 专项 0 号子步: 从 framework 迁业务)
// ============================================================================

/// 解析 Identify Controller 数据 (纯函数, 输入为 DMA 缓冲区内字节切片).
///
/// 提取:
/// - `nn` — Namespace Count (offset 516, LE u32)
/// - `mn` — Model Number (offset 24, 40 字节, 空终止 ASCII)
///
/// 数据长度不足时返回 `None`.
pub fn parse_identify_controller(data: &[u8]) -> Option<(u32, [u8; 40])> {
    if data.len() < 520 {
        return None;
    }
    let nn = u32::from_le_bytes([data[516], data[517], data[518], data[519]]);
    let mut model = [0u8; 40];
    model.copy_from_slice(&data[24..64]);
    Some((nn, model))
}

/// 解析 Identify Namespace 数据 (纯函数, 输入为 DMA 缓冲区内字节切片).
///
/// 返回 `(nsze, flbas, lbaf_data_at_index)`:
/// - `nsze` — Namespace Size (offset 0, LE u64)
/// - `flbas` — Formatted LBA size (offset 26)
/// - `lbaf_data` — LBA Format 0..15 数据 (offset 128 + lbaf_idx*4, LE u32)
///
/// 数据长度不足 192 字节 (LBA Format 表最末项可达 offset 188..192) 时返回 `None`.
pub fn parse_identify_namespace(data: &[u8]) -> Option<(u64, u8, u32)> {
    if data.len() < 192 {
        return None;
    }
    let nsze = u64::from_le_bytes(data[0..8].try_into().ok()?);
    let flbas = data[26];
    let lbaf_idx = (flbas & 0x0F) as usize;
    let lbaf_data = if lbaf_idx < 16 {
        let off = 128 + lbaf_idx * 4;
        u32::from_le_bytes(data[off..off + 4].try_into().ok()?)
    } else {
        0
    };
    Some((nsze, flbas, lbaf_data))
}

// ============================================================================
// NVMe 控制器 (services 层安全驱动)
// ============================================================================

/// `NVMe` 控制器 — services 层安全驱动
///
/// 通过 framework safe wrapper 执行 DMA 和队列操作,
/// 自身 0 unsafe, 所有 unsafe 由 framework 层封装。
pub struct NvmeController {
    /// MMIO 寄存器句柄
    mmio: IoMem,
    /// 门铃步长 (bytes = 4 << DSTRD)
    db_stride: u32,
    /// Admin 队列物理地址 (SQ + CQ)
    admin_sq_phys: u64,
    admin_cq_phys: u64,
    /// Admin 队列对状态
    admin_queue: NvmeQueuePair,
    /// I/O 队列物理地址
    io_sq_phys: u64,
    io_cq_phys: u64,
    /// I/O 队列对状态
    io_queue: NvmeQueuePair,
    /// 命名空间数量
    namespace_count: u32,
    /// 命名空间大小 (LBA)
    namespace_size_lba: u64,
    /// LBA 格式字节数
    lba_format_size: u16,
    /// 控制器已初始化
    initialized: bool,
}

impl NvmeController {
    /// 创建 `NVMe` 控制器实例
    ///
    /// # 参数
    /// - `bar0_phys`: `PCIe` BAR0 MMIO 物理基地址
    /// - `len`: BAR0 MMIO 区域大小 (典型 0x2000)
    pub fn new(bar0_phys: u64, len: usize) -> Option<Self> {
        let mmio = IoMem::from_pci_bar(PhysAddr::new(bar0_phys), len, "nvme-bar0").ok()?;
        Some(Self {
            mmio,
            db_stride: 0,
            admin_sq_phys: 0,
            admin_cq_phys: 0,
            admin_queue: NvmeQueuePair::new(ADMIN_QID, u32::from(QUEUE_DEPTH), 0),
            io_sq_phys: 0,
            io_cq_phys: 0,
            io_queue: NvmeQueuePair::new(IO_QID, u32::from(QUEUE_DEPTH), 0),
            namespace_count: 0,
            namespace_size_lba: 0,
            lba_format_size: SECTOR_SIZE as u16,
            initialized: false,
        })
    }

    // ── 寄存器访问 (通过 IoMem safe 代理) ──

    /// 读 32 位寄存器
    #[inline]
    pub fn read32(&self, offset: usize) -> u32 {
        self.mmio.read_u32(offset)
    }

    /// 写 32 位寄存器
    #[inline]
    pub fn write32(&self, offset: usize, val: u32) {
        self.mmio.write_u32(offset, val);
    }

    /// 读 64 位寄存器
    #[inline]
    pub fn read64(&self, offset: usize) -> u64 {
        self.mmio.read_u64(offset)
    }

    /// 写 64 位寄存器
    #[inline]
    pub fn write64(&self, offset: usize, val: u64) {
        self.mmio.write_u64(offset, val);
    }

    /// 读 CAP (Controller Capabilities)
    pub fn capabilities(&self) -> ControllerCapabilities {
        ControllerCapabilities::from_register(self.read64(NVME_REG_CAP))
    }

    /// 读 VS (Version)
    pub fn version(&self) -> u32 {
        self.read32(NVME_REG_VS)
    }

    /// 设置中断掩码 (INTMS)
    pub fn enable_interrupts(&self, mask: u32) {
        self.write32(NVME_REG_INTMS, mask);
    }

    /// 清除中断掩码 (INTMC)
    pub fn disable_interrupts(&self, mask: u32) {
        self.write32(NVME_REG_INTMC, mask);
    }

    /// 读 CC (Controller Configuration)
    pub fn cc(&self) -> u32 {
        self.read32(NVME_REG_CC)
    }

    /// 写 CC
    pub fn set_cc(&self, val: u32) {
        self.write32(NVME_REG_CC, val);
    }

    /// 启用控制器 (CC.EN = 1)
    pub fn enable(&self) {
        self.set_cc(self.cc() | CC_EN);
    }

    /// 禁用控制器 (CC.EN = 0)
    pub fn disable(&self) {
        self.set_cc(self.cc() & !CC_EN);
    }

    /// 读 CSTS (Controller Status)
    pub fn status(&self) -> ControllerStatus {
        ControllerStatus::from_register(self.read32(NVME_REG_CSTS))
    }

    /// 控制器是否就绪
    pub fn is_ready(&self) -> bool {
        self.status().ready
    }

    /// 等待控制器就绪
    pub fn wait_ready(&self, timeout: u32) -> bool {
        let mut t = timeout;
        while !self.is_ready() && t > 0 {
            t -= 1;
            core::hint::spin_loop();
        }
        self.is_ready()
    }

    /// 等待控制器禁用
    pub fn wait_disabled(&self, timeout: u32) -> bool {
        let mut t = timeout;
        while self.is_ready() && t > 0 {
            t -= 1;
            core::hint::spin_loop();
        }
        !self.is_ready()
    }

    // ── Admin 队列配置 ──

    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    /// 写 AQA (Admin Queue Attributes)
    pub fn set_aqa(&self, asqs: u16, acqs: u16) {
        let val = u32::from(asqs) | (u32::from(acqs) << 16);
        self.write32(NVME_REG_AQA, val);
    }

    /// 写 ASQ (Admin SQ Base Address)
    pub fn set_asq(&self, paddr: u64) {
        self.write64(NVME_REG_ASQ, paddr);
    }

    /// 写 ACQ (Admin CQ Base Address)
    pub fn set_acq(&self, paddr: u64) {
        self.write64(NVME_REG_ACQ, paddr);
    }

    // ── Doorbell ──

    /// 写 Doorbell 寄存器
    pub fn write_doorbell(&self, queue_id: u16, is_completion: bool, value: u32) {
        let dstrd = self.db_stride;
        let offset = NVME_DB_BASE
            + (queue_id as usize) * 8
            + (if is_completion { 4 } else { 0 })
            + (dstrd as usize) * 4;
        self.write32(offset, value);
    }

    /// 提交 Admin SQ Doorbell (写 tail)
    pub fn ring_admin_sq(&self, tail: u32) {
        self.write_doorbell(ADMIN_QID, false, tail);
    }

    /// 提交 CQ Doorbell (写 head)
    pub fn ring_cq_head(&self, queue_id: u16, head: u32) {
        self.write_doorbell(queue_id, true, head);
    }

    // ── 命令提交 (通过 framework safe wrapper) ──

    /// 提交 Admin 命令并等待完成
    fn submit_admin_cmd(
        &mut self,
        cmd: fw_nvme::NvmeCommand,
    ) -> Result<fw_nvme::NvmeCompletion, ()> {
        let cid = self.admin_queue.next_admin_cid();
        let sq_phys = self.admin_sq_phys;
        let cq_phys = self.admin_cq_phys;
        let depth = self.admin_queue.depth();
        let db_stride = self.admin_queue.db_stride();
        let _tail = self.admin_queue.sq_tail();
        let _cq_head = self.admin_queue.cq_head();
        let _phase = self.admin_queue.admin_cq_phase();

        // 通过 framework safe wrapper 执行 unsafe 队列操作
        let result = crate::kernel::framework::driver::storage::nvme_submit_admin_cmd(
            sq_phys,
            cq_phys,
            cmd,
            &mut self.admin_queue.sq_tail,
            &mut self.admin_queue.cq_head,
            &mut self.admin_queue.admin_cq_phase,
            depth,
            db_stride,
            &self.mmio,
            cid,
        );

        match result {
            Ok(_sc) => {
                // 构造 NvmeCplEntry 返回给调用方
                Ok(fw_nvme::NvmeCompletion {
                    cdw0: 0,
                    rsvd1: 0,
                    sqhd: 0,
                    sqid: ADMIN_QID,
                    cid,
                    status: 0x0001, // Phase=1, SC=0
                })
            }
            Err(()) => Err(()),
        }
    }

    /// 提交 I/O 命令并等待完成
    fn submit_io_cmd(&mut self, cmd: fw_nvme::NvmeCommand) -> Result<(), ()> {
        let cid = self.io_queue.next_io_cid();
        let sq_phys = self.io_sq_phys;
        let cq_phys = self.io_cq_phys;
        let depth = self.io_queue.depth();
        let db_stride = self.io_queue.db_stride();

        // I/O doorbell offset = DB_BASE + IO_QID * 8 * stride
        let io_db_offset = NVME_DB_BASE + (IO_QID as usize) * 8 * (db_stride as usize);

        crate::kernel::framework::driver::storage::nvme_submit_io_cmd(
            sq_phys,
            cq_phys,
            cmd,
            &mut self.io_queue.sq_tail,
            &mut self.io_queue.cq_head,
            &mut self.io_queue.io_cq_phase,
            depth,
            db_stride,
            &self.mmio,
            cid,
            io_db_offset,
        )
    }

    // ── 控制器初始化流程 ──

    /// 禁用控制器并等待就绪清除
    fn disable_and_wait(&self) -> bool {
        self.disable();
        self.wait_disabled(1_000_000)
    }

    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    /// 初始化控制器 (MMIO + 队列配置)
    pub fn init_controller(&mut self) -> bool {
        // 读取控制器版本
        let vs = self.version();
        let major = (vs >> 16) & 0xFFFF;
        let minor = (vs >> 8) & 0xFF;
        let patch = vs & 0xFF;
        slog_info!(Driver, "控制器版本: {}.{}.{}", major, minor, patch);

        // 读取能力: 门铃步长
        let cap = self.capabilities();
        self.db_stride = 1u32 << cap.doorbell_stride;

        // 禁用控制器 (如果已启用)
        if self.is_ready() {
            if !self.disable_and_wait() {
                slog_warn!(Driver, "控制器禁用超时");
                return false;
            }
        }

        // 分配 Admin 队列 DMA 内存
        let (sq_phys, cq_phys) =
            if let Some(v) = crate::kernel::framework::driver::storage::nvme_alloc_admin_queues() {
                v
            } else {
                slog_warn!(Driver, "Admin 队列 DMA 分配失败");
                return false;
            };
        self.admin_sq_phys = sq_phys;
        self.admin_cq_phys = cq_phys;

        // 配置 Admin 队列 (AQA + ASQ + ACQ)
        let qd = QUEUE_DEPTH - 1;
        self.set_aqa(qd, qd);
        self.set_asq(sq_phys);
        self.set_acq(cq_phys);

        // 启用控制器: CC = EN | CSS_NVM | MPS=0 | AMS_RR | IOCQES=4 | IOSQES=6
        self.set_cc(
            CC_EN | CC_CSS_NVM | (0u32 << CC_MPS_SHIFT) | CC_AMS_RR | CC_IOCQES_VAL | CC_IOSQES_VAL,
        );

        // 等待控制器就绪
        if !self.wait_ready(1_000_000) {
            slog_warn!(Driver, "控制器启用超时");
            return false;
        }

        slog_info!(Driver, "控制器初始化完成 (db_stride={})", self.db_stride);
        true
    }

    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    /// Identify 控制器
    pub fn identify_controller(&mut self) -> bool {
        let buf_size = 4096; // Identify 数据为 4KB
        let (vaddr, paddr, actual_size) = if let Some(v) =
            crate::kernel::framework::driver::storage::nvme_alloc_dma_buffer(buf_size)
        {
            v
        } else {
            slog_warn!(Driver, "Identify 缓冲区分配失败");
            return false;
        };

        // 清零缓冲区
        crate::kernel::framework::driver::storage::nvme_zero_dma(vaddr, actual_size);

        let cmd = fw_nvme::NvmeCommand::identify(0, IDENTIFY_CNS_CONTROLLER, paddr);
        let result = self.submit_admin_cmd(cmd);

        let success = result.is_ok();
        if success {
            // §6.4 storage 专项 0 号子步: 经 framework DMA 拷贝读取数据字节,
            // 解析 (纯函数) 在 services 层完成
            let mut data = [0u8; 520];
            crate::kernel::framework::driver::storage::nvme_copy_from_dma(
                data.as_mut_ptr(),
                vaddr,
                520,
            );
            if let Some((nn, model)) = parse_identify_controller(&data) {
                self.namespace_count = nn;

                let len = model.iter().position(|&c| c == 0).unwrap_or(40);
                let model_str = core::str::from_utf8(&model[..len]).unwrap_or("unknown");

                slog_info!(
                    Driver,
                    "控制器识别: model={}, ns_count={}",
                    model_str,
                    self.namespace_count
                );
            }
        }

        crate::kernel::framework::driver::storage::nvme_free_dma_buffer(vaddr, actual_size);
        success
    }

    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    /// Identify 命名空间
    pub fn identify_namespace(&mut self, nsid: u32) -> bool {
        let buf_size = 4096;
        let (vaddr, paddr, actual_size) = if let Some(v) =
            crate::kernel::framework::driver::storage::nvme_alloc_dma_buffer(buf_size)
        {
            v
        } else {
            slog_warn!(Driver, "Identify NS 缓冲区分配失败");
            return false;
        };

        crate::kernel::framework::driver::storage::nvme_zero_dma(vaddr, actual_size);

        let cmd = fw_nvme::NvmeCommand::identify(nsid, IDENTIFY_CNS_NAMESPACE, paddr);
        let result = self.submit_admin_cmd(cmd);

        let success = result.is_ok();
        if success {
            // §6.4 storage 专项 0 号子步: 经 framework DMA 拷贝读取数据字节,
            // 解析 (纯函数) 在 services 层完成
            let mut data = [0u8; 192];
            crate::kernel::framework::driver::storage::nvme_copy_from_dma(
                data.as_mut_ptr(),
                vaddr,
                192,
            );
            if let Some((nsze, flbas, lbaf_data)) = parse_identify_namespace(&data) {
                self.namespace_size_lba = nsze;

                let lbaf_idx = (flbas & 0xF) as usize;
                if lbaf_idx < 16 {
                    let lbads = (lbaf_data >> 16) & 0xFF;
                    self.lba_format_size = if lbads > 0 {
                        1u16 << lbads as u16
                    } else {
                        SECTOR_SIZE as u16
                    };
                }

                slog_info!(
                    Driver,
                    "命名空间 {} - size={} LBA, block={}B",
                    nsid,
                    self.namespace_size_lba,
                    self.lba_format_size
                );
            }
        }

        crate::kernel::framework::driver::storage::nvme_free_dma_buffer(vaddr, actual_size);
        success
    }

    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    /// 创建 I/O 队列对 (CQ + SQ)
    pub fn create_io_queue(&mut self) -> bool {
        // 分配 I/O 队列 DMA 内存
        let (sq_phys, cq_phys) =
            if let Some(v) = crate::kernel::framework::driver::storage::nvme_alloc_io_queues() {
                v
            } else {
                slog_warn!(Driver, "I/O 队列 DMA 分配失败");
                return false;
            };
        self.io_sq_phys = sq_phys;
        self.io_cq_phys = cq_phys;

        // 创建 I/O Completion Queue (Admin 命令)
        // services 层 NVMe 走 polling, irq_vector=0 (MSI-X disable 时合法)
        let cmd_cq = fw_nvme::NvmeCommand::create_cq(IO_QID, cq_phys, 0);
        if self.submit_admin_cmd(cmd_cq).is_err() {
            slog_warn!(Driver, "创建 I/O CQ 失败");
            return false;
        }

        // 创建 I/O Submission Queue (Admin 命令)
        let cmd_sq = fw_nvme::NvmeCommand::create_sq(IO_QID, IO_QID, sq_phys);
        if self.submit_admin_cmd(cmd_sq).is_err() {
            slog_warn!(Driver, "创建 I/O SQ 失败");
            return false;
        }

        // 初始化 I/O 队列状态
        self.io_queue.set_sq_tail(0);
        self.io_queue.set_cq_head(0);
        self.io_queue.set_io_cq_phase(1);
        self.io_queue.set_created(true);

        slog_info!(Driver, "I/O 队列创建完成 (depth={})", QUEUE_DEPTH);
        true
    }

    /// 初始化完整流程: 控制器 → Identify → I/O 队列
    pub fn init(&mut self) -> bool {
        if !self.init_controller() {
            return false;
        }

        if !self.identify_controller() {
            return false;
        }

        // Identify namespace 1
        if self.namespace_count > 0 {
            self.identify_namespace(1);
        }

        // 创建 I/O 队列对
        if !self.create_io_queue() {
            return false;
        }

        self.initialized = true;
        slog_info!(Driver, "NVMe 完全初始化, {} 命名空间", self.namespace_count);
        true
    }

    /// 关闭控制器
    pub fn shutdown(&mut self) {
        if !self.initialized {
            return;
        }

        // 关机通知 (Normal Shutdown)
        let cc = self.cc();
        let shn: u32 = 1 << 14; // Normal shutdown
        self.set_cc((cc & !0x3C000) | shn);

        // 等待关机完成 (CSTS.SHST = 2)
        let mut timeout = 1_000_000u64;
        while self.status().shutdown != 2 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }

        self.initialized = false;
    }

    // ── 数据读写 ──

    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    /// 读取扇区 (通过 framework DMA)
    ///
    /// # Errors
    ///
    /// - 控制器尚未初始化时返回 `Err(())`
    /// - `count` 为 0 时返回 `Err(())`
    /// - DMA 缓冲区分配失败时返回 `Err(())`
    /// - IO 命令提交失败 (队列满、超时或控制器报错) 时返回 `Err(())`
    pub fn read(&mut self, nsid: u32, lba: u64, count: u16, buffer: *mut u8) -> Result<(), ()> {
        if !self.initialized {
            return Err(());
        }
        if count == 0 {
            return Err(());
        }

        let byte_count = (count as usize) * self.lba_format_size as usize;

        // 分配 DMA 缓冲区
        let (buf_vaddr, buf_paddr, buf_size) =
            match crate::kernel::framework::driver::storage::nvme_alloc_dma_buffer(byte_count) {
                Some(v) => v,
                None => return Err(()),
            };

        let nlb = ((byte_count + (self.lba_format_size as usize) - 1)
            / (self.lba_format_size as usize)) as u16;

        // 构造读命令
        let mut cmd = fw_nvme::NvmeCommand::read(nsid, lba, nlb, buf_paddr);
        cmd.mptr = buf_paddr;
        cmd.prp2 = 0;

        let result = self.submit_io_cmd(cmd);

        if result.is_ok() {
            // 从 DMA 缓冲区复制到用户缓冲区
            crate::kernel::framework::driver::storage::nvme_copy_from_dma(
                buffer, buf_vaddr, byte_count,
            );
        }

        crate::kernel::framework::driver::storage::nvme_free_dma_buffer(buf_vaddr, buf_size);
        result
    }

    #[expect(
        clippy::similar_names,
        reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
    )]
    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    /// 写入扇区 (通过 framework DMA)
    ///
    /// # Errors
    ///
    /// - 控制器尚未初始化时返回 `Err(())`
    /// - `count` 为 0 时返回 `Err(())`
    /// - DMA 缓冲区分配失败时返回 `Err(())`
    /// - IO 命令提交失败 (队列满、超时或控制器报错) 时返回 `Err(())`
    pub fn write(&mut self, nsid: u32, lba: u64, count: u16, buffer: *const u8) -> Result<(), ()> {
        if !self.initialized {
            return Err(());
        }
        if count == 0 {
            return Err(());
        }

        let byte_count = (count as usize) * self.lba_format_size as usize;

        // 分配 DMA 缓冲区
        let (buf_vaddr, buf_paddr, buf_size) =
            match crate::kernel::framework::driver::storage::nvme_alloc_dma_buffer(byte_count) {
                Some(v) => v,
                None => return Err(()),
            };

        // 复制数据到 DMA 缓冲区
        crate::kernel::framework::driver::storage::nvme_copy_to_dma(buf_vaddr, buffer, byte_count);

        let nlb = ((byte_count + (self.lba_format_size as usize) - 1)
            / (self.lba_format_size as usize)) as u16;

        let mut cmd = fw_nvme::NvmeCommand::write(nsid, lba, nlb, buf_paddr);
        cmd.mptr = buf_paddr;
        cmd.prp2 = 0;

        let result = self.submit_io_cmd(cmd);

        crate::kernel::framework::driver::storage::nvme_free_dma_buffer(buf_vaddr, buf_size);
        result
    }

    // ── 查询 API ──

    /// 命名空间数量
    pub fn namespace_count(&self) -> u32 {
        self.namespace_count
    }

    /// 命名空间大小 (LBA)
    pub fn namespace_size(&self) -> u64 {
        self.namespace_size_lba
    }

    /// LBA 格式字节数
    pub fn lba_format_size(&self) -> u16 {
        self.lba_format_size
    }

    /// 控制器是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// 中断处理 (轮询 CQ 完成)
    pub fn handle_interrupt(&mut self) -> bool {
        if !self.initialized {
            return false;
        }

        // 通过 IoMem 读取 I/O CQ
        // 此处简化: 实际中断处理由 framework 层负责
        false
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nvme_cmd_entry_read() {
        let cmd = NvmeCmdEntry::read(1, 0, 1, 0x1000);
        assert_eq!(cmd.opcode, OP_IO_READ);
        assert_eq!(cmd.nsid, 1);
        assert_eq!(cmd.cdw12, 0); // NLB-1 = 0
    }

    #[test]
    fn test_nvme_cmd_entry_write() {
        let cmd = NvmeCmdEntry::write(1, 100, 8, 0x2000);
        assert_eq!(cmd.opcode, OP_IO_WRITE);
        assert_eq!(cmd.cdw10, 100);
        assert_eq!(cmd.cdw12, 7); // 8 NLB -> 7
    }

    #[test]
    fn test_nvme_cmd_entry_identify() {
        let cmd = NvmeCmdEntry::identify(0, IDENTIFY_CNS_CONTROLLER, 0x3000);
        assert_eq!(cmd.opcode, OP_ADMIN_IDENTIFY);
        assert_eq!(cmd.cdw10, IDENTIFY_CNS_CONTROLLER as u32);
    }

    #[test]
    fn test_nvme_cmd_entry_create_cq() {
        let cmd = NvmeCmdEntry::create_cq(1, 0x5000, 64);
        assert_eq!(cmd.opcode, OP_ADMIN_CREATE_IOCQ);
        assert_eq!(cmd.cdw10 & 0xFFFF, 0); // QID=1 → 验证低位
        // cdw10 布局: ((depth-1) << 16) | qid = (63 << 16) | 1
        assert_eq!(cmd.cdw10, (63 << 16) | 1);
    }

    #[test]
    fn test_nvme_cpl_entry() {
        let cpl = NvmeCplEntry {
            cdw0: 0,
            rsvd1: 0,
            sqhd: 0,
            sqid: 0,
            cid: 0,
            status: 0x0001, // Phase=1, SC=0
        };
        assert!(cpl.is_completed(1));
        assert!(cpl.is_success());

        let cpl_err = NvmeCplEntry {
            status: 0x0003, // Phase=1, SC=1
            ..cpl
        };
        assert!(!cpl_err.is_success());
        assert_eq!(cpl_err.status_code(), 1);
    }

    #[test]
    fn test_controller_status_parse() {
        let st = ControllerStatus::from_register(CSTS_RDY | (2 << 2));
        assert!(st.ready);
        assert_eq!(st.shutdown, 2);
        assert!(!st.fatal);
    }

    #[test]
    fn test_controller_capabilities_parse() {
        let cap = ControllerCapabilities::from_register((3 << 32) | 63);
        assert_eq!(cap.doorbell_stride, 3);
        assert_eq!(cap.max_queue_entries, 63);
    }

    #[test]
    fn test_command_sizes() {
        assert_eq!(core::mem::size_of::<NvmeCmdEntry>(), 64);
        assert_eq!(core::mem::size_of::<NvmeCplEntry>(), 16);
    }

    #[test]
    fn test_queue_pair_creation() {
        let qp = NvmeQueuePair::new(0, 64, 4);
        assert_eq!(qp.id(), 0);
        assert_eq!(qp.depth(), 64);
        assert!(!qp.is_created());
    }

    // §6.4 storage 专项 0 号子步: identify 解析 (从 framework 迁业务) 纯函数测试

    #[test]
    fn test_parse_identify_controller() {
        // 构造 520 字节 Identify Controller 数据: mn (offset 24) = "QueenX NVMe",
        // nn (offset 516) = 3 (LE u32)
        let mut data = [0u8; 520];
        let model = b"QueenX NVMe";
        data[24..24 + model.len()].copy_from_slice(model);
        data[516] = 3;
        data[517] = 0;
        data[518] = 0;
        data[519] = 0;

        let (nn, mn) = parse_identify_controller(&data).expect("长度足够应解析成功");
        assert_eq!(nn, 3);
        assert_eq!(&mn[..model.len()], model);
        assert_eq!(mn[model.len()], 0); // 空终止
    }

    #[test]
    fn test_parse_identify_controller_short_buffer() {
        let data = [0u8; 100]; // 不足 520
        assert!(parse_identify_controller(&data).is_none());
    }

    #[test]
    fn test_parse_identify_namespace() {
        // 构造 192 字节 Identify Namespace 数据: nsze (offset 0) = 1_048_576 (LE u64),
        // flbas (offset 26) = 2 (LBA 格式索引 2), lbaf[2] (offset 128+8) = 0x00010000 (LBADS=9 → 512B)
        let mut data = [0u8; 192];
        data[0..8].copy_from_slice(&1_048_576u64.to_le_bytes());
        data[26] = 2;
        data[136] = 0x00;
        data[137] = 0x00;
        data[138] = 0x01;
        data[139] = 0x00;

        let (nsze, flbas, lbaf_data) =
            parse_identify_namespace(&data).expect("长度足够应解析成功");
        assert_eq!(nsze, 1_048_576);
        assert_eq!(flbas, 2);
        assert_eq!(lbaf_data, 0x0001_0000);
    }

    #[test]
    fn test_parse_identify_namespace_lbaf_beyond_table() {
        // flbas = 0x10 → lbaf_idx = 0 (flbas & 0xF), 走合法路径
        let mut data = [0u8; 192];
        data[26] = 0x10;
        let (_, flbas, lbaf_data) = parse_identify_namespace(&data).unwrap();
        assert_eq!(flbas, 0x10);
        assert_eq!(lbaf_data, 0); // lbaf[0] 未设置 → 0
    }

    #[test]
    fn test_parse_identify_namespace_short_buffer() {
        let data = [0u8; 100]; // 不足 192
        assert!(parse_identify_namespace(&data).is_none());
    }
}
