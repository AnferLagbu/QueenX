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

- **DECISION-060**
  - 描述：KPTI-17 修复取**方案 B（统一上下文模型）**（用户 2026-09-23 裁定）：aarch64 侧 `Process.context` 为唯一上下文载体，删除 `enter_user` 直进与 ctx 双轨，首进程与 fork 子进程统一经"预置 ctx + 调度器首切"进入 EL0；`context_switch_asm` 按目标 `SPSR.M[3:0]` 分派内核续跑/EL0 进入两条路径（单一恢复模型）。`ProcessContext` 的 aarch64 字段语义随之固定为：`@96 = SP_EL1`、`@104 = TTBR0`、`@112 = SPSR_EL1`、`@120 = ELR_EL1`、`@128 = SP_EL0`、`extra_regs[0..7] = x0..x7`。
  - 状态：[X]

- **DECISION-061**
  - 描述：aarch64 `SP_EL1` 不通过 `cpu::arch::set_kernel_stack` 管理（该路径的 `msr spsel,#1; mov sp` 会破坏调度器自身栈），而由 `Process.context.@96` 承载：调度切换时保存/恢复，未调度过的新任务由创建方预置。`set_kernel_stack` 的 aarch64 分支保持空实现。
  - 状态：[X]

- **DECISION-062**
  - 描述：`ProcessContext` aarch64 的 `@104`/`@136` 语义按**实施可行性**细分（补充 DECISION-060 的字段表，非替代）：`@104` = **EL1 侧 TTBR0**（保存侧写 live `TTBR0_EL1` = 本进程 EL1 视图根 / 内核表），`@136`（复用 `_fpu_pad`）= **用户页表**（EL0 的 TTBR0）。理由：（a）内核续跑路径必须以 `@104` 切回"EL1 可用的表"——若 `@104` 存用户表，内核低半区 `.text`/栈经 TTBR0 不可达（TTBR1 只映射高别名）⇒ 恢复即崩；（b）内核任务（idle）的 `@104` 必须为内核表，取 live 值天然安全；若改存 `KPTI_GLOBALS.user_ttbr0`，内核任务会被写入陈旧值 ⇒ 恢复时可能命中已销毁页表（UAF）。配套两点：内核续跑路径把 `@136` 回写 `KPTI_GLOBALS.user_ttbr0`（被抢占期间该槽被别的任务改写，而本任务续跑后必经 `el0_return` 读它切表回 EL0，不刷新会用错页表），且 `TTBR0` 变更后补 `tlbi vmalle1is`。
  - 状态：[X]

- **DECISION-063（KPTI-19 修复路径：删除跨层写入 + 长期消除双调度器）**
  - 描述：2026-09-23 用户就 KPTI-19 反问"长期最优是哪个"，裁定为"给出长期最优判断并落地不返工的前置步骤"。结论：**长期最优 = 方案 C（消除 `SCHEDULER` / `SCHEDULER_EX` 双调度器）**；**本轮落地 = 删除 `SCHEDULER::schedule()` 中对 `SCHEDULER_EX.current` 的 pid 语义写入**（恢复净写者不变式）。
  - 理由：（a）`SCHEDULER_EX.current` 的权威语义为 `*mut Thread`（`init`/`schedule` 及 4 处读点均按此使用），而进程级调度器持有的是 `Pid` ⇒ 写入即类型混用，`tick_accounting` 把 pid 当指针解引用（aarch64 对齐异常崩溃 / x86 低地址静默写坏）；（b）替代方案"经 `find_by_pid` 解析后同步"**不可行**——`create_thread` 无生产调用者、非测试 `Thread` 对象仅 idle 一个，按 AGENTS §9 禁止为无意义调用造实现；（c）"统一为 pid"（方案 B）会与 `run_queues` 的 `*mut Thread` 节点反复互转，改动面与出错面更大；（d）消除双调度器属调度器核心重构，**不阻塞**本次崩溃修复，且删除该写入与未来合并方向一致（合并后不再存在跨调度器写入）。
  - 状态：[X]（本轮删除写入 + 回归测试 + 端到端 QEMU 判据均已达成；方案 C 登记为后续工程）

