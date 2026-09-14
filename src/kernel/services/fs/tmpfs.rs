#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! tmpfs 基于内存的文件系统


use alloc::sync::Arc;
use crate::framework::fs::KernelError;
use crate::framework::fs::ramfs::RamFsData;
use crate::services::fs::vfs_types::*;
use crate::services::fs::inode::Inode;
use crate::framework::sync::IrqSpinLock as Mutex;

// ============================================================================
// TmpFs Inode — 临时文件 Inode 实现
// ============================================================================

/// 临时文件 Inode — TmpFS 的 Inode 实现
pub struct TmpFsInode {
    node_id: u32,
    mount_idx: u32,
}

impl TmpFsInode {
    pub fn new(node_id: u32, mount_idx: u32) -> Self {
        Self { node_id, mount_idx }
    }
}

impl Inode for TmpFsInode {
    fn read(&self, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
        let mut offset_i32 = offset as i32;
        let result = fs.inner.read(self.node_id, &mut offset_i32, buf, 0);
        if result < 0 { Err(KernelError::Io) } else { Ok(result as usize) }
    }

    fn write(&self, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
        let mut offset_i32 = offset as i32;
        let result = fs.inner.write(self.node_id, &mut offset_i32, buf, 0);
        if result < 0 { Err(KernelError::Io) } else { Ok(result as usize) }
    }

    fn stat(&self, pwm: u64) -> KernelResult<VfsStat> {
        let fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;
        match fs.inner.get_stat(self.node_id, pwm) {
            Ok(s) => Ok(s),
            Err(_) => Err(KernelError::FileNotFound),
        }
    }

    fn truncate(&self, size: u64, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
        let rc = fs.inner.truncate(self.node_id, size, 0);
        if rc == 0 { Ok(()) } else { Err(KernelError::Io) }
    }

    fn seek(&self, offset: i64, whence: VfsSeekWhence, current_offset: u64) -> KernelResult<u64> {
        let file_size = {
            let fs_guard = TMPFS_DATA.lock();
            let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;
            fs.inner.get_file_size(self.node_id).unwrap_or(0) as u64
        };
        let new_offset = match whence {
            VfsSeekWhence::Set => offset as u64,
            VfsSeekWhence::Cur => current_offset.saturating_add(offset as u64),
            VfsSeekWhence::End => file_size.saturating_add(offset as u64),
        };
        Ok(new_offset)
    }

    fn is_dir(&self) -> bool {
        let fs_guard = TMPFS_DATA.lock();
        if let Some(fs) = fs_guard.as_ref() {
            if (self.node_id as usize) < 256 {
                fs.inner.nodes[self.node_id as usize].file_type == 1
            } else {
                false
            }
        } else {
            false
        }
    }

    fn set_times(&self, _atime: u64, _mtime: u64, _pwm: u64) -> KernelResult<()> {
        // TmpFS: 内存文件系统, 无持久时间戳
        Ok(())
    }

    fn node_id(&self) -> u32 {
        self.node_id
    }

    fn mount_idx(&self) -> u32 {
        self.mount_idx
    }
}

/// tmpfs 默认最大大小 (64MB)
const TMPFS_DEFAULT_MAX_SIZE: u64 = 64 * 1024 * 1024;

/// tmpfs 数据结构
pub struct TmpFsData {
    /// 内部 ramfs 数据
    pub inner: RamFsData,
    /// 最大容量限制 (字节)
    max_size: u64,
    /// 当前已用空间 (字节)
    used_size: u64,
}

impl TmpFsData {
    /// 创建新的 tmpfs 数据结构
    pub fn new(max_size: u64) -> Self {
        Self {
            inner: RamFsData::new(),
            max_size,
            used_size: 0,
        }
    }

    /// 创建默认 tmpfs (64MB 限制)
    pub fn new_default() -> Self {
        Self::new(TMPFS_DEFAULT_MAX_SIZE)
    }

    /// 获取最大容量
    pub fn max_size(&self) -> u64 {
        self.max_size
    }

    /// 获取已用空间
    pub fn used_size(&self) -> u64 {
        self.used_size
    }

    /// 获取可用空间
    pub fn free_size(&self) -> u64 {
        self.max_size.saturating_sub(self.used_size)
    }

    /// 检查是否有足够空间
    pub fn has_space(&self, size: u64) -> bool {
        self.used_size + size <= self.max_size
    }

    /// 增加已用空间
    pub fn add_used(&mut self, size: u64) {
        self.used_size = self.used_size.saturating_add(size);
    }

