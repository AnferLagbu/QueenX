#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! overlayfs 文件系统实现


use crate::framework::fs::KernelError;
use crate::services::fs::vfs_types::*;
use crate::framework::sync::IrqSpinLock as Mutex;
use alloc::string::String;
use alloc::vec::Vec;

/// whiteout 文件标记 (文件名以 "." 开头表示已删除)
const WHITEOUT_PREFIX: u8 = b'.';

/// overlayfs 目录项
#[derive(Debug, Clone)]
pub struct OverlayEntry {
    /// 文件名
    pub name: String,
    /// 文件类型 (0=文件, 1=目录)
    pub file_type: u8,
    /// 是否在 upperdir 中
    pub in_upper: bool,
    /// 是否为 whiteout (已删除)
    pub is_whiteout: bool,
    /// 原始 inode 号 (来自 lowerdir)
    pub lower_inode: Option<u32>,
    /// upperdir inode 号 (如果存在)
    pub upper_inode: Option<u32>,
}

/// overlayfs 挂载配置
#[derive(Debug, Clone)]
pub struct OverlayMount {
    /// 上层目录路径 (写入目标)
    pub upperdir: String,
    /// 下层目录路径 (只读源)
    pub lowerdir: String,
    /// 工作层路径 (copy_up 临时存储)
    pub workdir: String,
    /// 合并后的视图路径
    pub merged: String,
}

/// overlayfs 数据结构
pub struct OverlayFsData {
    /// 挂载配置
    pub mount: OverlayMount,
    /// upperdir 的 ramfs 数据
    pub upper_data: crate::framework::fs::ramfs::RamFsData,
    /// workdir 的 ramfs 数据
    pub work_data: crate::framework::fs::ramfs::RamFsData,
    /// lowerdir 路径 (只读引用)
    pub lower_path: String,
}

impl OverlayFsData {
    /// 创建新的 overlayfs 数据结构
    pub fn new(mount: OverlayMount) -> Self {
        Self {
            mount,
            upper_data: crate::framework::fs::ramfs::RamFsData::new(),
            work_data: crate::framework::fs::ramfs::RamFsData::new(),
            lower_path: mount.lowerdir.clone(),
        }
    }

    /// 解析路径，确定文件来自哪个层
    pub fn resolve_layer(&self, path: &str) -> OverlayEntry {
        // 1. 检查 upperdir
        if let Some(node_id) = self.upper_data.resolve_path(path) {
            let node = &self.upper_data.nodes[node_id as usize];
            if node.used {
                // 检查是否为 whiteout
                let is_whiteout = path.starts_with('.');
                return OverlayEntry {
                    name: path.to_string(),
                    file_type: node.file_type,
                    in_upper: true,
                    is_whiteout,
                    lower_inode: None,
                    upper_inode: Some(node_id),
                };
            }
        }

        // 2. 检查 lowerdir (通过 VFS 接口)
        // 注意: lowerdir 是只读的，需要通过 VFS 读取
        OverlayEntry {
            name: path.to_string(),
            file_type: 0, // 默认文件
            in_upper: false,
            is_whiteout: false,
            lower_inode: None,
            upper_inode: None,
        }
    }

    /// copy_up: 将文件从 lowerdir 复制到 upperdir
    pub fn copy_up(&mut self, path: &str) -> Result<u32, KernelError> {
        // 1. 检查 upperdir 是否已存在
        if let Some(node_id) = self.upper_data.resolve_path(path) {
            let node = &self.upper_data.nodes[node_id as usize];
            if node.used {
                return Ok(node_id);
            }
        }

        // 2. 从 lowerdir 读取文件内容
        // 注意: 需要通过 VFS 读取 lowerdir 的文件
        // 这里简化处理，实际需要调用 lowerdir 的 FileSystem trait

        // 3. 在 upperdir 创建新文件
        // 4. 复制文件内容
        // 5. 复制文件属性

        Err(KernelError::NotSupported)
    }

    /// 创建 whiteout 文件 (标记删除)
    pub fn create_whiteout(&mut self, path: &str) -> Result<u32, KernelError> {
        let whiteout_path = format!(".{}", path);
        self.upper_data.create_file(&whiteout_path, 0, 0)
            .ok_or(KernelError::NoSpace)
    }
}

/// overlayfs 文件系统实例 (全局单例)
static OVERLAY_FS: Mutex<Option<OverlayFsData>> = Mutex::new(None);

// ============================================================================
// OverlayFsInode — OverlayFS 文件 Inode 实现
// ============================================================================

use alloc::sync::Arc;
use crate::services::fs::inode::Inode;

/// OverlayFS 文件 Inode — 委托给 upperdir 的 RamFsData
pub struct OverlayFsInode {
    node_id: u32,
    mount_idx: u32,
    file_type: u8,
    rel_path: alloc::string::String,
}

impl OverlayFsInode {
    pub fn new(node_id: u32, mount_idx: u32, file_type: u8, rel_path: &str) -> Self {
        Self { node_id, mount_idx, file_type, rel_path: alloc::string::String::from(rel_path) }
    }
}

impl Inode for OverlayFsInode {
    fn read(&self, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;
        let mut offset_i32 = offset as i32;
        let result = fs.upper_data.read(self.node_id, &mut offset_i32, buf, _pwm);
        if result < 0 { Err(KernelError::Io) } else { Ok(result as usize) }
    }

    fn write(&self, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
        let mut offset_i32 = offset as i32;
        let result = fs.upper_data.write(self.node_id, &mut offset_i32, buf, _pwm);
        if result < 0 { Err(KernelError::Io) } else { Ok(result as usize) }
    }

