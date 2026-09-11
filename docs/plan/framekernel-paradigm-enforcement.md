# F/S 分层范式落实与反向依赖全面整治

> **优先级（2026-09-11 用户裁决）：本工程优先于分册 9**。理由：范式落实（归属判据 + 依赖方向）直接影响后续一切开发的代码归属与接口设计，是分册 9（死代码/TODO 治理）及其后工程的前置。分册 9 的 B09-13 反向依赖治理被本工程 §7 吸收。

> 工程定位：独立架构工程。落实 Asterinas framekernel 范式（机制/策略分离 + Minimalism + 依赖单向），全面整治 framework→services 反向依赖。**实施由 AI 全权接手（用户委派），用户审查。**

## 1. 背景与依据

描述：QueenX 当前 framework 承担功能层（driver/net/fs-vfs/proc/syscall/ipc/wasm），TCB 占比 60.1%（目标 <30%），framework→services 反向依赖 136 处/78 文件，与 Asterinas framekernel 范式偏离（功能应在 services、framework 仅机制契约与安全代理、依赖单向）。
方案：按 Asterinas 范式整治——功能下沉 services、framework 只留机制契约与安全代理、依赖经 trait 注入解决。
详情：
- 依据 1：Asterinas APSys'24 论文 OSTD 四准则（Soundness / Expressiveness / **Minimalism** / Efficiency）。
- 依据 2：`other/asterinas-0.18.1/` 源码实证——kernel/core 含全部功能（device/fs/net/process），ostd 仅机制（io_mem/io_port/dma/mm/task/sync/arch），kernel `#![deny(unsafe_code)]` 0 unsafe，驱动经 `ostd::mm::IoMem/VmIo` 安全访问硬件。
- 依据 3：AGENTS.md §4.1 + explain-framekernel.md（2026-09-11 补全归属决策树 Q1/Q2/Q3）。

## 2. 核心判据（归属决策树）

描述：模块归属判据（与 AGENTS.md §4.1 一致）。2026 年审核员复核后修正：**"0 unsafe" 只说明"不必须 framework"，不决定归属；归属看服务对象**（见 Q1' 服务对象准则）。
方案：
```
Q1: 该功能必须 unsafe 吗（直接碰硬件/页表/裸内存）？
 ├─ 否 → Q1': 服务对象是谁？（服务对象准则，审核员 DECISION-F 定案）
 │       ├─ framework 机制对 services 的安全导出面 → "保留"
 │       │     （框架封装 unsafe 为 safe API 供 services 消费 = 合法且核心的交互模式，
 │       │       例: IoMem::from_pci_bar / userptr safe 构造 / proto_block::register_block_device）
 │       ├─ 仅被 services 内部消费 / 经既有分发机制（dispatch_trait）消费 → "下沉"
 │       └─ 被 framework 机制直接调用 → 下沉需接口化（trait/回调/Chitin 注册），否则 "保留"
 └─ 是 → Q2: 它是"机制"还是"功能"？
       ├─ 机制（页表/切换/寄存器原语/同步/安全代理/FFI 边界）→ "保留"
       └─ 功能（驱动/FS/网络/进程/signal/syscall）→ Q3
             Q3: 能否封装为 safe API 供 services 用？
              ├─ 能 → "封装+下沉"（framework 留机制原语 + IoMem/IoPort/DmaStream/UserPtr safe API，功能迁 services）
              └─ 不能（self-referential / FFI ABI / 中断上下文）→ "保留薄层"
```
另附两个判定：**壳**（纯 re-export，0 unsafe 几行）→ 删壳；**双份**（services 已有 0 unsafe 权威实现）→ services 权威、framework 删业务。
要点：**"要 unsafe" ≠ "放 framework"**——功能要 unsafe 也应由 framework 封装 safe API 后实现在 services（Minimalism 落地关键）；**"0 unsafe" ≠ "放 services"**——framework 的安全 API 出口（机制的"嘴"）留在 framework，下沉的永远是使用它的功能，而不是提供它的接口。

**服务对象准则（2026-09-11 补）**：`0 unsafe` 只说明"不必须 framework"，**不决定归属**——归属看**服务对象**：
- 文件是 **framework 机制对 services 的安全导出面**（如 chitin 注册包装 `register_block_device`、proc/mechanism.rs、IoMem::from_pci_bar）→ **保留 framework**（框架提供安全 API，services 消费 = 合法方向 services→framework）
- 文件仅被 **services 内部使用 / 经既有分发机制**（dispatch_trait）消费 → **下沉**
- 文件被 **framework 机制直接调用** → 下沉需接口化（trait/回调/Chitin 注册），否则保留

## 3. 工程目标（验收标准）

描述：达成 Asterinas 范式对齐。
方案：
1. [ ] TCB 占比 < 30%（当前 60.1%，framework 123K vs services 71K LoC）
2. [ ] framework→services 反向依赖 = 0（当前 136 处/78 文件）
3. [ ] 功能层 services 权威（driver / fs 核心 / net 策略 / proc 策略 / syscall 业务）
4. [ ] framework 仅机制/契约/安全代理（保留 200 文件，见 §6.6）
5. [ ] services 保持 0 unsafe（F1 不回归）
6. [ ] §2.3 五条验证门槛（双架构 0w0e / clippy 0 / 核心审计 / host-tests / QEMU）

