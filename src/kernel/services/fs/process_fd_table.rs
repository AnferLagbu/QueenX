#![deny(unsafe_code)]
//! Per-process FD 表 — 进程级文件描述符管理 (Plan B)
//!
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! ## 职责
//!
//! - 管理每个进程的文件描述符表
//! - 直接持有 `Arc<OpenFile>` 引用 (替代全局 OPEN_FILE_TABLE 间接引用)
//! - 支持 POSIX dup 语义 (共享 Arc<OpenFile>)
//! - 支持 fork 时 FD 表复制
//! - 支持 exec 时 FD 表清理 (CLOEXEC)
//!
//! ## 设计
//!
//! Plan B: 使用 `Vec<Option<Arc<OpenFile>>>` 替代固定数组.
//! 每个 FD 直接持有 `Arc<OpenFile>`, dup 通过 `Arc::clone` 共享,
//! close 通过 `Arc::drop` 减少引用计数.

use super::vfs_types::OpenFile;
use crate::framework::sync::IrqSpinLock as Mutex;
use crate::framework::syscall::Errno;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// FD 条目 — Plan B: 直接持有 `Arc<OpenFile>`
#[derive(Debug)]
pub struct FdEntry {
    /// 打开文件描述 (共享引用)
    pub open_file: Arc<OpenFile>,
    /// FD 级标志 (CLOEXEC, 不随 dup 共享)
    pub cloexec: bool,
}

/// Per-process FD 表 (Plan B: Vec 动态扩展)
pub struct ProcessFdTable {
    /// FD 条目数组, index = fd 编号
    entries: Mutex<Vec<Option<FdEntry>>>,
    /// 最大 FD 编号上限
    max_fds: usize,
}

impl ProcessFdTable {
    /// 创建新的 FD 表
    pub fn new(max_fds: usize) -> Self {
        let mut entries = Vec::with_capacity(max_fds);
        entries.resize_with(max_fds, || None);
        Self {
            entries: Mutex::new(entries),
            max_fds,
        }
    }

    /// 创建默认大小的 FD 表 (256 个 FD)
    pub fn new_default() -> Self {
        Self::new(256)
    }

    /// 分配一个新的 FD ( lowest-available 策略)
    pub fn alloc_fd(&self, open_file: Arc<OpenFile>, cloexec: bool) -> Option<u32> {
        let mut entries = self.entries.lock();
        // 从 3 开始搜索 (0/1/2 保留给 stdin/stdout/stderr)
        for fd in 3..entries.len() {
            if entries[fd].is_none() {
                entries[fd] = Some(FdEntry { open_file, cloexec });
                return Some(fd as u32);
            }
        }
        None
    }

    /// 分配指定编号的 FD (dup2 用)
    ///
    /// # Errors
    /// 当 `fd` 超出 FD 表范围时返回 `EBADF`.
    pub fn alloc_fd_at(
        &self,
        fd: u32,
        open_file: Arc<OpenFile>,
        cloexec: bool,
    ) -> Result<u32, Errno> {
        let fd_usize = fd as usize;
        let mut entries = self.entries.lock();
        if fd_usize >= entries.len() {
            return Err(Errno::EBADF);
        }
        // 如果该 FD 已打开, 先关闭
        if let Some(old) = entries[fd_usize].take() {
            drop(old); // Arc 引用计数自动减少
        }
        entries[fd_usize] = Some(FdEntry { open_file, cloexec });
        Ok(fd)
    }

    /// 获取 FD 的 `OpenFile` 引用
    pub fn get_fd(&self, fd: u32) -> Option<Arc<OpenFile>> {
        let entries = self.entries.lock();
        let fd_usize = fd as usize;
        if fd_usize < entries.len() {
            entries[fd_usize].as_ref().map(|e| e.open_file.clone())
        } else {
            None
        }
    }

    /// 获取 FD 的 CLOEXEC 标志
    pub fn get_cloexec(&self, fd: u32) -> bool {
        let entries = self.entries.lock();
        let fd_usize = fd as usize;
        if fd_usize < entries.len() {
            entries[fd_usize].as_ref().is_some_and(|e| e.cloexec)
        } else {
            false
        }
    }

    /// 关闭 FD, 返回被关闭的 `OpenFile` 引用
    pub fn close_fd(&self, fd: u32) -> Option<Arc<OpenFile>> {
        let mut entries = self.entries.lock();
        let fd_usize = fd as usize;
        if fd_usize < entries.len() {
            entries[fd_usize].take().map(|e| e.open_file)
        } else {
            None
        }
    }

    /// 复制 FD (dup 语义) — 共享同一个 `Arc<OpenFile>`
    pub fn dup_fd(&self, old_fd: u32) -> Option<u32> {
        let open_file = {
            let entries = self.entries.lock();
            let old_usize = old_fd as usize;
            if old_usize >= entries.len() {
                return None;
            }
            entries[old_usize].as_ref()?.open_file.clone()
        };
        let cloexec = self.get_cloexec(old_fd);
        self.alloc_fd(open_file, cloexec)
    }

    /// 复制 FD 到指定编号 (dup2 语义)
    ///
    /// # Errors
    /// 当 `old_fd` 越界、未打开或 `old_fd == new_fd` 且无效时返回 `EBADF`;
    /// 当 `new_fd` 超出范围时也返回 `EBADF`.
    pub fn dup2_fd(&self, old_fd: u32, new_fd: u32) -> Result<u32, Errno> {
        if old_fd == new_fd {
            // dup2(fd, fd) 是 no-op, 但检查 fd 是否有效
            let entries = self.entries.lock();
            if (new_fd as usize) < entries.len() && entries[new_fd as usize].is_some() {
                return Ok(new_fd);
            }
            return Err(Errno::EBADF);
        }

        let open_file = {
            let entries = self.entries.lock();
            let old_usize = old_fd as usize;
            if old_usize >= entries.len() {
                return Err(Errno::EBADF);
            }
            entries[old_usize]
                .as_ref()
                .ok_or(Errno::EBADF)?
                .open_file
                .clone()
        };
        let cloexec = self.get_cloexec(old_fd);
        self.alloc_fd_at(new_fd, open_file, cloexec)
    }

    /// 关闭所有 CLOEXEC FD
    pub fn close_cloexec_fds(&self) {
        let mut entries = self.entries.lock();
        for entry in entries.iter_mut() {
            if let Some(e) = entry {
                if e.cloexec {
                    *entry = None; // Arc drop
                }
            }
        }
    }

    /// 清空 FD 表 (exec 时调用, 保留 CLOEXEC FD)
    pub fn clear_non_cloexec(&self) {
        let mut entries = self.entries.lock();
        for entry in entries.iter_mut() {
            if let Some(e) = entry {
                if !e.cloexec {
                    *entry = None; // Arc drop
                }
            }
        }
    }

    /// 获取已使用的 FD 数量
    pub fn used_count(&self) -> u32 {
        let entries = self.entries.lock();
        entries.iter().filter(|e| e.is_some()).count() as u32
    }

    /// 获取最大 FD 数量
    pub fn max_fds(&self) -> usize {
        self.max_fds
    }
}