- **DECISION-064（KPTI-18a 落点修复 + KPTI-18b 长期方案：内核零隐式 FP/SIMD）**
  - 描述：2026-09-23 用户就 KPTI-18 反问"长期最优是什么"。裁定分两层：
    1. **KPTI-18a（本轮已修）**：`arch/aarch64/context.rs` 的 FPCR/FPSR 保存/恢复落点由 **640/648 改为 656/664**，与 `ProcessContext.fpcr/fpsr` 字段、`proc/switch.asm:36-37` 注释、`context.rs` 文件头布局表三方一致；640/648 属 `fpu_state` 的 `q31` 高 16 字节，原实现造成双向污染（覆盖 V31 + 用 q31 残值写 FPCR/FPSR）。
    2. **KPTI-18b（长期最优，本轮不实施）**：不是候选 ①②③ 任一（全量保存 / lazy FPU / 仅 caller-saved），而是**"内核零隐式 FP/SIMD"**（对齐 Linux arm64 `fpsimd`）：内核不得隐式执行 FP/SIMD，用户 FP 状态由线程上下文承载并按需（懒式）保存；内核确需 FP 时须显式声明并使用独立的内核状态。理由：根因是"内核在 EL1 执行 FP/SIMD 却无声明"（实测 `CPACR_EL1.FPEN=0b11` 放开 EL1 + 内核产物 913 `fmov` / 337 `stp q*`），①②③ 均为逐次进出边界的代价补偿；且"内核不隐式用 FP"是内核工程惯例（x86_64 侧正因 `x86_64-unknown-none` 目标禁用 SSE/MMX 而天然无此问题）。
  - 落地路径（登记）：(1) `-C target-feature=-neon` 抑制隐式 NEON；(2) 消除内核 `f64/f32`（`services/mm/pmm_policy.rs`、`services/fs/procfs_core.rs`、`services/fs/nestfs/arc_trait.rs` 等）+ 反汇编审计白名单；(3) 线程 FP 懒式保存（`TIF_FOREIGN_FPSTATE` 式）。**工具链限制（实测）**：Rust/LLVM aarch64 不识别 `+general-regs-only` ⇒ Linux `-mgeneral-regs-only` 等价路径不存在；`aarch64-unknown-none-softfloat` 仅改 ABI + 去 `-neon`，标量 FP 仍在。
  - 状态：[X]（18a 已修 + 回归测试；18b 长期方案已裁定并登记，性质判定为**契约级缺陷、暂不可观测**）

### 遗留与登记项（Phase 1 收口后深度排查完成；KPTI-18a / KPTI-19 已修复）

