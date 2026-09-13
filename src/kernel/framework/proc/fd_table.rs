#![deny(unsafe_code)]
//! Per-process 文件描述符表 — framework 机制实现
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 原定义（P1-I-01/D8 于 2026-06-16 提取到 `services::proc::fd_table`）按
//! 统一判据"机制持有的数据结构/常量归 framework"迁回 — FdTable 是 Process
//! 结构体字段 (framework 进程机制状态), 被 framework/proc/process.rs 消费。
//! framework/proc 新增 `pub mod fd_table`; process.rs 改引 framework 本地
//! 路径。services 侧改 glob re-export 保持 API 兼容。
//!
//! ## 设计
//!
//! FD 表仅存储指向 OpenFile 的 handle_id, 不存储 offset/flags.
//! dup() 通过共享 OpenFile 实现 offset 共享 (POSIX 合规).
//!
//! ## 与旧实现的差异
//!
//! 旧: entries`[`i`]` = global_fd (i32)
//! 新: entries`[`i`]` = handle_id (u32) → OpenFile (共享 offset)

use crate::kernel::framework::sync::IrqSpinLock;

/// 每进程 FD 表上限
pub const MAX_FDS_PER_PROCESS: usize = 64;

/// Per-process FD 表
///
/// `entries[local_fd] = handle_id` (指向全局 `OpenFile` 表)
/// `u32::MAX` 表示 slot 空闲.
#[derive(Debug)]
pub struct FdTable {
    /// `handle_id` 映射 (指向 `OpenFile`)
    entries: IrqSpinLock<[u32; MAX_FDS_PER_PROCESS]>,
    /// CLOEXEC 标志 (per-FD, 不随 dup 共享)
    cloexec: IrqSpinLock<[bool; MAX_FDS_PER_PROCESS]>,
}

impl FdTable {
    /// 创建未初始化的 `FdTable`
    pub const fn new() -> Self {
        Self {
            entries: IrqSpinLock::new([u32::MAX; MAX_FDS_PER_PROCESS]),
            cloexec: IrqSpinLock::new([false; MAX_FDS_PER_PROCESS]),
        }
    }

    /// 初始化 FD 表 (清空所有 slot)
    pub fn init(&self) {
        let mut entries = self.entries.lock();
        let mut cloexec = self.cloexec.lock();
        for i in 0..MAX_FDS_PER_PROCESS {
            entries[i] = u32::MAX;
            cloexec[i] = false;
        }
    }

    /// 分配 per-process FD slot, 返回本地 fd 编号.
    ///
    /// 策略: first-fit.
    pub fn alloc_fd(&self, handle_id: u32, cloexec: bool) -> Option<usize> {
        let mut entries = self.entries.lock();
        let mut cloexec_lock = self.cloexec.lock();
        for i in 0..MAX_FDS_PER_PROCESS {
            if entries[i] == u32::MAX {
                entries[i] = handle_id;
                cloexec_lock[i] = cloexec;
                return Some(i);
            }
        }
        None
    }

    /// 通过本地 fd 获取 `handle_id`.
    pub fn get_handle_id(&self, local_fd: usize) -> Option<u32> {
        let entries = self.entries.lock();
        if local_fd < MAX_FDS_PER_PROCESS {
            let hid = entries[local_fd];
            if hid == u32::MAX { None } else { Some(hid) }
        } else {
            None
        }
    }

    /// 关闭本地 fd, 返回被关闭的 `handle_id`.
    pub fn close_fd(&self, local_fd: usize) -> Option<u32> {
        if local_fd >= MAX_FDS_PER_PROCESS {
            return None;
        }
        let mut entries = self.entries.lock();
        let mut cloexec = self.cloexec.lock();
        let hid = entries[local_fd];
        if hid == u32::MAX {
            None
        } else {
            entries[local_fd] = u32::MAX;
            cloexec[local_fd] = false;
            Some(hid)
        }
    }

    /// 获取 CLOEXEC 标志
    pub fn is_cloexec(&self, local_fd: usize) -> bool {
        if local_fd >= MAX_FDS_PER_PROCESS {
            return false;
        }
        let cloexec = self.cloexec.lock();
        cloexec[local_fd]
    }

    /// 设置 CLOEXEC 标志
    pub fn set_cloexec(&self, local_fd: usize, cloexec: bool) {
        if local_fd >= MAX_FDS_PER_PROCESS {
            return;
        }
        let mut cloexec_lock = self.cloexec.lock();
        cloexec_lock[local_fd] = cloexec;
    }

    /// 获取所有已分配的 FD 列表
    pub fn get_all_fds(&self) -> alloc::vec::Vec<(usize, u32)> {
        let entries = self.entries.lock();
        entries
            .iter()
            .enumerate()
            .filter(|&(_, &hid)| hid != u32::MAX)
            .map(|(local, &handle)| (local, handle))
            .collect()
    }

    /// 获取所有 CLOEXEC 的 FD (用于 exec 时关闭)
    pub fn get_cloexec_fds(&self) -> alloc::vec::Vec<usize> {
        let entries = self.entries.lock();
        let cloexec = self.cloexec.lock();
        (0..MAX_FDS_PER_PROCESS)
            .filter(|&i| entries[i] != u32::MAX && cloexec[i])
            .collect()
    }
}

impl Default for FdTable {
    fn default() -> Self {
        Self::new()
    }
}
