#![deny(unsafe_code)]
//! `NestFS` (Hypervisor File System) — 模块入口
//!
//! 热插拔监听 + 公共类型重导出.

use crate::framework::driver::hotplug::{HotplugEvent, HotplugListener};
use alloc::boxed::Box;

// 公共类型重导出 (必须在 HotplugListener 之前, 因为热插拔代码使用 get_nestfs)
pub use super::nestfs_data::*;
pub use super::nestfs_inode::*;

/// `NestFS` 热插拔监听器 — 将块设备热插拔事件转发到 `NestFS`
struct NestfsHotplugListener;

impl HotplugListener for NestfsHotplugListener {
    fn on_device_added(&self, event: &HotplugEvent) -> bool {
        if let HotplugEvent::DeviceAdded { location } = event {
            // 使用 slot 作为 drive_id (PCI 热插拔槽位号)
            let drive_id = location.slot;
            crate::slog_info!(
                FS,
                "[NestFS] HOTPLUG: device added (slot={}, bus={}/{}",
                drive_id,
                location.bus,
                location.device
            );
            get_nestfs().hotplug_add_disk(drive_id)
        } else {
            false
        }
    }

    fn on_device_removed(&self, event: &HotplugEvent) {
        if let HotplugEvent::DeviceRemoved { location } = event {
            let drive_id = location.slot;
            crate::slog_info!(FS, "[NestFS] HOTPLUG: device removed (slot={})", drive_id);
            get_nestfs().hotplug_remove_disk(drive_id);
        }
    }
}

/// 注册 `NestFS` 热插拔监听器到全局热插拔管理器
pub fn nestfs_hotplug_register() {
    use crate::framework::driver::hotplug::HOTPLUG_MANAGER;
    HOTPLUG_MANAGER.register_listener(Box::new(NestfsHotplugListener));
}