- **KPTI-17. aarch64 fork 子进程零上下文崩溃（预存缺陷，非本轮 KPTI 改动直接导致）**
  - 描述：Phase 1 跑通后（首次让 aarch64 真正到达 EL0 并发出 `fork`），`Entering EL0` 后 4 次 SVC 成功往返，随后调度切到 pid=5 时崩溃：`sched CSW prev_pid=4 next_pid=5 next_ctx=0x444BC050 sp=0x0 ttbr0=0x0 spsr=0x0 elr=0x0` → `Exception return from AArch64 EL1 to AArch64 EL0 PC 0x0` → Prefetch Abort（`SPSR 0x0`/`ELR 0x0`）→ `SP_EL1=0` ⇒ `handle_el1h_sync` 压帧失败（`FAR 0xfffffffffffffee8`）反复级联。
  - 方案：三段因果链已确证——（a）`enter_user` 直接改写 `SP_EL0/ELR_EL1/SPSR_EL1` 进入 EL0，**从不写 `Process.context`** ⇒ pid=4（父）上下文恒全 0（`git diff` 确认**改动前同样如此**，非本轮引入）；（b）`clone.rs` `*child_ctx = parent_ctx` ⇒ pid=5 零上下文；（c）`cfs_enqueue(5)` + `Scheduler::schedule()` 切到它 ⇒ `context_switch_asm` 以 `SPSR=0/ELR=0` `eret`。
  - 状态：[X]（D1–D4 已实施并实测通过；D5 未实施，理由见下）
  - 修复路线：**方案 B（统一上下文模型）**，2026-09-23 用户裁定。以 aarch64 侧 `Process.context` 为唯一上下文载体，删除"`enter_user` 直进"与"ctx"双轨，首进程与 fork 子进程统一经"预置 ctx + 调度器首切"进入 EL0。分五组实施：
    - **D1 用户态快照捕获（根因 ①）**：`framework/syscall/dispatch.rs::syscall_dispatch_from_frame` 的 B05-55 捕获是 `#[cfg(x86_64)]`，aarch64 侧 `svc_handler` 只调架构中立的 `syscall_dispatch(num,a0..a5)`（无 frame）⇒ ctx 恒全 0。修法：在 `arch/aarch64/exception.rs::svc_handler` 入口（SVC 后、dispatch 前）调新增 `proc_save_user_regs_aarch64(pid, frame)`，把 `ExceptionFrame` 全量写入当前进程 `Process.context`。
    - **D2 `context_switch_asm` 路径分派（根因 ③④）**：保存侧不再存 live `SPSR_EL1/ELR_EL1`（syscall 中途取出的是**被打断的 EL0 状态** 0x3C0 + 用户 PC，非 EL1 续跑点），改存 `SPSR = 0x3C5`（EL1h + DAIF 屏蔽；本函数开头已 `daifset #0xF`）+ `ELR = x30`（返回地址），并新增存 `SP_EL0`。恢复侧按目标 `SPSR.M[3:0]` 分派：
      - **内核路径**（`M != 0`）：保持既有 `eret`，语义等价 x86 `mov rsp,[rsi+64]; jmp qword [rsi+56]`。**不改用 `br x30`**：`init_kernel_idle_context`（aarch64）以 `SPSR=0x5`（EL1h + 中断使能）+ `ELR=idle_entry` 预置 idle，靠 `eret` 打开中断，否则 `wfi` 永不被唤醒。
      - **用户路径**（`M == 0`，即 EL0t）：读 ctx 的 用户表/`SP_EL0`/`ELR`/`SPSR` → 写入 `KPTI_GLOBALS.user_ttbr0` → 设 `SP_EL0/ELR_EL1/SPSR_EL1` → 恢复 `extra_regs[0..7]`（x0–x7）→ `br` 到 `.vectors` 内高别名 `kpti_enter_user_trampoline`（切 TTBR0=用户表 / TTBR1=tramp 后 `eret`）。**所有 ctx 读取必须在切 TTBR0 之前完成**（切后 ctx 所在内核堆在用户表下不可达）。**实施按 DECISION-062 落地**（与本节早期草图的偏差）：`@104` 存 **EL1 侧 TTBR0**（live `TTBR0_EL1`，非用户表），用户表移存 `@136`（复用 `_fpu_pad`）；用户路径读 `@136`，内核路径读 `@104` 并把 `@136` 回写 `KPTI_GLOBALS.user_ttbr0`。另：保存侧额外存 `@112 = 0x3C5`、`@120 = x30`、`@128 = SP_EL0`、`@136 = KPTI_GLOBALS[24]`；恢复侧两条路径均先恢复 `x19–x30` 与 `sp`（`@96`）再分派，FPU 恢复块保持在分派前。
    - **D3 `SP_EL1` 归属（根因 ④）**：**不**实装 `cpu::arch::set_kernel_stack` 的 aarch64 分支——`msr spsel,#1; mov sp` 会破坏调度器自身栈。改由 `ctx` 承载目标任务内核栈顶（`@96`）：运行中任务由保存侧自动维护，未调度过的新任务（fork 子进程 / init）由创建方显式预置。
    - **D4 fork/clone 子进程 ctx 初始化（根因 ②）**：`proc/proc_ops.rs::sys_fork` 与 `syscall/clone.rs` 的子 ctx 初始化拆为分架构分支。aarch64 侧须写 `es`(@104)=子进程用户表、`ds`(@96)=子内核栈顶、`extra_regs[0]`=0（fork 返回值走 **x0**）；**禁止**沿用 `child_ctx.cr3 = cr3`（@80 在 aarch64 是 x29/FP）与 `child_ctx.rax = 0`（@48 是 x25）。**实施结果**：`sys_fork` 的 aarch64 分支写 `_fpu_pad`(@136)=`child_cr3`（子用户页表）、`es`(@104)=`vmm_build_el1_view` 返回的子视图根（原 fail-closed 检查改为绑定返回值，不重复调用）、`ds`(@96)=子内核栈顶、`extra_regs[0]`=x0=0；x86_64 分支保持原 `cr3`/`rax`。`clone.rs`（CLONE_VM 共享父页表）aarch64 分支写 `extra_regs[0]`=0、`ss`(@128)=`child_stack`、`ds`(@96)=子内核栈顶，并**新增** `map_kernel_stack_top_page(parent_cr3, 子内核栈顶)`——超出早期草图：EL0→EL1 入口在切 TTBR0 **之前**就把异常帧压入 `SP_EL1` 顶页，子线程自己的栈顶页不映射则首次陷入即 Data Abort（原 `clone.rs` 无任何 aarch64 KPTI 补建）。
    - **D5 首进程进入时序统一**：aarch64 侧 init 不再走 `proc/user_proc.rs::enter` 尾部的 `crate::arch!(enter_user(...))` 直进，改为预置 init 的 ctx（入口/用户栈/`SP_EL0`/用户表/内核栈顶/`SPSR=0x3C0`）后交调度器首切进入 EL0；随后移除 aarch64 的 `MmuArch::enter_user` 直进实现。**本轮未实施**：触面为 init 首切时序 + `MmuArch::enter_user` 契约 + `proc/user_proc.rs` 与 `usermode.rs` 调用方，属架构关键路径，且不影响本轮验收判据（init 仍经 `enter_user` 直进、其 ctx 由首次抢占的保存侧补齐）；与 KPTI-19 调查不宜并行（避免归因混淆）。
  - 详情：aarch64 `ProcessContext` 字段语义（以 `arch/aarch64/context.rs` 汇编为权威）：`x19..x28`@0..72、`x29`@80、`x30`@88、`sp`(=`SP_EL1`)@96、`ttbr0`@104、`spsr`@112、`elr`@120、`ss`@128 **改用作 `SP_EL0`**（原写 0）、`extra_regs[0..7]`@672..728 **改用作 x0–x7**（x86 语义为 rdi…r11）。`context.rs` 第 6–15 行文件头布局注释与代码不符（陈旧），随本轮一并纠正。原登记的"k3g 三项"经全仓检索确认**无对应仓库条目**（仅本工程文档自述），故直接以源码事实驱动设计，不再引用为待办。
  - 未覆盖项：aarch64 异常帧（`ExceptionFrame`）不含 V0–V31，`context_switch_asm` 保存的是**内核态** FPU ⇒ 用户态 FPU 上下文跨 syscall 不保留，登记为 KPTI-18，本轮不修（init 不使用 FP）。
  - 附带观察：`scheduler.rs:715-717` 把 pid（`u64::from(next)`）写入 `SCHEDULER_EX.current`，而其语义为 `Thread` 指针；`scheduler_ex::tick_accounting` 会 `ThreadRef::new_unchecked(current)` 解引用 ⇒ 类型混用缺陷。**已于 2026-09-23 定位为 KPTI-19 的唯一根因**（见该条目"根因"），不再作为独立观察项。
  - 实施记录（本轮改动面，共 5 文件）：`framework/proc/proc_ops.rs`（新增 `proc_save_user_regs_aarch64`；`sys_fork` 子 ctx 分架构 + 视图根绑定）、`framework/arch/aarch64/exception.rs`（`svc_handler` 入口接线；`kpti_enter_user_trampoline` 改 `pub(crate)` 供 `sym` 引用）、`framework/arch/aarch64/context.rs`（保存侧改存 `0x3C5`/`x30`/`SP_EL0`/用户表；恢复侧路径分派 + EL0 路径 + trampoline 高别名跳转；文件头与 `Aarch64Context` 字段语义表纠正）、`framework/syscall/clone.rs`（aarch64 子 ctx 语义修正 + 栈顶页补映射）。
  - 实测证据（2026-09-23，QEMU aarch64 25s 运行，日志 `build/log/qemu_boot_aarch64.log`）：`Entering EL0 (init pid=4)` → 串口出现 `X` → `exit: pid=5 code=0` → `Y` → `exit: pid=6 code=0`，即 **fork 双子进程各自从正确用户返回点续跑并以 0 退出**（改动前 aarch64 在 `enter_user_asm` 即崩，从未进入 EL0）。收集到的遗留项见 KPTI-19。
  - 验证门槛：`./ci/build.sh all`（双架构 0 error / 0 warning）、`./ci/audit.sh quick`（AUDIT_RC=0，0 处 ✗）、`make test-host`（11 passed）、`make test-unit`（QEMU 33：ALL TESTS PASSED）、`./scripts/qemu_boot_test.sh x86_64`（1/1 通过，`X`/`Y`/`exit code=0`，无 SYNC）、`./scripts/qemu_boot_test.sh aarch64`（1/1 通过，判据达成）。

