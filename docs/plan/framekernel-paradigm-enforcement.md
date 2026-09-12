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

### §7 ipc 现状分析 + 收敛方案（2026-09-12 复核）

> 实测 framework/ipc 的 services 引用重新归类（原表"24 处"含壳与测试，生产引用为 13 处 FFI 边界调用）：

| 类别 | 文件 | 引用 | 处置 |
|---|---|---|---|
| 纯 re-export 壳 | types / sem / signal / scheduler_integration / async_ipc.rs | `pub use services::ipc::*` | §6.5 壳删除 |
| **FFI 边界调 services（核心 13 处）** | pipe.rs（5：is_pipe_fd + 4 FFI）、shm.rs（4）、msgq.rs（4）| extern "C" 薄层持 namespace/UserPtr（framework 机制）→ 调 services `*_safe` | **trait 注入**：framework 定义 `IpcStrategy` 契约，services 实现 + OnceLock 注册（复用 pmm_trait/swap_trait 模式）；FFI 边界调 trait（framework→services 归零） |
| cfg(test) | mod.rs tests（11）、stress_tests.rs | 测试访问 services 真实代码 | §7.3 另议（合理） |

- **收敛方案（pipe 起）**：① framework/ipc 定义 `IpcStrategy` trait（方法签名含 `&mut IpcNamespace`/`&mut IpcId`/`pid` 等 framework 机制类型，与 `*_safe` 一一对应）；② services/ipc 实现 trait（内部调既有 `*_safe`）；③ framework 提供 `OnceLock<&'static dyn IpcStrategy>` 注册点 + `ipc_init` 期回调（boot 早期注册）；④ FFI 边界改调 trait（`IPC_STRATEGY.get().pipe_create(...)`），framework→services 引用归零；⑤ 验证链全绿。
- **风险**：注册时序（boot 早期须先于首个 syscall）；trait 对象动态分派微开销（可接受，与 pmm_trait 同级）。
- **进度**：本分析为 §7 ipc 施工前置。**§6.5 壳删除（types/sem/signal/scheduler_integration/async_ipc 5 壳 + 全仓 70 壳）是更低风险的首批收敛动作**，可与 trait 注入并行推进。

### §7 壳删除真实障碍：所有权纠缠（2026-09-12 复核，需裁决）

> 实测发现：**所有 §6.5 壳并非纯机械删除**——壳存在的原因正是 framework 生产代码依赖 services 项。壳删除 = 逐项判定归属 + 所有权反转，而非删文件。

| 壳组 | framework 生产依赖 | 归属判定 | 处置 |
|---|---|---|---|
| ipc 类型壳（types.rs）| `IPC_NAMESPACE: RacyCell<IpcNamespace>`（framework 机制）持有 services 类型（IpcNamespace/Pipe/MsgQueue/ShmSegment/Semaphore/Message/WaitQueue/WaitQueueItem + IPC_MAX_* 常量，均定义于 services/ipc/types.rs）| 命名空间是 framework 机制（§6.6），其数据类型 = 机制类型 | **类型所有权反转**：迁回 framework，services re-export（合法 services→framework）——DECISION-A 式反转 |
| config 常量壳（11 文件）| `framework::config::{PAGE_SIZE, MAX_CPUS}` 等被 iobuf/cpu_local/smp/rcu/irq/mm 等 framework 机制大量使用；`config::procfs/caps/validate` 被 framework/tests 使用 | 页大小/最大 CPU 等 = 机制常量 | **常量迁回 framework**，services config re-export |
| 其余壳（proc/debug/barrier/wasm 等）| 同构——壳为 framework 消费 services 项的兼容层 | 逐项判定 | 分批所有权反转 |

**裁决请求**：壳删除的正确执行路径 = **DECISION-A 式所有权反转**（机制项迁回 framework，services 侧改 re-export 保持 API 兼容），而非机械删文件。是否确认此方向？若是，§7 壳删除批次将按"先迁移机制项到 framework → services re-export → 删除 framework 壳 → 清理引用"执行（每批验证链全绿）。

### DECISION-J 第一批执行记录：ipc 类型反转（commit 3519410e）

> 审核员裁决（DECISION-J）：采纳所有权反转路径，ipc 类型反转第一批通过。边界：仅迁机制类型/常量、策略逻辑禁止随迁、依赖闭包检查、services re-export 保兼容。

- **迁回 framework**：`framework/ipc/types.rs` 由 re-export 壳改为真实类型定义（约 600 行）——`IpcId`/`IPC_MAX_*`(8 常量, 含 E-03 any 门控)/`PIPE_BUFFER_SIZE`/`SHM_MAX_SIZE`/`MSG_MAX_SIZE`/`MSG_QUEUE_MAX_MSGS`/`IpcType`/`SignalNum`(+From)/`SignalAction`/`WaitQueueItem`/`WaitQueue`(+B07-15 中断安全实现)/`Pipe`/`SignalHandlerFn`/`SignalHandler`/`SignalPending`/`ShmSegment`/`Message`/`MsgQueue`/`Semaphore`/`IpcNamespace` + cfg(test) WaitQueue 测试。0 unsafe（依赖闭包：WaitQueue→IrqSpinLock 为 framework 自身类型）。
- **services 改 re-export**：`services/ipc/types.rs` 改为 `pub use crate::kernel::framework::ipc::types::*`（含 #![deny(unsafe_code)]），API 兼容。
- **策略逻辑未随迁**：services/ipc/{pipe,shm,msgq,sem,signal}.rs 的 `*_safe` 策略实现保持 services（T6 权威），且它们本就经 `crate::kernel::framework::ipc::types::*` 引用类型（services→framework 合法方向）。
- **引用计数**：framework 文件级反向依赖 79→78（types.rs 壳引用消除）；framework/ipc/mod.rs 的 `use types::*` 现解析到 framework 自身类型（0 services 引用）。
- **验证**：双架构 0w0e ✅ / clippy -D pedantic 双架构 0 ✅ / 核心审计全 0 ✅ / host-tests 待确认 / QEMU 未跑（纯类型归位不触 boot，下一批壳删除时合并冒烟）。

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
- ⚠ 真双份（framework wired active + services 影子）：storage×7、char×2、virtio×2 —— **方向裁决：直接方案 B**（见下）
- ⚠ display/hdmi（7 文件孤儿）待核