## 4. 治理原则

描述：治本双动作，禁止治标。
方案：
- 动作 1：**功能下沉 services**（清单 §6：下沉 20 + 封装+下沉 23 + 部分下沉 11 + 双份合并 20）。
- 动作 2：**framework 机制与 services 交互全 trait 注入**（清单 §7：壳删 82 + 直接 use trait 化 ~20）。
- 禁止：复用 B09-12 / B04-AUDIT-005 的"把 services 功能拉进 framework"模式——该模式让依赖方向"看似正确"但 **TCB 膨胀、机制/策略混淆**，违反 Minimalism。
- 依赖方向靠 trait 注入（framework 定义契约 + services 注册）解决，不靠移动实现代码。

## 5. 决策登记

描述：本工程推翻既往两处治理方向（用户 2026-09-11 裁决）。
方案：
- **DECISION-A（VFS 4 文件下沉）**：`fs/vfs/vfs.rs`（VfsManager）、`dcache.rs`、`types.rs`、`open_file_table.rs` 按 Asterinas 严格判据下沉 services——**推翻 B09-12/DECISION-H13 的 VFS 迁回**（B09-12 中仅 Errno/error 基础库迁回 framework 正确，保留）。依据：Asterinas 的 VFS 在 kernel/services（`kernel/core/src/fs/vfs`），OSTD 不含文件系统抽象。
- **DECISION-B（E1000 业务回迁）**：`driver/net/e1000.rs` 业务回迁 services，framework 留 `dma_ring.rs`（DMA 描述符机制）+ E1000Io（IoMem 封装）——**推翻 B04-AUDIT-005 的 E1000 上移**。依据：驱动是功能，经 safe API 在 services 实现。
- 统一原则：**依赖方向靠 trait 注入，不靠移动实现代码**。

## 6. 逐文件下沉清单（framework 363 + services 260+ 文件逐文件判定，2026-09-11 调研）

### 6.1 下沉（0 unsafe，服务对象准则复核终局）——20 文件 → 3 确认下沉 + 17 保留

> 终局（DECISION-F，审核员裁决）：原"0 unsafe → 直接下沉"判定作废。按服务对象准则复核：
> - ✅ **3 确认下沉**：syscall brk/canary/posix_timer（经 dispatch_trait，无 framework 内部调用残留）——已提交 `c6455358`
> - 🔒 **17 保留**：其余全部被 framework 机制直接调用或为机制安全导出面（逐文件依据见下表现"复核结论"列）
> - 审计白名单（如 audit_block_registration）仅在归属变更后同步路径，禁止先改白名单再迁移

| 文件 | 下沉目标 | 依据 |
|---|---|---|
| framework/syscall/brk.rs | services/syscall/brk | ✅ 确认下沉：经 dispatch_trait 分发 |
| framework/syscall/canary.rs | services/syscall/canary | ✅ 确认下沉：经 dispatch_trait |
| framework/syscall/posix_timer.rs | services/syscall/posix_timer | ✅ 确认下沉：经 dispatch_trait |
| framework/net/init/dns.rs | services/net/dns | ⚠ 0 unsafe 静态 hosts；**framework net 调用方确认后定** |
| framework/driver/hotplug.rs | services/driver | ⚠ 0 unsafe 事件分发；**framework 事件源确认后定** |
| framework/driver/bus/mod.rs | services/driver/bus | ⚠ 0 unsafe 编排；**init_all 调用需回调注册或保留** |
| framework/driver/bus/pci.rs | services/driver/bus | ⚠ 同上（bus 一组）|
| framework/driver/display/controller.rs | services/driver/display | ⚠ 0 unsafe 纯数据结构；display/mod 调用确认 |
| framework/driver/display/font.rs | **保留（framework 基础层）** | 被 framework console/gfx_console（保留）消费渲染字符——服务对象=framework 内部 |
| framework/driver/display/framebuffer.rs | **保留（framework 基础层）** | 同上，console 机制消费 |
| framework/driver/display/self_test.rs | services/driver/display | ⚠ 0 unsafe 自检图案；调用方确认 |
| framework/chitin/firmware.rs | services/driver/firmware | ⚠ 0 unsafe blob 管理；framework chitin 调用确认 |
| framework/chitin/proto_block.rs | **保留（安全注册 API 出口）** | 0 unsafe 但为 chitin 机制对 services 的安全导出面（同 IoMem::from_pci_bar 模式）；下沉反而制造 framework 调用方（storage/composite 5 处）→services 反向依赖 |
| framework/credo/engine.rs | services/credo | ⚠ 0 unsafe 能力检查；framework credo/api C ABI 调用确认 |
| framework/proc/canary.rs | **保留（framework 安全 API）** | 被 framework 内部（proc/process.rs、syscall/api.rs）直接依赖 + services 消费——安全导出面；下沉需回调注册（§7.4 已标）|
| framework/barrier/fault_inject.rs | services/barrier | ⚠ 0 unsafe；**经 BarrierDegradePolicy 类 trait 注入**（同 §6.3）|
| framework/barrier/reset/audit.rs | services/barrier | ⚠ 同上 trait 注入 |
| framework/barrier/reset/bbr.rs | services/barrier | ⚠ 依赖 RECOVERY_MANAGER 框架全局，经 trait 注入 |
| framework/barrier/reset/layered.rs | services/barrier | ⚠ 同上 |
| framework/barrier/reset/parallel.rs | services/barrier | ⚠ 同上 |