- **KPTI-18. aarch64 用户态 FPU 上下文跨 syscall 不保留**
  - 描述：`ExceptionFrame`（35×8，x0–x30 + elr/spsr/sp）不含浮点/SIMD 寄存器；`context_switch_asm` 的 `stp q0..q31` 保存的是**内核态** FPU 状态。故 EL0 使用 V0–V31 后经 SVC/IRQ 往返，用户 FPU 上下文不保证保留。
  - 方案：或在异常入口增存 V0–V31（帧膨胀 512 字节），或引入 lazy FPU（对齐 `ProcessContext.fpu_state` Phase 3 预留）。属独立工程，与 KPTI-17 无耦合。
  - 状态：[]（KPTI-18a 已修复并入本轮；KPTI-18b 为独立工程，长期方案已裁定见下）
  - 详情：当前用户态程序（`src/user/init`）不使用浮点，故不影响 Phase 1 验收判据；aarch64 用户态线程库/浮点应用出现前必须解决。**另发现（本轮读汇编时确认，未修）**：`context_switch_asm` 把 `FPCR`/`FPSR` 存入偏移 640/648，而该区间属 `fpu_state` 的 `q31`（`fpu_state` 占 144..656，`q31` = 640..656）⇒ 每次保存都会覆盖 `q31`；结构体内偏移 656/664 的 `fpcr`/`fpsr` 字段从未被汇编引用（等于死字段）。
  - 深度排查结论（2026-09-23，与代码逐行核对）：
    - 覆盖链路（双向）：保存侧 `arch/aarch64/context.rs:95-116` 先 `stp q30,q31,[x2,#480]`（写 624..655，`q31` = 640..655）再 `str x2,[x0,#640]`（FPCR）/`str x2,[x0,#648]`（FPSR）⇒ **`q31` 上半（640..647）与下半（648..655）分别被 FPCR/FPSR 覆盖**；`context.rs:112` 注释"FPCR/FPSR 保存在 `fpu_state[62]/[63]`"实际就是 `q31` 的两半。恢复侧 `:153-161` 先 `ldp q30,q31,[x2,#480]` 再从 640/648 读 FPCR/FPSR ⇒ 用被污染的 `q31` 恢复 SIMD 寄存器、又用被 `q31` 污染的值写 FPCR/FPSR（双向污染）。
    - 权威口径不一致（三方）：`context.rs:19-20` 文件头布局表写 `144..656 fpu_state` + `656, 664 fpcr, fpsr`（**正确**）；`proc/switch.asm:36-37` 注释写 `+656 fpcr` / `+664 fpsr`（**正确**，x86 侧口径）；`arch/aarch64/context.rs` 汇编实装写 640/648（**错误**）⇒ 仅 aarch64 汇编偏离。
    - 死字段确认：`proc/types.rs:210-223` 的 `fpcr: u64`(@656) / `fpsr: u64`(@664) 全仓仅 `types.rs` 初始化处引用，汇编从未读写 ⇒ 与 AGENTS §5 F9（死代码零容忍）冲突，修法落地即自然消除。
  - 修复规划（架构关键路径）：
    - **KPTI-18a（机械修复）— 已完成（2026-09-23）**：`arch/aarch64/context.rs` 保存侧 `str x2,[x0,#640/#648]` → `#656/#664`、恢复侧 `ldr x2,[x1,#640/#648]` → `#656/#664`，注释同步改为"落在 `ProcessContext` 专用字段 `fpcr`(@656)/`fpsr`(@664)，不得写 `fpu_state[62]/[63]`（= q31 高 16 字节）"。落地后与 `ProcessContext.fpcr/fpsr`、`proc/switch.asm:36-37`、`context.rs` 文件头布局表三方一致，656/664 死字段（F9 冲突）自然消除。回归测试：`host-tests/tests/aarch64_fpu_ctx_offset_test.rs`（断言汇编使用 656/664 且不含 640/648）。
    - **KPTI-18b（语义修复）— 长期方案已裁定（DECISION-064）**：候选与取舍——①全量保存（每 SVC/IRQ +528B 帧，语义最简）；②lazy FPU（实现复杂度最高）；③仅 caller-saved（成本约 ① 的 3/4，但偏离 `context_switch_asm` 全量口径）。**长期最优 = 上述三者之外的第 ④ 条："内核零隐式 FP/SIMD"（对齐 Linux arm64 `fpsimd` 模型）**：根因不是"边界没保存"，而是"内核在 EL1 执行 FP/SIMD 却无声明"，①②③ 都只是"每次进出边界付代价"的补偿。实测证据：(a) `CPACR_EL1.FPEN=0b11` 放开 EL1 FP 访问（`boot/aarch64/start.S:74-76`、`boot/aarch64/entry.rs:30-32`）；(b) aarch64 内核产物实测含 **913 `fmov` / 337 `stp q*`**（编译期 NEON 用于 memset/memcpy/结构体清零 + `services` 侧显式 `f64`），函数级抽样命中 `Framebuffer::draw_line_aa`、`Ext2Bitmap::count_used`、`Ext2SuperBlock::from_bytes`、`FallbackPmmPolicy::fragmentation_score`、`zerocopy::f32_ext::to_be_bytes`；(c) EL0 入口 `ExceptionFrame` 不含 V0–V31/FPCR/FPSR；(d) `context_switch_asm` 只覆盖"切换时刻"的 live 寄存器 ⇒ 内核一旦执行 FP/SIMD，用户 V0–V31 即被破坏。落地三层：(1) 编译期 `-C target-feature=-neon` 抑制隐式 NEON 代码生成；(2) 源码消除内核 `f64/f32`（`services/mm/pmm_policy.rs:52`、`services/fs/procfs_core.rs:179-207`、`services/fs/nestfs/arc_trait.rs:82` 等改整数运算）并加反汇编审计脚本（FP/SIMD 指令白名单仅 `context_switch_asm`）；(3) 线程 FP 状态懒式保存（对齐 `TIF_FOREIGN_FPSTATE`）。**Rust 侧限制（实测）**：`-C target-feature=+general-regs-only` 不被 rustc/LLVM aarch64 识别（Linux `-mgeneral-regs-only` 的等价路径**不存在**）；`aarch64-unknown-none-softfloat` 仅改 ABI + 去 `-neon`（`features: +v8a,+strict-align,-neon`），仍保留标量 FP ⇒ 只能走上述三层。**本轮不实施**（独立工程），理由：当前用户程序不使用 FP，不影响任何验收判据。
  - 性质判定：**契约级缺陷（非"潜在风险"）**——内核确实在 EL1 执行 FP/SIMD 且边界不保存 ⇒ "用户 FP 上下文跨 SVC/IRQ 保留"的契约已被破坏；仅因当前用户态不使用 FP 而**暂不可观测**。