**方向裁决（审核员，2026-09-12）——真双份 11 项执行"直接方案 B"，取消"先 A 后 B"**：
- **理由**：services 影子是"真实实现但未接线"（内容完整）——直接 B = 接线切到 services + framework 留机制删业务，一步到位；"先 A"会误删可复用 services 实现，B 时仍需从 framework 迁回（重复搬移）。接线改造风险是 A 后 B 也必经的，A 只推迟不消除。
- **执行要求**：
  1. **前置核实**：11 项 services 影子内容自足性（0 unsafe ✓ / 硬件访问经 IoMem/IoPort/DmaStream/Chitin 机制 API / 业务自含不依赖 framework 内部）。**完整 → 直接接线**；**不完整（半成品/依赖 framework 内部）→ 该项改从 framework 迁业务**（仍是 B 形态）。核实不通过不构成选 A 的理由。
  2. **接线改造**：init_all 从"调 framework 驱动"改为 **Chitin 注册分发指向 services 驱动**（§7.4 接口化模式）。
  3. **framework 退位**：留 IoMem/IoPort/DmaStream/Chitin 注册机制，删驱动业务。
  4. **分子类推进**：先 1 子类（char 或 storage）→ QEMU 驱动冒烟 → 再扩展；每子类跑 audit_services_boundary。
  5. **MIG-005 收尾**：真双份 11 项即 MIG-005 遗留，本阶段接管（§10 关联登记）。

**状态**: [X]（复核登记 + 方向裁决完成；§6.4 施工按直接方案 B 执行）

### DECISION-H: storage 转独立专项（内容自足性核实发现半成品，2026-09-12 审核员裁决）

> **前置核实结论**（委托人）：services storage 是 Phase 2.1.3/2.1.4 的"并行实现但内容不等价"半成品——缺 MSI-X/IRQ 路径（NVMe B07）、I-42 IRQ、ATA PIO 真实驱动、MSIX-03 验收钩子、`_block` 适配器与注册路径；services/driver/mod.rs 头注自认"迁移中"。**不符合"直接接线"前置**（DECISION-G 的"不完整"分支）。

**裁决**：
1. **storage 从 §6.4 剥离为独立专项工程**（framework 迁业务路径，非接线路径）：
   - 0 号子步（立即）：`nvme_read_identify_*` 解析 helper 迁 services（纯逻辑 + host 测试）
   - services 补 `_block` 适配器 + `impl BlockDevice` + Chitin 注册路径
   - services 补 MSI-X/IRQ（NVMe B07）+ I-42 中断路径
   - framework `storage_init`（x86_64 大函数：PCI 扫描 + MSI-X + 验收钩子）整体退位
   - 接线 + QEMU 存储冒烟 + audit_services_boundary
2. **方案 C（强行接线）排除**：丢 MSI-X/I-42/ATA PIO/验收钩子，违反"不损失功能"原则。
3. **主线程行（不阻塞）**：§7 反向依赖治理（核心目标，独立于 driver 双份）；§6.2 复核。
4. **char/virtio-blk 同步前置核实**（同 storage 标准：MSI-X/IRQ/_block 适配器/注册路径等值存在？）——半成品 → 转独立专项；完整 → 按 DECISION-G 直接接线。

**状态**: [X]（裁决完成；storage 专项另立，主线转 §7 + §6.2 复核）

### DECISION-I: virtio-blk IRQ + §7 施工顺序（长期最优，2026-09-12 审核员裁决）

1. **virtio-blk IRQ**：**转专项补 I-42 路径**（services 补中断驱动 + framework 留 virtqueue 机制），**不接受轮询为功能等值**——轮询 CPU 占用/延迟不等值；丢 IRQ = 降级迁移，违反"不损失功能"铁律；与 storage 缺 MSI-X 转专项同一标准；Asterinas virtio-blk 中断驱动在 kernel。轮询仅作专项完成前过渡兜底，不作终态。
2. **§7 施工顺序**：采纳**先 §6.5 壳删除（分批，每批双架构 0w0e + audit_services_boundary + host-tests）→ 再 trait 注入**。理由：壳删 -70 反向依赖清障、验证 services 顶层 API 完备、为 IpcStrategy 注册时序设计提供干净依赖面。**IpcStrategy trait 注入为 trait 化首战**（ipc 24 处最大头，13 处 FFI 集中）；注册时序（framework 机制先启 → services 注册 → 使用）单独评审。

