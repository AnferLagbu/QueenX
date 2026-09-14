#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 `FileSystem` trait 实现

use super::read::Ext2Fs;
use crate::framework::fs::KernelError;
use crate::framework::sync::IrqSpinLock as Mutex;
use crate::services::fs::vfs_types::{
    FileSystem, KernelResult, VfsDirEntry, VfsSeekWhence, VfsStat,
};

/// ext2 文件系统实例 (全局单例)
static EXT2_FS: Mutex<Option<Ext2Fs>> = Mutex::new(None);

// ============================================================================
// Ext2 Inode — ext2 文件 Inode 实现
// ============================================================================

use crate::services::fs::inode::Inode;

/// ext2 文件 Inode — 直接持有 inode 编号
pub struct Ext2Inode {
    inode_num: u32,
    mount_idx: u32,
}

impl Ext2Inode {
    pub fn new(inode_num: u32, mount_idx: u32) -> Self {
        Self {
            inode_num,
            mount_idx,
        }
    }
}

impl Inode for Ext2Inode {
    fn read(&self, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
        fs.read_file(self.inode_num, offset, buf)
    }

    fn write(&self, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
        fs.write_file(self.inode_num, offset, buf)
    }

    fn stat(&self, _pwm: u64) -> KernelResult<VfsStat> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
        let inode = fs.read_inode(self.inode_num)?;
        Ok(VfsStat {
            node_id: self.inode_num,
            mode: inode.perm(),
            uid: u32::from(inode.i_uid),
            gid: u32::from(inode.i_gid),
            size: inode.i_size,
            atime: u64::from(inode.i_atime),
            mtime: u64::from(inode.i_mtime),
            ctime: u64::from(inode.i_ctime),
            owner_pwm: 0,
            group_pwm: 0,
            perm: inode.perm(),
            file_type: inode.file_type(),
            sensitivity: 0,
        })
    }

    fn truncate(&self, _size: u64, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnlyFilesystem)
    }

    fn seek(&self, offset: i64, whence: VfsSeekWhence, current_offset: u64) -> KernelResult<u64> {
        let file_size = {
            let mut fs_guard = EXT2_FS.lock();
            let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
            let inode = fs.read_inode(self.inode_num)?;
            u64::from(inode.i_size)
        };
        let new_offset = match whence {
            VfsSeekWhence::Set => offset as u64,
            VfsSeekWhence::Cur => current_offset.saturating_add(offset as u64),
            VfsSeekWhence::End => file_size.saturating_add(offset as u64),
        };
        Ok(new_offset)
    }

    fn is_dir(&self) -> bool {
        if let Some(mut fs_guard) = EXT2_FS.try_lock() {
            if let Some(fs) = fs_guard.as_mut() {
                if let Ok(inode) = fs.read_inode(self.inode_num) {
                    return inode.file_type() == 1; // DIR
                }
            }
        }
        false
    }

    fn set_times(&self, _atime: u64, _mtime: u64, _pwm: u64) -> KernelResult<()> {
        // ext2: 时间戳更新需修改磁盘 inode
        // TODO: 未来可接入 ext2 inode 时间戳更新
        Ok(())
    }

    fn node_id(&self) -> u32 {
        self.inode_num
    }

    fn mount_idx(&self) -> u32 {
        self.mount_idx
    }
}

/// ext2 `FileSystem` trait 实现
pub struct Ext2FileSystem;

