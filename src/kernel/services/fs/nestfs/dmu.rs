#![deny(unsafe_code)]
use crate::services::fs::nestfs::bp::NestBlockPointer;
use crate::services::fs::nestfs::spa::HV_POOL_BLOCK_SIZE;
use crate::services::sync::irq_lock::IrqSpinLock as Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const HV_DMU_OBJ_NUM: u64 = 0;
pub const HV_DMU_OBJ_META: u64 = 1;
pub const HV_DMU_OBJ_ROOT: u64 = 2;
pub const HV_DMU_MAX_BLOCKPTR: usize = 16;
pub const HV_DMU_MAX_NAME: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NestObjType {
    None = 0,
    File = 1,
    Dir = 2,
    Snapshot = 3,
    Zap = 4,
    ZapMicro = 5,
    Volume = 6,
    SpaceMap = 7,
    ObjSet = 8,
    Symlink = 9,
}

impl NestObjType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::File,
            2 => Self::Dir,
            3 => Self::Snapshot,
            4 => Self::Zap,
            5 => Self::ZapMicro,
            6 => Self::Volume,
            7 => Self::SpaceMap,
            8 => Self::ObjSet,
            9 => Self::Symlink,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct NestDmuObject {
    pub obj_id: u64,
    pub obj_type: NestObjType,
    pub block_size: u32,
    pub nblocks: u64,
    pub size: u64,
    pub bp: NestBlockPointer,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub owner_pwm: u64,
    pub group_pwm: u64,
    pub sensitivity: u8,
    pub pwm_perm: u16,
    pub link_count: u32,
    pub flags: u32,
    pub birth_txg: u64,
    pub data_hash: [u64; 4],
    pub fill: u64,
    pub dirty: bool,
    pub used: bool,
}

impl NestDmuObject {
    pub fn new_file(obj_id: u64, owner_pwm: u64) -> Self {
        Self {
            obj_id,
            obj_type: NestObjType::File,
            block_size: HV_POOL_BLOCK_SIZE as u32,
            nblocks: 0,
            size: 0,
            bp: NestBlockPointer::null(),
            atime: 0,
            mtime: 0,
            ctime: 0,
            owner_pwm,
            group_pwm: 0,
            sensitivity: 0,
            pwm_perm: 0o644,
            link_count: 1,
            flags: 0,
            birth_txg: 0,
            data_hash: [0; 4],
            fill: 0,
            dirty: false,
            used: true,
        }
    }

    pub fn new_dir(obj_id: u64, owner_pwm: u64) -> Self {
        Self {
            obj_id,
            obj_type: NestObjType::Dir,
            block_size: HV_POOL_BLOCK_SIZE as u32,
            nblocks: 0,
            size: 0,
            bp: NestBlockPointer::null(),
            atime: 0,
            mtime: 0,
            ctime: 0,
            owner_pwm,
            group_pwm: 0,
            sensitivity: 0,
            pwm_perm: 0o755,
            link_count: 2,
            flags: 0,
            birth_txg: 0,
            data_hash: [0; 4],
            fill: 0,
            dirty: false,
            used: true,
        }
    }

    pub fn new_zap(obj_id: u64) -> Self {
        Self {
            obj_id,
            obj_type: NestObjType::Zap,
            block_size: HV_POOL_BLOCK_SIZE as u32,
            nblocks: 0,
            size: 0,
            bp: NestBlockPointer::null(),
            atime: 0,
            mtime: 0,
            ctime: 0,
            owner_pwm: 0,
            group_pwm: 0,
            sensitivity: 0,
            pwm_perm: 0o644,
            link_count: 1,
            flags: 0,
            birth_txg: 0,
            data_hash: [0; 4],
            fill: 0,
            dirty: false,
            used: true,
        }
    }

    pub fn new_symlink(obj_id: u64, owner_pwm: u64) -> Self {
        Self {
            obj_id,
            obj_type: NestObjType::Symlink,
            block_size: HV_POOL_BLOCK_SIZE as u32,
            nblocks: 0,
            size: 0,
            bp: NestBlockPointer::null(),
            atime: 0,
            mtime: 0,
            ctime: 0,
            owner_pwm,
            group_pwm: 0,
            sensitivity: 0,
            pwm_perm: 0o777,
            link_count: 1,
            flags: 0,
            birth_txg: 0,
            data_hash: [0; 4],
            fill: 0,
            dirty: false,
            used: true,
        }
    }