**状态**: [X]（裁决完成）

### DECISION-J: 壳删除 = 所有权反转路径（机制项迁回 framework，2026-09-12 审核员裁决）

> **障碍实证**（委托人）：§6.5 壳非纯机械删除——壳存在 = framework 生产代码持有 services 定义的类型/常量（`framework/ipc/mod.rs:89` 的 `IPC_NAMESPACE: RacyCell<IpcNamespace>` 持有 services 类型；`framework::config::{PAGE_SIZE, MAX_CPUS}` 被 iobuf/cpu_local/smp/rcu/irq/mm 机制大量使用）。

**裁决**：
1. **壳删除路径 = DECISION-A 式所有权反转**：机制项（类型/常量）迁回 framework，services 侧改 `pub use framework::...::*` 保持 API 兼容，再删 framework 壳、清理引用。
2. **统一判据**：机制持有的数据结构/常量归 framework，功能实现归 services——与 DECISION-A（VFS 下沉）不矛盾，是同一判据的两面。
3. **边界（防 B09-12 治标重演）**：
   - 反转对象仅限机制项（framework 机制持有/使用的类型/常量）；
   - 策略逻辑禁止随迁（services 的 sem/msgq/pipe 策略权威保持 services）；
   - 依赖闭包检查（迁回类型依赖的 services 项一并处理，防迁一半）；
   - services re-export 保持 API 兼容，内部引用路径同步。
4. **第一批（ipc 类型反转）通过**：`IpcNamespace`/`Pipe`/`MsgQueue`/`ShmSegment`/`Semaphore`/`Message`/`WaitQueue`/`IPC_MAX_*` 迁回 `framework/ipc/types.rs`（~500 行）——衔接 DECISION-I（ipc 24 处第一优先 + IpcStrategy 首战清依赖面）。
5. **每批验证**：双架构 0w0e + audit_services_boundary 0 + host-tests + `kernel::services` 引用计数下降。

**状态**: [X]（裁决完成）

### DECISION-J 第二批执行记录：config 常量反转（memory + capacity）

> 依据 DECISION-J 统一判据（机制持有的常量归 framework）：`framework::config::{PAGE_SIZE, MAX_CPUS}` 等被 framework arch/mm/smp/cpu_local/rcu/irq 机制直接消费，属机制常量，迁回；services 侧改 re-export 保 API 兼容。本次为第二批发货（第一批 = ipc 类型，3519410e）。

- **迁回 framework**：`framework/config/memory.rs` 由 re-export 壳改为真实 20 项常量定义（PAGE_SIZE/PAGE_SHIFT/HUGE_PAGE_{2M,1G}_{SIZE,SHIFT}/USER_STACK_{SIZE,GUARD,TOP,MAX_SIZE}/USER_KSTACK_SIZE/USER_CODE_BASE/ASLR_{STACK,MMAP,HEAP,PIE}_BITS/USER_{MMAP,HEAP,PIE}_BASE/KERNEL_STACK_SIZE）+ 保留既有 ASLR 运行时函数与 kernel_test；`framework/config/capacity.rs` 由 re-export 壳改为真实 8 项常量定义（MAX_CPUS/MAX_IRQS/MAX_PROCESSES/MAX_THREADS/MAX_THREADS_PER_PROCESS/MAX_OPEN_FILES/MAX_SESSIONS）。均 0 unsafe，依赖闭包为空（纯常量）。
- **services 改 re-export**：`services/config/{memory,capacity}.rs` 改为从 `framework::config` **顶层**显式 re-export（`pub use crate::kernel::framework::config::{PAGE_SIZE, ...}`）。**关键约束**：framework/config 的 memory/capacity 子模块为私有（`mod capacity;`），services 无法路径访问 → 必须经 framework/config/mod.rs 既有顶层 re-export（L75-L85）转发；与 ipc 的 `pub mod types` 可直接 glob re-export 不同——两种 re-export 模式差异已确立。
- **策略逻辑未随迁**：ASLR 运行时函数本就保留在 framework/config/memory.rs；services 侧无策略逻辑需迁。
- **引用计数**：framework 文件级反向依赖 79→76、精确行数 137→135（memory/capacity 两壳引用消除；config 下仍余 8 壳待第三批：boot_image/caps/error/kaslr/procfs/sched/slab/validate）。
- **验证**：双架构 0w0e ✅ / clippy -D warnings 双架构 0 ✅ / audit.sh 核心审计全过（services 0 unsafe、6 不变式 PASS、注释/C 命名 0 违规）✅ / host-tests 全量通过 ✅ / QEMU 未跑（纯常量归位不触 boot，与下一批壳删除合并冒烟）。

### DECISION-J 第三批执行记录：config 其余 8 壳（caps/kaslr/sched/slab/boot_image 反转 + error/procfs 删壳 + validate 保留）

> 逐项调研 framework 机制消费点后按统一判据处理。**关键调研结论**：SCHED_\* 与 CFS_\* 分属不同消费方（前者被 framework proc 机制消费、后者仅 services sched_policy 消费）→ 拆分归属；KernelCapabilities 被 framework mm/vmm_x86_64 KPTI 决策直接消费 → 机制类型。

