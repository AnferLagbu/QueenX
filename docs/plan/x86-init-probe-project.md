# x86_64 init_all 探测阻塞根治工程（独立工程）

> 来源：[audit-fix-02-framework-arch-asm.md](./archive/audit-fix-02-framework-arch-asm.md) B02-25 调研根因问题 2（2026-08-23 审查拆开登记）。
> 与 KPTI 完整化工程（[kpti-complete-project.md](./kpti-complete-project.md)）、分册 2 均无归属关系——启动早期问题，CPU 全程 KERNEL_PML4。

## 工程计划 A: 现状与定位

### 背景

- **X86IP-01. 现象与证据（QEMU 实测）**
  - 描述：`./scripts/qemu_boot_test.sh x86_64`（`-nic none`，25s timeout）日志停在 `[DISPLAY] OK: 1024x768x32 @ 0xFD000000`（第 85 行）后无后续输出。`init_all()`（[driver/mod.rs:197](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/mod.rs#L197-L226)）在 `display_init()` 之后依次调用 `usb::usb_init()` / `hotplug_init()` / `nestfs_hotplug_register()` / `devtree_probe_composites()` / `open_softirq()`。
  - 方案：精确定位卡点并修复，使 x86_64 到达 Ring 3。
  - 状态：[]

- **X86IP-02. 已排除项（委托人与审查双重复核）**
  - 描述：e1000 挂起不是根因（`-nic none` 已隔离，仍卡）；25s timeout 不是根因（aarch64 60s 可达 EL0）；与 KPTI 半实现无关（启动早期未进用户态）。
  - 方案：维持排除结论，聚焦 `init_all` 内部。
  - 状态：[X]（2026-08-23 实测登记）

### 定位阶段（Phase 1）

- **X86IP-03. 卡点精确定位**
  - 描述：日志停在 `display_init` 最后一条 `[DISPLAY] OK` 之后，无 `[USB] discovered`（[usb/mod.rs:144](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/usb/mod.rs#L139-L162)）。卡点落在 `display_init` 返回 → `usb_init` xHCI PCI 探测 区间，或 `display_init` 内部返回路径。
  - 方案：在 `display_init` 返回后与 `usb_init` 各阶段插入 `klog_boot_info!` 里程碑日志（或 QEMU GDB 断点 `display_init`/`usb_init` 单步），确定具体阻塞函数。排查方向：xHCI PCI 扫描（QEMU 无 xHCI 设备时 PCI 枚举是否死循环）、PCI 总线后续设备扫描、存储/键盘驱动的轮询等待。
  - 状态：[]

- **X86IP-04. 根因确认**
  - 描述：基于 X86IP-03 定位结果，确认阻塞根因（如 PCI 枚举遗漏/死循环、设备驱动等待中断、display 帧缓冲后续初始化）。
  - 方案：根因确认后登记到本条目，附源码引用与复现命令。
  - 状态：[]

### 修复阶段（Phase 2）

- **X86IP-05. 修复实施**
  - 描述：按根因做外科手术式修复（只改必须改的）。
  - 方案：修复后补回归测试（若属驱动逻辑，host-tests 加对应用例；若属启动流程，QEMU 验证覆盖）。
  - 状态：[]

- **X86IP-06. 验证**
  - 描述：x86_64 QEMU 完整启动到达 Ring 3 + 用户态陷入/返回往返（顺带闭合分册 2 B02-25 的问题 2 缺口）。
  - 方案：`TIMEOUT_QEMU=60 ./scripts/qemu_boot_test.sh x86_64`，日志出现 `Entering Ring 3` 且 init 打印 'X'/'Y'（`src/user/init`）；无 panic/#PF。
  - 状态：[]
  - **下游依赖**：X86IP-06 产出的 x86_64 Ring 3 到达是 [kpti-complete-project.md](./kpti-complete-project.md) KPTI-09/KPTI-12（x86_64 侧验证）的隐式前置——x86 启动阻塞未解决前，KPTI 工程无法完成 x86_64 侧 QEMU 验证。

### 决策记录

- **DECISION-058**
  - 描述：x86_64 init_all 探测阻塞单独立项（用户决策，2026-08-23）；与 KPTI 工程、分册 2 隔离，独立回滚面。
  - 状态：[X]

### 验证标准

- x86_64 QEMU 完整启动：`VFS ready` → `Network Subsystem Ready` → `Entering Ring 3` → init 打印 `X`/`Y` → syscall 往返无异常
- 全程无 panic/#PF/Triple Fault；§2.3 5 条门槛全过（改动 boot/驱动相关时）

### 风险

- **卡点在 usb_init xHCI 扫描**：QEMU 无 xHCI 设备时 PCI 枚举若死循环，需检查 PCI 总线扫描终止条件。
- **卡点在 display_init 内部**：多路径（multiboot2/pci）需区分。
- **修复范围蔓延**：若根因是驱动通用问题（如 PCI 扫描），修复可能波及 aarch64——需双架构回归。
