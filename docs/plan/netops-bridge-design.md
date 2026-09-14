# 批次 Z ④ 设计：NetOps 安全桥 trait（实施完成）

> 状态：[X] 有条件通过 → P1 修正完成 → 实施完成 + 验证全绿（提交人：AI；审核人：用户/审核员）
>
> 关联：`docs/plan/framekernel-paradigm-enforcement.md` §6.4 委托批次 Z ④。前置：批次 Z ③ transport 去重已完成（`77407b8e`）。
>
> 审核记录（有条件通过）：
> - P1-1 §5 `try_receive` 同名方法解析风险 → 改 `VirtioNetDriver::try_receive(self, buf)` 显式全路径调用（已修正）。
> - P1-2 §5 初始化接线方向措辞含糊 → 澄清为 DECISION-K 单向注册契约：framework 定义槽位（`net_register_services_driver`，OnceLock set-once，同 storage `NVME_SERVICES_DISPATCH`），services `net_init` 填充，`nic_probe_all` 单向拉取，framework 不引用 services（已修正）。
> - P2-3 `net_send_impl` 保留 null/len 守卫（与旧 `virtio_net_send` 行为等价）（已实施）。
> - P2-4 `net_get_mac_impl` 补对齐保证注释（`Box::into_raw` 源自 `Box<T>`，分配器保证对齐）（已实施）。
> - P2-5 aarch64 QEMU 冒烟 virt 机型含 virtio-net 设备参数（已落地：`qemu_boot_test.sh` aarch64 段加 `-device virtio-net-device,netdev=n0 -netdev user,id=n0` + 桥探测断言）。
>
> 实施验证（2026-09-14，全绿）：
> - 双架构 `build all` 0w0e；clippy `--release -D warnings` 双架构 0；clippy pedantic (lib x86_64) 0。
> - 核心审计 + 8 项补充全绿（boundary/coupling/deadlock/repr_c/volatile/static_mut/reverse_deps/invariants/comment/once_cell/block_registration/SAFETY 100%）。
> - host-tests 755/0（99 套件，含新增 `net_device_ops_bridge_test` 3 测试）。
> - QEMU aarch64 virt 挂网卡冒烟 1/1（`virtio-net: probed successfully (services bridge)` + Network Subsystem Ready）；x86_64 boot 回归 1/1（Ring 3）。
> - 实施期遗留修复：AHCI `identify`（批次 Y `4994cbba` 引入）clippy pedantic 违规（similar-names + manual-let-else）本轮按审核裁决修复（let-else + expect，带 reason）。
> - 预存登记（实测修正）：`--features host-test --lib` clippy 报 E0152 `owned_box` 仅在 src/rust 目录内触发（rust-src + build-std 与 host std 冲突，DECISION-021 同族工具链限制）；CI 与 audit.sh 2b 从 repo 根跑均实测 0 error 通过，验证链无碍；根治已实施（framekernel-paradigm-enforcement.md §10「构建模式显式化工程」：删全局 build-std + 裸机入口显式注入，src/rust 内 host clippy E0152 已消失）。

## 1. 背景与目标

services `VirtioNetDriver`（virtio-net 设备驱动业务，0 unsafe）已完整（初始化/特性协商/MAC/链路/RX 迁业务/收发路径），但**无法接入 smoltcp**：smoltcp 适配层 `ChitinNetDevice`（framework 机制）要求 `&'static NetOps`——一个 extern "C" 函数指针表（send/try_receive/get_mac/handle_irq），其 4 个实现体需要裸指针转换（`&mut *(driver_data as *mut T)` + `from_raw_parts`）与指针契约，services 层 `#![deny(unsafe_code)]` 无法直接构造。

目标：framework 提供 **NetOps 安全桥 trait**（同 CharOps 桥模式，X 批次计划），services 设备 impl trait 后经 framework 机制函数接入 smoltcp；framework 旧 `VirtioNet` 驱动与 `virtio_net_*` FFI 删除（services 收敛，机制留 framework）。