- **迁回 framework（5 壳真实定义 + services re-export）**：
  - `config/caps.rs`：`ConfigSummary`/`KernelCapabilities`(+`detect`) 迁回（0 unsafe，依赖闭包为空）；services re-export。
  - `config/kaslr.rs`：`KASLR_*` 常量 + `KASLR_BASE_OFFSET` 全局状态 + `set/get/is_aligned` 迁回（机制持有全局状态）；`validate_kaslr_offset`（启动自检，返回 services `KernelError`）**留 services**——注意 framework 顶层以 `is_kaslr_aligned` 别名暴露 `is_aligned`，services 侧用 `is_kaslr_aligned as is_aligned` 还原名称保 API 兼容。
  - `config/sched.rs`：**拆分**——`SCHED_*`（6 项，被 framework proc user_proc/scheduler_ex 消费）迁回；`CFS_*`（7 项，仅 services proc/sched_policy 消费）留 services；services/config/sched.rs 变混合（CFS_* 定义 + SCHED_* re-export），sched_policy.rs 导入改 services::config。
  - `config/slab.rs`：`SLAB_*` 4 常量迁回（被 framework mm/slab 机制消费），`SLAB_DEFAULT_SIZE` 改引用 framework 自身 `PAGE_SIZE`。
  - `config/boot_image.rs`：`encode_boot_image`/`read_boot_image`/`BOOT_IMAGE`/`encoded_len` 全部迁回（被 framework config::init() 机制消费，依赖闭包 `get_config_summary`+`IrqSpinLock` 全在 framework 内）；services glob re-export。
- **删除壳（services 独有策略项，调用点改 services 路径）**：
  - `config/error.rs`：`ConfigError` 为 services validate 策略返回类型，framework 生产代码不消费 → **删壳**；validate.rs/tests 改 `services::config::ConfigError`。
  - `config/procfs.rs`：`read_sys_config` 等为 /proc 用户态接口服务，framework 无生产消费 → **删壳** + 移除 `pub mod procfs`；services/fs/procfs_core.rs + framework/tests 改 `services::config::procfs` 路径。
- **保留壳（后续 trait 注入批次）**：`config/validate.rs`（`validate_system_config`/`validate_drivers` 被 framework config::init() 调用，需 ConfigValidateHook trait 注入，按 DECISION-I「先壳删 → 再 trait 注入」顺序留待 ipc 之后的批次）。
- **引用计数**：framework 文件级反向依赖 76→70、精确行数 135→132；config 目录仅剩 validate.rs 壳。
- **验证**：双架构 0w0e ✅ / clippy -D warnings 双架构 0 ✅ / audit.sh 核心审计全过（services 0 unsafe、6 不变式 PASS、SAFETY 覆盖 0 缺漏、注释/C 命名 0 违规）✅ / host-tests 全量通过 ✅ / QEMU 未跑（常量/类型归位不触 boot，与下一批壳删除合并冒烟）。

### DECISION-J 第四批执行记录：ipc 4 纯壳删除（sem/signal/scheduler_integration/async_ipc）

> 调研确认：framework 生产代码（非 cfg(test)）对 4 壳项均无消费——`sem/signal` 系统调用走 `framework::proc::do_signal_*` 与 services syscall 层，不经 `framework::ipc::{sem,signal}`；`scheduler_integration` 仅被 services 内部（msgq/sem/pipe）消费；`async_ipc` 仅 cfg async re-export。services/ipc 已完全自足（含 `IpcLock` 完整 API）。

- **删除壳**：framework/ipc/{sem,signal,scheduler_integration,async_ipc}.rs 删除；mod.rs 移除对应 `pub mod` 声明 + 顶层 re-export（`block_current_thread` 等 4 函数 + cfg async 的 `AsyncMsgSender` 等）。
- **cfg(test) 引用同步**（测试代码允许访问 services，§7.3 精神）：
  - mod.rs `mod tests` 加 `use crate::kernel::services::ipc::{sem, signal};`
  - stress_tests.rs `use ...::ipc::{msgq, pipe, sem, shm}` 补 sem
  - framework/tests/test_ipc.rs `use ...::services::ipc::{pipe, sem, shm}` 替换 framework 壳路径
  - api.rs 头注释同步（scheduler_integration/sem/signal 指向 services）
- **引用计数**：framework 文件级反向依赖 70→66、精确行数 132→129；ipc 目录生产代码反向依赖仅剩 pipe.rs(5)/shm.rs(4)/msgq.rs(4) FFI 边界 13 处——**DECISION-I IpcStrategy trait 注入对象**（下一批）。
- **验证**：双架构 0w0e ✅ / clippy -D warnings 双架构 0 ✅ / audit.sh 核心审计全过（含 kernel_test/host-test clippy 维）✅ / host-tests 97 套件全通过 ✅ / QEMU 未跑（纯壳删除不触 boot，与 trait 注入批次合并冒烟）。

### DECISION-I 首战执行记录：IpcStrategy trait 注入（ipc FFI 13 处收敛）

> DECISION-I 裁决："IpcStrategy trait 注入为 trait 化首战（ipc 24 处最大头，13 处 FFI 集中）；注册时序单独评审"。本次完成 trait 注入主体，注册时序设计留待评审（见下）。