> **复核终局（DECISION-F）**：上述 ⚠ 待定项经逐文件核查"framework 侧保留代码是否直接调用"，全部判定保留（见 DECISION-F §6.1 复核终局）——§6.1 收口为 **3 下沉 + 17 保留**。

### 6.2 封装+下沉（safe API 后迁）——23 文件

> 复核纪律（DECISION-F）：本表 0 unsafe 项**先按服务对象准则（§2）查服务对象再动工**（安全导出面 → 保留；仅 services 消费 → 下沉；被 framework 机制直接调用 → 接口化后下沉或保留）；含 unsafe 的按原"封装+下沉"路径。

| 文件 | 下沉目标 | 依据 |
|---|---|---|
| framework/proc/coredump.rs | services/proc/coredump | 732 行，unsafe 仅 klog FFI |
| framework/proc/rlimit.rs | services/proc/rlimit | write_volatile 改 copy_to_user |
| framework/syscall/clone.rs | services/proc/clone（已存在）| 2 unsafe 可封装 |
| framework/syscall/epoll.rs | services/syscall/epoll | 588 行，4 unsafe 可封装 |
| framework/syscall/eventfd.rs | services/syscall/eventfd | 446 行，1 unsafe |
| framework/syscall/firmware.rs | services/syscall/firmware | 11 unsafe 集中用户指针拷贝 |
| framework/syscall/ftrace_kgdb.rs | services/syscall | 用户指针读写改 safe API |
| framework/syscall/info.rs | services/proc/info（已存在）| 用户指针写改 safe API |
| framework/syscall/io.rs | services/syscall/io | 用户指针/fcntl 拷贝 safe API |
| framework/syscall/sendfile.rs | services/syscall/sendfile | 2 unsafe，VFS/pipe safe API |
| framework/syscall/signalfd.rs | services/syscall/signalfd | 482 行，用户指针写 |
| framework/syscall/timerfd.rs | services/syscall/timerfd | 617 行，hrtimer safe API |
| framework/syscall/wait4.rs | services/proc/wait4（已存在）| 2 unsafe 用户指针写 |
| framework/net/init/query.rs | services/net/query | 查询纯 Atomic；reset 薄层留 |
| framework/driver/char/pl011.rs | services/driver/char/pl011 | MMIO 改 IoMem 封装 |
| framework/driver/display/mod.rs | services/driver/display | VBE 原语留框架，管理迁出 |
| framework/driver/input/keyboard.rs | services/driver/input | IoPort 原语留框架，scancode 迁出 |
| framework/driver/net/e1000.rs | services/driver/net/e1000（回迁）| DECISION-B，框架留 DMA 环 |
| framework/driver/net/e1000_io.rs | services/driver/net/e1000 | E1000Io 留框架，Driver 业务迁出 |
| framework/driver/usb/mod.rs | services/driver/usb | IoMem::from_pci_bar 后迁 |
| framework/driver/usb/xhci.rs | services/driver/usb/xhci | 20 unsafe 集中，机制留框架 |
| framework/driver/virtio/mod.rs | services/virtio/transport | 0 unsafe 已可下沉 |
| framework/credo/storage.rs | services/credo/storage | vfs C FFI 留薄层，序列化迁出 |

### 6.3 部分下沉（机制文件内策略拆分）——11 文件

> **拆分接口原则**：迁出的策略函数与 framework 机制的交互必须显式化——中断/panic 上下文经 **trait 注入**（framework 定义契约 + services 注册，OnceLock 全局，只读原子访问）；boot 早期经**回调注册**；普通路径经 **framework 机制 API**。禁止 framework 直接调用 services 函数（反向依赖）。