    fn stat(&self, _pwm: u64) -> KernelResult<VfsStat> {
        let fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;
        match fs.upper_data.get_stat(self.node_id, _pwm) {
            Ok(s) => Ok(s),
            Err(_) => Err(KernelError::FileNotFound),
        }
    }

    fn truncate(&self, size: u64, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;
        let rc = fs.upper_data.truncate(self.node_id, size, 0);
        if rc == 0 { Ok(()) } else { Err(KernelError::Io) }
    }

    fn seek(&self, offset: i64, whence: VfsSeekWhence, current_offset: u64) -> KernelResult<u64> {
        let file_size = {
            let fs_guard = OVERLAY_FS.lock();
            let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;
            fs.upper_data.get_file_size(self.node_id).unwrap_or(0) as u64
        };
        let new_offset = match whence {
            VfsSeekWhence::Set => offset as u64,
            VfsSeekWhence::Cur => current_offset.saturating_add(offset as u64),
            VfsSeekWhence::End => file_size.saturating_add(offset as u64),
        };
        Ok(new_offset)
    }

    fn is_dir(&self) -> bool {
        self.file_type == 1
    }

    fn set_times(&self, _atime: u64, _mtime: u64, _pwm: u64) -> KernelResult<()> {
        // OverlayFS: 委托给下层文件系统
        // TODO: 未来可实现 copy-up + 时间戳更新
        Ok(())
    }

    fn node_id(&self) -> u32 {
        self.node_id
    }

    fn mount_idx(&self) -> u32 {
        self.mount_idx
    }
}

/// overlayfs FileSystem trait 实现
pub struct OverlayFsFileSystem;

impl FileSystem for OverlayFsFileSystem {
    fn name(&self) -> &'static str {
        "overlay"
    }

    fn fs_init(&self) -> KernelResult<()> {
        Ok(())
    }

    fn fs_mount(&self, path: &str) -> KernelResult<()> {
        // 解析挂载选项 (upperdir, lowerdir, workdir)
        // 这里简化处理，实际需要解析 mount 命令的选项
        let mount = OverlayMount {
            upperdir: String::from("/upper"),
            lowerdir: String::from("/lower"),
            workdir: String::from("/work"),
            merged: String::from(path),
        };

        let mut guard = OVERLAY_FS.lock();
        *guard = Some(OverlayFsData::new(mount));
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, _flags: u32, _pwm: u64) -> KernelResult<alloc::sync::Arc<dyn crate::services::fs::inode::Inode>> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let entry = fs.resolve_layer(rel_path);

        if entry.is_whiteout {
            return Err(KernelError::FileNotFound);
        }

        if !entry.in_upper && (_flags & 0x0003 != 0) {
            fs.copy_up(rel_path)?;
        }

        if entry.in_upper {
            let node_id = entry.upper_inode.unwrap_or(0);
            Ok(alloc::sync::Arc::new(OverlayFsInode::new(node_id, 0, entry.file_type, rel_path)))
        } else {
            Err(KernelError::NotSupported)
        }
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        // 从 upperdir 读取
        let result = fs.upper_data.read(handle, &mut (offset as i32), buf, _pwm);
        if result < 0 {
            Err(KernelError::Io)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_write(&self, handle: u32, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 写入 upperdir
        let result = fs.upper_data.write(handle, &mut (offset as i32), buf, _pwm);
        if result < 0 {
            Err(KernelError::Io)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        // 解析路径，获取文件属性
        let entry = fs.resolve_layer(rel_path);

        if entry.is_whiteout {
            return Err(KernelError::FileNotFound);
        }

        // 从 upperdir 或 lowerdir 获取属性
        if entry.in_upper {
            let node_id = entry.upper_inode.unwrap_or(0);
            let node = &fs.upper_data.nodes[node_id as usize];
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
        } else {
            // 从 lowerdir 获取属性 (需要通过 VFS)
            Err(KernelError::NotSupported)
        }
    }

    fn fs_chmod(&self, _rel_path: &str, _mode: u16, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnlyFilesystem)
    }

    fn fs_chown(&self, _rel_path: &str, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnlyFilesystem)
    }

    fn fs_mkdir(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 在 upperdir 创建目录
        fs.upper_data.mkdir(rel_path, _pwm)
            .map(|_| ())
            .map_err(|_| KernelError::AlreadyExists)
    }

    fn fs_unlink(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 检查文件是否在 lowerdir
        let entry = fs.resolve_layer(rel_path);
        if !entry.in_upper {
            // 文件在 lowerdir，需要创建 whiteout
            fs.create_whiteout(rel_path)?;
            return Ok(());
        }

        // 文件在 upperdir，直接删除
        fs.upper_data.unlink(rel_path, _pwm)
            .map(|_| ())
            .map_err(|_| KernelError::FileNotFound)
    }

    fn fs_rmdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_rename(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        // 合并 upperdir 和 lowerdir 的目录项
        // 1. 先遍历 upperdir
        let upper_result = fs.upper_data.readdir(handle, offset, entry);

        if let Ok(true) = upper_result {
            return Ok(true);
        }

        // 2. 再遍历 lowerdir (需要通过 VFS)
        // 这里简化处理
        Ok(false)
    }

    // L4 重构: 扩展方法实现 (override trait 默认实现)
    fn fs_resolve_inode(&self, inode_id: u32, mount_idx: u32) -> Option<alloc::sync::Arc<dyn crate::services::fs::inode::Inode>> {
        Some(alloc::sync::Arc::new(OverlayFsInode::new(inode_id, mount_idx, 0, "")))
    }
}

/// 初始化 overlayfs 文件系统
pub fn init() {
    // overlayfs 需要手动挂载
}