## 2. 现状

```text
smoltcp Interface
└── phy::Device ── ChitinNetDevice (framework/net/smoltcp_impl.rs, 机制)
    └── &'static NetOps (framework/chitin/proto_net.rs, extern "C" 指针表)
        └── driver_data: *mut c_void
            ├── e1000 (framework 驱动, 保持)
            └── VirtioNet (framework/driver/virtio/net.rs, 旧驱动, 待删)
                └── virtio_net_send/recv/get_mac/irq (extern "C", unsafe 转换)
```

`nic_probe_all`（framework/net/init/probe.rs:42）在启动临界区探测 e1000 → virtio-net，构造 `ChitinNetDevice` 交 `init_stack` 入 smoltcp。

**问题**：virtio-net 的驱动业务在 framework（旧 `VirtioNet`），与 services `VirtioNetDriver` 平行；NetOps 指针表无 safe 构造路径，services 无法接入。

## 3. 桥 trait 定义（framework）

文件：`src/kernel/framework/net/net_device_ops.rs`（新）

```rust
/// 网络设备操作契约 (NetOps 安全桥) — services 0 unsafe impl
///
/// 每个方法对应 NetOps 指针表的一个槽位; framework 负责将
/// 该 trait 的方法转发为 extern "C" 回调 (unsafe 转换留在 framework)。
pub trait NetDeviceOps: Send {
    /// 发送网络包 (data 完整以太网帧, 不含 virtio 头)。返回 0 成功 / -1 失败。
    fn send(&mut self, data: &[u8]) -> i32;

    /// 尝试接收网络包到 buf。返回字节数, 0 = 无数据, <0 = 错误。
    fn try_receive(&mut self, buf: &mut [u8]) -> i32;

    /// 读取 MAC 地址。
    fn get_mac(&self) -> [u8; 6];

    /// 中断处理 (可选; 默认空实现 = 轮询模式)。
    fn handle_irq(&mut self) {}
}
```

`send` 的 `&[u8]` 直接对应 NetOps `send(driver_data, data, len)` 的 data/len 契约；`try_receive` 同理。**语义修正**：现状 `virtio_net_send/recv` 用 `UserReadPtr/UserWritePtr` 包装回调缓冲区——但 NetOps 回调的 data/buf 实际指向 smoltcp 持有的**内核缓冲区**（`ChitinNetDevice.rx_buf/tx_buf`），非用户指针；桥实现改用 `core::slice::from_raw_parts` 构建切片（SAFETY: NetOps 契约保证调用期有效），更准确且移除 userptr 误用。

## 4. unsafe 转换边界（全部留在 framework）

```rust
// framework 泛型桥函数 (monomorphization 生成具体类型的 extern 回调)
pub fn net_ops_for<T: NetDeviceOps + 'static>() -> &'static NetOps {
    // SIMPLIFIED: Box::leak 一次; 设备数量稀少 (每类型 1 个), 与既有 name.leak()
    // 注册模式一致; 若未来支持动态多实例需改 OnceLock<Box<NetOps>> 表
    Box::leak(Box::new(NetOps {
        send: net_send_impl::<T>,
        try_receive: net_recv_impl::<T>,
        get_mac: net_get_mac_impl::<T>,
        handle_irq: Some(net_irq_impl::<T>),
    }))
}

extern "C" fn net_send_impl<T: NetDeviceOps>(
    driver_data: *mut u8, data: *const u8, len: u32,
) -> i32 {
    // null/len 守卫 (P2-3): 与旧 virtio_net_send L551 行为等价, 防御
    // from_raw_parts 边界 (零长/空指针构造 slice 是 UB)
    if driver_data.is_null() || data.is_null() || len == 0 {
        return -1;
    }
    // SAFETY: driver_data 由 register_net_device 的 Box::into_raw 提供且存续于设备生命周期;
    // data/len 由 NetOps 契约保证 (smoltcp 内核缓冲区, 调用期有效)
    let dev = unsafe { &mut *(driver_data as *mut T) };
    let slice = unsafe { core::slice::from_raw_parts(data, len as usize) };
    dev.send(slice)
}
// net_recv_impl / net_irq_impl 同构 (recv 仅 null 守卫, 与旧 virtio_net_recv 一致);
// net_get_mac_impl 用 *const T + &self, 对齐保证注释 (P2-4): 指针源自
// Box::into_raw(Box<T>), 分配器保证 T 对齐.
```