| 文件 | 拆分说明 | 拆分接口（交互方式） |
|---|---|---|
| framework/mm/vma.rs | mremap/madvise_range/mlock/mincore/mprotect 策略迁出；MmStruct/Vma/find/insert/remove 保留 | ⚠ services 经 framework 提供的 VMA 查询/修改 API + vmm map/unmap 机制编排 mremap 流程（MmStruct 无私有访问权）|
| framework/mm/page_fault.rs | 栈扩展参数/阈值策略迁出；demand paging/COW/swap-in 机制保留 | ⚠ **PageFaultPolicy trait 注入**（#PF 中断上下文；参数由 boot 注册或框架承接）|
| framework/mm/pcache.rs | 容量参数策略化；哈希桶/引用计数/脏页机制保留 | 参数移 services config，机制经 framework 常量 API 读取 |
| framework/mm/swap.rs | LruList/kswapd 决策已 trait 化；slot I/O/swap-in-out 机制保留 | ✅ 已有 SwapPolicy trait（services/swap_policy 权威）|
| framework/timer/time_sync.rs | NTP/PLL 算法迁出；ClockAdjState 状态承接 | 状态留 framework API 承接，算法纯函数经 framework 时钟查询 |
| framework/timer/tickless.rs | NO_HZ 协议迁出；LAPIC 编程机制保留 | 决策经 framework 调度查询 safe API（sched_ops），编程经 framework apic API |
| framework/timer/sleep.rs | adaptive_sleep 阈值/轮询策略迁出；busy_wait 机制保留 | 阈值参数经 framework 常量 API；busy_wait 机制留框架 |
| framework/timer/calibration.rs | 采样算法迁出；static 频率缓存留框架 API 承接 | ⚠ boot 早期调用，经**回调注册**（framework 注册采样回调，services 实现）|
| framework/barrier/domain.rs | apply_degradation 降级策略迁出；RecoveryDomain 状态机保留 | ⚠ **BarrierDegradePolicy trait 注入**（panic/中断上下文；framework 提供原子状态访问 API）|
| framework/barrier/reset/bsr.rs | freeze/unfreeze/rollback 编排迁出；mmio_write32 机制保留 | 编排经 framework 恢复机制 API（RECOVERY_MANAGER）；mmio 写留框架 |
| framework/debug/ebpf.rs | 验证器策略已 trait 化（services）；解释执行引擎保留 | ✅ 已有 BpfVerifier trait（services/ebpf_verifier 权威）|

### 6.4 双份合并（services 权威，framework 删业务）——20 文件 ⛔ 暂缓（DECISION-G 复核后方向待裁决）

| framework 文件 | services 权威 |
|---|---|
| driver/char/serial.rs | services/driver/char/serial（0 unsafe 完整实现）|
| driver/char/vga.rs | services/driver/char/vga |
| driver/storage/mod.rs | services/driver/storage |
| driver/storage/ahci.rs + ahci_block.rs | services/driver/storage/ahci |
| driver/storage/ata.rs + ata_block.rs | services/driver/storage/ata |
| driver/storage/nvme.rs + nvme_block.rs | services/driver/storage/nvme |
| driver/usb/enumerate.rs | services/driver/usb/enumerate |
| driver/usb/hid.rs | services/driver/usb/hid |
| driver/usb/mass_storage.rs | services/driver/usb/mass_storage |
| driver/usb/ring.rs | services/driver/usb/ring |
| driver/usb/usb_core.rs | services/driver/usb/usb_core |
| driver/virtio/blk.rs | services/driver/virtio/blk |
| driver/virtio/net.rs | services/driver/virtio/net |
| chitin/composite.rs | services/chitin/composite |
| chitin/devtree.rs | services/chitin/devtree |
| credo/grant.rs | services/credo/grants |
| credo/session.rs | services/credo/sessions |
| （备注）driver/display/hdmi/ 7 文件 | **整目录未挂载孤儿**，services/driver/display/hdmi 权威，直接删除 |

### 6.5 壳删除（re-export 兼容层）——82 文件

| 子模块 | 壳文件 |
|---|---|
| proc（9）| cfs / cgroup / fd_alloc / madvise_mlock / namespace / oomd / seccomp / session / types |
| syscall（4）| madvise_mlock / mmap / mprotect / types |
| fs（10）| mod / vfs/mod / devfs/mod / devfs / procfs/mod / procfs / ramfs/mod / ramfs / hvfs/mod / flock |
| net（5）| mod / types / wait_queue / netfilter / driver/mod（+route/inotify 为"壳+薄层"）|
| ipc（5）| async_ipc / scheduler_integration / sem / signal / types |
| io（2）| mod / iouring |
| wasm（6）| mod / interpreter / leb128 / module / runtime / types |
| credo（3）| capability / sha256 / types |
| driver（8 + hdmi 7）| block / char/mod / input/mod / net/mod + display/hdmi/ 7 孤儿 |
| config（11）| 全部（boot_image/capacity/caps/error/kaslr/memory/procfs/sched/slab/validate + mod）|
| debug（2）| mod / api |
| barrier（4）| mod / types / reset/mod / reset/config |
| mm（5）| mod / api / mechanism / numa / pressure / kmalloc_slab |
| timer（1）| mod |

### 6.6 保留 framework（机制/TCB）——200 文件