    /// 减少已用空间
    pub fn sub_used(&mut self, size: u64) {
        self.used_size = self.used_size.saturating_sub(size);
    }
}

/// tmpfs 文件系统实例 (全局单例)
static TMPFS_DATA: Mutex<Option<TmpFsData>> = Mutex::new(None);

/// tmpfs FileSystem trait 实现
pub struct TmpFsFileSystem;

impl FileSystem for TmpFsFileSystem {
    fn name(&self) -> &'static str {
        "tmpfs"
    }

    fn fs_init(&self) -> KernelResult<()> {
        Ok(())
    }

    fn fs_mount(&self, _path: &str) -> KernelResult<()> {
        let mut guard = TMPFS_DATA.lock();
        *guard = Some(TmpFsData::new_default());
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, _flags: u32, _pwm: u64) -> KernelResult<Arc<dyn Inode>> {
        let mut fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let result = fs.inner.open(rel_path, 0, _pwm)
            .ok_or(KernelError::FileNotFound)?;

        Ok(Arc::new(TmpFsInode::new(result.0, 0)))
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let result = fs.inner.read(handle, &mut (offset as i32), buf, _pwm);
        if result < 0 {
            Err(KernelError::Io)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_write(&self, handle: u32, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 检查空间限制
        if !fs.has_space(buf.len() as u64) {
            return Err(KernelError::NoSpace);
        }

        let result = fs.inner.write(handle, &mut (offset as i32), buf, _pwm);
        if result < 0 {
            Err(KernelError::Io)
        } else {
            fs.add_used(result as u64);
            Ok(result as usize)
        }
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        let node_id = fs.inner.resolve_path(rel_path)
            .ok_or(KernelError::FileNotFound)?;

        let node = &fs.inner.nodes[node_id as usize];
        if !node.used {
            return Err(KernelError::FileNotFound);
        }

        Ok(VfsStat {
            node_id,
            mode: node.perm,
            uid: 0,
            gid: 0,
            size: node.size,
            atime: node.atime,
            mtime: node.mtime,
            ctime: node.ctime,
            owner_pwm: node.owner_pwm,
            group_pwm: node.group_pwm,
            perm: node.perm,
            file_type: node.file_type,
            sensitivity: 0,
        })
    }

    fn fs_chmod(&self, _rel_path: &str, _mode: u16, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnlyFilesystem)
    }

    fn fs_chown(&self, _rel_path: &str, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnlyFilesystem)
    }

    fn fs_mkdir(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let result = fs.inner.mkdir(rel_path, _pwm);
        if result < 0 {
            Err(KernelError::AlreadyExists)
        } else {
            Ok(())
        }
    }

    fn fs_unlink(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let result = fs.inner.unlink(rel_path, _pwm);
        if result < 0 {
            Err(KernelError::FileNotFound)
        } else {
            Ok(())
        }
    }

    fn fs_rmdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_rename(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let fs_guard = TMPFS_DATA.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        let node = &fs.inner.nodes[handle as usize];
        if !node.used || node.file_type != VfsFileType::Dir as u8 {
            return Err(KernelError::NotADirectory);
        }

        let block_num = node.direct_blocks[0];
        if block_num == 0 {
            return Ok(false);
        }

        let dirent_size = core::mem::size_of::<crate::framework::fs::ramfs::RamFsDirEntry>();
        let num_entries = node.size as usize / dirent_size;
        let idx = offset as usize;

        if idx >= num_entries {
            return Ok(false);
        }

        let block_offset = block_num as usize * crate::framework::mm::PAGE_SIZE as usize + idx * dirent_size;

        // 从 data_area 读取目录项
        let entry_data = &fs.inner.data_area[block_offset..block_offset + dirent_size];
        let ramfs_entry = crate::framework::fs::ramfs::RamFsDirEntry::read_at(entry_data, 0);

        entry.node = ramfs_entry.node;
        entry.file_type = ramfs_entry.file_type;
        entry.set_name(core::str::from_utf8(&ramfs_entry.name.iter().take_while(|&&b| b != 0).collect::<alloc::vec::Vec<_>>()).unwrap_or(""));

        Ok(true)
    }

    // L4 重构: 扩展方法实现 (override trait 默认实现)
    fn fs_resolve_inode(&self, inode_id: u32, mount_idx: u32) -> Option<Arc<dyn Inode>> {
        Some(Arc::new(TmpFsInode::new(inode_id, mount_idx)))
    }
}

/// 初始化 tmpfs 文件系统
pub fn init() {
    // tmpfs 需要手动挂载
}