**driver_data 契约**（framework 注册入口，唯一持有裸指针处）：

```rust
pub struct NetDeviceRegistration {
    pub ops: &'static NetOps,
    pub driver_data: *mut core::ffi::c_void,
    /// MAC 地址 (注册时读取, 供 ChitinNetDevice 构造, 免二次回调).
    pub mac: [u8; 6],
}

/// services 调用: 注册 services 网络设备, 返回 smoltcp 可用的适配数据。
/// 注册本体 0 unsafe (mac 在 Box::into_raw 前经 Box<T> 解引用读取)。
pub fn register_net_device<T: NetDeviceOps + 'static>(dev: Box<T>) -> NetDeviceRegistration {
    let mac = dev.get_mac();
    let raw = Box::into_raw(dev).cast::<core::ffi::c_void>(); // 所有权转移, 泄漏存续
    NetDeviceRegistration { ops: net_ops_for::<T>(), driver_data: raw, mac }
}
```

- `Box::into_raw` 泄漏：设备指针存续内核全生命周期（与现状 `VIRTIO_NET_DEVICE: Mutex<Option<Box<VirtioNet>>>` 语义等价，现状同样泄漏/独占）。
- `handle_irq` 恒 `Some`：trait 默认空实现兜底，IRQ 接线统一（SIMPLIFIED: 若需"无 IRQ 设备不注册回调"的精确性，可在 trait 提供 `fn has_irq() -> bool`，当前无消费者不引入）。

## 5. services 接线（0 unsafe）

services `VirtioNetDriver` impl `NetDeviceOps`：

```rust
impl crate::kernel::framework::net::NetDeviceOps for VirtioNetDriver {
    fn send(&mut self, data: &[u8]) -> i32 {
        match self.send_packet(data) { Ok(()) => 0, Err(()) => -1 }
    }
    #[expect(clippy::cast_possible_truncation, reason = "RX 缓冲区上限 2048 字节, usize→i32 截断不可达")]
    fn try_receive(&mut self, buf: &mut [u8]) -> i32 {
        // P1-1: 显式全路径调用 — trait impl 块内 `self.try_receive(buf)` 存在
        // 同名方法解析风险 (trait 方法 vs inherent 方法同签名), 必须显式限定
        // inherent 方法, 杜绝解析到 trait 方法自身造成无限递归.
        VirtioNetDriver::try_receive(self, buf) as i32
    }
    fn get_mac(&self) -> [u8; 6] { *self.mac() }
    // handle_irq: 默认空 (轮询模式; IRQ 驱动后续登记)
}
```

**初始化接线（DECISION-K 单向注册契约，P1-2 澄清）**：依赖方向严格 **services→framework 单向**，framework 不引用 services（F2）。framework 持有槽位机制，services 填充，framework 单向拉取：

1. **framework 定义槽位**（`net_device_ops.rs`）：`static NET_SERVICES_DRIVER: OnceLock<fn() -> Option<NetDeviceRegistration>>` + `net_register_services_driver(probe)`（set-once，同 storage `NVME_SERVICES_DISPATCH` / `nvme_register_services_msix_dispatch` 模式，storage/mod.rs）+ `net_services_driver()`（pub(crate)，framework 内部拉取入口）。
2. **services `net_init()` 填充**（services 权威，crate root lib.rs 在 `qx_net_init` 之前编排）：仅注册探测回调 `virtio_net_registration`（无捕获函数指针），此时不探测设备。
3. **framework `nic_probe_all` 单向拉取**：e1000 分支失败后调用 `net_services_driver()` → 拉取回调执行实际探测（`VirtioMmioDevice::probe` 扫描 → `device_id == VIRTIO_ID_NET` → `VirtioNetDriver::new` + `finalize`（vq0/vq1 MMIO 配置 + DRIVER_OK，与 blk `finalize` 同构）→ `register_net_device::<VirtioNetDriver>`）→ 取 `NetDeviceRegistration` 构造 `ChitinNetDevice`。