- **framework 侧（契约 + 注册点）**：新增 `framework/ipc/strategy.rs` —— `IpcStrategy` trait（13 方法：pipe×5 / shm×4 / msgq×4，签名对齐 services `*_safe`，引用 framework 类型 `IpcNamespace`/`IpcId`）+ `static IPC_STRATEGY: OnceLock<&dyn IpcStrategy>` + `register_ipc_strategy()` + `current_ipc_strategy()`（**无内建回退**：策略方法依赖 services 实现，framework 无法安全回退，未注册即调用 panic——与 `services::ipc::global()` 的 expect 契约一致，IPC FFI 仅在 syscall 时触发）。mod.rs 顶层 re-export trait + 注册/获取入口。
- **services 侧（实现 + 注册）**：新增 `services/ipc/strategy.rs` —— `DefaultIpcStrategy` impl（包装 `pipe/shm/msgq` 的 `*_safe`，保持 T6 权威）+ `register_default_ipc_strategy()`（幂等，`let _ =` 风格，`#![deny(unsafe_code)]`）。
- **FFI 边界改造**：pipe.rs(5)/shm.rs(4)/msgq.rs(4) 共 13 处 `crate::kernel::services::ipc::*::*_safe` 改经 `current_ipc_strategy().*` 调用，framework→services 直接引用归零（ipc 生产代码）。
- **注册时序（待评审，DECISION-I 要求单独评审）**：lib.rs kernel_init 编排中、VFS init 后 / UDS 前插入 `register_default_ipc_strategy().expect(...)`。时序论证：framework ipc 机制（IPC_NAMESPACE）由启动早期初始化 → services 注册在 scheduler 之后 → FFI 仅在用户态 IPC syscall 时触发（用户态启动远晚于注册点）。**评审点**：注册是否应更贴近 framework ipc_init 时机（更早）？`current_ipc_strategy()` 未注册 panic 是否可接受（vs 返回 Err）？
- **引用计数**：framework 文件级反向依赖 66→63、精确行数 129→116（ipc FFI 13 处消除）。
- **验证**：双架构 0w0e ✅ / clippy -D warnings 双架构 0 ✅ / audit.sh 核心审计全过 ✅ / host-tests 97 套件全通过 ✅ / QEMU 未跑（本次含 lib.rs 启动编排改动，QEMU 冒烟与后续批次合并执行）。

### DECISION-J 第五批执行记录：sync/types 反转

> 首批"机制类型壳"反转（剩余壳多为同构，模式已确立：机制消费→反转归位，services 独有→删壳）。调研确认：`SpinLockInner`/`MutexInner`/`RwLockInner`/RAII 守卫/IrqSaveFlags 被 framework sync 机制（spinlock/rwlock/mutex FFI 层）直接消费，且 `#[repr(C)]` 与 C 版本布局兼容 — 属机制类型。

- **迁回 framework**：`framework/sync/types.rs` 由 re-export 壳改为真实定义（LockState/TryLockResult/SpinLockInner/MutexInner/RwLockInner/CondVarInner/IrqSaveFlags/LockStatistics + 5 个 RAII 守卫，0 unsafe，依赖闭包为空）。mod.rs 顶层既有 `pub use types::{...}` 现解析到 framework 自身。
- **services 改 re-export**：`services/sync/types.rs` 改为 `pub use crate::kernel::framework::sync::types::*`（framework/sync/types 为 `pub mod`，glob 可行，同 ipc/types 模式）。services/sync/mod.rs 的显式 re-export 不变（解析到 framework 项）。
- **引用计数**：framework 文件级反向依赖 63→62。
- **验证**：双架构 0w0e ✅ / clippy -D warnings 双架构 0 ✅ / audit.sh 核心审计全过 ✅ / host-tests 全量通过 ✅ / QEMU 未跑（纯类型归位不触 boot，合并冒烟）。

### DECISION-J 第六批执行记录：机制常量/状态 4 壳反转（net/types + barrier/reset_config + mm/numa + io/iouring）

> 批量处理同构"机制持有项"壳。逐项调研 framework 消费点后判定：4 项均被 framework 机制直接消费且依赖闭包全在 framework 内 → 反转归位。模式与第五批相同（cp + 头部 DECISION-J 注释 + services glob re-export）。

- **net/types**：`NET_READY`/`NET_CONFIGURED` 全局状态 + `FALLBACK_*` 常量迁回（被 framework net init/dns 机制消费）。services glob re-export。**host-test 同步**：`dhcp_fallback_const_test.rs` 的 `TYPES_RS` 路径 `services/net/types.rs` → `framework/net/types.rs`（跨模块接口变更，§8）。
- **barrier/reset_config**：`RecoveryLayer`/`RecoveryResult`/`set_reset_*` 等恢复配置迁回（被 framework proc/scheduler Barrier 恢复域路径消费，0 外部依赖）。services glob re-export（framework 路径 `barrier::reset::config`）。
- **mm/numa**：`NumaTopology`/`NumaNode`/`NumaMempolicy`/`sys_*` 迁回（`NumaMempolicy` 被 framework proc/process 持有、`numa_init` 被 framework mm 调用；依赖闭包 `mm::PAGE_SIZE`+`sync::IrqSpinLock` 在 framework 内）。services glob re-export（services/syscall 的 4 处 `numa::sys_*` 引用经 glob 保持可用，无需改）。
- **io/iouring**：`Sqe`/`Cqe`/`RingBuffer`/`IoUring` + `sys_io_uring_*` 迁回（被 framework syscall dispatch 直接调用；依赖闭包 `sync::IrqSpinLock`+`errno::Errno` 在 framework 内）。services glob re-export。
- **引用计数**：framework 文件级反向依赖 62→58。
- **验证**：双架构 0w0e ✅ / clippy -D warnings 双架构 0 ✅ / audit.sh 核心审计全过 ✅ / host-tests 97 套件全通过 ✅ / QEMU 未跑（纯常量/类型归位不触 boot，合并冒烟）。