- **arch（19）**：x86_64/aarch64 全部（页表/中断/上下文切换/APIC/GIC/PSCI/UART/启动）
- **sync（14）**：全部同步原语（含 pi_mutex 优先级继承协议、lockdep）
- **idt/irq/smp/cpu/pci/dma（17）**：IDT 编程/softirq/SMP 基础设施/CPU 探测/PCI 配置/MSI/DMA 引擎
- **mm 机制（15）**：vmm 页表/copy_user/cow/frame/kmalloc/slab/pmm/kpti/arch + 4 trait 契约（alloc/pmm/slab/swap）
- **proc 机制（16 + canary）**：process/thread/user_proc/scheduler/scheduler_ex/信号投递/elf 加载/cpu_queue/proc_ops/mechanism + **canary（安全 API，§6.1 重判）** + sched/signal/dispatch trait 契约
- **syscall 入口（5）**：dispatch/api/mod（FFI + raw）/futex（用户原子）/dispatch_trait
- **fs 契约（6）**：backend_trait/inode/vfs_poll_trait/handle/mount/path（userptr + FFI 机制）
- **net 集成（12）**：init 状态机/raw（static mut）/smoltcp_impl/sockets（self-referential）/sm_fi/syscall/save/iface_trait/api
- **ipc 机制（6）**：mod（命名空间）/dynamic/msgq（侵入式链表）/pipe/shm FFI 薄层/api
- **driver 机制（11 + 安全注册包装）**：mod/framework（端口 I/O 原语）/kexec/uefi/power 硬件原语/net/dma_ring/virtio/queue/chitin 注册表 + **安全注册包装（proto_block，§6.1 重判）** + proto 指针表（char/input/net）+ user_driver
- **credo 机制（8）**：mod/api（C ABI）/audit 环形缓冲/bootstrap/csprng/identity/secure_boot
- **debug 机制（3）**：ftrace/kgdb/ringbuf
- **barrier 机制（10）**：api/manager/recoverable/recovery/snapshot/undo_log/bhr + reset/mod
- **console（2 + 基础层）**：mod/gfx_console + **display/font + framebuffer（基础算法层，§6.1 重判，console 机制消费）**
- **顶层杂项（25）**：cpu_local/iomem/ioport/dma_buf/frame/iobuf/irqline/userptr/usermode/userctx/vmspace/errno/error/credo_pwm/net_socket/proc_elf/process_cleanup/racy_cell/rlimit_query/syscall_init/tick_query/fd_notify/page_table/prelude/mod
- **lib/klog/constants/alloc/boot（18）**：基础库/日志/常量/分配 trait/引导

### 6.7 services 侧现状（260+ 文件确认）

- **权威实现 ~89 文件**：T1-T9/E6 系列已完成（config 全、credo 类型层、ipc 策略/类型、mm 策略、net 策略、proc 策略、wasm、fs 伪文件系统 devfs/procfs_core/hvfs/flock/inotify/ramfs_core/iouring）。
- **策略实现 ~90 文件**：exfat/ext2（独立 FS）、cgroupfs/configfs/devpts/sysfs/systree/virtiofs/overlayfs/tmpfs、wasi（9）、sync barrier/once/scoped、proc memfd/pidfd、driver display dp/ddc。
- **壳/代理 ~63 文件**：framework 权威，services 薄层（chitin/credo 运行时/debug/ipc 命名空间/mm 物理层/net syscall/proc 进程表/sync/syscall/timer/klog 等）。
- **影子双份**：driver char/storage/usb/virtio + display/hdmi——即 §6.4 合并对象。
- 注意：B09-12 已把 fs 的 dcache/vfs_types/vfs_manager/open_file_table 迁回 framework——按 DECISION-A 重新下沉（§5）。

## 7. trait 化改造清单（反向依赖全面整治）

描述：framework 机制与 services 交互全部经 trait/回调/注册表，清除直接引用。
方案：
- **7.1 壳删除（§6.5 82 文件）**：framework→services 引用 -70 处（`pub use` 壳）。
- **7.2 直接 use trait 化（~20 处）**：framework 生产代码直接 `use services` 的逐处改造——
  - syscall 分发 → dispatch_trait 注入（已有雏形）
  - 调度/信号决策 → sched_trait/signal_trait（保留契约位，确认无直接 use）
  - 其余逐处：sm_fi/sendfile/user_proc 等 → 封装 API 或回调注册。
- **7.3 framework/tests 访问 services（7 处）**：测试载体访问 services 真实代码属合理，不纳入整治（另议）。
- **7.4 下沉后 framework 残留调用点 + 接口方案**（2026-09-11 实测）：

| 下沉类 | framework 残留调用点 | 接口方案 |
|---|---|---|
| syscall 17 个（brk/epoll/eventfd/timerfd/signalfd/info/io/sendfile/wait4/firmware/ftrace_kgdb/clone/posix_timer/canary）| dispatch.rs:207 已走 dispatch_trait；**但 dispatch.rs:96/208 直接引用 `services::syscall::types::{SYS_rt_sigreturn, ENOSYS_RET}`** | ✅ dispatch_trait（已有雏形）；收敛 2 处类型引用为 framework 类型 |
| driver 12 个（bus/hotplug/display/char/input/storage/usb/virtio/e1000）| driver/mod.rs:197 `init_all` 直接调 `char::char_init()`/display_init/bus 等 | ⚠ **Chitin 注册**（驱动注册表分发）或 init 回调注册 |
| canary 2 个（proc/syscall）| syscall/api.rs:260/265 直接调 `super::canary::sys_getrandom`（api.rs 保留）| ⚠ 回调注册（framework 提供注册点）|
| coredump/rlimit/wait4 | framework signal/process 机制调用点 | ⚠ 实施前调用方确认 |
| net 类（dns/query）| framework net init 调用点 | ⚠ 实施前调用方确认 |

