use super::check;
use crate::kernel::framework::fs::vfs::types::{FsType, VfsDirEntry, VfsFileType, VfsSeekWhence};
use crate::kernel::framework::fs::vfs::vfs::VfsManager;
use crate::kernel::framework::tests::{TestResult, runner};
use crate::register_tests_inner;

fn test_fstype_from_name() -> TestResult {
    let ramfs = FsType::from_name("ramfs");
    check!(ramfs == FsType::RamFs, "ramfs should be RamFs");

    let hvfs = FsType::from_name("hvfs");
    check!(hvfs == FsType::HvFs, "hvfs should be HvFs");

    let unknown = FsType::from_name("ext4");
    check!(unknown == FsType::Unknown, "ext4 should be Unknown");
    TestResult::Pass
}

fn test_fstype_as_str() -> TestResult {
    check!(FsType::RamFs.as_str() == "ramfs", "RamFs as_str mismatch");
    check!(FsType::HvFs.as_str() == "hvfs", "HvFs as_str mismatch");
    check!(
        FsType::Unknown.as_str() == "unknown",
        "Unknown as_str mismatch"
    );
    TestResult::Pass
}

fn test_vfs_file_type() -> TestResult {
    check!(
        VfsFileType::from_u8(0) == Some(VfsFileType::File),
        "0 should be File"
    );
    check!(
        VfsFileType::from_u8(1) == Some(VfsFileType::Dir),
        "1 should be Dir"
    );
    check!(
        VfsFileType::from_u8(2) == Some(VfsFileType::Dev),
        "2 should be Dev"
    );
    check!(
        VfsFileType::from_u8(3) == Some(VfsFileType::Symlink),
        "3 should be Symlink"
    );
    check!(
        VfsFileType::from_u8(99).is_none(),
        "invalid should return None"
    );

    check!(VfsFileType::Dir.as_u8() == 1, "Dir as_u8 should be 1");
    TestResult::Pass
}

fn test_vfs_seek_whence() -> TestResult {
    check!(
        VfsSeekWhence::from_u32(0) == Some(VfsSeekWhence::Set),
        "0 should be Set"
    );
    check!(
        VfsSeekWhence::from_u32(1) == Some(VfsSeekWhence::Cur),
        "1 should be Cur"
    );
    check!(
        VfsSeekWhence::from_u32(2) == Some(VfsSeekWhence::End),
        "2 should be End"
    );
    check!(
        VfsSeekWhence::from_u32(99).is_none(),
        "invalid should return None"
    );
    TestResult::Pass
}

fn test_vfs_mount_unmount() -> TestResult {
    let mgr = VfsManager::new();
    let result = mgr.mount("/", "ramfs");
    check!(result.is_ok(), "mount / should succeed");

    let dup = mgr.mount("/", "ramfs");
    check!(dup.is_err(), "duplicate mount should fail");

    let found = mgr.find_mount("/");
    check!(found.is_some(), "should find / mount");

    let unmount_result = mgr.unmount("/");
    check!(unmount_result.is_ok(), "unmount / should succeed");

    let not_found = mgr.find_mount("/");
    check!(not_found.is_none(), "should not find / after unmount");
    TestResult::Pass
}

fn test_vfs_resolve_mount() -> TestResult {
    let mgr = VfsManager::new();
    let _ = mgr.mount("/", "ramfs");
    let _ = mgr.mount("/home", "hvfs");

    let root = mgr.resolve_mount("/");
    check!(root.is_some(), "should resolve /");
    let (_idx, fs_type) = root.unwrap();
    check!(fs_type == FsType::RamFs, "/ should be RamFs");

    let home = mgr.resolve_mount("/home/user/file.txt");
    check!(home.is_some(), "should resolve /home/user/file.txt");
    let (_, home_fs) = home.unwrap();
    check!(home_fs == FsType::HvFs, "/home should be HvFs");

    let rel = mgr.get_relative_path("/home/user/file.txt", home.unwrap().0);
    check!(rel == "user/file.txt", "relative path mismatch");
    TestResult::Pass
}

fn test_vfs_fd_alloc_free() -> TestResult {
    let mgr = VfsManager::new();
    let fd1 = mgr.alloc_fd();
    check!(fd1.is_some(), "first alloc should succeed");

    let fd2 = mgr.alloc_fd();
    check!(fd2.is_some(), "second alloc should succeed");
    check!(fd1.unwrap() != fd2.unwrap(), "fds should be different");

    mgr.free_fd(fd1.unwrap());
    let fd3 = mgr.alloc_fd();
    check!(fd3.is_some(), "alloc after free should succeed");
    TestResult::Pass
}

fn test_vfs_dirent() -> TestResult {
    let mut dirent = VfsDirEntry::new();
    dirent.set_name("test.txt");
    let name = dirent.get_name();
    check!(name == "test.txt", "dirent name mismatch");
    TestResult::Pass
}

fn test_vfs_cwd() -> TestResult {
    let mgr = VfsManager::new();
    mgr.set_cwd("/home/user");
    let cwd = mgr.get_cwd();
    check!(cwd == "/home/user", "cwd mismatch");
    TestResult::Pass
}

