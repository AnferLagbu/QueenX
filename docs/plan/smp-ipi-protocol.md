# SMP IPI 协议工程（0xFD TLB shootdown / 0xFE reschedule）

> **定位**：独立工程。2 核 boot 打通后，单核时代恒不执行的 IPI 路径**首次真正进入执行面**，须补全 IPI 协议（IDT 门 + 处理程序 + 定向投递 + 完成确认 + 帧释放顺序）。本工程不属分册 8/9 内容，独立成档。
>
> **目标**：使 IPI 在 2 核及以上（含 3/4 核）配置下**不崩溃、不挂死、且语义正确**——远程 TLB 失效带完成确认，被失效帧不得在确认前被重新分配。
>
> **来源**：分册 9 ledger 的 D-9-3 / D-9-5 登记项（[audit-fix-09-hard-rules-deadcode.md](audit-fix-09-hard-rules-deadcode.md)），因涉及协议设计与锁纪律变更，提升为独立工程。
>
> **关联**：D-9-3（`0xFD`/`0xFE` 无 IDT 门）/ D-9-5（跨核 TLB shootdown 门控首次生效）/ D-9-7（aarch64 SMP 不对称）/ D-8（TLB shootdown 锁序论证前提失实）。
>
> **文件状态**：本文件的 **S-3 / S-6 / S-7 中依赖 ack 计数的部分**已被 [tlb-shootdown-epoch.md](./tlb-shootdown-epoch.md)（代计数路线）**取代**，非增量 —— 新路线不建 ack 计数协议、不做"临界区外等 ack"、不设定长 deferred 缓冲；**S-5 的"定向投递"能力不废止**，由新路线的 `tlb_gen_publish_and_shoot` 承接。骨架类条目（S-1 `0xFD`/`0xFE` 的 stub 与 IDT 门、S-2 `handle_irq` 前置分支、S-4 `0xFE` 接线、S-8 aarch64 SGI 分支、S-9 参数化）已在工作区落地并沿用。台账 **D-9-3 / D-9-5 / D-9-7 现指向 [tlb-shootdown-epoch.md](./tlb-shootdown-epoch.md)**。

## 1. 决策记录

| 决策点 | 选择 | 说明 |
|---|---|---|
| D1 修复深度 | **定向 cpumask shootdown + ack 完成栅栏 + aarch64 SGI 同语义** | 用户裁定；要求 2+ 核（含 3/4 核）均可工作，不得只硬编码 2 核 |
| D2 ack 栅栏落点 | **shootdown 移出 `VMM_LOCK` 临界区** | 原设想「临界区内等 ack」与本锁纪律冲突（见 §2.3），改为释放锁后广播 + 等 ack |
| D3 帧释放顺序 | **deferred-free** | 临界区内只收集待释放帧，shootdown + ack 完成后才真正 `free_page` |
| D4 aarch64 对齐 | **本轮一并实现 SGI 处理分支** | 与 `0xFD`/`0xFE` 同语义；aarch64 无 AP 上线路径，本轮无运行验证载体（见 §5.3） |
| D5 远程失效粒度 | **远程 `tlb_flush_all`（重载 CR3），不做地址载荷数组** | 简约实现；理由见 S-6 详情 |

## 2. 调研结论（源码依据，均已核实行号）

### 2.1 发送侧