- 复用既有策略注入模式：pmm_trait/slab_trait/swap_trait/alloc_trait/sched_trait/dispatch_trait。
- **7.5 保留 framework 文件内的 services 引用（43 处需收敛，2026-09-11 全量统计）**——136 处反向依赖中，除壳（70）/下沉文件（12）/tests（9 另议）外，保留机制文件内仍有 43 处 `use services` 需逐处收敛：

| 保留文件 | 引用数 | 处置 |
|---|---|---|
| framework/ipc/mod.rs | 11 | 收敛：机制保留，services 策略类型改经 framework 顶层 re-export / 接口抽象 |
| framework/ipc/pipe.rs | 5 | 收敛：FFI 薄层保留，策略类型经 services 顶层暴露 |
| framework/ipc/shm.rs | 4 | 同上 |
| framework/ipc/msgq.rs | 4 | 收敛：机制（侵入式链表）与策略 re-export 拆分 |
| framework/net/syscall.rs | 4 | 收敛：services 类型 → framework 类型（copy-in/out 桥接）|
| framework/net/init/sm_fi.rs | 3 | 收敛：FFI 边界，wire 类型翻译集中框架 |
| framework/syscall/dispatch.rs + dispatch_trait.rs | 5 | 收敛 2 处类型引用（SYS_rt_sigreturn/ENOSYS_RET）为 framework 常量 |
| framework/proc/{user_proc.rs,process.rs,sched_ops.rs} | 4 | 收敛：services 类型 → framework 类型 |
| framework/credo/{identity.rs,mod.rs} | 3 | 收敛：services 类型 → framework 类型 |
| framework/driver/power.rs | 2 | ✅ 正确"薄层+re-export"形态（services 策略权威 + framework 硬件原语），保留 |
| framework/tests + ipc/stress_tests.rs | 9 | 另议（测试载体访问 services 合理）|

**合计**：43 处收敛（ipc 24 为第一优先）+ 2 处正确形态 + 9 处另议 = 54 处；加壳 70 + 下沉 12 = **136 处全覆盖**。

## 8. 批次实施计划

描述：分 6 阶段，每阶段独立可验证（实施交委托人）。
方案：
- 阶段 0：**safe API 缺口补齐 + 策略注入 trait 定义**。[X]（本工程首批交付）
  - ✅ **PageFaultPolicy trait**（[framework/mm/page_fault_policy.rs](../../src/kernel/framework/mm/page_fault_policy.rs)）：栈扩展三参数策略化，fallback=历史值；page_fault.rs 已接入（`current_page_fault_policy`）
  - ✅ **BarrierDegradePolicy trait**（[framework/barrier/degrade_policy.rs](../../src/kernel/framework/barrier/degrade_policy.rs)）：降级矩阵策略化，fallback=历史矩阵；domain.rs `apply_degradation` 已接入
  - ✅ **IoMem::from_pci_bar 化**：usb/mod.rs xhci-pci 改安全包装（-1 unsafe）；xhci.rs 测试用 fake MMIO 保留 `IoMem::new`
  - ✅ **UserPtr safe 构造**：`UserReadPtr/UserWritePtr::checked_new`（先 `validate_user_buf` 再构造）+ 单测；FFI 调用点（unsafe extern "C" 契约）随对应下沉阶段迁移
  - ✅ **VMA 查询/修改 API**：已满足——services/mm/{mremap,mprotect}.rs 已委托 framework `MmStruct::{mremap,mprotect}`，无需新增
  - 📝 **DmaStream 收敛裸指针**：已无 pub 裸指针（`cpu_addr` 返回 `NonNull<u8>`），随阶段 3 下沉驱动验证
  - 📝 **FFI 薄层分离**：随阶段 6.2/6.3 下沉实施（syscall 用户指针拷贝集中框架）
  - 📝 **calibration 采样回调**：boot 早期路径，随阶段 6.3 timer 部分下沉实施
- 阶段 1：**纯策略下沉**（§6.1 20 文件）。[X] 收口——3 确认下沉（syscall×3，已提交）+ 17 保留（服务对象准则复核终局，DECISION-F）
- 阶段 2：**封装+下沉**（§6.2 23 文件）。[]
- 阶段 3：**驱动双份合并 + E1000 回迁**（§6.4 20 文件 + DECISION-B）。[]
- 阶段 4：**VFS 4 文件下沉 + backend_trait 扩展**（DECISION-A）。[]
- 阶段 5：**壳删除 82 + 直接 use trait 化 20 + 保留文件 43 处收敛**（§7.5，ipc 24 第一优先）。[]
- 阶段 6：**全量验证**（§3 验收 + §9 门槛）。[]