fn test_vfs_snapshot_restore() -> TestResult {
    let mgr = VfsManager::new();
    let _ = mgr.mount("/", "ramfs");
    mgr.capture_snapshot();

    let _ = mgr.unmount("/");
    check!(
        mgr.find_mount("/").is_none(),
        "mount should be gone after unmount"
    );

    mgr.restore_from_snapshot();
    let found = mgr.find_mount("/");
    check!(
        found.is_some(),
        "mount should be restored after snapshot restore"
    );
    TestResult::Pass
}

// ============================================================================
// DECISION-K 项 6 回归测试 (第二十四批): services::fs::init 注册激活
//
// 回归背景: services::fs::init 此前全库无调用者, 第二十三批 ramfs 回迁引入
// 的 make_ramfs_inode 钩子恒命中 FallbackFsBackend → Err(NotInitialized),
// ramfs open/create 生产路径被回退策略拦截.
// ============================================================================

fn test_fs_backend_registered_make_inode() -> TestResult {
    // 激活注册 (幂等: 重复注册 Err 被忽略)
    crate::kernel::services::fs::init();
    // 钩子必须返回真实 Inode — FallbackFsBackend 恒 Err, 本断言锁定回归
    let result = crate::kernel::framework::fs::vfs::backend_trait::current_fs_backend()
        .make_ramfs_inode(0, 0);
    check!(
        result.is_ok(),
        "make_ramfs_inode 命中回退策略 — services::fs::init 未生效"
    );
    TestResult::Pass
}

fn test_ramfs_fs_open_via_backend_hook() -> TestResult {
    use crate::kernel::framework::fs::ramfs::{RAMFS_DATA, RamFsData};
    use crate::kernel::framework::fs::FileSystem;

    crate::kernel::services::fs::init();
    // 建根目录 (幂等): RAMFS_DATA 初始为空, resolve_path("/") 需先 mount
    crate::kernel::framework::fs::ramfs::init();

    // 在 RamFS 根目录建文件 (锁内操作, 作用域结束释放锁)
    let created = {
        let mut ramfs = RAMFS_DATA.lock();
        ramfs.create_file("/", "backend_reg_t", 0)
    };
    check!(created.is_some(), "create_file 应成功");
    let Some(_node_id) = created else {
        return TestResult::Fail("create_file 失败");
    };

    // SAFETY: 全局 static RAMFS_DATA 拥有 RamFsData, 裸指针提升后生命周期为
    // 'static (mount.rs 同款手法); fs_open 内部自行加锁, 此处不持锁调用, 无死锁.
    // 注意: 守卫必须收窄到块内 — rustc 1.98 nightly (RFC 3606 临时生命周期
    // 延长) 下 `let p = &raw const *RAMFS_DATA.lock()` 会使守卫存活至绑定
    // 作用域结束, fs_open 内部重入 lock() 将同线程自旋死锁 (cast 形式无此
    // 延长, 块作用域强制语句末释放).
    let ramfs_ptr: *const RamFsData = {
        let guard = RAMFS_DATA.lock();
        &raw const *guard
    };
    let fs: &'static RamFsData = unsafe { &*ramfs_ptr };

    // fs_open → make_inode 钩子 → services RamFsInode (回归路径本体)
    let opened = fs.fs_open("/backend_reg_t", 0, 0);
    check!(
        opened.is_ok(),
        "fs_open 应经 backend 钩子返回 Inode (命中 Fallback 即回归)"
    );
    TestResult::Pass
}

fn test_hvfs_fs_registered() -> TestResult {
    crate::kernel::services::fs::init();
    let Some(fs) = crate::kernel::framework::fs::vfs::backend_trait::hvfs_fs() else {
        return TestResult::Fail("hvfs_fs() 未注册 — services::fs::init 未生效");
    };
    check!(fs.name() == "hvfs", "hvfs name mismatch");
    // 注: fs_format 行为不在单测覆盖 (内存模式调 format_drive 有底层 IO 副作用),
    // 语义等价性由 fsformat 路径代码搬移保证, QEMU boot 覆盖挂载分发链路
    TestResult::Pass
}

pub fn register_vfs_tests() {
    let r = runner();
    register_tests_inner! { r:
        "vfs::types": {
            "fstype_from_name": test_fstype_from_name,
            "fstype_as_str": test_fstype_as_str,
            "file_type": test_vfs_file_type,
            "seek_whence": test_vfs_seek_whence,
            "dirent": test_vfs_dirent,
        },
        "vfs::mgr": {
            "mount_unmount": test_vfs_mount_unmount,
            "resolve_mount": test_vfs_resolve_mount,
            "fd_alloc_free": test_vfs_fd_alloc_free,
            "cwd": test_vfs_cwd,
            "snapshot_restore": test_vfs_snapshot_restore,
        },
        "vfs::backend": {
            "fs_backend_registered_make_inode": test_fs_backend_registered_make_inode,
            "ramfs_fs_open_via_backend_hook": test_ramfs_fs_open_via_backend_hook,
            "hvfs_fs_registered": test_hvfs_fs_registered,
        },
    }
}
