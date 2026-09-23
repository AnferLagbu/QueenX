# KPTI 完整化工程（原 B02-39，独立工程）

> 从 [audit-fix-02-framework-arch-asm.md](./archive/audit-fix-02-framework-arch-asm.md) B02-39 擢升为独立工程（2026-08-21 用户决策）。
> 来源：审计附录 A F-04/F-08/F-10/F-16 + TOP 20 #3 + [code-audit-final-summary.md](./code-audit-final-summary.md)。
> 复核结论：两架构 KPTI 均为"半 KPTI"（页表名目隔离 + U/S 位权限，映射面未缩小），Meltdown 侧信道防御未实现。

## 工程计划 A: 现状与目标

### 背景

- **KPTI-01. 半 KPTI 证据（已复核）**
  - 描述：x86_64 `kpti.rs:333-338` `USER_PML4[256..512] = KERNEL_PML4[256..512]` 完整复制内核高半区；`kpti.rs:350-380` 整个 `.text` 映射进用户页表（PRESENT only）。aarch64 `kpti_aarch64.rs:158-167` `TRAMP_TTBR1` 复制完整 L0[256..511]；`arch/aarch64/mod.rs:272-310` `enter_user` 只切 TTBR0 未激活 trampoline。
  - 方案：本工程按 Phase 0-3 完整化两架构 KPTI 隔离。
  - 状态：[]