## 9. 验证门槛

描述：每阶段提交必须满足（§2.3 + 本工程专项）。
方案：
1. 双架构 cargo check --release 0 error / 0 warning
2. clippy -D warnings 0
3. 核心审计全过（audit_services_boundary / audit_coupling / audit_tcb_ratio 关注 TCB 下降）
4. host-tests 全过
5. QEMU 集成测试（boot/驱动改动必跑）
6. **反向依赖 grep 计数单调下降**（`kernel::services` 引用，136 → 0）
7. **TCB 占比逐阶段下降**（60.1% → <30%）

## 10. 关联工程

- 分册 9 B09-13（反向依赖治理）→ 本工程 §7 吸收。
- 分册 9 B09-12（vfs/api）→ VFS 部分被 DECISION-A 推翻；Errno/error 基础库迁回保留。
- B04-AUDIT-005（E1000 上移）→ 被 DECISION-B 回迁。
- 分册 5/6 系列迁移（T1-T9/E6）→ 已下沉成果为本工程基础。
- AGENTS.md §4.1 / explain-framekernel.md（2026-09-11 决策树补全）→ 判据来源。

## 11. 中途问题与决策记录

> 本工程实施过程中遇到的问题与用户决策登记（变更历史由 git 提交承载，此处仅登记决策内容与理由）。

### DECISION-C: aarch64 clippy 回归处置（用户裁决方案 B — 显式导入）

- **发现时机**: 阶段 0 验证 aarch64 `clippy -D pedantic` 时，`mm/vmm_aarch64.rs:14` 报 `wildcard_imports` 错误（`use super::*;`）。
- **根因**: 既有 commit `a7851509`（分册 9 死代码治理，2026-09-11）将本文件的 `#![allow(clippy::wildcard_imports, clippy::cast_possible_truncation)]` 当作"过时 allow"删除，但 `use super::*;` 仍在——该 allow 保护的是 aarch64 移植约定（glob 导入 mm 父模块全部公开 API，与 x86_64 侧同步），删除即 CI 回归。
- **候选方案**:
  - A: 恢复 `#![allow(clippy::wildcard_imports)]`——保留移植约定，改动 1 行，风险最低；
  - B: 按 clippy 建议改显式导入清单——消除 glob，风格与 x86_64 侧（已无 glob）一致，更干净。
- **用户裁决**: B。已将 `use super::*;` 改为 `use super::{PAGE_NX, PAGE_SIZE, PAGE_USER, PAGE_WRITABLE, PageFlags, PageSize, PhysAddr, VirtAddr, get_pmm};`；`super::KERNEL_BASE` / `super::kpti::kpti_init` 保留显式路径。
- **状态**: [X]

### DECISION-D: 阶段 0 范围登记（三项顺延至对应阶段）

- **描述**: 阶段 0 原列的 DmaStream 收敛裸指针 / FFI 薄层分离 / calibration 采样回调，经调研确认与后续阶段绑定，登记顺延：
  - **DmaStream 收敛裸指针**: 当前已无 pub 裸指针返回（`cpu_addr` 返回 `NonNull<u8>`，构造走 safe `from_frame`），待阶段 3 驱动下沉时随业务验证；
  - **FFI 薄层分离**: 属阶段 6.2/6.3 下沉动作的一部分（syscall 用户指针拷贝集中框架、中断/panic 上下文经 trait 注入）；
  - **calibration 采样回调**: boot 早期路径（PIT 参考时钟），随阶段 6.3 timer 部分下沉一并实施。
- **状态**: [X]（登记，不在阶段 0 交付）

### 阶段 0 验证结果（§9 门槛）

| 门槛 | 结果 |
|---|---|
| 双架构 cargo check --release 0w0e | ✅ x86_64 + aarch64（RUSTFLAGS=-D warnings） |
| clippy -D pedantic 0（除 cast_*） | ✅ x86_64 + aarch64 |
| 核心审计 | ✅ boundary 0 / safety 100% / coupling 0 / comment 0 / deadlock 0（1 项 HIGH 为既有 smp_init.rs）/ invariants 全 PASS |
| host-tests | ✅ 全过（exit 0） |
| QEMU | ⚠️ 未跑——x86_64 进 Ring 3 卡点（display→usb 区间）为既有 ISSUE-RT-001，与本阶段改动正交；本阶段为纯机制重构 + fallback 保持行为 |

### DECISION-E: proto_block 归属（初裁 → 被审核员推翻，见 DECISION-F）

- **矛盾**: §6.1 列 `framework/chitin/proto_block.rs` 下沉 services（0 unsafe 纯注册逻辑）；§6.6 保留清单含"chitin 注册表 + proto 指针表 + user_driver"。
- **初裁（本工程，已作废）**: 依据 Asterinas 范式实证（`kernel/core/src/device/registry/block.rs` 在 safe 层）+ Q1/Q2/Q3 判据，裁决"§6.1 正确、下沉 services"。
- **审核员推翻（最终裁决，DECISION-F 吸收）**: proto_block **保留 framework**——它是 chitin 注册表机制的**安全导出面**（机制之"嘴"），下沉会制造 5 处 framework 调用方（driver/storage ×4、chitin/composite ×1）→ services 反向依赖，违背 Minimalism + 依赖单向。
- **状态**: [X]（初裁作废，最终裁决见 DECISION-F）