impl FileSystem for Ext2FileSystem {
    fn name(&self) -> &'static str {
        "ext2"
    }

    fn fs_init(&self) -> KernelResult<()> {
        // ext2 不需要特殊初始化
        Ok(())
    }

    fn fs_mount(&self, _path: &str) -> KernelResult<()> {
        // ext2 挂载需要指定设备
        // 当前实现: 假设设备 0
        let fs = Ext2Fs::open(0).map_err(|_| KernelError::Io)?;
        let mut guard = EXT2_FS.lock();
        *guard = Some(fs);
        Ok(())
    }

    fn fs_open(
        &self,
        rel_path: &str,
        _flags: u32,
        _pwm: u64,
    ) -> KernelResult<alloc::sync::Arc<dyn crate::services::fs::inode::Inode>> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let inode_num = fs.lookup_path(rel_path)?;

        Ok(alloc::sync::Arc::new(Ext2Inode::new(inode_num, 0)))
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        fs.read_file(handle, offset, buf)
    }

    fn fs_write(&self, handle: u32, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        fs.write_file(handle, offset, buf)
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let inode_num = fs.lookup_path(rel_path)?;
        let inode = fs.read_inode(inode_num)?;

        Ok(VfsStat {
            node_id: inode_num,
            mode: inode.perm(),
            uid: u32::from(inode.i_uid),
            gid: u32::from(inode.i_gid),
            size: inode.i_size,
            atime: u64::from(inode.i_atime),
            mtime: u64::from(inode.i_mtime),
            ctime: u64::from(inode.i_ctime),
            owner_pwm: 0,
            group_pwm: 0,
            perm: inode.perm(),
            file_type: inode.file_type(),
            sensitivity: 0,
        })
    }

    fn fs_chmod(&self, _rel_path: &str, _mode: u16, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnlyFilesystem)
    }

    fn fs_chown(
        &self,
        _rel_path: &str,
        _owner_pwm: u64,
        _group_pwm: u64,
        _pwm: u64,
    ) -> KernelResult<()> {
        Err(KernelError::ReadOnlyFilesystem)
    }

    fn fs_mkdir(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 解析路径
        let (parent_path, name) = rel_path.rfind('/').map_or(("/", rel_path), |pos| {
            if pos == 0 {
                ("/", &rel_path[1..])
            } else {
                (&rel_path[..pos], &rel_path[pos + 1..])
            }
        });

        fs.mkdir(parent_path, name)?;
        Ok(())
    }

    fn fs_unlink(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 解析路径
        let (parent_path, name) = rel_path.rfind('/').map_or(("/", rel_path), |pos| {
            if pos == 0 {
                ("/", &rel_path[1..])
            } else {
                (&rel_path[..pos], &rel_path[pos + 1..])
            }
        });

        // 获取父目录 inode
        let parent_inode_num = fs.lookup_path(parent_path)?;
        let inode_num = fs.remove_dir_entry(parent_inode_num, name)?;

        // 释放 inode 和块
        let inode = fs.read_inode(inode_num)?;
        for i in 0..12 {
            if inode.i_block[i] != 0 {
                fs.deallocate_block(inode.i_block[i])?;
            }
        }

        let group_idx =
            (inode_num - fs.super_block.first_inode()) / fs.super_block.s_inodes_per_group;
        if (group_idx as usize) < fs.block_groups.len() {
            super::alloc::free_inode(
                fs.device_idx,
                &fs.super_block,
                &fs.block_groups[group_idx as usize],
                inode_num,
            )?;
        }

        Ok(())
    }

    fn fs_rmdir(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 解析路径
        let (parent_path, name) = rel_path.rfind('/').map_or(("/", rel_path), |pos| {
            if pos == 0 {
                ("/", &rel_path[1..])
            } else {
                (&rel_path[..pos], &rel_path[pos + 1..])
            }
        });

        fs.rmdir(parent_path, name)?;
        Ok(())
    }

    fn fs_rename(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let entries = fs.read_dir(handle)?;
        let idx = offset as usize;

        if idx >= entries.len() {
            return Ok(false);
        }

        let ext2_entry = &entries[idx];
        entry.node = ext2_entry.inode;
        entry.file_type = ext2_entry.file_type;
        entry.set_name(ext2_entry.get_name());

        Ok(true)
    }

    // L4 重构: 扩展方法实现 (override trait 默认实现)
    fn fs_symlink(&self, target: &str, link_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        fs.symlink(target, link_path)?;
        Ok(())
    }

    fn fs_readlink(&self, rel_path: &str, buf: &mut [u8]) -> KernelResult<usize> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let inode_num = fs.lookup_path(rel_path)?;
        let target = fs.readlink(inode_num)?;

        let copy_len = target.len().min(buf.len());
        buf[..copy_len].copy_from_slice(&target[..copy_len]);

        Ok(copy_len)
    }

    fn fs_link(&self, old_path: &str, new_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        fs.link(old_path, new_path)?;
        Ok(())
    }

    fn fs_resolve_inode(
        &self,
        inode_id: u32,
        mount_idx: u32,
    ) -> Option<alloc::sync::Arc<dyn crate::services::fs::inode::Inode>> {
        Some(alloc::sync::Arc::new(Ext2Inode::new(inode_id, mount_idx)))
    }
}

/// 初始化 ext2 文件系统
pub fn init() {
    // ext2 需要手动挂载, 不自动初始化
}
