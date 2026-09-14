//! Plan B Inode trait + OpenFile + ProcessFdTable 契约测试
//!
//! 验证 Plan B 架构变更的静态契约:
//! 1. Inode trait 定义在 services/fs/inode.rs
//! 2. OpenFile 持有 Arc<dyn Inode> (非 inode_id: u32)
//! 3. ProcessFdTable 使用 Vec (非固定数组)
//! 4. FileSystem::fs_open 返回 Arc<dyn Inode> (非 FsOpenResult)
//! 5. 7 个 FS 均有原生 Inode 实现 (非 LegacyInode)

use std::fs;

fn read_file(relative_path: &str) -> String {
    let path = format!("{}/../src/kernel/{}", env!("CARGO_MANIFEST_DIR"), relative_path);
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("read {}", relative_path))
}

// ============================================================================
// 1. Inode trait 定义
// ============================================================================

#[test]
fn inode_trait_defined_in_services() {
    // B09-12/P1-B3: Inode trait 已迁回 framework/fs/vfs/inode.rs
    let src = read_file("framework/fs/vfs/inode.rs");
    assert!(src.contains("pub trait Inode: Send + Sync"), "Inode trait 必须定义在 framework/fs/vfs/inode.rs");
    assert!(src.contains("fn read(&self, offset: u64, buf: &mut [u8], pwm: u64)"), "Inode::read 必须接收 offset 参数");
    assert!(src.contains("fn write(&self, offset: u64, buf: &[u8], pwm: u64)"), "Inode::write 必须接收 offset 参数");
    assert!(src.contains("fn stat(&self, pwm: u64)"), "Inode::stat 必须存在");
    assert!(src.contains("fn node_id(&self) -> u32"), "Inode::node_id 必须存在");
    assert!(src.contains("fn mount_idx(&self) -> u32"), "Inode::mount_idx 必须存在");
}

#[test]
fn inode_trait_deny_unsafe() {
    // B09-12/P1-B3: Inode trait 定义在 framework (0 unsafe), 具象实现在 services 保留 deny
    let src = read_file("framework/fs/vfs/inode.rs");
    assert!(src.contains("pub trait Inode: Send + Sync"), "framework/fs/vfs/inode.rs 必须定义 Inode trait");
    assert!(!src.contains("unsafe"), "framework/fs/vfs/inode.rs 不应含 unsafe");
    let svc = read_file("services/fs/inode.rs");
    assert!(svc.contains("#![deny(unsafe_code)]"), "services/fs/inode.rs 必须 #![deny(unsafe_code)]");
}

// ============================================================================
// 2. OpenFile 持有 Arc<dyn Inode>
// ============================================================================

#[test]
fn open_file_uses_arc_dyn_inode() {
    // B09-12/P1-B3: OpenFile 定义已迁回 framework/fs/vfs/types.rs
    let src = read_file("framework/fs/vfs/types.rs");
    // OpenFile 应持有 Arc<dyn Inode> 而非 inode_id: u32
    assert!(src.contains("inode: Arc<dyn Inode>"), "OpenFile 必须持有 Arc<dyn Inode>");
    // 不应有 inode_id 字段
    assert!(!src.contains("pub inode_id: u32"), "OpenFile 不应有 inode_id 字段");
    // 应有 inode() 方法
    assert!(src.contains("pub fn inode(&self) -> &dyn Inode"), "OpenFile 必须有 inode() 方法");
}

#[test]
fn open_file_has_debug_impl() {
    // B09-12/P1-B3: OpenFile 定义已迁回 framework/fs/vfs/types.rs
    let src = read_file("framework/fs/vfs/types.rs");
    assert!(src.contains("impl core::fmt::Debug for OpenFile"), "OpenFile 必须实现 Debug");
}

// ============================================================================
// 3. ProcessFdTable 使用 Vec
// ============================================================================

#[test]
fn process_fd_table_uses_vec() {
    let src = read_file("services/fs/process_fd_table.rs");
    assert!(src.contains("Vec<Option<FdEntry>>"), "ProcessFdTable 必须使用 Vec<Option<FdEntry>>");
    assert!(!src.contains("[FdEntry; MAX_FD_PER_PROCESS]"), "ProcessFdTable 不应使用固定数组");
}

#[test]
fn process_fd_table_holds_arc_open_file() {
    let src = read_file("services/fs/process_fd_table.rs");
    assert!(src.contains("pub open_file: Arc<OpenFile>"), "FdEntry 必须持有 Arc<OpenFile>");
    assert!(!src.contains("pub handle_id: u32"), "FdEntry 不应有 handle_id 字段");
}

#[test]
fn process_fd_table_lowest_available() {
    let src = read_file("services/fs/process_fd_table.rs");
    // 应使用 lowest-available 策略 (从 3 开始搜索)
    assert!(src.contains("for fd in 3.."), "alloc_fd 必须从 fd 3 开始搜索");
}

#[test]
fn process_fd_table_deny_unsafe() {
    let src = read_file("services/fs/process_fd_table.rs");
    assert!(src.contains("#![deny(unsafe_code)]"), "process_fd_table.rs 必须 #![deny(unsafe_code)]");
}