- 唯一发送面：[smp/mod.rs:74-96](../../src/kernel/framework/smp/mod.rs#L74-L96)——`send_tlb_invalidate_ipi`（`0xFD`）、`broadcast_tlb_invalidate`（`0xFD`，all-excluding-self）、`send_reschedule_ipi`（`0xFE`）、`broadcast_reschedule`（`0xFE`）。
- 另一发送点：[sync/rcu.rs:183](../../src/kernel/framework/sync/rcu.rs#L183) 调 `send_reschedule_ipi`；[proc/cpu_queue.rs:111](../../src/kernel/framework/proc/cpu_queue.rs#L111) 直接 `arch!(send_ipi(target, 0xFE))`。
- `apic::send_ipi` 为固定投递、无 shorthand：[apic.rs:153-160](../../src/kernel/framework/arch/x86_64/apic.rs#L153-L160)；`apic::broadcast_ipi` 用 `ICR_ALL_EXCLUDE_SELF`：[apic.rs:164-174](../../src/kernel/framework/arch/x86_64/apic.rs#L164-L174)。
- 定向投递所需的 CPU 索引↔APIC ID 映射已具备：`CPU_APIC_IDS`/`CPU_ONLINE`/`get_apic_id`/`get_cpu_count`/`get_current_cpu`（[smp/mod.rs:11-72](../../src/kernel/framework/smp/mod.rs#L11-L72)）。

### 2.2 接收侧缺口

- **`0xFD`/`0xFE` 无 stub**：[isr.asm:51-58](../../src/kernel/framework/boot/isr.asm#L51-L58) 的 `%macro irq_stub 2`（N=符号后缀、V=入栈向量），实例化区间 [isr.asm:402-532](../../src/kernel/framework/boot/isr.asm#L402-L532) 为 `irq_stub 0,32` … `irq_stub 127,175`，即符号 `irq0-irq127`、入栈向量 `0x20-0xAF`（其中仅 `0x20-0x2F` 与 `0x40-0x9F` 注册了门）；无 `0xFD`/`0xFE`。
- **门注册点**：[idt.rs:273-379](../../src/kernel/framework/idt/idt.rs#L273-L379) 的 `init()`（0-31 + 32-47 + `0x80` + `0x82`）与 `init_msi_idt()`（`0x40-0x9F`）。
- **越界崩溃（关键）**：[idt.rs:725](../../src/kernel/framework/idt/idt.rs#L725) `let irq = vector - IRQ_BASE;` → `0xFD` 得 221；[idt.rs:736](../../src/kernel/framework/idt/idt.rs#L736) `state.irq_descriptors[irq as usize].handler` 数组仅 `[IrqDescriptor; 128]`（[idt.rs:161](../../src/kernel/framework/idt/idt.rs#L161)）⇒ 中断上下文 panic。**仅补 IDT 门不够，必须前置 IPI 分支**。
- **`0xFE` 处理程序早已存在但零调用**：[proc/cpu_queue.rs:118-121](../../src/kernel/framework/proc/cpu_queue.rs#L118-L121) `resched_ipi_handler()`（`#[unsafe(no_mangle)] pub extern "C"`，内部 `raise_softirq(Sched)`），全仓库仅定义无调用点 ⇒ 本工程把它接成真实调用路径。
- 本地 TLB 刷封装：[mm/arch.rs:26-38](../../src/kernel/framework/mm/arch.rs#L26-L38) `tlb_flush_page`/`tlb_flush_all`（x86_64 实现见 [arch/x86_64/mod.rs:398-426](../../src/kernel/framework/arch/x86_64/mod.rs#L398-L426)）。
- EOI：`send_eoi` 已优先走 LAPIC `apic::eoi()`（[idt.rs:835-862](../../src/kernel/framework/idt/idt.rs#L835-L862)、[apic.rs:145-149](../../src/kernel/framework/arch/x86_64/apic.rs#L145-L149)）。

### 2.3 锁纪律冲突（推翻「临界区内等 ack」）

- `acquire_lock` **先 `disable_interrupts()` 再自旋 CAS**：[vmm_x86_64.rs:1925-1932](../../src/kernel/framework/mm/vmm_x86_64.rs#L1925-L1932)。
- `flush_tlb` 的全部 20 处调用点（[vmm_x86_64.rs:266,279,284,353,373,385,571,943,1030,1093,1107,1113,1310,1373,1422,1614,1647,1689,1701](../../src/kernel/framework/mm/vmm_x86_64.rs#L266)）都在 `acquire_lock`…`release_lock` 之间（例：[unmap_page](../../src/kernel/framework/mm/vmm_x86_64.rs#L215-L290)）。
- ⇒ 核 A 持锁（IF=0）发 IPI 等 ack，核 B 正自旋等锁（IF=0）**永远收不到该 IPI**，双向自旋 = 结构性死锁（两核争 `VMM_LOCK` + 一方 unmap 即触发，属高概率路径）。故 D2 选「移出临界区」。
- 推论（写入施工要求）：**任何等 ack 的循环必须保持 IF=1**，否则两核互 shootdown 会互等死锁。
- **代计数路线下的更新**（见 [tlb-shootdown-epoch.md](./tlb-shootdown-epoch.md)）：新路线**不引入任何 ack 等待**——发送侧"发布新代 + 定向 IPI（含本核）"后立即返回，完成判定改为"全部在线核已追平代 ≥ 帧释放代"。故"等 ack 必须 IF=1"不再有适用对象，但其**同源结论仍然成立**：任何等待都不得发生在持 `VMM_LOCK` 且 IF=0 的临界区内（新路线把排空点放在 `release_lock` 出口正出于此）。新增的正确性规则是 **P1：接收侧必须"先读代 → 再全量 flush → 再声明"**，反序（flush 后读代）会"假追平"（推导见 epoch 文件 §2.3 P1）。

### 2.4 帧释放与 TLB 失效同临界区

- 同一临界区内既 `flush_tlb` 又 `pmm.free_page`：
  - [vmm_x86_64.rs:1093-1129](../../src/kernel/framework/mm/vmm_x86_64.rs#L1093-L1129)：刷 TLB 后释放 `pt`/`pd`/`pdpt` 帧（3 处）。
  - [vmm_x86_64.rs:1212-1229](../../src/kernel/framework/mm/vmm_x86_64.rs#L1212-L1229)：释放 `user_phys`/`pt`/`pd`/`pdpt`/`pml4` 帧（5 处）。
- 对照：[vmm_x86_64.rs:633](../../src/kernel/framework/mm/vmm_x86_64.rs#L633) 的 `free_page(pml4_phys)` 在 [L638 `acquire_lock`](../../src/kernel/framework/mm/vmm_x86_64.rs#L638) **之前**（失败回退路径，无同区 shootdown）⇒ **不在 deferred-free 范围**。
- ⇒ 「release → ack 完成」之间存在「已释放帧被重新分配 + 第三核仍持陈旧 TLB 映射」窗口，D3 用 deferred-free 消除。
- **代计数路线下的更新**：消除该窗口的方式由"等 ack 后释放"改为"**等全部在线核代追平后释放**"（判据 `tlb_gen_min_online() >= 帧释放代`）；容器改为**无上限的侵入式帧链表**——原"局部定长数组 + 溢出即释放"已被证伪为正确性漏洞（`destroy_page_table` 在单临界区内 defer 十万级帧，溢出路径必然触发）。见 epoch 文件 §1 D10 与 §3 S-4。

### 2.5 测试面

- `test-smp` 硬编码 `-smp 2` 并断言 `[SMP] online CPUs: 2`：[Makefile:555-579](../../Makefile#L555-L579)。
- 计数打印为真实值（[smp_init.rs:148](../../src/kernel/framework/arch/x86_64/smp_init.rs#L148) `klog_info!("[SMP] online CPUs: {}", get_cpu_count())`）⇒ 断言可安全参数化。
- 结构上限 `MAX_CPUS = 1024`（[config/capacity.rs:20](../../src/kernel/framework/config/capacity.rs#L20)）。

### 2.6 aarch64 现状

- `send_ipi` 用 `ICC_SGI1R_EL1` 单播 SGI（目标核 `& 0xF`）、`broadcast_ipi` 置 IRM：[arch/aarch64/mod.rs:199-220](../../src/kernel/framework/arch/aarch64/mod.rs#L199-L220)。
- 接收侧无 TLB/reschedule SGI 分支：[arch/aarch64/exception.rs:636-704](../../src/kernel/framework/arch/aarch64/exception.rs#L636-L704) 的 `irq_handler` 仅特判 barrier recovery SGI(7) 与 timer PPI，忽略 `intid >= 1020`。
- 既有 SGI 先例可参照：[arch/aarch64/barrier/mod.rs:32-64](../../src/kernel/framework/arch/aarch64/barrier/mod.rs#L32-L64)。
- **无 AP 上线路径**：`start_ap`/`ap_entry` 仅在 `arch/x86_64/smp_init.rs`，aarch64 无 `register_cpu` 调用点 ⇒ `SMP_ENABLED` 恒 false、`CPU_COUNT` 恒 1。

## 3. 施工条目

- **S-1. `0xFD`/`0xFE` 的 stub 与 IDT 门**
  - 描述：IPI 向量无 stub 也无门，投递即无描述符可用。
  - 方案：`boot/isr.asm` 用既有 `%macro irq_stub 2` 新增两个实例（`irq_stub 253, 0xFD` / `irq_stub 254, 0xFE`，沿用 `irq_common` 入口）；`idt/mod.rs` 的 `extern "C"` 块补 `fn irq253(); fn irq254();`；`idt.rs` 在 `init_msi_idt()` 之后注册两门（`GDT_KERNEL_CODE` + `IDT_TYPE_INTERRUPT`，DPL0、非 IST）。
  - 状态：[]
  - 详情：向量号 `0xFD`/`0xFE` 空闲——既有门占用为 `0-31`、`32-47`、`0x40-0x9F`、`0x80`、`0x82`（`0x82` 与 MSI 段 `0x80-0x9F` 重叠属既有面，不在本工程处理）。stub 符号名取 `irq253`/`irq254`（按向量号命名），避开既有 `irq0-irq127` 符号。`push 0xFD` 会按 imm8 符号扩展成 `0xFFFF…FD`，但接收侧 [idt/mod.rs:573](../../src/kernel/framework/idt/mod.rs#L573) 以 `frame_ref.int_no as u8` 截断取向量，与既有 `0xAF` 等 stub 同构，无额外风险。

- **S-2. `handle_irq` 前置 IPI 分支（消除 `irq_descriptors[128]` 越界）**
  - 描述：`irq = vector - IRQ_BASE` 对 `0xFD` 得 221，直索 128 项数组 → panic（§2.2）。
  - 方案：在 [idt.rs:725](../../src/kernel/framework/idt/idt.rs#L725) 取 `irq` 之前，按 `vector` 判定 `0xFD`/`0xFE` 并走独立分支：执行对应处理（S-3/S-4）→ 发 LAPIC EOI（`send_eoi`）→ `return`。IPI 分支**不进** `do_softirq`/信号投递路径（`0xFE` 经 `raise_softirq` 自行登记 Sched softirq）。
  - 状态：[]
  - 详情：置于 `handle_irq` 最前，先于 `stats.record_irq`，避免 `irq` 越界值污染统计。

- **S-3. `0xFD` 处理：本核失效 + 完成确认**
  - 描述：目标核需刷新本核 TLB 并向发起核确认，否则发起核无法界定「失效已完成」。
  - 方案：处理体内调 `mm::arch::tlb_flush_all()`；随后对 `TLB_ACK_PENDING` 做**有下界保护**的递减（仅当 `>0` 时减，用 `fetch_update` 或 CAS 循环），避免迟到/多余 IPI 造成下溢污染下一次 shootdown 的计数。
  - 状态：[]
  - 详情：粒度取 `tlb_flush_all`，取页级由 S-6 的 SIMPLIFIED 说明承载。

- **S-4. `0xFE` 处理：接线既有 `resched_ipi_handler`**
  - 描述：[cpu_queue.rs:118-121](../../src/kernel/framework/proc/cpu_queue.rs#L118-L121) 的 `resched_ipi_handler` 全仓库零调用（§2.2）。
  - 方案：`0xFE` 分支调用该函数（`raise_softirq(Sched)`），使跨核重新调度请求真正落地；不改其内部逻辑。
  - 状态：[]
  - 详情：`0xFE` **不等 ack**（reschedule 天然可 fire-and-forget），仅 `0xFD` 走确认栅栏。

- **S-5. 定向 cpumask 投递替代 all-excluding-self 广播**
  - 描述：`broadcast_ipi` 用 `ICR_ALL_EXCLUDE_SELF`（[apic.rs:164-174](../../src/kernel/framework/arch/x86_64/apic.rs#L164-L174)），无法表达「只发给在线核」，多核下会把 IPI 投给未上线的目标。
  - 方案：`smp/mod.rs` 新增定向 shootdown 发送：遍历 `0..get_cpu_count()`，对 `CPU_ONLINE[i] && get_apic_id(i) != 本核 APIC ID` 的核逐一向其 APIC ID `send_ipi(.., 0xFD)`，并累加目标数。保留既有 `broadcast_ipi` 供 reschedule 广播使用。
  - 状态：[]
  - 详情：本核 APIC ID 取 `get_current_cpu()`（`arch!(cpu_id())`，与 [smp/mod.rs:22](../../src/kernel/framework/smp/mod.rs#L22) 初始化同源）。

- **S-6. shootdown 移出 `VMM_LOCK` 临界区 + ack 栅栏（D2）**
  - 描述：临界区内等 ack 必死锁（§2.3）；须改为「临界区内只记录，释放锁后广播并等 ack」。
  - 方案：
    1. `flush_tlb(addr)` 保留本地 `invlpg`，并把「本临界区有远程失效需求」置入一个原子标志（锁内单写者）。
    2. `release_lock()` 在**仍持锁**时读并清该标志，然后照原序 `VMM_LOCK.store(false)` → `restore_interrupts(flags)`；此后若标志置位且 `smp::is_enabled() && get_cpu_count() > 1`，调用新增的 `smp::tlb_shootdown_sync()`。
    3. `smp::tlb_shootdown_sync()`：以 **IF=1** 自旋等待 `SHOOTDOWN_IN_FLIGHT` 清零（串行化在途 shootdown，避免并发计数错乱）→ 置 in-flight → `TLB_ACK_PENDING.store(目标数)` → 按 S-5 定向发送 `0xFD` → 以 **IF=1** 自旋等待 `TLB_ACK_PENDING == 0` → 清 in-flight。
    4. 等 ack 期间 IF 必须保持开启：本核仍需响应他核 `0xFD`（否则两核互 shootdown 互等死锁，§2.3 推论）。
  - 状态：[]
  - 详情：**`// SIMPLIFIED:` 标记必填**——远程失效粒度为 `tlb_flush_all`（重载 CR3）而非页级：影响面为 shootdown 触发时远程核全量 TLB 失效（性能开销，非正确性）；扩展条件为 shootdown 成为热路径或出现页级失效收益时，再引入地址载荷（需同时解决载荷数组的复制、溢出回退与并发写者规避）。另：`release_lock` 现被约 21 处调用（含大量早退路径），标志为空时零开销直通。

- **S-7. deferred-free：帧释放延后至 ack 完成（D3）**
  - 描述：§2.4 的 8 处 `free_page` 与同区 TLB 失效共享临界区，release 后到 ack 完成之间存在「刚释放帧被重分配 + 第三核陈旧映射」窗口。
  - 方案：临界区内把待释放帧记入局部缓冲（内核栈上的定长数组），`release_lock` 中在 shootdown + ack 完成后才真正调 `pmm.free_page`。缓冲设上限，溢出时对超出部分立即释放并一次性告警。
  - 状态：[]
  - 详情：范围以「同一临界区内既有 `flush_tlb` 又有 `pmm.free_page`」为判据逐一核实落点，已核实两区共 8 处（[L1093-1129](../../src/kernel/framework/mm/vmm_x86_64.rs#L1093-L1129) 3 处 + [L1212-1229](../../src/kernel/framework/mm/vmm_x86_64.rs#L1212-L1229) 5 处）；[L633](../../src/kernel/framework/mm/vmm_x86_64.rs#L633) 经核实不在范围内。缓冲溢出属 SIMPLIFIED 路径，须以 `// SIMPLIFIED:` 注明（简化了什么 + 影响 + 何时扩展）。`page_fault.rs`/`cow.rs`/`swap.rs` 的 `free_page` 不在本工程临界区内，属既有面，只登记不动（§12.5）。

- **S-8. aarch64 SGI 同语义分支（D4）**
  - 描述：aarch64 接收侧无 TLB/reschedule SGI 分支，与 x86_64 语义不对称（§2.6）。
  - 方案：`arch/aarch64/exception.rs` 的 `irq_handler` 按 SGI intid 增补分支：TLB 失效 SGI → `tlb_flush_all` + ack 递减；reschedule SGI → 复用 `resched_ipi_handler`。向量编号与既有 barrier SGI(7) 不冲突。
  - 状态：[]
  - 详情：aarch64 无 AP 上线路径（`SMP_ENABLED` 恒 false），本轮**仅编译验证，无运行验证载体**（见 §5.3）；须避免不可达分支触碰 F9 死代码红线（分支由 SGI intid 判定，属真实可执行路径）。

- **S-9. `test-smp` 参数化至 2/3/4 核**
  - 描述：`test-smp` 硬编码 `-smp 2` 与断言 `online CPUs: 2`，无法验证 2 核以上（§2.5）。
  - 方案：`Makefile` 以变量表达核数（默认 2），断言文本随核数生成；新增一次性覆盖 2/3/4 核的验证入口（或按核数循环调用），任一核数未达「全部上线 + 进入 Ring 3」即 `exit 1`（保持 fail-closed）。
  - 状态：[]
  - 详情：核数上限受 QEMU 与 trampoline 握手窗口约束，3/4 核结果需实测记录；若 3/4 核暴露新缺陷，按分册 9 台账格式登记，不在本工程内擅自扩范围。

- **S-10. 文档同步**
  - 描述：台账 D-9-3 / D-9-5 / D-9-7 与本工程状态需一致（§9.2）。
  - 方案：本工程完成后更新 D-9-3（`0xFD`/`0xFE` 门与处理程序已补）、D-9-5（跨核 shootdown 门控语义已复核，指向本文档）、D-9-7（aarch64 差异标注指向本文档 §5.3）；`docs/explain/` 如需描述 IPI 协议则用自由描述风格（禁用结构化字段）。
  - 状态：[]

## 4. 验证门槛（§2.3 五条底线 + 本工程附加项）

1. `./ci/build.sh all`：双架构 0 error / 0 warning。
2. `./ci/audit.sh quick`：核心审计全部通过（含 F1/F2 边界、F4 SAFETY 覆盖、F7 中文注释、F9 死代码零豁免）。
3. `make test-host`：退出码取脚本自身（注意：该目标当前带 `; true` 属 fail-open，见 D-9-4，本工程不改，按内容判读并另记）。
4. `make test-unit`。
5. `scripts/qemu_boot_test.sh x86_64`。
6. **本工程附加**：`make test-smp` 在 2/3/4 核下均通过（S-9）。

**fail-closed 复验（必做）**：注入式反向验证——临时移除 `0xFD` 的 IDT 门或处理分支，确认 `test-smp` **必须 FAIL**（而非静默通过），验证后完全还原。

## 5. 遗留与登记项

### 5.1 本工程不处理的既有面（§12.5）

- `Makefile` `test-host` 的 `; true` fail-open（D-9-4）。
- `acpi.rs` MADT 日志占位串（D-9-6）。
- `lib.rs:97` 的 `#![allow(unused_unsafe)]`（已核实）。
- 启动日志 `Caps: ...` 行的 `smp`/`preempt`/`kaslr`/`barrier` 取值由 `KernelCapabilities::detect()` 的 `cfg!(feature = ...)` 决定（[caps.rs:49-61](../../src/kernel/framework/config/caps.rs#L49-L61)，已核实）；因当前无构建传入 `--features smp`，故日志恒为 `smp=off`。**注**：该行是编译期能力报告，不反映运行期 TLB shootdown 是否生效——D6 后后者由 `smp::is_enabled() && get_cpu_count() > 1` 运行期门控决定。
- **更正（原句失实）**：原 §5.1 曾记「`smp`/`preempt`/`kaslr`/`barrier` feature 未在 `Cargo.toml` 声明」，实测为**误**——四者均已声明（[Cargo.toml](../../src/kernel/Cargo.toml) 的 `[features]` 段：`smp=[]`、`preempt=[]`、`kaslr=[]`、`barrier=[]`，`host-test=[]` 亦在其中）。故本项不属于「既有面遗留」，而是原文表述错误，现予更正。
- `page_fault.rs`/`cow.rs`/`swap.rs`/`pcache.rs` 的 `free_page` 与跨核失效关系（不在本工程临界区内）。
- `0x82`（barrier）与 MSI 段 `0x80-0x9F` 的门重叠。

### 5.2 本工程产生的后续项（如出现）

- 若 3/4 核验证暴露握手/调度缺陷 → 按台账格式登记，另行裁定。
- 页级 shootdown（S-6 的 SIMPLIFIED 扩展点）。

### 5.3 aarch64 验证缺口的明确标注

aarch64 侧本轮实现 SGI 处理分支，但**无 AP 上线路径**（`arch/x86_64/smp_init.rs` 之外的平台无 `register_cpu` 调用点），故 `SMP_ENABLED` 恒 false、`CPU_COUNT` 恒 1，IPI 路径在 aarch64 上**不可运行验证**。该平台差异须在台账 D-9-7 与本文件同时标注，避免后续误判「已验证」。