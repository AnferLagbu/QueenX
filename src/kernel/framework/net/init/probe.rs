//! 网络设备探测 (B04-09 优化拆分 Step F, 2026-08-25)
//!
//! 原 init.rs 内联定义: `nic_probe_all` / `E1000_NET_OPS_STATIC` /
//! `VIRTIO_NET_OPS_STATIC`. 抽出为独立子模块后, init.rs 主体通过
//! `probe::nic_probe_all()` 调用 (仅 init 模块内部可见, 不对外暴露).
//!
//! 批次 Z ④ (NetOps 安全桥): `VIRTIO_NET_OPS_STATIC` 与旧 framework
//! `VirtioNet` 驱动已删除, virtio-net 分支改为经 DECISION-K 注册契约槽位
//! 单向拉取 services `VirtioNetDriver` 注册数据 (services→framework 单向,
//! framework 不引用 services)。

use alloc::boxed::Box;

use crate::framework::driver::Driver;
use crate::framework::net::ChitinNetDevice;

use super::raw;

#[cfg(not(feature = "kernel_test"))]
static E1000_NET_OPS_STATIC: crate::framework::chitin::NetOps = crate::framework::chitin::NetOps {
    send: crate::framework::driver::e1000_net_send,
    try_receive: crate::framework::driver::e1000_net_recv,
    get_mac: crate::framework::driver::e1000_net_get_mac,
    handle_irq: Some(crate::framework::driver::e1000_net_irq),
};

/// # Safety
///
/// - 在网络子系统初始化入口被调用, 期间无其他并发探测
/// - 依赖的 chitin/driver 框架 (`Driver::init`) 自身保证设备独占
#[cfg(not(feature = "kernel_test"))]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
// SAFETY: 仅由 qx_net_init 在启动临界区调用一次 (单线程), 无并发探测;
// 返回的 ChitinNetDevice 所有权转移给调用方, 内部裸指针由驱动生命周期保证.
pub(super) unsafe fn nic_probe_all() -> Option<ChitinNetDevice> {
    // SAFETY: 调用方保证单线程进入初始化临界区 (与 qx_net_init 同一调用栈);
    // 各驱动 probe/take_device/init 内部保证设备独占访问, 无并发裸指针共享.
    unsafe {
        // I-53 修复: 去除编译时架构互斥, 双架构二进制按运行时探测顺序
        // 尝试 e1000 (PCI 设备) 与 virtio-net (MMIO 设备). 两者驱动代码
        // 均架构无关, 仅依赖 IoMem / PCI 抽象. QEMU 配置决定哪一个会成功.
        //
        // 探测顺序固定: e1000 -> virtio-net. 真实硬件 (e.g. PC 上) e1000
        // 优先; QEMU virt 上 e1000 探测返回非 0 走 fallthrough 到 virtio.
        //
        // 失败: 全部探测返回非 0 / Box::into_raw 失败 / Driver::init 失败.

        // 1) e1000 探测 (PCI 设备, 走 PCI 总线)
        // aarch64: e1000_probe() 内部安全返回 -1 (无 PCI ECAM)
        {
            let probe_result = crate::framework::driver::e1000_probe();
            if probe_result == 0 {
                let mut dev = crate::framework::driver::e1000_take_device()?;
                if Driver::init(&mut *dev).is_err() {
                    raw::klog_err("e1000: hardware init failed");
                    return None;
                }
                let mac = dev.mac();
                let raw_ptr = Box::into_raw(dev) as *mut core::ffi::c_void;
                let nic = ChitinNetDevice::new(&E1000_NET_OPS_STATIC, raw_ptr, mac);
                raw::klog_msg("e1000: probed successfully");
                return Some(nic);
            }
        }

        // 2) virtio-net (services 权威, 批次 Z ④ NetOps 安全桥)
        // DECISION-K 单向注册契约: services net_init 已注册探测回调, 此处
        // 经 framework 槽位单向拉取 (framework 不引用 services, F2 合规)。
        // 未注册/探测失败返回 None → nic_probe_all 返回 None (与旧行为一致)。
        {
            if let Some(reg) = crate::framework::net::net_device_ops::net_services_driver() {
                let nic = ChitinNetDevice::new(reg.ops, reg.driver_data, reg.mac);
                raw::klog_msg("virtio-net: probed successfully (services bridge)");
                return Some(nic);
            }
        }

        None
    }
}