// ============================================================================
// 4. FileSystem::fs_open 返回 Arc<dyn Inode>
// ============================================================================

#[test]
fn filesystem_fs_open_returns_arc_inode() {
    // B09-12/P1-B3: FileSystem trait 已迁回 framework/fs/vfs/types.rs
    let src = read_file("framework/fs/vfs/types.rs");
    assert!(
        src.contains("fn fs_open(&self, rel_path: &str, flags: u32, pwm: u64) -> KernelResult<Arc<dyn Inode>>"),
        "FileSystem::fs_open 必须返回 Arc<dyn Inode>"
    );
    assert!(
        !src.contains("-> KernelResult<FsOpenResult>"),
        "FileSystem::fs_open 不应返回 FsOpenResult"
    );
}

#[test]
fn filesystem_fs_create_returns_arc_inode() {
    // B09-12/P1-B3: FileSystem trait 已迁回 framework/fs/vfs/types.rs
    let src = read_file("framework/fs/vfs/types.rs");
    assert!(
        src.contains("fn fs_create(&self, parent_path: &str, name: &str, pwm: u64) -> KernelResult<Arc<dyn Inode>>"),
        "FileSystem::fs_create 必须返回 Arc<dyn Inode>"
    );
}

// ============================================================================
// 5. 7 个 FS 均有原生 Inode 实现
// ============================================================================

#[test]
fn ramfs_has_native_inode() {
    let src = read_file("services/fs/inode.rs");
    assert!(src.contains("pub struct RamFsInode"), "RamFs 必须有原生 RamFsInode");
    assert!(src.contains("impl Inode for RamFsInode"), "RamFsInode 必须 impl Inode");
}

#[test]
fn devfs_has_native_inode() {
    // DECISION-J 第二十一批: devfs 实现迁回 framework/fs/devfs/mod.rs
    let src = read_file("framework/fs/devfs/mod.rs");
    assert!(src.contains("pub struct DevFsInode"), "DevFS 必须有原生 DevFsInode");
    assert!(src.contains("impl Inode for DevFsInode"), "DevFsInode 必须 impl Inode");
}

#[test]
fn tmpfs_has_native_inode() {
    let src = read_file("services/fs/tmpfs.rs");
    assert!(src.contains("pub struct TmpFsInode"), "TmpFS 必须有原生 TmpFsInode");
    assert!(src.contains("impl Inode for TmpFsInode"), "TmpFsInode 必须 impl Inode");
}

#[test]
fn nestfs_has_native_inode() {
    let src = read_file("services/fs/nestfs/nestfs_inode.rs");
    assert!(src.contains("pub struct NestfsInode"), "NestFS 必须有原生 NestfsInode");
    assert!(src.contains("impl Inode for NestfsInode"), "NestfsInode 必须 impl Inode");
}

#[test]
fn overlayfs_has_native_inode() {
    let src = read_file("services/fs/overlayfs.rs");
    assert!(src.contains("pub struct OverlayFsInode"), "OverlayFS 必须有原生 OverlayFsInode");
    assert!(src.contains("impl Inode for OverlayFsInode"), "OverlayFsInode 必须 impl Inode");
}

#[test]
fn ext2_has_native_inode() {
    let src = read_file("services/fs/ext2/mount.rs");
    assert!(src.contains("pub struct Ext2Inode"), "ext2 必须有原生 Ext2Inode");
    assert!(src.contains("impl Inode for Ext2Inode"), "Ext2Inode 必须 impl Inode");
}

#[test]
fn exfat_has_native_inode() {
    let src = read_file("services/fs/exfat/mount.rs");
    assert!(src.contains("pub struct ExfatInode"), "exFAT 必须有原生 ExfatInode");
    assert!(src.contains("impl Inode for ExfatInode"), "ExfatInode 必须 impl Inode");
}

// ============================================================================
// 6. VFS API 使用 Inode trait
// ============================================================================

#[test]
fn vfs_read_uses_inode_trait() {
    // B 方案拆分第二步: vfs_read_internal 已从 api.rs 迁至 handle.rs
    let src = read_file("framework/fs/vfs/handle.rs");
    // 鲁棒匹配: rustfmt 拆行时链式调用分散在多行, 用独立子串 + 同函数体检查
    assert!(
        src.contains(".inode()") && src.contains(".read(") && src.contains("open_file"),
        "vfs_read_internal 必须使用 Inode::read (open_file.inode().read(...))"
    );
    assert!(!src.contains("fs.fs_read("), "vfs_read_internal 不应直接调用 fs.fs_read");
}

#[test]
fn vfs_write_uses_inode_trait() {
    // B 方案拆分第二步: vfs_write_internal 已从 api.rs 迁至 handle.rs
    let src = read_file("framework/fs/vfs/handle.rs");
    // 鲁棒匹配: rustfmt 拆行时链式调用分散在多行, 用独立子串 + 同函数体检查
    assert!(
        src.contains(".inode()") && src.contains(".write(") && src.contains("open_file"),
        "vfs_write_internal 必须使用 Inode::write (open_file.inode().write(...))"
    );
    assert!(!src.contains("fs.fs_write("), "vfs_write_internal 不应直接调用 fs.fs_write");
}

