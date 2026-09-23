# aarch64 高半区内核迁移（L1，独立工程）

> 来源：KPTI 完整化工程（[kpti-complete-project.md](./kpti-complete-project.md)）Phase 1 收口时擢升为独立工程。
> 决策依据：aarch64 内核当前驻留低半区恒等映射，迫使 KPTI 的 per-process EL1 视图以 DRAM 1 GiB 块把内核镜像/数据/栈纳入 `TTBR0` 视图；迁移到高半区后 EL1 视图回归"只承载用户页"的自然形态。

## 背景与动机

aarch64 侧 `KERNEL_BASE` 取 [mm/mod.rs:184](../../src/kernel/framework/mm/mod.rs#L184) 的 `0`，即内核**恒等映射**在低半区（镜像 @ PA `0x40080000` ⇒ VA `0x40080000`）。而 aarch64 用两条 TTBR 划分地址空间：

- `TTBR0_EL1` → 低半区 `0x0000_0000_0000_0000 - 0x0000_FFFF_FFFF_FFFF`
- `TTBR1_EL1` → 高半区 `0xFFFF_0000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF`

低半区 VA 由 `TTBR0` 承载。KPTI 全切换模型下 EL0 与 EL1 使用**不同的** `TTBR0`（EL0 视图 vs EL1 视图），因此内核代码/数据既然住在低半区，EL1 视图就必须把它一并纳入（[vmm_aarch64.rs](../../src/kernel/framework/mm/vmm_aarch64.rs) 的 `L1_el1[1]` = 内核 `L1_IDMAP[1]` DRAM 1 GiB 块）。

**针对 x86_64 的对照**：x86 内核住高半区（`KERNEL_BASE = 0xFFFF800000000000`），低半区只承载用户映射；切 CR3 时高半区由同一套内核页表条目覆盖，内核不需要出现在"用户 CR3 的低半区"。aarch64 的等价形态要求内核住 `TTBR1` 领地。

## 现状盘点

`KERNEL_BASE` 在 `src/` 下共 **154 处 / 36 文件**（`grep -c KERNEL_BASE src/` 实测）。引用面分布（Top）：

| 文件 | 处数 | 备注 |
|---|---|---|
| [proc/user_proc.rs](../../src/kernel/framework/proc/user_proc.rs) | 24 | 用户态装载 / 栈地址换算 |
| [mm/vmm_x86_64.rs](../../src/kernel/framework/mm/vmm_x86_64.rs) | 21 | x86_64 专属，aarch64 迁移不需改动 |
| [mm/pmm.rs](../../src/kernel/framework/mm/pmm.rs) | 16 | `phys_to_virt` 族 |
| [proc/process.rs](../../src/kernel/framework/proc/process.rs) | 8 | 内核栈地址 |
| [mm/kpti.rs](../../src/kernel/framework/mm/kpti.rs) | 6 | x86_64 专属 |
| [memory_allocator.rs](../../src/kernel/memory_allocator.rs) | 7 | aarch64 取 0，非 aarch64 取高半区 |
| 其余 30 文件 | ≤5 各 | — |

关键收敛点：
- [memory_allocator.rs:13](../../src/kernel/memory_allocator.rs#L13) `#[cfg(aarch64)] KERNEL_BASE = 0` —— 迁移的核心开关
- [mm/mod.rs:184](../../src/kernel/framework/mm/mod.rs#L184) `#[cfg(aarch64)] pub const KERNEL_BASE = 0` —— 与上者同源
- `phys_to_virt` 族（pmm.rs / kpti_aarch64.rs `HIGH_ALIAS_BASE`）—— 迁移后低半区恒等假设失效，需改为"物理地址 → 高半区别名"

> 说明：`vmm_x86_64.rs`、`mm/kpti.rs` 等 x86_64 专属文件的引用**不计入** aarch64 迁移改动面，实际待评估面向量小于 154。

## 任务分解

- **L1-01. 迁移面精确盘点**
  - 描述：把 154 处引用按"编译目标"筛分：aarch64 参与编译的实改动面 vs x86_64 专属（`#[cfg(target_arch = "x86_64")]` 或 x86 文件）虚改动面。
  - 方案：逐文件核对 cfg 门控，产出"实改动清单"（文件 + 行 + 语义分类：地址换算 / 链接脚本 / 常量定义 / 断言）。
  - 状态：[]

- **L1-02. 内核 VA 基址决策**
  - 描述：确定 aarch64 高半区基址（候选：`0xFFFF_0000_4000_0000`，即现有 DRAM 别名的自然延伸；或独立于别名的 `0xFFFF_8000_0000_0000`，与 x86 `KERNEL_BASE` 对齐）。二者取舍影响 EL1 视图、`phys_to_virt` 族与链接脚本。
  - 方案：**决策灰色地带**（§9.1）——须由用户裁定，不在本工程自行选择。列候选 + 各自对 EL1 视图/别名体系的影响后再施工。
  - 状态：[]

- **L1-03. 链接脚本与启动路径迁移**
  - 描述：[link/aarch64.ld](../../src/kernel/framework/link/aarch64.ld) 当前按低半区地址布局（`. = 0x40080000` 级别）；`entry.rs`/bootloader 以恒等地址取指。迁移须改链接脚本 LMA/VMA 与早期 MMU 开启前的恒等窗口。
  - 方案：保留 boot 早期恒等映射作为跳板，MMU 开启后跳转高半区；`_vectors` 段的高别名/新基址映射同步调整。
  - 状态：[]

- **L1-04. `phys_to_virt` 族收敛**
  - 描述：迁移后"PA 与低半区 VA 相同"的假设失效。`pmm.rs`、`kpti_aarch64.rs` 的 `HIGH_ALIAS_BASE`、`iomem.rs::mmio_virt` 需统一到单一"PA → 内核 VA"换算入口。
  - 方案：以 `KERNEL_BASE`/`HIGH_ALIAS_BASE` 为唯一来源，消除分散的隐式恒等假设；audit 侧补断言。
  - 状态：[]

- **L1-05. KPTI EL1 视图简化**
  - 描述：迁移完成后，EL1 视图不再需要 DRAM 1 GiB 块（内核经 `TTBR1` 高半区可达）。`L1_el1[1]` 条目可移除，EL1 视图缩为"用户半区 + `L0_el1[0]→L1_el1`"。
  - 方案：改 [vmm_aarch64.rs](../../src/kernel/framework/mm/vmm_aarch64.rs) `build_el1_view`；同步 §KPTI-13 描述与 host-tests 断言。
  - 状态：[]
  - 详情：**本项是 L1 的收益兑现点**；未完成前 S3 结构（含 DRAM 块）仍是有效形态，非缺陷。

- **L1-06. 回归与验证**
  - 描述：迁移后双架构不回归。
  - 方案：§2.3 五门槛（`./ci/build.sh all` / `./ci/audit.sh quick` / `make test-host` / `make test-unit` / QEMU x86_64+aarch64 boot）；QEMU aarch64 需确认 `Entering EL0` 后 SVC 往返仍成立（KPTI-06 判据）。
  - 状态：[]

## 验证标准

- §2.3 5 条门槛全过（双架构 build/clippy/test-host/test-unit/QEMU）
- QEMU aarch64：`Entering EL0` 之后 SVC 往返日志成立（沿用 KPTI-06 判据）
- `KERNEL_BASE` 引用面全部完成"目标口径"复核（L1-01 清单逐项勾销）
- L1-05 完成后，host-tests 断言 EL1 视图**不含**内核 DRAM 块

## 风险与回退

- **启动期恒等窗口**：MMU 开启前指令取指依赖恒等映射，迁移需保留一段 boot 跳板窗口；漏处理 ⇒ 启动即 Prefetch Abort。缓解：分步（L1-03 先迁链接脚本、L1-04 后迁换算），每步 QEMU 验证。
- **分散恒等假设**：154 处引用中隐含"PA == VA"的部分未必显式写 `KERNEL_BASE`。缓解：L1-01 深挖 + audit 脚本兜底。
- **与 KPTI 交织**：迁移期间 KPTI 的 EL1 视图处于过渡态（含 DRAM 块），须避免两工程并行施工导致 QEMU 问题难以归因（同 §9.1 T1/T5 并行约束的先例）。
- **回退**：`KERNEL_BASE` 两处常量（memory_allocator.rs / mm/mod.rs）回退为 `0` 即可恢复低半区形态；L1-05 未做前 S3 结构可原样保留。

## 关联

- 上游：[kpti-complete-project.md](./kpti-complete-project.md)（KPTI-13 EL1 视图 S3 / 遗留登记项 L1）
- 重叠待办：**k3g**（`context_switch_asm` 用户表语义 + 目标 EL0 时切 TTBR1=TRAMP + aarch64 `set_kernel_stack` 空实现）——与 aarch64「fork 子进程零上下文」缺陷合并设计（见 KPTI-17）