### 前置核实执行记录（步骤 1，2026-09-12）

> 对 11 项 services 影子逐项核实"内容自足性"（0 unsafe / 硬件经 IoMem/IoPort/DmaStream/Chitin 机制 API / 业务自含不依赖 framework 内部）：

| 子类 | 项 | 核实结果 | 结论 |
|---|---|---|---|
| char | serial.rs | ✅ 0 unsafe；PIO 全经 `framework::ioport::IoPort`（new_safe）；业务自含 | **直接接线** |
| char | vga.rs | ✅ 0 unsafe；MMIO/PIO 经 `IoMem::from_pci_bar` + `IoPort::new_safe`；业务自含 | **直接接线** |
| virtio | blk.rs / net.rs | ⚠ 依赖 `framework::driver::virtio::queue::{DmaBuffer, VirtQueue}`（DMA 环机制，合法机制依赖）；blk/net 业务待完整核实 | 待续核 |
| storage | nvme.rs | ⚠ 依赖 `fw_nvme::NvmeCommand/Completion`（wire 类型，机制可留）+ `fw_storage::nvme_read_identify_*`（解析 helper，业务）→ 解析 helper 需迁 services | **从 framework 迁业务**（解析 helper） |
| storage | ahci.rs / ata.rs / mod.rs | 待核 | 待续核 |

**接线改造的关键耦合点（步骤 2/3 设计确认）**：
- Chitin 注册安全路径 = `chitin_register_driver(name, proto, io_base, irq, Box<dyn Driver>)`，`Driver` trait **全 safe 方法**（framework/driver/framework.rs:287）→ **services 可 0 unsafe impl Driver 并注册**（合法方向）。
- 但**读写路径** `ChitinOps::Char(&CharOps)` 是 `extern "C" fn(driver_data: *mut u8, ...)` 指针表（framework/chitin/proto_char.rs:11-24），实现体需 unsafe 指针转换（framework 侧 serial.rs:687/718）→ **services 0 unsafe 无法直接构造** → 需 framework 提供**安全桥 trait**（framework 定义 `CharDeviceOps` + 构造 CharOps 的机制函数，unsafe 转换留在 framework；services impl trait）——即 §7.4 trait 注入模式，与审核员裁决一致。
- 接线编排：crate root `src/rust/src/lib.rs` 是合法双向编排者（L763 调 `framework::driver::init_all()`、L833 调 `services::syscall::init()`）→ char_init 迁至 services 后由 lib.rs 调用，framework init_all 移除 char 项。

### char 子类接线实施记录（步骤 4 首批，commit 9ba997e3）

- **services 侧（新增权威）**：`services/driver/char/serial.rs` + `vga.rs` 各加 `impl Driver`（name/device_type/init/shutdown 全 safe）；`services/driver/char/mod.rs` 新增 `char_init()`（x86_64）将 VgaConsole + COM1 SerialPort 注册进 Chitin（`chitin_register_driver`，合法方向）。
- **framework 退位**：删除 `framework/driver/char/{serial,vga}.rs`；`char/mod.rs` 仅保留 aarch64 pl011；`driver/mod.rs` 移除 char serial/vga 顶层 re-export（VgaColor/SerialPort/BaudRate/RingBuffer 等）与 init_all 中 x86_64 char 项。
- **接线**：`lib.rs` init_all 后新增 `#[cfg(x86_64)] services::driver::char::char_init()`。
- **测试同步**：framework/tests/driver.rs 移除 serial 测试块（framework serial 已删，纯逻辑测试迁 host-tests 为后续项）；tests/driver_test.rs 的 vga/serial 输出改走 services VgaConsole/SerialPort（§7.3 允许 framework/tests 访问 services）。
- **SIMPLIFIED（已登记）**：注册走 `chitin_register_driver` 无 CharOps 读写绑定——Chitin char 读写路径当前无生产消费者（休眠）；待 devfs char 读写接入时按 §6.2 补 framework 安全桥 trait。
- **验证**：双架构 0w0e ✅ / clippy -D pedantic 双架构 0 ✅ / 核心审计全 0 ✅ / host-tests 全量通过 ✅（fs_permissions_regression_test 单跑 28s 通过，为慢二进制非挂起）。

### virtio 子类接线实施记录（步骤 4 第二批，commit e47c04ad）

> 验证：双架构 0w0e ✅ / clippy -D pedantic 双架构 0 ✅ / 核心审计全 0 ✅ / host-tests 全量通过 ✅ / QEMU x86_64 完整启动到 Ring 3（blk_init 探测 0 设备干净跳过，Chitin 计数不变）。