- **KPTI-19. aarch64 双进程退出后 EL1 数据异常（根因已定位：`SCHEDULER_EX.current` 被写入 pid 而非 `*mut Thread`）**
  - 描述：KPTI-17 D1–D4 落地后，aarch64 QEMU 运行达验收判据（`X`/`Y`/`exit code=0`）约 2 ms 后崩于 EL1 数据异常：`SYNC! ESR=0000000096000061 FAR=000000000000002A ELR=FFFF000040111CC0`。ESR 解码：EC=0x25（同 EL 数据异常），DFSC=0x21（对齐异常）。
  - 方案：**已定位唯一根因并完成修复（DECISION-063）**。根因与实施记录见下；原"aarch64 特有""归因未定论"结论已被证伪并纠正。
  - 状态：[X]
  - 根因（2026-09-23 静态 + 运行时双重证据锁死；**纠正**：原登记称 ELR 落在 outlined `Arc::clone`，实为 outliner 生成的 64 位 `fetch_add(1)` 共享出口，与 `Arc` 无关）：
    - 崩溃点 = `framework/proc/scheduler_ex.rs::SchedulerEx::tick_accounting`（L604-609）：`let current = self.current.load()` → `ThreadRef::new_unchecked(current as *mut Thread)` → `fetch_sub_time_slice()`（`Thread.time_slice` @32）/ `fetch_add_cpu_time()`（`Thread.cpu_time` @40）。
    - ELR `0x40111CC0` 反汇编（`aarch64-linux-gnu-objdump`）= `ldaxr x9,[x8]; add x9,x9,#1; stlxr w10,x9,[x8]; ret`；结合 `FAR=0x2A` ⇒ `x8 = self.current + 0x28`，即 `self.current` 被当作 `*mut Thread` 解引用后访问 `cpu_time`。
    - QEMU `-s -S` + gdb 实测（断在共享出口 `0x40111CC0`，条件 `x8 < 0x100000`）：`x21 = self.current = 2`、`x19 = &SCHEDULER_EX`、`x30` 落在 `tick_accounting` 内 ⇒ **`SCHEDULER_EX.current` 的取值是一个 pid（2），不是 `*mut Thread`**；`x8 = x21 + 0x28 = 0x2A` 与日志 `FAR` 完全吻合。
    - 写入方唯一性（全仓 `SCHEDULER_EX` 共 16 处引用）：语义为 `*mut Thread` 的有 `scheduler_ex.rs:564`（init 存 idle `Thread`）、`:693`（`schedule()` 存 next `Thread`）、读点 `:604/663/784/832` 与 `sched_ops.rs:24`（`scheduler_current_cputime` 读 `Thread.cpu_time`）；**唯一以 pid 语义写入的是 `scheduler.rs:714-716`**（`SCHEDULER::schedule()` 内 `SCHEDULER_EX.current.store(u64::from(next), SeqCst)`）。
    - 触发链：`SCHEDULER::schedule()` 切换任务（写 pid）→ 下一个定时器 tick → `scheduler_tick()`（`sched_ops.rs:95` → `SCHEDULER_EX.tick()` → `tick_accounting()`）⇒ 解引用 pid ⇒ 崩溃。`SCHEDULER_EX.schedule()` 在生产路径无独立调用者（仅 `tick()`/`yield_current()` 内部触发），故 `SCHEDULER_EX.current` 实际取值几乎恒为这条错误写入的 pid。
  - 为何 x86 未复现（2026-09-23 实测确认，**证伪"aarch64 特有代码"假设**）：
    - x86 执行同一条缺陷路径：gdb 断点实测 `timer_irq0_handler` 与 `scheduler_tick` 均命中；`Scheduler::schedule` 内 `0x217a8e: xchg %rax,0x60(%rcx)`（`rcx = &SCHEDULER_EX`）实测 `rax = 0x5` ⇒ **x86 同样把 pid 写进 `SCHEDULER_EX.current`**；`tick_accounting`（`0x1a3d30`）的字段偏移与 aarch64 一致（`mov 0x60(%rdi),%r14` → `lock xadd %ebp,0x20(%r14)` / `lock incq 0x28(%r14)`）。
    - 差异在体系结构对**非对齐原子访问**的容忍度：aarch64 独占访问要求自然对齐 ⇒ DFSC=0x21（Alignment fault）直接陷入；x86 允许非对齐 `lock` 访问，且实测 VA `0x22`/`0x2A` 在 x86 内核页表中**已映射**（gdb 读 `0x2a` 返回 `0xd422f000d422f000`）⇒ 原子写成功、不陷入。
    - **结论**：x86 表现为**静默内存破坏**（向低地址 VA `0x22`/`0x2A` 写时间片与 CPU 时间），比 aarch64 的显式崩溃更隐蔽 ⇒ 两架构都必须修，修法同源。
  - 修复实施（方案 A′：「删除跨层写入」，2026-09-23 完成；DECISION-063）：
    - 落地改动：删除 `framework/proc/scheduler.rs` 中 `SCHEDULER::schedule()` 内的 `super::scheduler_ex::SCHEDULER_EX.current.store(u64::from(next), SeqCst)`（3 行），并在原址留中文注释说明"此处不得写 + 原因 + SIMPLIFIED 标记"。`SchedulerEx::current` 恢复**净写者不变式**（仅 `init` 写 idle / `schedule` 写 next 两处），`tick_accounting` 的 `// SAFETY: current 由调度器自管理, 必指向有效 Thread` 重新成立。
    - **与早期草图的偏差（重要）**：原"方案 A"拟经 `THREAD_MANAGER::find_by_pid(pid)` 解析出 `*mut Thread` 后写入。本轮核实**该方案不可行**：`ThreadManager::create_thread`（唯一 `THREAD_TABLE` 插入点）在全仓**无生产调用者**，非测试代码中 `Thread` 对象仅 `SchedulerEx::init` 的 idle 一个 ⇒ 进程没有对应 `Thread` 可解析，`find_by_pid` 恒返回 `None`；按 AGENTS §9"找不到调用点的虚项必须退「未来功能」，禁止为无意义调用造实现"，改为直接删除跨层写入。
    - 语义影响：`SCHEDULER_EX` 的线程级记账只作用于其自身 idle 线程（进程级 `user_time`/`sys_time` 记账由 `proc_account_tick` 经 `CURRENT_PROCESS_PTR` 独立承担，不受影响，`TD-10` 契约测试仍通过）；`sched_ops::scheduler_current_cputime` 由"读 pid 解引用（未定义行为）"变为"读 idle 的 `cpu_time`（有定义）"。`credo_proc_cputime` 忽略 `target_pid` 的既有质量问题不属本项，未动。
    - 回归测试：`host-tests/tests/sched_current_type_consistency_test.rs` —— (a) `scheduler.rs` 去注释后不得访问 `SCHEDULER_EX.current`；(b) `scheduler_ex.rs` 中 `self.current.store(` 恰为 2 处。
    - 端到端判据（aarch64 QEMU 实测，2026-09-23）：修复前"达判据后约 2ms 出现 `SYNC! ESR=0x96000061 FAR=0x2A`"；修复后 `build/log/qemu_boot_aarch64.log` 共 1649 行、`SYNC|panic|ESR=|Data Abort|EXCEPTION` 匹配数 **0**，时间线由 0.102s（`Entering EL0`）推进至 3.486s（`[NET] DHCP deconfigured` 计时器循环）⇒ 崩溃消除。
  - 长期方案（登记，超本轮范围）：**方案 C —— 消除双调度器**：`SCHEDULER`（per-CPU + `PROCESS_TABLE`，pid 为主键，含 CFS/RT/DL 与负载均衡）与 `SCHEDULER_EX`（`run_queues` + `*mut Thread`，含 5 级 MLFQ/冻结/僵尸回收）并存且**各自独立做硬件上下文切换**，是 `current` 双语义的结构性来源（方案 B「统一为 pid」因 `run_queues` 节点为 `*mut Thread` 而需反复互转，取舍更差）。消解方式为"线程为调度实体、进程为容器"的单一调度器；前置条件是先完成线程层接线（`create_thread` 无调用者）。
  - 详情：`framework/proc/scheduler.rs` 的 `SCHEDULER::schedule()` 曾以 pid 语义写 `SCHEDULER_EX.current`，是两套调度器 `current` 双语义的**唯一泄漏点**（已于本轮删除）。**本项与"调度器 pid/Thread 指针双语义"为同一根因，合并处置，不另立条目。**
  - 验证门槛（2026-09-23，本轮修复后全量复跑）：`./ci/build.sh all` → `Passed: 5 Failed: 0`；`./ci/audit.sh quick` → `AUDIT_RC=0`（含 clippy pedantic 与 kernel_test / host-test 两维）；`make test-host` → 全绿（含新增 2 个回归测试文件）；`make test-unit` → `✅ ALL TESTS PASSED (QEMU exit: 33)`；`./scripts/qemu_boot_test.sh all` → **2/2 通过**（x86_64 252 行 / 里程碑 `VFS ready` + Ring 3；aarch64 1649 行 / `VFS ready` + `virtio-net` 桥 + `Entering EL0`），两架构日志致命异常匹配数均为 0。

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