### DECISION-F: 服务对象准则定案 + §6.1 复核终局（审核员裁决）

- **判据修正（写入 §2）**: `0 unsafe` 只说明"不必须 framework"，**不决定归属**——归属看**服务对象**：
  1. framework 机制对 services 的**安全导出面** → 保留（例: IoMem::from_pci_bar / userptr safe 构造 / proto_block::register_block_device）；
  2. 仅被 services 内部 / 经既有分发机制（dispatch_trait）消费 → 下沉；
  3. 被 framework 机制直接调用 → 下沉需接口化（trait/回调/Chitin 注册），否则保留。
- **§6.1 复核终局**（逐文件查"framework 侧保留代码是否直接调用它"）:
  - ✅ **3 确认下沉**: syscall brk/canary/posix_timer（经 dispatch_trait、无 framework 残留调用）——已提交 `c6455358`；
  - 🔒 **17 保留**: 其余全部被 framework 机制直接调用或是机制安全导出面——proto_block（chitin 注册表安全导出）、proc/canary（process.rs:332 机制直接依赖 + services 顶层 re-export 消费）、net/init/dns（net init cmd.rs 调用）、driver/hotplug（syscall/dispatch.rs:960 + init_all 调用）、driver/bus×2（init_all 调用）、driver/display×4（显示机制内部 + console 消费）、chitin/firmware（devtree.rs:94 直接引用）、credo/engine（session/api/user_driver 直接调用）、barrier/fault_inject（recoverable.rs:71 调用）、barrier/reset×4（恢复机制本体）。
- **审计脚本纪律**: 凡确认下沉的文件，其审计白名单（如 audit_block_registration）仅在归属变更后同步路径，禁止先改白名单再迁移。
- **状态**: [X]

### DECISION-G: §6.4 双份合并复核（接线实证，2026-09-12 调研）

> 按"核查后再动工"纪律，对 §6.4 全部 20 项做接线实证（谁被 init_all/services 实际调用），发现原表方向与真实状态有较大出入，**不能按原表直接执行**。

**实证结论**：
1. **framework/driver/mod.rs `init_all` 全部接线 framework 侧驱动**（char/bus/storage/input/display/usb/hotplug，L197-226）——framework 驱动是 **active 权威实现**；services 侧驱动（storage/nvme+ahci、char/serial+vga、virtio/blk+net）是**真实实现但未接入启动路径**的影子（即既有 MIG-005 未理清的双份）。
2. **services/usb×5（enumerate/hid/mass_storage/ring/usb_core）、chitin/devtree、driver/net/e1000、uefi、kexec、firmware** 均为 `pub use crate::kernel::framework::...::*` 的 **re-export 壳** → 属 §6.5 壳删除，**不是** §6.4 合并对象。
3. **services/credo/grants+sessions、services/chitin/composite** 是 framework 机制（grant/session/composite 机制）之上的**策略层/安全代理**——按服务对象准则（安全导出面保留）是**正确形态**，framework 版本应保留，无"删业务"。
4. **display/hdmi**：framework/driver/display/hdmi/（7 文件）是否孤儿待核（services/driver/display/hdmi.rs 权威）。

**处置**：§6.4 原表**暂缓执行**，分类改为：
- 🔒 壳（→§6.5 删壳，非本阶段）：usb×5、chitin/devtree、e1000、uefi、kexec、firmware
- 🔒 机制/策略正确形态（保留 framework，无重复）：credo/grant+session、chitin/composite
- ⚠ 真双份（framework wired active + services 影子）：storage×7、char×2、virtio×2 —— 合并方向需用户/审核员裁决（保留 framework 机制并删 services 影子，还是按 §6.2 封装+下沉把业务迁 services 并改接线）
- ⚠ display/hdmi（7 文件孤儿）待核

**状态**: [X]（复核登记；§6.4 实际施工待方向裁决）

### 阶段 1 首批（syscall brk/canary/posix_timer 下沉）验证结果

| 门槛 | 结果 |
|---|---|
| 双架构 cargo check --release 0w0e | ✅ x86_64 + aarch64（RUSTFLAGS=-D warnings） |
| clippy -D pedantic 0（除 cast_*） | ✅ x86_64 + aarch64 |
| 核心审计 | ✅ boundary 0 / safety 100% / coupling 0 / comment 0 / deadlock 0 / invariants 全 PASS |
| host-tests | ✅ 全过（exit 0）——首次全量并行 `test_fsx_stress` 偶发失败（单独跑 33s 通过，全量重跑通过），判定为并行负载 flaky，非本次改动引入 |

> 备注：`scripts/audit_coupling.py` 中 `framework::syscall::{brk,canary,posix_timer}` 检测模式随文件删除失效（不再匹配），无害保留。