- **前置核实**：services `virtio/blk.rs` 自足（0 unsafe，经 `transport::VirtioDevice` 安全代理 + framework `queue::{DmaBuffer,VirtQueue}` DMA 机制；完整 I/O 路径）→ **直接接线**。
- **services 侧（新增权威）**：`services/driver/virtio/blk.rs` 新增 `finalize()`（vq0 MMIO 配置 + DRIVER_OK，等价 framework `VirtioBlk::new` 收尾）+ `impl BlockDevice`（blk_read/write/is_present/total_sectors，IoMem/VirtQueue 均为 framework unsafe Send+Sync → 0 unsafe 可实现）；`services/driver/virtio/mod.rs` 新增 `blk_init()`（探测 virtio-mmio 区域，为块设备建 `VirtioBlkDriver`，finalize 后经 `proto_block::register_block_device` 注册）。
- **framework 退位**：删除 `framework/driver/virtio/blk.rs`；`virtio/mod.rs` 移除 `pub mod blk`（保留 `VirtioMmioDevice` 传输机制 + `queue` DMA 环机制）；**aarch64 `storage_init` 变空操作**（原 virtio-blk 探测注册迁 services；x86_64 storage_init 走 PCI AHCI/NVMe 不受影响；klog imports + 旧 #[expect] 随之 cfg 门控/清理）。
- **接线**：`lib.rs` init_all 后新增 `services::driver::virtio::blk_init()`（双架构；HvFS 块扫描之前，x86_64 无 virtio-mmio 即跳过）。
- **SIMPLIFIED（已登记）**：services blk 走 spin-loop 轮询（framework 版有 I-42 IRQ 事件驱动路径），功能等价、效率略低；IRQ 驱动为后续优化项。
- **待登记**：services `transport::VirtioDevice` 与 framework `VirtioMmioDevice` 存在**传输层双份**（各自 IoMem 探测）——按服务对象准则 transport 属机制应保留 framework，services 版是否删除/改为薄代理留待 §7 反向依赖治理阶段裁决。

### char/virtio-blk 前置核实结果（同 storage 标准，回报裁决，2026-09-12）

> 审核员要求：按 storage 标准核实 char/virtio-blk 的 services 实现是否"IRQ 路径 / _block 适配器 / 注册路径等值存在"，结果回报后定"直接接线 or 转专项"。

| 子项 | IRQ 路径 | 适配器/注册路径 | 结论 |
|---|---|---|---|
| char（vga/serial）| **不适用**——vga/serial 为轮询 console，framework 原实现同样无 IRQ（串口轮询）；services 无缺失 | 注册路径已建：`char_init` → Chitin ✓（9ba997e3）| ✅ **直接接线成立** |
| virtio-blk | **缺失**——framework VirtioBlk 有 I-42 `enable_irq`（IRQ 事件驱动完成），services VirtioBlkDriver 仅 `ack_interrupt`（清中断状态），I/O 走 spin-loop 轮询 | 适配器已建：`impl BlockDevice` ✓ + 注册路径 `blk_init` ✓（e47c04ad）| ⚠ **按严格标准 IRQ 路径不等值** |

**裁决请求**：virtio-blk 的 IRQ 路径（I-42）是否必须迁 services（→ 转专项，补 IRQ 完成路径）？还是接受 spin-loop 轮询为"功能等值、效率略低"（维持直接接线，SIMPLIFIED 已登记为后续优化项）？framework virtio-net 亦无 IRQ（同为轮询）可作旁证。

### storage 前置核实记录（步骤 1 续核，2026-09-12）

> 按 DECISION-G 规则核实 services storage 内容自足性，结论：**services storage 为"并行实现但内容不等价"（Phase 2.1.3/2.1.4 迁移半成品）——不满足"直接接线"，属"从 framework 迁业务"（B 形态），且迁移量显著大于 char/virtio**。

| 子项 | 核实结果 | 结论 |
|---|---|---|
| services nvme.rs | 依赖 framework C-FFI 机制（`nvme_submit_*_cmd`/`nvme_alloc_*`/`nvme_copy_*`，DMA/队列提交=机制留 framework 合理）+ `fw_nvme::NvmeCommand/Completion`（wire 类型）+ `fw_storage::nvme_read_identify_*`（解析 helper=业务，需迁 services）；**缺 MSI-X/IRQ 路径**（framework 版有 enable_msix + I-42 事件驱动） | 迁业务：identify helper + MSI-X |
| services ahci.rs | 仅依赖 IoMem/PhysAddr（自足 ✓）；**缺 _block 适配器与接线**（framework ahci_block.rs 是 active 注册） | 迁业务：_block 适配器 + 接线 |
| services ata.rs | **桩模块**（AtaController 极简）；framework ata.rs 是真实 PIO 驱动（C-FFI + BlockDevice 适配） | 迁业务：真实 ATA 驱动 |
| services storage 整体 | **无 `impl BlockDevice`、无 `impl Driver`、无 init/注册入口**——控制器实现（队列/identify/I/O）与注册路径（_block 适配器）分离，注册全在 framework | 需补齐注册路径 |
| framework storage_init | x86_64 巨大函数（PCI 扫描 + AHCI/NVMe 创建 + **MSI-X 接入** + MSIX-03 测试钩子 + I-42）；aarch64 已空操作（virtio-blk 迁出） | 退位后 x86_64 需迁出业务 |

**风险提示**：storage 下沉若操之过急将**丢失功能**——MSI-X 中断驱动 NVMe（B07）、I-42 IRQ 路径、ATA PIO 真实驱动、MSIX-03 测试钩子均在 framework 侧且 services 无等价实现。**建议作为独立专项工程推进**（子步：identify helper 迁 services → services 补 _block 适配器 + MSI-X → 接线 → QEMU 存储冒烟），或与 §6.2/§7 并行规划。