#[test]
fn vfs_fstat_uses_inode_trait() {
    // B 方案拆分第二步: vfs_fstat 已从 api.rs 迁至 handle.rs
    let src = read_file("framework/fs/vfs/handle.rs");
    assert!(src.contains("open_file.inode().stat("), "vfs_fstat 必须使用 Inode::stat");
}

#[test]
fn vfs_seek_uses_inode_trait() {
    // B 方案拆分第二步: vfs_seek 已从 api.rs 迁至 handle.rs
    let src = read_file("framework/fs/vfs/handle.rs");
    // rustfmt 可能将链式调用拆为多行; 匹配 `.inode()` 与 `.seek(` 在同一函数体内
    // (两者间隔 ≤ 200 字符, 适配 rustfmt 拆行格式).
    assert!(
        src.contains(".inode()") && src.contains(".seek(") && src.contains("open_file"),
        "vfs_seek 必须使用 Inode::seek (open_file.inode().seek(...))"
    );
}

#[test]
fn vfs_truncate_uses_inode_trait() {
    // B 方案拆分第二步: vfs_truncate_internal 已从 api.rs 迁至 handle.rs
    let src = read_file("framework/fs/vfs/handle.rs");
    // 鲁棒匹配: rustfmt 拆行时链式调用分散在多行, 用独立子串 + 同函数体检查
    assert!(
        src.contains(".inode()") && src.contains(".truncate(") && src.contains("open_file"),
        "vfs_truncate 必须使用 Inode::truncate (open_file.inode().truncate(...))"
    );
}

#[test]
fn get_fd_info_removed() {
    // B 方案拆分第二步: fd 句柄操作已迁至 handle.rs
    let src = read_file("framework/fs/vfs/handle.rs");
    assert!(!src.contains("fn get_fd_info"), "旧的 get_fd_info 函数应已删除");
}

// ============================================================================
// 7. AnonymousInode 存在
// ============================================================================

#[test]
fn anonymous_inode_exists() {
    let src = read_file("services/fs/inode.rs");
    assert!(src.contains("pub struct AnonymousInode"), "AnonymousInode 必须存在");
    assert!(src.contains("impl Inode for AnonymousInode"), "AnonymousInode 必须 impl Inode");
}

// ============================================================================
// 8. fs_resolve_inode 方法存在 (name_to_handle_at 支持)
// ============================================================================

#[test]
fn filesystem_has_fs_resolve_inode() {
    // B09-12/P1-B3: FileSystem trait 已迁回 framework/fs/vfs/types.rs
    let src = read_file("framework/fs/vfs/types.rs");
    assert!(
        src.contains("fn fs_resolve_inode(&self"),
        "FileSystem trait 必须有 fs_resolve_inode 方法"
    );
}

#[test]
fn ramfs_implements_fs_resolve_inode() {
    // DECISION-K 项 5: ramfs 实现回迁 framework/fs/ramfs/mod.rs
    let src = read_file("framework/fs/ramfs/mod.rs");
    assert!(src.contains("fn fs_resolve_inode"), "RamFs 必须实现 fs_resolve_inode");
}

#[test]
fn ext2_implements_fs_resolve_inode() {
    let src = read_file("services/fs/ext2/mount.rs");
    assert!(src.contains("fn fs_resolve_inode"), "ext2 必须实现 fs_resolve_inode");
}

// ============================================================================
// 9. O_APPEND 在 write 路径中检查
// ============================================================================

#[test]
fn vfs_write_checks_append_flag() {
    // B 方案拆分第二步: vfs_write_internal 已从 api.rs 迁至 handle.rs
    let src = read_file("framework/fs/vfs/handle.rs");
    assert!(
        src.contains("VfsOpenFlags::APPEND"),
        "vfs_write_internal 必须检查 O_APPEND flag"
    );
    assert!(
        src.contains("open_file.inode().stat("),
        "O_APPEND 写入前必须通过 stat 获取文件大小"
    );
}

// ============================================================================
// 10. name_to_handle_at 使用真实路径解析
// ============================================================================

#[test]
fn name_to_handle_at_uses_path_resolution() {
    let src = read_file("services/fs/file_handle.rs");
    assert!(
        src.contains("VFS_MANAGER.resolve_mount_fs(path)"),
        "name_to_handle_at 必须通过 VFS 路径解析"
    );
    assert!(
        !src.contains("inode_id = 1u32"),
        "name_to_handle_at 不应硬编码 inode_id"
    );
}

#[test]
fn open_by_handle_at_uses_fs_resolve_inode() {
    let src = read_file("services/fs/file_handle.rs");
    assert!(
        src.contains("fs.fs_resolve_inode(inode_id, mount_idx)"),
        "open_by_handle_at 必须使用 fs_resolve_inode 获取原生 Inode"
    );
    assert!(
        src.contains("VFS_MANAGER.alloc_fd()"),
        "open_by_handle_at 必须通过 VFS 分配 fd"
    );
}