**framework 侧改造**：
- `nic_probe_all` 的 virtio-net 分支改为：经 `net_services_driver()` 槽位拉取 `NetDeviceRegistration` 构造 `ChitinNetDevice`；e1000 分支保持。
- 删除 `framework/driver/virtio/net.rs`（旧 `VirtioNet` 驱动 + `virtio_net_*` FFI）+ `VIRTIO_NET_OPS_STATIC`。

## 6. 删除项（services 收敛）

| 删除 | 替代 |
|---|---|
| `framework/driver/virtio/net.rs` 旧 `VirtioNet` 驱动（~620 行） | services `VirtioNetDriver`（已存在，impl NetDeviceOps） |
| `virtio_net_probe/take_device/send/recv/get_mac/irq` FFI | services `net_init` + `NetDeviceOps` |
| `VIRTIO_NET_OPS_STATIC` | `net_ops_for::<T>` 泛型桥 |

保留：`ChitinNetDevice`（smoltcp 适配机制）、`NetOps`（指针表契约）、`nic_probe_all` 骨架（e1000 分支）、`framework/driver/virtio/mod.rs` transport + `queue.rs`（机制）。

## 7. 验证方案

1. 双架构 `cargo check --release` 0w0e + clippy 双架构 0
2. 核心审计全绿（boundary/safety/coupling/comment/deadlock/invariants/reverse_deps/once_cell）
3. host-tests（NetDeviceOps trait 单测 + 现有 virtio 测试）
4. **aarch64 QEMU virt 挂网卡冒烟**：`virtio-net: probed successfully` + 网络子系统启动 + NIC 注册（对比 ③ 挂盘冒烟）。**P2-5**: 需确认 virt 机型命令行含 virtio-net 设备参数（`-device virtio-net-device,netdev=n0 -netdev user,id=n0`，对齐 Y 批次挂盘冒烟配置；默认机型 NIC 与显式参数行为需实测确认）。
5. x86_64 boot 回归 1/1（e1000 分支不变）

## 8. 风险与待决

- **`&'static NetOps` 泄漏**：每次 `net_ops_for` 泄漏一次（注册数量稀少）。待决：接受（SIMPLIFIED）或改 `ChitinNetDevice` 生命周期参数化（影响 smoltcp_impl，成本高，不建议本批）。
- **services→framework 网络分发契约**：已按 P1-2 澄清为 DECISION-K 单向注册契约（`NET_SERVICES_DRIVER: OnceLock<fn() -> Option<NetDeviceRegistration>>` set-once 槽位，services `net_init` 填充，`nic_probe_all` 单向拉取，framework 不引用 services）。调用时序：crate root lib.rs 在 `qx_net_init` 之前调用 `net_init` 注册回调；回调实际执行发生在 `nic_probe_all`（启动临界区单线程，与旧 `virtio_net_probe` 同一时点）。
- **e1000**：保持 framework（无 services 版本）；桥模式为通用 trait，e1000 未来可迁（不在本批）。
- **IRQ 驱动**：`handle_irq` 默认空（轮询）；virtio-net IRQ 驱动登记为后续子步（I-42 风格），本批先打通 smoltcp 接入。

## 9. 范围外

- CharOps 桥（休眠，devfs 接入时补）——本设计为通用"安全桥 trait"模式，CharOps 可复用同一泛型手法。
- virtio-net IRQ 中断驱动路径。
- e1000 迁 services。