### storage 专项 0 号子步实施记录（identify 解析迁 services，commit 05c9a648）

> 审核员裁决（2026-09-12）：storage 走 A 独立专项；0 号子步 = `nvme_read_identify_*` 解析 helper 迁 services（纯逻辑、可 host 测试、零风险）。

- **迁出（framework 删业务）**：删除 `framework/driver/storage/mod.rs` 的 `nvme_read_identify_controller` / `nvme_read_identify_namespace`（volatile 裸读 + 解析，-2 unsafe 块）。
- **迁入（services 纯函数）**：`services/driver/storage/nvme.rs` 新增 `parse_identify_controller(data: &[u8]) -> Option<(u32, [u8; 40])>`（nn@516 LE u32 + mn@24 40B）与 `parse_identify_namespace(data: &[u8]) -> Option<(u64, u8, u32)>`（nsze@0 LE u64 + flbas@26 + lbaf_data@128+idx*4；长度检查 192B 覆盖 LBA 表末项）。输入从裸 vaddr 改为字节切片 → 0 unsafe。
- **调用点改造**：`identify_controller` / `identify_namespace` 经 `nvme_copy_from_dma` 拷 DMA 字节到栈数组（520/192）后调用纯解析（与既有 read 路径一致，plain-copy 读 DMA）。
- **边界修复（测试驱动发现）**：原 framework 实现 lbaf_idx=15 时读 offset 188..192 但无界检查——services 版显式要求 192B，杜绝越界 panic。
- **host 测试（实际运行）**：新增 `host-tests/tests/storage_identify_parse_host_test.rs`（7 用例：controller 全解析/短缓冲/空缓冲 + namespace 全解析/lbaf_idx 15/lbaf 越表/短缓冲）直接 import 内核真实函数（B08-12 路线 C）→ **全部通过**。services nvme.rs 内 cfg(test) 同语义单测同步补齐（该文件既有 dormant 单测风格，kernel_test 构建下不可直接 cargo test——build-std 冲突，实测 `cargo test -p queenx` 失败，属既有工程问题另议）。
- **验证**：双架构 0w0e ✅ / clippy -D pedantic 双架构 0 ✅ / 核心审计全 0 ✅ / host-tests 全量 ✅ / QEMU x86_64 启动 ✅。

> 验证：双架构 0w0e ✅ / clippy -D pedantic 双架构 0 ✅ / 核心审计全 0 ✅ / host-tests 全量通过 ✅（RX 为硬件路径 host 无法功能测试，纯逻辑按 framework 同构迁移，实际验证依赖后续 aarch64/QEMU virt 冒烟）。

- **前置核实结论**（DECISION-G 规则）：services `VirtioNetDriver` 的 **RX 数据路径是半成品**——`try_receive` 原实现"无法访问 DMA 缓冲区内容，返回 0 丢弃包，需维护 desc_idx→DmaBuffer 映射"（L453-475）→ 不满足"内容完整"，**该项属"从 framework 迁业务"（仍是 B 形态）**。
- **RX 迁业务（已完成，0 unsafe）**：`services/driver/virtio/net.rs` 新增 `rx_buffers: [Option<DmaBuffer>; 32]`（desc_idx==槽位索引，同 framework `rx_buffers` 同构）+ `refill_rx()`（逐槽分配 DmaBuffer + 提交设备可写描述符）+ 重写 `try_receive`（pop used → 槽位校验 → `read_slice` 拷贝有效载荷 → 回收 + 同槽重提交）+ `refill_single_rx`（复用槽位缓冲区，取代原 `mem::forget` 泄漏式重填）。`new()` 初始化后预填 RX。
- **接线待办（未做，需 framework 安全桥）**：framework net init `nic_probe_all`（net/init/probe.rs:75）经 `ChitinNetDevice` + `VIRTIO_NET_OPS_STATIC`（extern "C" NetOps 指针表，unsafe 转换到 framework `VirtioNet`）接入 smoltcp——services 0 unsafe 无法直接构造 NetOps，需 **framework NetOps 安全桥 trait**（同 char CharOps 桥模式）。且 net init 为机制，services 设备需经注册表分发。
- **验证**：双架构 0w0e ✅ / clippy -D pedantic 双架构 0 ✅ / 核心审计待跑 / host-tests 待跑（RX 为硬件路径，host 无法功能测试——纯逻辑已按 framework 同构迁移，实际验证依赖后续 aarch64/QEMU virt 冒烟）。

### 阶段 1 首批（syscall brk/canary/posix_timer 下沉）验证结果

| 门槛 | 结果 |
|---|---|
| 双架构 cargo check --release 0w0e | ✅ x86_64 + aarch64（RUSTFLAGS=-D warnings） |
| clippy -D pedantic 0（除 cast_*） | ✅ x86_64 + aarch64 |
| 核心审计 | ✅ boundary 0 / safety 100% / coupling 0 / comment 0 / deadlock 0 / invariants 全 PASS |
| host-tests | ✅ 全过（exit 0）——首次全量并行 `test_fsx_stress` 偶发失败（单独跑 33s 通过，全量重跑通过），判定为并行负载 flaky，非本次改动引入 |

> 备注：`scripts/audit_coupling.py` 中 `framework::syscall::{brk,canary,posix_timer}` 检测模式随文件删除失效（不再匹配），无害保留。