    pub fn is_file(&self) -> bool {
        self.obj_type == NestObjType::File
    }
    pub fn is_dir(&self) -> bool {
        self.obj_type == NestObjType::Dir
    }
    pub fn is_zap(&self) -> bool {
        self.obj_type == NestObjType::Zap || self.obj_type == NestObjType::ZapMicro
    }
    pub fn is_snapshot(&self) -> bool {
        self.obj_type == NestObjType::Snapshot
    }

    pub fn mark_dirty(&mut self, txg: u64) {
        self.dirty = true;
        self.birth_txg = txg;
    }

    pub fn cow_bp(&mut self, new_bp: NestBlockPointer, txg: u64) {
        self.bp = new_bp;
        self.birth_txg = txg;
        self.dirty = true;
    }
}

pub struct NestObjSet {
    pub objects: Mutex<Vec<NestDmuObject>>,
    pub next_obj_id: AtomicU64,
    pub root_obj: u64,
    pub initialized: AtomicBool,
}

// SAFETY (Framekernel P2.2.2): NestObjSet 全部字段 (Mutex<T>, Atomic*, Vec) 自动 Send + Sync。

impl NestObjSet {
    pub fn new() -> Self {
        Self {
            objects: Mutex::new(Vec::new()),
            next_obj_id: AtomicU64::new(HV_DMU_OBJ_ROOT + 1),
            root_obj: HV_DMU_OBJ_ROOT,
            initialized: AtomicBool::new(false),
        }
    }

    pub fn init(&self, owner_pwm: u64) {
        let mut objs = self.objects.lock();
        objs.clear();
        let mut root = NestDmuObject::new_dir(HV_DMU_OBJ_ROOT, owner_pwm);
        root.birth_txg = 1;
        objs.push(root);
        let zap = NestDmuObject::new_zap(HV_DMU_OBJ_META);
        objs.push(zap);
        self.next_obj_id
            .store(HV_DMU_OBJ_ROOT + 2, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
    }

    pub fn alloc_obj(&self, obj_type: NestObjType, owner_pwm: u64) -> Option<u64> {
        let obj_id = self.next_obj_id.fetch_add(1, Ordering::AcqRel);
        let obj = match obj_type {
            NestObjType::File => NestDmuObject::new_file(obj_id, owner_pwm),
            NestObjType::Dir => NestDmuObject::new_dir(obj_id, owner_pwm),
            NestObjType::Zap | NestObjType::ZapMicro => NestDmuObject::new_zap(obj_id),
            NestObjType::Symlink => NestDmuObject::new_symlink(obj_id, owner_pwm),
            _ => return None,
        };
        self.objects.lock().push(obj);
        Some(obj_id)
    }

    pub fn free_obj(&self, obj_id: u64) -> bool {
        let mut objs = self.objects.lock();
        if let Some(obj) = objs.iter_mut().find(|o| o.obj_id == obj_id) {
            obj.used = false;
            obj.link_count = obj.link_count.saturating_sub(1);
            if obj.link_count == 0 {
                obj.used = false;
            }
            true
        } else {
            false
        }
    }

    pub fn get_obj(&self, obj_id: u64) -> Option<NestDmuObject> {
        let objs = self.objects.lock();
        objs.iter().find(|o| o.obj_id == obj_id && o.used).copied()
    }

    pub fn get_obj_mut(&self, obj_id: u64) -> Option<NestDmuObject> {
        self.get_obj(obj_id)
    }

    pub fn update_obj(&self, obj: &NestDmuObject) -> bool {
        let mut objs = self.objects.lock();
        objs.iter_mut()
            .find(|o| o.obj_id == obj.obj_id)
            .map_or(false, |existing| {
                *existing = *obj;
                true
            })
    }

    pub fn get_root(&self) -> Option<NestDmuObject> {
        self.get_obj(self.root_obj)
    }

    pub fn obj_count(&self) -> u64 {
        self.objects.lock().iter().filter(|o| o.used).count() as u64
    }
}