- **KPTI-02. 目标架构**
  - 描述：用户态运行的页表（x86 `USER_PML4` / 每进程用户页表；aarch64 `TRAMP_TTBR1`）只含：用户空间 + 异常/中断/syscall 入口 trampoline 代码 + 入口路径必需内核数据页（含内核栈首页）——不含其余内核 `.text`/`.data`/`.bss` 映射。
  - 方案：x86 收敛到 `_kernel_text_start ~ _kpti_trampoline_end`（链接脚本 [x86_64.ld](../../src/kernel/framework/link/x86_64.ld#L46-L56) 已划出该区域）；aarch64 收敛到异常向量表所在 L1 条目。
  - 状态：[]
  - 详情：**aarch64 侧已达成**——EL0 视图（用户页表 + TTBR1 trampoline 表）现只含用户映射、`.vectors` 全部页、`KPTI_GLOBALS` 页与内核栈顶页（EL1-only）；其余内核 `.text`/`.data`/`.bss` 在 EL0 下不可见（见 KPTI-04/KPTI-13/KPTI-15）。x86_64 侧待 Phase 2。

### 前置调研（Phase 0）

- **KPTI-03. 入口路径内存依赖清单（Phase 0）**
  - 描述：枚举 x86_64（isr/irq/syscall/int 0x80）与 aarch64（EL0 sync/irq/svc）入口在 CR3/TTBR 切换前访问的全部代码页/数据页/栈页。**含 aarch64 用户态首个 SVC 陷入路径卡死定位**（来源：分册 2 B02-25 调研根因问题 1——init `_start` 第一动作 `print_char` = `fs_write` = `svc #0` 未返回，卡点在 handle_el0_sync → KERNEL_TTBR1 切换 → svc_handler 路径）。
  - 方案：逐入口静态分析汇编（isr.asm / mod.rs enter_user_asm / exception.rs global_asm），输出"入口 → 依赖内存"矩阵。已知起点：x86 依赖 USER_CR3_SAVE（.bss）+ SyscallPerCpu（per-CPU）+ GDT/IDT/TSS（用户态中断 CPU 硬件访问）+ TSS.RSP0/IST 内核栈页（CPU 在用户 CR3 下推帧）；aarch64 依赖异常向量表页 + KERNEL_TTBR1 数据页 + SP_EL1 内核栈页。
  - 状态：[X]
  - 详情：aarch64 侧依赖矩阵已在 Phase 1 实施中实证收敛（"入口 → 依赖内存"见表下"aarch64 EL0↔EL1 边界依赖面"）。结论：入口切换**前**仅依赖三处——异常向量表页（`.vectors`，取指面）、`KPTI_GLOBALS` 所在页（入口汇编读全局量）、内核栈**顶页**（压 280 字节异常帧，经 EL1-only 映射进用户页表）；切换**后**还需内核镜像/数据（低半区恒等 DRAM 块）。x86_64 侧依赖清单待 Phase 2（KPTI-07/08）落实。

#### aarch64 EL0↔EL1 边界依赖面（Phase 0 产出，Phase 1 实证）

| 依赖对象 | 访问时机 | 承载映射 | 实施位置 |
|---|---|---|---|
| `.vectors` 全部页 | 异常取指 + 入口/出口汇编 + 进入 EL0 trampoline 取指 | trampoline 表（TTBR1，页级最小化） | `kpti_aarch64::kpti_init` |
| `KPTI_GLOBALS` 页 | 入口/出口读 `tramp_ttbr1`/`kernel_ttbr*`/`user_ttbr0` | trampoline 表（TTBR1，单页） | `kpti_aarch64::kpti_init` |
| 内核栈**顶页** | 入口压 280B 异常帧（切 TTBR0 **之前**） | 用户页表 EL1-only 映射（无 USER 位） | `kpti_aarch64::map_kernel_stack_top_page` |
| 内核镜像/数据/BSS/内核栈 | 切换后 EL1 运行期（`copy_from_user` 等直访） | EL1 视图 DRAM 1 GiB 块（TTBR0） | `vmm_aarch64::build_el1_view` |
| 用户页 | 切换后 EL1 解引用用户裸指针（`userptr.rs` 直访） | EL1 视图用户半区（TTBR0，与用户表共享 L2） | `vmm_aarch64::build_el1_view` |

### aarch64 完整化（Phase 1，近期）

- **KPTI-04. TRAMP_TTBR1 最小化**
  - 描述：`kpti_aarch64.rs:158-167` 当前复制完整 L0[256..511]，需改为仅映射异常向量表所在 L1 条目 + 入口代码页 + KERNEL_TTBR1 数据页 + SP_EL1 内核栈页。
  - 方案：基于 KPTI-03 依赖清单，`kpti_init` 重建 trampoline 页表；异常向量表位于 [exception.rs](../../src/kernel/framework/arch/aarch64/exception.rs#L46-L49) `.vectors` section。
  - 状态：[X]
  - 详情：改为**页级最小化**（不再复制任何 L0 槽位）：`kpti_init` 分配并清零一张 4 级根表，仅映射两个对象——`.vectors` 段全部页（新增链接脚本符号 `_vectors_start`/`_vectors_end`，段长 `ALIGN(4096)` 保证末页完整计入，见 [aarch64.ld](../../src/kernel/framework/link/aarch64.ld#L30-L36)）与 `KPTI_GLOBALS` 所在单页；其余高半区条目恒为 0 ⇒ 用户态下内核代码/数据完全不可见。4 级形态硬约束：T1SZ=16 决定硬件**从 level 0 起**遍历，tramp 根必须是 4 级表（旧实现"T1SZ=16 ⇒ 指向 L1 形态表"的认知已被证伪，见 KPTI-16）。实现：`kpti_aarch64::kpti_init`。

- **KPTI-05. enter_user 激活 trampoline**
  - 描述：[arch/aarch64/mod.rs:272-310](../../src/kernel/framework/arch/aarch64/mod.rs#L272-L310) `enter_user` 只切 TTBR0，TTBR1 保持完整内核页表，首次进入 EL0 无隔离。
  - 方案：eret 前调用 `kpti_exit_to_user()` 切换 TRAMP_TTBR1；`return_to_user` 确认一致。
  - 状态：[X]
  - 详情：采用**全切换模型**。`enter_user`（[mod.rs:298-342](../../src/kernel/framework/arch/aarch64/mod.rs#L298-L342)）改为：`msr sp_el0/elr_el1/spsr_el1` → `msr spsel,#1; mov sp,kstack`（写 SP_EL1；`msr sp_el1` 编在 EL1 为 UNDEFINED）→ `br` 到 `.vectors` 内高别名 trampoline `kpti_enter_user_trampoline`，由后者在**高半区**切 `TTBR0→user_ttbr0、TTBR1→tramp_ttbr1` 后 `eret`（低半区切表会立即 Prefetch Abort）。对称地，异常入口 `handle_el0_sync`/`handle_el0_irq` 与统一出口 `el0_return` 完成切回；`return_to_user` 保留为 eret 桩。用户页表记录：`kpti_set_user_ttbr0(user_cr3)`。

- **KPTI-06. aarch64 验证**
  - 描述：QEMU aarch64 启动 + EL0 往返 + 隔离断言。
  - 方案：`./scripts/qemu_boot_test.sh aarch64`；host-tests 断言 trampoline 页表内容（不含内核 `.text`/`.data`）；EL0 访问高半区应触发异常而非可读。
  - 状态：[X]
  - 详情：**成功判据 = "Entering EL0" 之后出现 SVC 往返日志**，已实证达成。
    - `-d int` trace（`-serial file:... -d int -D ...`）实测：`Exception return from AArch64 EL1 to AArch64 EL0 PC 0x401000`（首次进入 EL0）后连续 **4 次 SVC 往返**——`Taking exception 2 [SVC] on CPU 0` / `...with ELR 0x40101c|0x401038|0x401040|0x401058` / `SPSR 0x3c0`，其中 3 次有对应 `Exception return ... to EL0 PC 0x4010xx`。
    - 串口实测：用户 init 经 syscall 写 UART 输出 `X`（[src/user/init/src/main.rs:14](../../src/user/init/src/main.rs#L14) `print_char(b'X')`）；随后 EL1 timer IRQ 正确向量化（`to EL1 PC 0xffff0000401bfa80`，即 `.vectors` 高别名 ⇒ 证明 trampoline 表取指面有效）并返回内核，日志续打 `IRQ: intid=30 count=1` / `TIMER IRQ count=1 ready=true` / `[NET] DHCP deconfigured`。
    - §2.3 五门槛：`./ci/build.sh all` Passed 5/Failed 0；`./ci/audit.sh quick` RC=0；`make test-host` 全 ok；`make test-unit` `✅ ALL TESTS PASSED (QEMU exit: 33)`；QEMU x86_64 1/1（252 行，里程碑 `VFS ready`）+ QEMU aarch64 1/1（里程碑 `VFS ready` + `virtio-net 经 NetOps 安全桥探测成功` + `完整启动成功! 进入 EL0 启动 init 进程`）。
    - 未覆盖项：host-tests 对 trampoline 表的"不含内核 `.text`/`.data`"断言、EL0 访问高半区的异常断言（属 KPTI-11 范畴，Phase 3）。

#### Phase 1 实施记录（方案 A：per-process EL1 视图）

- **KPTI-13. EL1 视图（方案 S3）**
  - 描述：全切换模型下 EL0 与 EL1 使用不同 `TTBR0`。EL0 视图只含用户映射（Meltdown 面最小），但内核态必须**同时**看见内核镜像与用户页——`copy_from_user` / `UserReadPtr` 直接解引用用户裸指针（`userptr.rs`，不做页表遍历），地址空间缺映射即翻译故障。故需为每进程维护一份 EL1 专用视图。
  - 方案：路线 = 方案 A，per-process 双视图：**EL1 视图 = 用户半区 ∪ 内核恒等**；**EL0 视图 = 用户 + 内核栈顶页（EL1-only）**。EL1 视图结构取 **S3**：内核 MMIO 统一走 TTBR1 高别名 ⇒ EL1 视图**刻意不含 Device 段**，其 L2/L3 与用户表**零同步共享**，每进程仅 +2 页（`L0_el1` + `L1_el1`）：`L0_el1[0]→L1_el1`、`L0_el1[170]/[255]` 与用户表同值、`L1_el1[0]→L2_u`（与用户视图同一页 ⇒ 用户页 map/unmap 对两侧同时生效）、`L1_el1[1]` = 内核 `L1_IDMAP[1]` 的 DRAM 1 GiB 块（内核镜像/数据/BSS/内核栈可达，VA 1-2 GiB）。
  - 状态：[X]
  - 详情：实现于 `vmm_aarch64::build_el1_view` / `destroy_el1_view`（拆视图**仅**释放自身两页，不得递归——`L0_el1[170]/[255]` 与 `L1_el1[0]→L2_u` 均共享，递归会双释放/UAF）。关联方式（AI 自定）：用户表**保留槽 `EL1_VIEW_SLOT = 1`**（VA 512 GiB~1 TiB，用户态从不使用）存视图根物理地址，**只写地址不置 `bits[1:0]`** ⇒ 硬件视为无效描述符（该 VA 段保持未映射），软件可直接读出。收益：入口汇编只需 `ldr x4, [x2, #8]`（x2 = 当前 `TTBR0`）即取到本进程视图——关联**随页表一起切换**，无全局槽的陈旧值风险。`create_user_page_table` 建视图失败即 fail-closed 返回 `None`。fork 路径（`proc_ops.rs`）子页表由 COW 克隆产生（保留槽被 `[1:0]==0b11` 过滤掉）⇒ 须经 `vmm_build_el1_view` 补建，失败即回滚子进程。

- **KPTI-14. EL1 视图 L0 槽位镜像同步**
  - 描述：EL1 视图的 L0 是 `create_user_page_table` 时刻用户 L0 槽位 1..255 的**快照**，而用户栈（VA `0x7FFF_FFFF_F000` ⇒ L0[255]）与 PIE（L0[170]）都是之后经 `map_page_in_table` 惰性映射的。不同步则视图停留在全 0 快照 ⇒ 内核态直访用户裸指针触发 **level-0 翻译故障**（实测 `ESR 0x25/0x96000004`、DFSC=0x04、FAR=用户栈地址、ELR=memcpy）。
  - 方案：新增 `vmm_aarch64::mirror_l0_slot_to_el1_view(user_l0, idx)`，在 `map_page_in_table`（L0 扩级后）与 `unmap_page_in_table`（槽清零后）两处调用；`idx == 0 || idx == EL1_VIEW_SLOT || idx >= 256` 为空操作（视图自占槽 / 保留槽 / 高半区恒为内核副本）。
  - 状态：[X]

- **KPTI-15. UART PL011 高别名接线**
  - 描述：S3 下 EL1 视图不含 Device 段，内核态若仍按低半区地址 `0x0900_0000` 访问 PL011 会触发 L2 翻译故障（实测 `ESR 0x25/0x96000006`、DFSC=0x06、FAR=`0x9000018` UARTFR）。根因：`uart::switch_to_high_half()` 本轮前无任何调用者。
  - 方案：`iomem.rs::mmio_virt` 在 aarch64 返回 `HIGH_ALIAS_BASE + phys`，且 aarch64 跳过 `ensure_mmio_mapped`（Device 别名由 TTBR1 `L1_IDMAP[0] → L2_DEVICE` 静态覆盖，2 MiB 粒度，无需动态建映射 ⇒ 整个函数 `#[cfg(target_arch = "x86_64")]` 门控，避免死代码 F9）；`enter_user` 在进入 EL0 前调用 `uart::switch_to_high_half()` 把 `PL011_BASE` 切到高别名。
  - 状态：[X]

- **KPTI-16. 前置修正（本轮前既有缺陷）**
  - 描述：两处 aarch64 前置缺陷在 Phase 1 暴露并修正。
  - 方案：
    1. **TTBR1 = 4 级根**：`init_kernel_ttbr1` 旧实现基于「T1SZ=16 ⇒ 硬件从 level 1 起遍历」的错误认知，令 TTBR1 指向一张 L1 形态表。实测（`AT S1E1R` + 根表转储）表明该表被硬件当作 L0 消费，高半区 DRAM 别名的物理基址被截断（`0xFFFF_0000_401B_F000` 解析到 PA `0x1BF000` 而非 `0x401BF000`）⇒ `VBAR_EL1` 指向的向量表不可取指，内核**从未进入过任何异常/中断处理**（`TIMER IRQ` 恒 0 次）。修正：TTBR1 复用 TTBR0 的同一 4 级根 `L0_TABLE`（高半区别名 `VA[47:39]` 恒 0 ⇒ L0 索引恒 0，L1/L2/L3 索引与恒等映射相同）。
    2. **MAIR[1]**：补 `Attr1 = 0xFF`（Normal IWBWA OWBWA），供 trampoline 表映射 `.vectors`（需可取指）与全局量页使用；未定义索引（0x00）会被硬件按 Device-nGnRnE 解释。
    3. **写 SP_EL1 的架构等价写法**：`msr sp_el1` 编码（S3_4_C4_C1_0）在 EL1 为 UNDEFINED（QEMU `max`/`cortex-a72` 实测均报 Undefined Instruction），改为 `msr spsel,#1; mov sp,kstack`。
  - 状态：[X]

### x86_64 完整化（Phase 2，中长期）

- **KPTI-07. .text 映射收窄到 trampoline 区域**
  - 描述：[kpti.rs:482-551](../../src/kernel/framework/mm/kpti.rs#L482-L551) `map_text_region_in_user_pml4` 当前映射 `_kernel_text_start ~ _kernel_text_end`（整个内核代码），应收窄到 `_kernel_text_start ~ _kpti_trampoline_end`（含 .kpti_trampoline + isr.o 全部入口代码，链接脚本已保证入口代码位于该区域）。
  - 方案：`kpti_init` 与 `create_user_page_table`（vmm_x86_64.rs:636）同步收窄；映射后断言其余内核代码页在用户页表中不存在。
  - 状态：[]

- **KPTI-08. USER_PML4 高半区复制移除**
  - 描述：`kpti.rs:333-338` 不再复制 `KERNEL_PML4[256..512]`，改为按 KPTI-03 依赖清单显式映射必需数据页（USER_CR3_SAVE、SyscallPerCpu、GDT/IDT/TSS、TSS.RSP0/IST 内核栈页）。
  - 方案：新增"必需内核页清单"集中管理（链接脚本符号 + 运行时枚举）；`kpti_sync_pml4_entry` 语义调整（高半区新增映射不再自动同步，改显式登记）。
  - 状态：[]

- **KPTI-09. x86_64 验证**
  - 描述：QEMU x86_64 Ring 3 + syscall/中断往返 + 隔离断言。
  - 方案：`./scripts/qemu_boot_test.sh x86_64`（含 Ring 3 到达，顺带闭合分册 2 B02-25）；host-tests 断言进程用户页表不含内核 `.text`/`.data` 映射。
  - 状态：[]
  - **前置依赖（已解除）**：[x86-init-probe-project.md](./x86-init-probe-project.md) 已收口 —— X86IP-06 实测达成 x86_64 Ring 3 到达（`[USER] Entering Ring 3 (init pid=4)` + init 打印 `X`/`Y` + `exit: pid=5/6 code=0`，登记时的启动阻塞现象不复现），本条的 QEMU 验证**不再被阻塞**。

### 每进程一致性与强化验证（Phase 3）

- **KPTI-10. 每进程页表与共享模板统一**
  - 描述：[vmm_x86_64.rs:623-659](../../src/kernel/framework/mm/vmm_x86_64.rs#L623-L659) `create_user_page_table` 与 `kpti_init` 的映射逻辑保持同步（Phase 2 收窄后两者都只映射 trampoline + 必需数据页）。
  - 方案：抽公共函数；host-tests 对任意进程页表断言隔离属性。
  - 状态：[]

- **KPTI-11. 页表内容断言 host-tests**
  - 描述：当前无任何测试验证用户页表"不含内核映射"（分册 2 审查已指出 B02-39 仅表层检查）。
  - 方案：新增 host-tests 遍历用户页表（每进程 + 共享模板），断言高半区仅含 trampoline 区域与白名单数据页。
  - 状态：[]

- **KPTI-12. 完整回归 + 文档同步**
  - 描述：双架构 QEMU 完整回归（Ring 3 到达 + 用户态陷入/返回）+ docs 同步。
  - 方案：§2.3 门槛 + 专项 QEMU；本工程文档与分册 2 B02-39 状态联动更新。
  - 状态：[]

### 决策记录

- **DECISION-056**
  - 描述：工程分阶段：aarch64 先行（Phase 1，工程量小、机制可验证），x86_64 后行（Phase 2，依赖枚举复杂）。来源：2026-08-21 用户决策。
  - 状态：[X]

- **DECISION-057**
  - 描述：x86_64 采用"渐进收敛"而非一次性严格最小化：先收窄 `.text` 到 trampoline 区域（低风险，trampoline 区域已含全部入口代码），再按依赖清单逐项最小化数据页。降低漏映射 Triple Fault 风险。
  - 状态：[X]

- **DECISION-058**
  - 描述：aarch64 隔离模型取**全切换模型**（EL0 用 EL0 视图、EL1 用 EL1 视图），而非"半 KPTI"（TTBR1 常驻完整内核表）。路线取**方案 A：per-process EL1 视图**（用户半区 ∪ 内核恒等），EL1 视图结构取 **S3**（不含 Device 段，MMIO 走 TTBR1 高别名，L2/L3 与用户表零同步共享，每进程 +2 页）。
  - 状态：[X]

- **DECISION-059**
  - 描述：EL1 视图根与用户表的关联方式 = **用户表保留槽 `EL1_VIEW_SLOT = 1`**（存物理地址，不置 `bits[1:0]`），而非全局槽。理由：关联随页表一起切换，杜绝"调度后忘记更新全局槽 ⇒ 用错视图"的陈旧值风险。保留槽所在 VA 段（512 GiB~1 TiB）用户态从不使用。
  - 状态：[X]

### 遗留与登记项（本轮不修）

- **KPTI-17. aarch64 fork 子进程零上下文崩溃（预存缺陷，非本轮 KPTI 改动直接导致）**
  - 描述：Phase 1 跑通后（首次让 aarch64 真正到达 EL0 并发出 `fork`），`Entering EL0` 后 4 次 SVC 成功往返，随后调度切到 pid=5 时崩溃：`sched CSW prev_pid=4 next_pid=5 next_ctx=0x444BC050 sp=0x0 ttbr0=0x0 spsr=0x0 elr=0x0` → `Exception return from AArch64 EL1 to AArch64 EL0 PC 0x0` → Prefetch Abort（`SPSR 0x0`/`ELR 0x0`）→ `SP_EL1=0` ⇒ `handle_el1h_sync` 压帧失败（`FAR 0xfffffffffffffee8`）反复级联。
  - 方案：三段因果链已确证——（a）`enter_user` 直接改写 `SP_EL0/ELR_EL1/SPSR_EL1` 进入 EL0，**从不写 `Process.context`** ⇒ pid=4（父）上下文恒全 0（`git diff` 确认**改动前同样如此**，非本轮引入）；（b）`clone.rs` `*child_ctx = parent_ctx` ⇒ pid=5 零上下文；（c）`cfs_enqueue(5)` + `Scheduler::schedule()` 切到它 ⇒ `context_switch_asm` 以 `SPSR=0/ELR=0` `eret`。
  - 状态：[]
  - 详情：修复牵涉 aarch64 上下文切换语义——`context_switch_asm` 不保存 x0–x18，`fork` 子进程"返回 0"无法经 `Process.context` 表达，与 **k3g 待办**（`context_switch_asm` 用户表语义 + 目标 EL0 时切 TTBR1=TRAMP + aarch64 `set_kernel_stack` 空实现）**重叠**，须与之合并设计。登记位置：k3g 待办 + 本工程。本轮按"最保守路径"仅登记不修。
  - 附带观察：`scheduler.rs:715-717` 把 pid（`u64::from(next)`）写入 `SCHEDULER_EX.current`，而其语义为 `Thread` 指针；`scheduler_ex::tick_accounting` 会 `ThreadRef::new_unchecked(current)` 解引用 ⇒ 潜在类型混用缺陷（本轮未改动、未登记为独立项）。

- **L1（aarch64 高半区内核迁移）——已擢升为独立 plan 工程**
  - 描述：aarch64 内核当前驻留**低半区恒等映射**（`KERNEL_BASE = 0`，镜像 @ `0x40080000`），因而 EL1 视图必须以 DRAM 1 GiB 块把内核镜像/数据/栈一并纳入 `TTBR0` 视图。这是 S3 的"大映射面"来源。
  - 方案：把内核迁移到高半区（TTBR1 领地），使命中路径回归"EL1 视图只承载用户页 + 内核经 TTBR1 可达"的自然形态。引用面盘点：`KERNEL_BASE` 在 `src/` 下 **154 处 / 36 文件**（`vmm_x86_64.rs` 21、`user_proc.rs` 24、`pmm.rs` 16、`process.rs` 8 等）。
  - 状态：[]
  - 详情：独立工程文档见 [aarch64-high-half-migration.md](./aarch64-high-half-migration.md)。**不属本工程 Phase 1 交付范围**，登记为后续演进项。

### 验证标准

- §2.3 5 条门槛全过（双架构 cargo build / clippy / make / host-tests / QEMU）
- 专项：QEMU 双架构 + Ring 3 往返（补分册 2 B02-25）；页表内容 host-tests（KPTI-11）
- 隔离断言：用户态访问内核高半区（x86 高半区 VMA、aarch64 TTBR1 空间）触发异常而非可读

### 风险与回退

- **漏映射 Triple Fault**：x86 最小化漏掉入口依赖页 → 用户态首个中断即 Triple Fault。缓解：Phase 0 依赖清单 + Phase 2 渐进收敛 + QEMU 每步验证。
- **G 位/TLB 残留**（审计 F-16）：依赖无 G 位 + CR3 切换刷新；若其他路径设 G 位需一并清理（审计已识别 boot 路径）。
- **性能**：trampoline 页表缩小可能增加页表分配/切换成本；PCID（已启用）缓解。
- **回退**：`KernelCapabilities::kpti` 编译期开关可整关（kpti.rs:12 设计），任何阶段可回退到现状。
