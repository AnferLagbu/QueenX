# 审计修复分册 09：硬规则合规与死代码治理

> 修复 F1/F9 硬规则违反（services 缺 deny、host-tests allow(dead_code)）与全项目死代码（R1-R4 分类 + framework→services 反向依赖 D8）。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.6 节 + 第 6.5 章 + 第 7 章 TOP 20 + 第 11 章决策点。

## 工程计划 A: 硬规则合规（F1/F9）

### 背景

- **B09-01. F1/F9 硬规则违反**
  - 描述：services 42 文件缺 `#![deny(unsafe_code)]`（F1）；host-tests 20 处 `#![allow(dead_code)]`（F9 零容忍）。
  - 方案：一次性补齐 deny；死代码 allow 通过实现使用路径消除。
  - 状态：[X]（2026-09-14：子项 B09-02（deny 补齐）+ B09-03（host-tests allow 消除）均完成）

### 待办

- **B09-02. services 缺 deny(unsafe_code) 补齐（P0-22）**
  - 描述：非 vendored 260 文件中 42 个缺 `#![deny(unsafe_code)]`（wasm/wasi 9、fs/nestfs 16、driver/display 3、fs/snapshot.rs、fs/xattr.rs、proc/canary.rs、proc/memfd.rs、proc/oomd.rs、proc/pidfd.rs、sync/lockdep.rs、config/*、timer/mod.rs、credo/storage/disk.rs 等）。
  - 方案：一次性在缺 deny 文件首行添加；含 unsafe 的先迁移（分册 01 F2 门禁修复后验证）。
  - 状态：[X]（2026-09-14：全量实测（排除 vendored smoltcp）services 层**全部文件均含 `#![deny(unsafe_code)]` 属性**——head-5 检测曾误报 52 文件缺（实际 deny 在 `//! doc` 之后），经正则匹配属性（非 doc 字样）确认无缺失；文档登记 42 缺为历史状态，期间治理已补齐）

- **B09-03. host-tests allow(dead_code) 消除（P0-23）**
  - 描述：host-tests/src + tests 下 20 处 `#![allow(dead_code)]` 违反 F9 零容忍。
  - 方案：逐处审查——真死代码删除，被 cfg 引用则用 cfg_attr 精确化；不保留裸 allow。
  - 状态：[X]（2026-09-14：grep 命中 10 处中仅 1 处真实残留——tests/common/mod.rs:22 `#![allow(dead_code)]`（文件无实际 helper 代码，为空属性），已删除并注明治理；其余 9 处为注释描述/静态断言检查非真实 allow。host-tests 755/0 全绿）

- **B09-20. src/kernel 核心 allow 抑制消除（2026-09-11 登记，F9 隐性死代码治理）**
  - 描述：2026-09-11 隐性死代码全量扫描（编译器 dead_code lint 三态：默认/kernel_test/host-test 链 **均 0 warning**，私有零引用项为零）。**死代码抑制全形式清点**（Rust 全部抑制机制，排除 vendored smoltcp）：
    - `#[expect(dead_code)]`/`#[expect(unused*)]`：**0 处**
    - 多 lint 合并 allow 含 dead_code：**0 处**；带 reason 的 allow(dead_code)：**0 处**
    - **模块级 `#![allow(dead_code)]`：1 处**（framework/constants/limits.rs:12——"各模块按需使用"豁免，F9 违规，对应 D-7 已登记）
    - `#[allow(unused*)` 单项：3 处（debug/ebpf_verifier.rs:28 unused_imports、idt/safety.rs:7 unused_imports、dma/engine.rs:422 unused_variables）
    - **`#[cfg_attr(..., allow(unused*))]` 条件抑制：1 处**（framework/dma/engine.rs:511 `#[cfg_attr(target_arch = "x86_64", allow(unused_variables))]`——x86_64 专用豁免，F9 违规）
    - 全局配置核查：Cargo.toml 无 `[lints]`/`[workspace.lints]`；clippy.toml 无死代码抑制——**无全局死代码豁免**
    - 合计 **5 处**抑制残留（非死代码类的 `#[expect(clippy::*)]`/`#[allow(clippy::*)]` ~150+ 处为规范豁免，带 reason 合规，非 F9 范畴）
    - **死代码类抑制实测闭环（2026-09-11，双架构 clippy 验证）**：
      - **冗余移除 2 处**：limits.rs:12 模块级 `#![allow(dead_code)]`（全为 pub const，不受 dead_code 管辖，移除后双架构 0 触发）；dma/engine.rs:422 `#[allow(unused_variables)]`（cache_flush 无未用变量，双架构 0 触发）
      - **有使用者保留 3 处**（加注释说明）：ebpf_verifier.rs:28 unused_imports（x86_64 下 BpfInsn 未用）；dma/engine.rs:510 cfg_attr(x86_64) unused_variables（x86_64 下 cache_invalidate 的 addr/size 未用）；idt/safety.rs:7 unused_imports（**aarch64 下 KERNEL_BASE 未用**——x86_64 不报、aarch64 报，实测揭示跨架构差异）
      - 连同 mmu.rs:182（allow(clippy::identity_op)，见下）**合计净移除 3 处冗余抑制**；最终 x86_64 + aarch64 `clippy -D warnings` 均 0 warning
    - **allow(clippy) 冗余核实（2026-09-11，实测闭环）**：22 处 allow(clippy) 逐处实测——
      - **5 处 upper_case_acronyms（errno/syscall·types/nestfs·bp/cgroup/klog·mod）实测移除后 clippy 报 87 处触发（全大写 enum 变体 EPERM 等）——有使用者，保留**（静态曾误判，实测纠正）
      - **17 处静态确认项二轮实测（2026-09-11）**：
        - **9 处冗余移除**：aarch64 模块级 7 处（exception/gic/psci/uart/mod/kpti/vmm 的 cast_possible_truncation/sign_loss + vmm wildcard——实测双架构 0 触发，静态"必然触发"判断被推翻，多为扩展/同宽 cast 不触发 lint）；user_proc:647 too_many_arguments（双架构 0 触发）；klog:110 cast（双架构 0 触发）
        - **8 处有使用者保留**：mmu:134 identity_op（0b00<<14 触发）；virtio/net:570 absurd_extreme_comparisons（**仅 aarch64 恒真比较触发**）；test_proc:99 eq_op；userptr:178 should_implement_trait；nestfs/arc:79 slow_vector；nestfs/dataset:39 should_implement_trait；boot_image:62 explicit_auto_deref
      - **合计本轮移除 12 处冗余 allow**（含此前 mmu.rs:182 identity_op）；x86_64 + aarch64 `clippy -D warnings` 均 0 warning
  - 方案：逐处核实——真死代码删除或接入使用路径；cfg 门控引用则 cfg_attr 精确化；不保留裸 allow（对齐 B09-03 治理模式）。
  - 状态：[X]（2026-09-11 核实与消除已完成：死代码类 5 处 + allow(clippy) 22 处全部实测定性，**移除 12 处冗余**（limits.rs 模块级 dead_code、dma/engine.rs:422 unused_variables、mmu.rs:182/134 见上、aarch64 模块级 cast×6 + wildcard、user_proc too_many_arguments、klog cast），14 处确认有使用者保留并加注释；x86_64 + aarch64 clippy -D warnings 0；剩余 F9 死代码类治理（如未来新增代码规范）并入 B09-03 统一把关）

## 工程计划 B: 死代码分类治理（R1-R4）

### 背景

- **B09-04. 死代码零容忍（§9.3）**
  - 描述：第 6.5 章对死代码分类标注：R1 pub fn 死代码 362 项、R2 未接线 syscall 161 项、R3 零引用 pub mod 36 项、R4 核心 pub struct/enum 零引用 1 项。
  - 方案：按处置工作流（阶段 1 高确定删除 → 阶段 2 中确定 → 阶段 3 决策类）推进。
  - 状态：[]

### 待办

- **B09-05. R2 未接线 syscall 处置（161 项）**
  - 描述：表 A `[A:激活]` 5 项（dispatch 缺项，函数已实装）→ 接线；表 R `[R:替代]` 119 项（QX_* 备用命名，禁用）→ 核实后删除或保留；表 D `[D:删除]` 37 项（真未实装）→ 删除。
  - 方案：用 `audit_unwired_pub_fn.py`（分册 01 修复后）生成清单，按表分类逐项处置。
  - 状态：[]

- **B09-06. R1 pub fn 死代码（362 项）**
  - 描述：pub fn 死代码 362 项，高密度文件 Top 5 需优先（vfs/api.rs、syscall/types.rs 等）；已确认 `[X:CFG]` 跨架构项 3 项保留。
  - 方案：按模块分布 Top 10 逐文件清理；跨架构项保留并标注。
  - 状态：[X]（2026-09-18：T5 批 1 完成，实测 R1 = 447 → 438。**核实结论修正**：R1 主体为有意保留的 API 面（services 安全代理壳 / FS mount API 面 / 调试统计查询面 / 原语预留 / 驱动硬件面），非"死代码"；批 1 仅删确证无用 9 项。余项按 [syscall-followup.md](syscall-followup.md) T5 台账分批判别）
    - **2026-09-18 T5-B 收尾（六次修订）**：R1 口径 **438 → 437**（`write_log_line` 试删已删除）；**T5 甄别已完成**——删候选 **0** / 完整性保留 **75**（43 硬件原语 + 族残缺 2 + B-5.2 判据待补定桶 32）/ 待裁 **2**（ramfs，安全面待 reviewer 授权）/ 接线 **8**（移出，另立功能接线批次）/ 未来功能 **352**（339 + B-5.1 等路线图 13）；**未分类项 = 0**（B-5 归零表逐项三态 + 三字段）。**剩余工作不在本项内**：CI 噪音治理单开 **B09-21**（裁定五，T5 关闭条件之一）；**本项（B09-06）状态在 B09-21 落地后随 T5 一并置 `[X]`**（已达成）。
    - **2026-09-18 T5-C 关闭（七次修订）**：**T5 五条验收全部达成 ⇒ T5 已置 `[X]`**；B09-21 已落地（台账 **B-6** 区块 437 项 + 脚本 `load_classified_set()` 降噪，实测 **HIGH=0 / INFO=437**）；ramfs 2 项试删**门槛失败退桶**（host-tests 文本计数断言）。详见 [syscall-followup.md](syscall-followup.md) **B-7**。
    - **2026-09-18 甲批 C-1（八次修订；本项最终口径）**：**R1 口径 437 → 436** —— `framework/fs/vfs/vfs.rs::set_fd` 由 C-1 接线落地（`vfs_open_internal` 两分支填充 fd 表元数据）⇒ **不再零引用**，已从 **B-6** 区块移除；接线桶 **8 → 0**、未来功能 **352 → 359**、合计 **436**（`0 + 75 + 2 + 0 + 359`，算术闭合）。实测 `audit_unwired_pub_fn.py`：**HIGH=0 / INFO=436（＝ R1 435 + R4 1）**。详见 [syscall-followup.md](syscall-followup.md) **B-8.4 / B-8.5**。

- **B09-07. R3 零引用 pub mod（36 项）**
  - 描述：36 个 pub mod 零引用。
  - 方案：核实后删除（或确认 cfg 条件引用）。
  - 状态：[]

- **B09-08. R4 核心 pub struct/enum 零引用（1 项）**
  - 描述：核心 pub struct/enum 零引用 1 项。
  - 方案：核实后删除。
  - 状态：[]

- **B09-09. framework 78 处 pub use re-export 反向依赖（H.3.5 P2-B，2026-09-11 定性修正）**
  - 描述：原登记"services 策略上移违反 OSTD Minimalism"——**2026-09-11 依据 Asterinas framekernel 定义（APSys'24）定性为错误分类**：策略放 services（Service OS）是 framekernel 标准设计（Asterinas aster-kernel 承担 all OS policy），OSTD Minimalism 约束的是 Framework 层最小化而非 Services 策略量——**"策略上移 vs Minimalism"冲突不存在**。P2-B 实际内容 = framework 约 78 处 `pub use crate::kernel::services::*` re-export 壳（config/credo/driver/nestfs 等），属 **F2 反向依赖**（framework 引用 services 类型）。
  - 方案：**合并入 B09-13**（全量反向依赖治理）——撤销 framework re-export 壳，services 类型只经顶层 API 暴露。不构成独立架构策略决策点。
  - 状态：[X]（2026-09-11 定性修正完成，处置并入 B09-13）

- **B09-10. 28 处 TODO(TRACK-...) 注释（H.3.5 P2-C）**
  - 描述：28 处 `TODO(TRACK-...)` 注释违反 AGENTS.md §9.4"不留 TODO"；其中 ISSUE-SRC-002（Ed25519）等已在分册 07 登记。
  - 方案：逐一处置——实装、转 plan 任务或删除；完成后 grep 复核为 0。
  - 状态：[X]（2026-09-14 处置完成，全仓 grep `TODO(TRACK-` + 普通 `TODO` 均 0 残留）：
    - **8 处过时删除**（功能已实装，注释残留）：framework/syscall/types.rs 的 mremap/getitimer/setitimer/clone/hard links/symlinks/fchown/times（dispatch 均已接线）。
    - **16 处转 plan**（简化实现 + 完整实装待办，代码删 TODO 标记保留简化实现）：tickless hrtimer 集成、uefi EFI_SYSTEM_TABLE 解析 + SetTime、power 调频压 + S3 挂起、shadow_stack PMM 物理页 + CR4 #GP 检测、signal 处理注册/blocked 位图/分发 ×4、idt CPUID 完整解析、iouring VFS fd 表/网络异步/超时/缓冲区注册 ×4。
    - **2 处 host-tests 注释引用更新**：td11_12_13（历史清理描述）、mmap_pwm_test（TRACK-5B3EBC 失同步引用，内核 TRACK- 已清零）。
    - **10 处普通 TODO 转 plan**（去 TODO 标记，保留描述 + 登记引用）：xhci Event Ring、oomd SIGKILL、memfd per-process fd 表 + CLOEXEC、pidfd Task 4、handle.rs per-process fd 表、ext2/exfat/overlayfs/nestfs 时间戳更新 ×4。
    - **2026-09-18 乙批（引用错配澄清）**：**本项（B09-10）＝ TODO 治理任务，不是 per-process fd 表工程**；但 [syscall-followup.md](syscall-followup.md) **B-8.3 / B-9.5** 与 `framework/fs/vfs/handle.rs:61` 注释均把「per-process fd 表」路线图挂在本编号下 ⇒ **标签错配**，待 reviewer 分配新编号后统一改指。per-process fd 表／FD_CLOEXEC 的独立登记条目 + 成本线索（Plan B 表已实装）见该文件 **B-9.5**。

- **B09-17. QX_* 私有编号归位 SYS_*（2026-09-09 新增，R2 syscall 编号空间治理）**
  - 描述：实测 2026-09-09 `audit_unwired_pub_fn.py` 扫描（分册 1 修复版）：R2 未接线 syscall 157 项 = **SYS_* 38 项 + QX_* 119 项**；另有 dispatch 已接线的 QX_* 46 项。经 Linux x86_64 syscall 表对照，**大量基础 syscall 错误挂在 QX_* 私有区（500+）**，违背编号空间设计（DECISION-037：0-299 直接用 Linux 标准编号、500+ 留给 QX 独有功能）。
  - 方案（编号空间归位）：
    - **应转 SYS_*（133 项）**：Linux 已有标准编号的功能从 QX_* 迁至 SYS_*——R2 未接线 116 项（read/write/open/mmap/fork/clone/kill/futex/epoll/timerfd/... 全为基础 Linux syscall）+ dispatch 已接线 17 项（RT_SIGRETURN→15、SECCOMP→317、PRCTL→157、EXECVE→59、SETRLIMIT→160、TGKILL→234、SENDFILE→40、SPLICE→275、UNSHARE→272、SETNS→308、BPF→321、GETSOCKNAME→50、GETPEERNAME→51、TCGETPGRP→130、TCSETPGRP→133、KEXEC→246、IO_URING_*→425/426/427）。处置：dispatch 增 SYS_* 分支（复用已实装函数）+ types.rs 编号对齐 Linux + audit_unwired_pub_fn.py 应判定为已接线。
    - **保留 QX_*（32 项）**：Linux 无对应 syscall 的 QX 独有功能——R2 未接线 3 项（FB_OPEN/FB_MMAP/FB_RELEASE，framebuffer 走 ioctl/DRM 故无独立 Linux syscall）+ dispatch 已接线 29 项（FW_LOAD/FW_GET/FW_GET_INFO/FW_DETACH、FTRACE_ENABLE/DISABLE/READ/STAT、KGDB_ENTER、ROUTE_ADD/DEL/QUERY、NF_ADD_RULE/NF_DEL_RULE、CGROUP_CREATE/DESTROY/ATTACH/SET_LIMIT/GET_STAT、PM、SECURE_BOOT、TPM、CET、TICKLESS、TIMESYNC、UEFI）。处置：保留 500+ 区，按治理决策接线或登记预留。
    - **R2 SYS_* 38 项接线**：execve/fdatasync/tgkill/inotify_init/mbind/readv/writev/sendfile/preadv/pwritev/fchownat/statx/fallocate/utimensat/close_range/epoll_pwait/ppoll/set_robust_list/get_robust_list/execveat/waitid/process_vm_readv/process_vm_writev/userfaultfd/recvmmsg/sendmmsg/socketpair/seccomp/prctl/arch_prctl/capget/capset/pivot_root/chroot/clock_nanosleep/settimeofday/adjtimex/setdomainname——部分已实装挂 QX 编号（execve/tgkill/seccomp），其余需按实装计划推进。
  - 验证：迁移后 `audit_unwired_pub_fn.py` R2 应大幅下降（133 项从 QX_* 区消除）；双架构 0w0e + host-tests + QEMU（syscall 路径改动必跑）。
  - 状态：[X]（2026-09-15 编号空间归位完成，commit aea73cd1——**已实装项全部归位**：types.rs 唯一权威（删 163 个挂标准功能的 QX_* 常量、补 16 个 SYS_*）、framework dispatch 30 分支 QX_*→SYS_*（原 500+ 编号与用户态 Linux 编号 0-299 不一致 → 30 syscall 不可达 bug）、services xattr 分支同步、seccomp STRICT_ALLOWED 白名单归位（原永不命中 bug）、api.rs 43 重复常量 + 双份 MAX_SYSCALLS 消除、双 dispatch 重叠分支归零（收尾工程 [syscall-dispatch-cleanup.md](syscall-dispatch-cleanup.md)：回退层 65 编号 = 27 机制独有/24 未迁移登记/14 ENOSYS 哨兵）。**剩余**：R2 未实装 SYS_*（116 项）的功能实装接线归 B09-05/D-2 推进，非编号治理范畴）

- **B09-18. R2 syscall 三分类治理清单（2026-09-09 新增，附 B09-05 实测数据）**
  - 描述：实测 2026-09-09 扫描数据（与 B09-05 文档登记数字的出入）：
    - R2 实际 **157 项**（文档 161，差 4——可能已接线/移除）
    - R1 pub fn 实际 **445 项**（文档 362，**多 83**——脚本修复后计数更全，需更新清单）
    - R3 pub mod 实际 **7 项**（文档 36，少 29——多数已治理或误报排除）
    - R4 struct 实际 **1 项**（DomainFlags，与文档一致）
  - 方案：按 2026-09-09 治理决策三分层（DECISION-052）重排 B09-05/06/07 处置——垃圾删除 / 已实装接线 / 未来预留迁移文档。R1 445 项高密度文件 Top：apic.rs 19、xhci.rs 14、display/controller.rs 11、nestfs/dedup.rs 10、aarch64/mmu.rs 9、credo/identity.rs 9、cgroup.rs 8。
  - 状态：[]（2026-09-15 注记：dispatch 层三分类数据已由 [syscall-dispatch-cleanup.md](archive/syscall-dispatch-cleanup.md) B2 audit 刷新——framework 回退层 65 编号 = 27 机制独有（QX_*）/24 未迁移（登记）/14 ENOSYS 哨兵；全量 R1/R3 清单刷新仍随 B09-06/07 处置时更新）

- **B09-19. 内核源码 `#[cfg(test)]` 内联单元测试迁移（孤儿测试治理，2026-09-11 登记）**
  - 描述：实测 2026-09-11 完整扫描（排除 vendored smoltcp ~14 处）——内核源码 **~81 处** `#[cfg(test)] mod tests` 内联单元测试（framework：sync 原语/mm/lib/idt/proc/driver/net/timer/ipc/chitin/cpu/arch + driver 深层 storage/display/usb/e1000 + net/save；services：credo/barrier/sync/net/mm/proc/config/debug/driver + fs/nestfs traits×9 + vfs_poll_policy/wait_queue/smoltcp_impl），因 `[lib] test = false`（Cargo.toml:19）+ 依赖 crate 不激活 `cfg(test)`，**从不编译、从不执行**（孤儿测试）。项目已确立演进方向：`cfg(test)` → register 模式（framework/tests/ 载体 + `check!`/`assert_eq_test!` + `register_tests_inner!`，经 `register_all_tests()` QEMU/host 双跑）；部分源文件 cfg(test) 为"迁移后未删旧副本"（如 string.rs 的 strlen/strcmp/strncmp 断言与 framework/tests/string.rs 内容一致）。
  - **调研盲区（2026-09-11 复核识别）**：首批调研清单（80 处，首次 grep 被 head_limit=80 截断）未覆盖 **~32 处**——framework driver/storage（nvme/ata/ahci）、display（framebuffer/controller/hdmi×2）、usb 深层（mass_storage/hid/ring/enumerate）、e1000×2、net/save、timer（sleep/hrtimer/pit/mod）、services fs/nestfs traits×9、vfs_poll_policy、net/wait_queue、smoltcp_impl、services driver/storage×3。处置判定待委托人核实（同三层判据：已覆盖删/未覆盖迁/私有 API 公共改写）。
  - 方案（2026-09-11 用户选：**全量迁移 + 断言审计**）：逐处甄别 80 处——
    - **已覆盖 → 删除残留副本**：断言已被 framework/tests/*.rs 或 host-tests/tests/*.rs 等价覆盖的，直接删源文件 cfg(test) 模块（0 断言丢失）；
    - **独特断言 → 迁移**：改写为 `fn() -> TestResult` + `check!`/`assert_eq_test!` + 注册进对应载体；纯逻辑 → `#[cfg(any(kernel_test, host-test))]` 载体（host 可跑）；依赖裸机硬件 → `#[cfg(feature = "kernel_test")]` 载体。
  - 判据（2026-09-11 用户确认：**KT/HT 完全取代内联测试**）：
    - 内联测试当前 0 执行（`[lib] test=false` 从不编译），迁入载体后 HT（纯逻辑）/ KT（kernel_test 硬件路径）双端执行，运行价值净增益无损失
    - 私有 API 边界：载体仅可访问 pub API，私有细节断言按公共 API 等价改写（如 lockdep overflow 截断语义、pop_not_present 不 panic），符合 AGENTS.md §8"测公共 API 不测内部实现"
    - 硬件边界：内联硬件断言（xhci/dp MMIO 等）用 QEMU 仿真 + 测试桩，从未在任何环境执行，KT 可跑即净增益
    - 量化：80 处中仅 lockdep 2 个私有精确断言弱化，其余全部可执行等价覆盖
    - 目标：**源码零 `#[cfg(test)]`**，全部断言收敛 register_all_tests 双端体系
  - 批次计划（2026-09-11 用户分批推进，逐批验证汇报）：
    - **批次 1 ✅ 完成**（framework lib/sync/cpu，12 处）：删 5（mutex/atomic/seqlock/rwlock/cpu·mod）+ 迁 7（string 补 memcmp、cstr 新组、error 新组、lockdep 新组、spinlock debug_assert、rcu 补断言、cpuid 实现）。host 352 PASSED / kernel_test 0w0e
    - **批次 2**（services，33 处）：删 4（sha256/sync·types/unix/credo·types 已覆盖部分）；迁 ~20——test_credo（policy 补 PolicyEngine 层、grants、sessions）、test_pwm（identity）、sync.rs（scoped/irq_lock/once/barrier）、test_ipc（ipc/types WaitQueue）、sys.rs（syscall/mod）、sched.rs（sched_policy）、test_mm（mmap/swap_policy/slab_policy/pmm_policy）、test_config（sysctl parse/write 纯函数）；新载体 test_ebpf（27 测试）、test_audit（9）、test_crypto（9）；sysctl 注册表路径需隔离
    - **批次 3**（framework mm/arch/idt/vfs，14 处）：删 7（kmalloc_slab/slab/tss/gdt/idt·types/handlers/statistics）；迁 5——test_vfs（dcache 10）、test_mm 或 test_new_features（frame 5、cow 补 virt_index、page_fault 补 stack_expansion）、idt.rs（idt/mod 3 + safety rflags/rdtsc）、copy_user（STAC/CLAC + 异常表 → kernel_test）——**【2026-09-11 已由委托波 1B 完成并验证：host 377 PASSED / kernel_test 0w0e；含 3 处源错误断言修正（tss_size %16、cow pml4_idx、idt "Device Not Available"）+ dcache invalidate_entry 私有 API 丢弃登记】**
    - **批次 4**（framework proc/timer/ipc/chitin/driver/net + services barrier/driver，18+ 处）：删 5（elf/dynamic/driver·framework/driver·mod/keyboard）；迁 ~25——sched.rs（scheduler_ex 10 缺）、tick/calibration 已有载体注册补缺、test_ipc（ipc/mod msgq/signal_validation + stress_tests 缺失项）、新 chitin 载体（mod 13 kernel_test / composite 2 / user_driver 2 / devtree 补）、driver.rs（xhci 12 kernel_test / usb_core 5）、新 test_net 或修 net.rs 死载（net/init 7 纯逻辑 + 3 kernel_test、iface_trait 16）、services barrier 新载体 ×5（recovery_policy/cascade/audit_export/attribution/health_monitor）、dhcp_policy、proc/signal、display/dp 30（kernel_test 无 DpIo fallback）
    - **批次 5**：全量验证——双架构 0w0e + `make test-host` + QEMU + 断言审计（断言总数 ≥ 迁移前、逐载体 0 丢失）+ `grep '#[cfg(test)]'` 复核源码 0 残留
  - 验证：迁移后双架构 0w0e + `make test-host`（host_test_runner_main 全量注册无 Fail）+ QEMU（kernel_test 硬件路径）；断言审计——迁移后断言总数 ≥ 迁移前，逐载体比对 0 丢失。
  - 状态：[ ]（2026-09-11 **整体回退**：批次 1 + 波 1B 代码改动已全部还原至 HEAD `94ba4281`，工作区仅剩本规划文档改动。下文"批次 1 完成/波 1B 完成/漏删修复/验证补全"均为**历史验证记录**（代码已不生效），保留作为外部委托人交接依据——判据/批次计划/处置清单/盲区登记全部有效，**全部实施待委托人**）
    - **批次 1（12 处，host 352 PASSED / kernel_test 0w0e / clippy 0 新增）**：
      - 删除残留副本 5：sync/mutex、sync/atomic、sync/seqlock、sync/rwlock、cpu/mod（源 effective_family 0x0F bug 随删消除，载体已修正 0x15）
      - 迁移 7：lib/string 载体补裸 memcmp 断言；lib/cstr 新增 lib::cstr 组（4 测试）；error 新增 error::kernel_error 组（源 cfg(test) 依赖不存在的 `From<KernelError> for i32`，证明从未编译，载体以 as_errno().as_i32() 覆盖）；sync/lockdep 新增 sync::lockdep 组（10 测试，LockDepMap 私有 API 以公共 API 等价改写，overflow/pop_not_present 断言弱化已注释）；sync/spinlock 补 debug_assert（`cfg(all(host-test, debug_assertions))` 双门控，no_std 裸机不可编译）；sync/rcu 在 test_new_features 补 read_lock_unlock/nested 断言；cpu/cpuid 实现 arch.rs register_cpuid_tests（x86_64 门控 + aarch64 空 stub）
      - 遗留登记：rcu gp_counter/call_rcu 依赖裸机 SMP 栅障 → kernel_test 后续批次；error `from_i32(95)=NotSupported` vs `as_errno=ENOSYS(38)` 映射不对称 → 预存生产问题（未改生产逻辑）；clippy 预存 3 warning（dma_stream drop_non_drop / e1000 doc / strlen_safe constant）非本次引入
    - **波 1B 完成（2026-09-11：framework mm/arch/idt/vfs 14 处，host 377 PASSED / kernel_test 0w0e / 双架构 0w0e）**：
      - 删除残留 7：mm/kmalloc_slab、mm/slab、arch/tss、arch/gdt、idt/types、idt/handlers、idt/statistics
      - 迁移 7：idt.rs 补 anomalous_frame + idt/mod 3 + safety rflags/rdtsc；test_new_features 补 cow virt_index、page_fault stack_expansion、frame 5（UFrame/USegment unsafe 构造带 SAFETY）、copy_user 4 纯校验双端 + 9 复制/清零 `cfg(kernel_test)` 门控（STAC/CLAC）；test_vfs 补 dcache/icache 9（公共 API 改写 + flush_all 隔离）
      - 源错误断言修正 3：tss_size %16（TSS_SIZE=104 实为 8 余）、cow pml4_idx 0xFF（非 0）、idt "Device Not Available"→"No Coprocessor"
      - 断言丢弃登记：dcache invalidate_entry（私有无公共等价）→ 丢弃并注释
    - **services 侧批次（原批次 2）已回退，交外部委托人实施（2026-09-11）**：本条目前期由子智能体实施产生半成品（test_pwm/test_credo 编译失败），经用户裁决**回退 services 全部 22 源 + 5 载体 + 4 新载体（test_ebpf/test_audit/test_crypto/test_init）+ sys.rs 的 syscall 组**，工作区恢复干净（只保留批次 1 + 波 1B 已验证改动 + 本规划）。后续 services 迁移由用户委派的外部委托人实施，AI 仅审查。批次 2 处置清单见上，含已知覆盖关系（sha256/sync·types/unix/credo·types 已覆盖可删；policy 补 PolicyEngine；ipc/types WaitQueue；sysctl 注册表隔离等）。
    - **审查复核补全（2026-09-11）**：
      - **漏删修复**：波 1B 漏删的 idt/mod.rs（integration_tests 3 例：ffi_interface/exception_names_coverage/constants_sanity）、idt/safety.rs（tests 3 例：cpu_features/rflags/rdtsc）源 cfg(test) 已补删——断言均已在 idt.rs 载体等价覆盖（含 MODULE_INIT_SUCCESS != FAILURE、cpu_features_no_panic）
      - **调研盲区登记**：完整 grep 发现首批清单不完整（head_limit 截断），新增 ~32 处盲区（见描述"调研盲区"），处置判定待委托人核实
      - **验证补全（批次 1 + 波 1B 合并态）**：默认 `cargo check --release` 0w0e ✓、`kernel_test` 0w0e ✓、clippy 双态 `-D warnings` 0 ✓、host-tests 全量（cargo test）全部通过 ✓、双架构 `./ci/build.sh all` 见状态（构建中/结果）——QEMU 集成（test-unit）未跑（改动涉及 idt/copy_user kernel_test 路径，建议委托人批次收尾时跑）

- **B09-21. R1 审计噪音治理（裁定五，单开任务；2026-09-18 登记）**
  - 描述：`scripts/audit_unwired_pub_fn.py`（R1）在 CI **反复报出全部零引用 pub fn**（T5 已甄别分类的 437 项口径即其输出）；[syscall-followup.md](syscall-followup.md) T5 关闭后若不治理，**甄别成果无法固化、清单必漂移**。
  - 方案：给 R1 增加「**已分类清单**」数据源，**不新建独立文件**——清单以 T5 台账内**机器可读区块**承载：`<!-- audit-classified-begin -->…<!-- audit-classified-end -->`；脚本读该区块作为「已知分类」集合，**仅对未分类的零引用 pub fn 报 HIGH**；**fail-closed**＝区块缺失 / 解析失败**视同未分类（仍报）**；**只降噪不豁免**＝**不改变「零引用」这一事实判定**，只改变报告分级。
  - 定位：**单开任务**，**不作为 T5 的前置**（T5 可先完成甄别），但**是 T5 关闭条件之一**（裁定四.④；来源见 [syscall-followup.md](syscall-followup.md) 裁定五记录块）。
  - 状态：[X]（2026-09-18 落地：数据源＝台账 **B-6** 区块 437 项；脚本新增 `load_classified_set()`——R1 未分类 ⇒ HIGH、已分类 ⇒ INFO（零引用事实保留），退出码仅计未分类；**fail-closed 六条失败路径实测全部回落空集 ⇒ 全量报 HIGH**；正向实测 **HIGH=0 / INFO=437**。详见 [syscall-followup.md](syscall-followup.md) **B-7.2**）
  - **2026-09-18 甲批 C-1 后（最终口径）**：数据源＝**B-6** 区块 **436 项**（`set_fd` 接线后不再零引用而移出）；正向实测 **HIGH=0 / INFO=436**。详见 [syscall-followup.md](syscall-followup.md) **B-8.4**。

## 工程计划 C: 架构责任边界（F2/D8）

### 背景

- **B09-11. framework→services 反向依赖**
  - 描述：实测 framework→services 反向依赖 84 文件 / 146 处（TOP 20 #19、决策点 D8），vfs/api.rs 严重违反 F2 单向数据流。
  - 方案：按子模块分类治理（决策点 D8），DECISION-039 仅修 userctx 一处，覆盖不足。
  - 状态：[] (2026-08-31 注记：**部分进展**——vfs/api.rs 反向依赖已由 B09-12 治理（api.rs 3 处直 use 消除，见 B09-12 状态）；但 framework 仍有 ~10 处直接 `use crate::kernel::services`（net/syscall.rs、sm_fi.rs、user_proc.rs、sendfile.rs 等）+ ~70 处 pub use re-export 壳，全量治理未完成，详见 B09-13)
  - 2026-09-11 全量复查（grep `kernel::services` 含内联路径）：**当前 136 处 / 78 文件**（较登记 146/84 少 10 处/6 文件，B09-12 治理后自然减少）——子模块分布 ipc 31、fs 24、syscall 20、proc 14、net 11、config 10、credo 6、wasm 5、driver 3、mm 2、sync/io/barrier 各 1（tests 7 为测试载体访问 services 真实代码，非生产路径）；形态：pub use 壳 ~70+ 处 + 直接 use ~20 处（sm_fi/syscall/sendfile/user_proc 为生产重点）+ tests 7

### 待办

- **B09-12. vfs/api.rs 反向依赖治理（H.5.1 P0-31）**
  - 描述：framework/fs/vfs/api.rs 直调 services 层类型。
  - 方案：services 类型迁回 framework（DECISION-H13/H19），api.rs 恢复单向依赖。
  - 状态：[X] (2026-08-31 实装，commit 0fd608b9 + ee7dab4b + 53b7bf51 + 5a9c69b6：Errno/KernelError 迁回 framework 打破循环依赖，dcache/icache/VFS 机制迁回；api.rs 原 3 处直 use services (devfs::DevfsData / open_file_table::OPEN_FILE_TABLE / vfs_types::OpenFile) 已消除——open_file_table/vfs_types 迁回 framework，devfs 经 vfs.rs re-export 壳访问。复验 (2026-08-31)：audit_services_boundary 0 违规。注：本项由分册 6 委托人实施，作为 B06-10 串行前置)
  - 详情：⚠ **同文件冲突约束**——`vfs/api.rs` 同时被 B06-10（拆分）、B06-11（直调 F2）涉及。**必须串行执行，顺序：B09-12（依赖方向治理）→ B06-10（文件拆分）**。先修依赖方向再拆分，避免拆分后 import 返工；并发委派时 B09-12 与 B06-10/11 不得并行。（已按序执行：B09-12 → B06-10，见分册 6 B06-10/11 状态）

- **B09-13. framework→services 全量反向依赖清单与治理（D8）**
  - 描述：**136 处反向依赖 / 78 文件**（2026-09-11 全量复查，grep `kernel::services` 含内联路径；原登记 146 处，B09-12 治理后减少）按子模块分类（ipc 31、fs 24、syscall 20、proc 14、net 11、config 10、credo 6、wasm 5、driver 3、mm 2、sync/io/barrier 1）；含 B09-09 并入的 ~78 处 pub use re-export 壳。
  - 方案：建立清单 → 分类（类型迁回 / 顶层 re-export / 接口抽象）→ 分批治理；**20 处直接 use（sm_fi/syscall/sendfile/user_proc 等）为生产治理重点，优先于 re-export 壳**；每批跑 F2 门禁（分册 01 修复后）+ audit_services_boundary 0 违规。
  - 状态：[]（2026-09-11 **优先级让位**：本条目被独立工程 [framekernel-paradigm-enforcement.md](../plan/framekernel-paradigm-enforcement.md) §7 吸收——该工程优先于分册 9，含逐文件下沉清单 + trait 化改造 + 残留调用点接口方案；实施并入该工程）

- **B09-14. F3 循环依赖门禁接入**
  - 描述：audit_coupling.py 修复（分册 01）后接入 CI，新增代码禁止引入模块间循环依赖。
  - 方案：见分册 01 coupling/invariants 接入项。
  - 状态：[]

### 验证门槛

- **B09-15. 死代码回归**
  - 描述：每批删除后跑双架构编译（0 error/0 warning）+ host-tests。
  - 方案：`./ci/build.sh all` + `make test-host`。
  - 状态：[]

- **B09-16. 边界回归**
  - 描述：F2 治理后跑修复版 `audit_services_boundary.py` 0 违规。
  - 方案：分册 01 完成后的门禁脚本。
  - 状态：[]

### 决策记录

- **DECISION-051**
  - 描述：死代码治理采用"审计清单驱动"方式，每批删除后立即跑验证门槛，不做无目标的大规模清扫。
  - 方案：R2 表 A 接线优先，表 D 删除次之，表 R 核实后处置。
  - 状态：[]

- **DECISION-052（2026-09-09 用户决策：死代码/TODO 三层治理模式）**
  - 描述：死代码与 TODO **不一律消除**——部分死代码/TODO 是未来内核真实需要的功能记录（如 QX_* 预留 API 面、TRACK- 追踪的功能规划），全消除（无论删除或实装）会破坏记录。
  - 方案（三层分类处置）：
    1. **垃圾/过期**（如 mremap 的 TODO 注释——dispatch 已实装但注释残留）→ **删除**
    2. **已实装未接线**（挂 QX 编号的 execve/tgkill/seccomp、R1 完整函数）→ **接线激活**（兑现资产，成本低）
    3. **未来功能记录**（TRACK- 追踪的未实装功能、QX_* 独有预留编号）→ **迁移到规划文档保留**（代码不留 TODO 合规 §9.4，规划不丢失；不删不盲目实装，按优先级渐进兑现）
  - 编号空间原则（关联 B09-17）：**Linux 有标准 syscall 的功能必须用 SYS_*（0-299）**，500+ 区仅保留 QX 独有功能（FB/FW/FTRACE/KGDB/ROUTE/NF/CGROUP/平台能力等 32 项）。
  - 状态：[] (2026-09-09 用户确认登记，作为 B09-05/06/07/10/17/18 治理总纲)
  - 关联：**预留功能清单见本章"工程计划 D"**（2026-09-09 并入分册 9：QX_* 独有 32 项 + R2 未实现 SYS_* ~33 项 + R3 7 项 + R1 甄别代表 + TODO 转正式 30 项；每条目含来源/判据/状态，兑现后标记 [X]）。分册 9 阶段 4"未来预留迁移"以此为执行依据。

## 工程计划 D: 预留功能转正式清单（2026-09-09 并入）

> 判据：DECISION-052 第三层——死代码/TODO 中"未来内核真实需要的能力"（POSIX 兼容 / 安全 / 驱动功能 / QX 独特设计），
> 由代码注释迁移至本清单正式保留（代码不留 TODO 合规 §9.4，规划不丢失）。
> 处置：不删除、不立即实装；按优先级渐进兑现（兑现后标记 [X]）。

### D-0. 处置方向总则（2026-09-09 补齐，2026-09-09 更新为"内核需不需要"核心判据）

**第一判据（2026-09-09 用户定）：功能对内核是否真实需要？**
- **需要**（POSIX 兼容 / 安全 / 驱动 / QX 设计 / 文件系统语义）→ **转正式保留**（实现治理，按优先级兑现）——即使低优先级（如时间戳写回是 POSIX 语义完善项）也转正式，不删除
- **不需要**（纯注释残留 / 功能已存在只剩注释 / 确认无价值）→ **直接删**

**第二判据（执行方式，三分类）**：

| 处置方向 | 判据 | 适用 |
|---|---|---|
| **直接删** | 功能已存在（接线/实装已完成）只剩注释/豁免残留，或确认内核不需要 | 过期 TODO、纯注释残留、无用途 struct/mod |
| **实现治理** | 内核真实需要但未实装 | 未实现 SYS_*、TRACK- TODO、真实缺口 TODO、QX_* 独有、时间戳写回、overlayfs copy-up |
| **接线治理** | 内核需要且已完整实现，只是没接进调用链 | 已实装挂 QX 编号、R1 完整函数 |

### D-1. QX_* 独有功能（32 项，保留 500+ 私有编号区）

> Linux 无对应 syscall，为 QX 独特设计预留 API 面（DECISION-052 编号空间原则）。
> 处置：**实现治理**（接线或实装激活，保留 500+ 区）。

| 组 | 条目 | 来源 |
|---|---|---|
| 设备/固件（7）| FB_OPEN/FB_MMAP/FB_RELEASE（framebuffer，Linux 走 ioctl/DRM）；FW_LOAD/FW_GET/FW_GET_INFO/FW_DETACH（固件管理）| R2 未接线 QX_*（FB）+ dispatch 已接线（FW）|
| 追踪/调试（5）| FTRACE_ENABLE/DISABLE/READ/STAT；KGDB_ENTER | dispatch 已接线 |
| 网络/资源（10）| ROUTE_ADD/DEL/QUERY（路由，QX 用 syscall 非 netlink）；NF_ADD_RULE/NF_DEL_RULE；CGROUP_CREATE/DESTROY/ATTACH/SET_LIMIT/GET_STAT | dispatch 已接线 |
| 平台能力（9）| PM/SECURE_BOOT/TPM/CET/TICKLESS/TIMESYNC/UEFI | dispatch 已接线 |

> 注：编号归位争议项（RT_SIGRETURN/KEXEC/IO_URING_*）以 B09-17 应转 SYS_* 为准，不入本组。

### D-2. R2 未实现 SYS_*（~33 项，Linux 标准编号功能缺口）

> Linux 有标准编号但未实装，POSIX 兼容必需——转正式功能开发规划（独立工程，非清理范畴）。
> 处置：**实现治理**（实装对应 syscall，独立功能工程）。

- **文件 I/O**：preadv/pwritev/readv/writev/sendfile/fallocate/statx/fchownat/utimensat/close_range/process_vm_readv/process_vm_writev
- **信号/进程**：waitid/prctl/arch_prctl/capget/capset/set_robust_list/get_robust_list
- **网络**：recvmmsg/sendmmsg/socketpair
- **内存**：mbind/userfaultfd
- **inotify/epoll**：inotify_init/epoll_pwait/ppoll
- **时间**：clock_nanosleep/settimeofday/adjtimex
- **文件系统**：pivot_root/chroot/setdomainname
- **exec**：execveat

> 已实装挂 QX 编号的 execve/tgkill/seccomp 属"接线层"（B09-17 应转组），不入本组。

### D-3. R3 零引用 pub mod（7 项，架构预留接口待核实）

- **dmu_trait/raidz_trait/spa_trait/txg_trait/zap_trait/zil_persist_trait/zil_trait**
  - 描述：nestfs 各子系统 trait 抽象模块（[nestfs/mod.rs](../../src/kernel/services/fs/nestfs/mod.rs)）。
  - 处置：**核实后二选一**——架构预留接口（非误报）→ 转正式保留（实现治理）；误报/无用 → 直接删。
  - 状态：[X]（2026-09-18 核实：7 项全部为**纯预留抽象**——零生产引用 / 内联测试在门禁中从不编译 / host-tests 已有平行实现；**处置＝直接删**，详见 [syscall-followup.md](syscall-followup.md) 的 T4 实施记录）

### D-4. R1 甄别"未来功能"类（逐项核实，代表清单）

> 完整实现但跨文件零引用，属"安全/策略/能力"预留——转正式比接线更合理（避免为消除而接线）。
> 处置：**逐项核实三选一**——接线治理（完整函数接入调用链）/ 实现治理（预留能力按需求激活）/ 直接删（确认无价值）。

- **credo/identity.rs（9 项）**：身份/凭据校验能力预留
- **credo/secure_boot.rs（8 项）**：安全启动度量/校验函数
- **cgroup.rs（8 项）**：资源管理接口
- **framework sync/\*（atomic/rwlock/seqlock 等）**：同步原语预留方法
- **mm/swap.rs、numa.rs**：交换/NUMA 策略预留
- **driver/\*（xhci 14 / display 11 / apic 19 等）**：硬件操作函数（部分应接线，部分转正式）

> 完整 445 项见 B09-18 实测报告；逐项核实后分拣（接线 vs 转正式 vs 删除）。

> 状态（2026-09-18）：**T5 批 1 完成**——实测确认 R1 主体（438 项）为上述**有意保留的 API 面**（services 安全代理壳 / FS mount API 面 / 调试统计查询面 / 原语预留 / 驱动硬件面），粗删会移除 API 面；批 1 仅删「重复能力 / 等价公共入口 / 废弃兼容壳」9 项（R1 447 → 438）。
>
> 状态（2026-09-18）：**T5 批 2 完成（只读扫描 + 登记）**——438 项已逐项四分类（原始口径：删 70 / 接线 142 / 预留 205 / 待裁 21），未改任何源码。上述 D-4 类别在本台账中被细化为逐项判据（D-4 结论不变，仅补粒度）。
>
> 状态（2026-09-18，**reviewer 复核后修订**）：原台账**缺「判定工具 / 判定构建维」元信息**，且 R1 为 `rg -c -w` **文本并集**、**声明侧无 cfg 感知** ⇒ 对 `#[cfg(target_arch = "…")]` / `#[cfg(feature = "…")]` / `kernel_test` 门控代码产生**系统性误判**（实例：`mm/mod.rs:51-53` 门控的 `vmm_aarch64.rs`、`atomic.rs:182` 的 `atomic_stats`）。已修订：补元信息与误判源清单、重划桶边界（**删候选 23 / 硬件原语完整性保留 41 / 接线 8 / 未来功能 339 / 待裁 27**，均暂定；优先级＝硬件原语保留 > aarch64 门控 > 删候选）、`sync/atomic.rs` 4 项退回待裁并援引 [subsystem-sync.md §9.3 [P2]](archive/audit-2026-08-14/subsystem-sync.md#L1075-L1092)。**删候选施工方式＝逐项试删 + 跑既有五条门槛**（`build.sh all` / clippy `kernel_test` 维 / clippy `host-test` 维 / `make test-host` / QEMU `kernel_test`+boot）→ **任一维硬失败即回退**；已裁定**不立项新工具**（编译/链接器即权威判据）。
>
> 状态（2026-09-18，**reviewer 第二轮复核后二次修订**）：**A-2 删候选退回重做，不得开工**。① **判据升格为三合一**（该构建维零引用 ＋ 存在能力等价的公共入口 ＋ 非 API/FFI/feature/硬件原语面），「有等价入口」单独不成立；② 原 23 项四档处置＝**11 留 + 4 转「安全面待确认」（不走试删）+ 8 退桶**；③ 桶数二次修订：删候选 **11** / 安全面待确认 **4** / 硬件原语完整性保留 41 / 接线 8 / 未来功能 339 / 待裁 **35**（合计 438）；④ **试删存在原理性盲区**——发现不了「安全校验被移除」，安全敏感项须先经安全面确认；⑤ **顺序闸门**：不得并行 T1 收尾与 T5 施工（`pcid_is_enabled` 与 T1 P2′ 所改 `kpti.rs` 文件级冲突），A-2 开工须先满足 5 条解锁条件。
>
> 状态（2026-09-18，**reviewer 第三轮复核后三次修订**）：**A-2 11 项试删已授予开工**，安全面桶清零。① **裁定一**：`services/credo/crypto.rs` 的 `ct_eq_salt` / `ct_eq_password` **不删**，定型为**族残缺**（与 `ct_eq_hash` 同族，删则残缺且不对称，且诱导调用方拆字段绕过类型包装）⇒ 移入「硬件原语完整性保留」（41 → **43**）；附核实＝全仓零引用、无 `no_mangle`/FFI、非跨 crate / 用户态 API 面。② **裁定二**：`services/fs/ramfs.rs` 的 `split_path` / `validate_path` **不得按「零调用」删**——`validate_path` 实为空 / 长度 / NUL 三项检查（**不含 `..` 穿越检查**，穿越由 VFS `resolve_path` 负责，上轮威胁模型更正），须先由 **T3** 给出「VFS 是否已提供等价校验」结论后二选一（冗余 ⇒ 可删 + 登记契约；缺陷 ⇒ 不删 + 登记缺陷 + 按 C-1 接线）⇒ 并入待裁（35 → **37**），**不进试删队列**。③ **裁定三**：「二次分桶」产出规格固定为**三档＝冗余 / 缺陷 / 族残缺**（三合一判据不足以区分三档）⇒ 写入 [syscall-followup.md](syscall-followup.md) 的 **B-0**。④ 桶数三次修订：删候选 **11** / 硬件原语完整性保留 **43** / 接线 8 / 未来功能 339 / 待裁 **37**（合计 438）。⑤ **执行约束**：逐项独立提交可单独回退 / 每项跑五门槛全量 / **QEMU boot 硬闸门** / ramfs 2 项不入本批。⑥ **QEMU 日志已留存归档**（boot `build/log/qemu_boot_x86_64_t1wrap_20260918.log`、kernel_test `tests/reports/unit_test_20260918_165355.log`；两目录按 B08-10 为 gitignore，故可追溯性＝归档文件 + 逐字行入档）。
>
> 状态（2026-09-18，**试删开工后逐项复核四次修订**）：**A-2 退桶 9 项入待裁 ⇒ 删候选 11 → 2**。试删启动后按三档规格逐项复核「零调用 ＝ 冗余 / 缺陷 / 族残缺」，以**全仓 `grep -rn` 实测引用计数**为判据，发现 9 项判据不成立：① **族残缺 6 项**（`tss_64bit`——`Granularity` 构造器族 `code_64bit`/`data_32bit` 在用、字段 `pub(crate)` ⇒ 删后调用方须写 `Granularity(Granularity::LONG_MODE)` **绕过构造器族**，与裁定一判据同构；`vfs_close_safe`/`vfs_seek_safe`/`vfs_readdir_safe`——`vfs_*_safe` 全族 **17 员 / 14 员在用**；`get_ap`、`get_fs_name`——同族访问器在用）；② **台账 ② 判据事实错误 / 无等价入口 3 项**（`consume_quota_tick`/`is_quota_exceeded`——原记「与**在用** `check_quota` 语义重复」，实测 `check_quota` **自身零引用**且已列待裁；`pipe_exists`——原记「可由**在用** `get_pipe`/`pipe_count` 等价判定」，全仓**无 `get_pipe` 符号**、`pipe_count` 不判定指定 id 是否存在），两处错误已就地订正并据此退桶。③ **新增原理性证据（重要）**：`get_ap` 试删后五门槛 **5/5 全过**（含 QEMU boot 硬闸门）仍被判族残缺退回 ⇒ **试删 + 五条门槛在原理上无法识别族残缺**（与「发现不了安全校验被移除」同源），**门槛通过 ≠ 判据成立**。④ 桶数四次修订：删候选 **2** / 硬件原语完整性保留 **43** / 接线 8 / 未来功能 339 / 待裁 **46**（合计 438）。本轮仅 `write_log_line` / `format_duration` 2 项进入试删。⑤ **档位争议如实登记**：`vfs_*_safe` 三项弱于 `tss_64bit`（实测 `services/fs/dir_ops.rs` 已直接调用裸 `extern "C"` `vfs_seek`/`vfs_readdir`，该族本就不是封装边界），最终档位由 reviewer 复核确定。
>
> 状态（2026-09-18，**reviewer 第四轮「范围裁定」后五次修订**）：**A-2 删候选清零**。① `write_log_line`（`framework/console/gfx_console.rs`，与在用 `write_str` **逐字节相同**的纯复本）试删五门槛 **5/5 全过** ⇒ **已删除**（commit `23681a14`，1 file changed / 6 deletions；R1 438 → **437**）。② `format_duration`（`framework/timer/tick.rs`，`#[cfg(feature = "alloc")]`）经裁定三「第 2 项自判」逐条核：逐字节复本 ✗ / 能力等价复本 ✗（全仓无等价时长格式化公共入口，`core::fmt` 为通用设施）/ 族完整性——`tick.rs` 的 `pub fn` 族为 tick↔单位换算（`ticks_to_ms`/`ms_to_ticks`/`get_uptime_*`/`get_time_info`），`format_duration` **不属该族** ⇒ **判据待补退桶**，入待裁（A-6）。③ **范围裁定（裁定一）**：**接线 8 / 未来功能 339 移出 T5**（接线＝「加功能」、甄别＝「清死代码」，混桶使 T5 无法关闭且归因困难）⇒ **T5 内桶数＝删候选 0 / 硬件原语完整性保留 43（仅登记，不施工）/ 待裁 47 ＝ 合计 90**（＝ 438 − 1 − 8 − 339）。④ **待裁归零路径（裁定二）**：每项须落**三态**（等路线图 / 判据待补 / 安全面待 T3）并补**三字段**（等待原因 + 解锁条件 + 责任方），**禁裸待裁**。⑤ **T5 关闭验收五条（裁定四）**：删候选清零 / 待裁带三字段 / 桶数算术闭合且转移可逐项追 / 审计噪音治理落地 / 文档双向引用。⑥ **审计噪音治理＝单开任务（裁定五）**：给 `scripts/audit_unwired_pub_fn.py`（R1）增加「已分类清单」数据源，**不新建独立文件**，以台账内机器可读区块 `<!-- audit-classified-begin -->…<!-- audit-classified-end -->` 承载；**fail-closed**（区块缺失/解析失败视同未分类仍报）、**只降噪不豁免**（不改「零引用」事实判定，只改报告分级）；**不作为 T5 前置**，但**是 T5 关闭条件之一**。⑦ **授权边界（裁定六）**：新增删候选 / 删除涉及安全面（credo·权限·路径·内存保护）/ 删除涉及 TCB 核心（GDT·IDT·MMU·中断·调度）/ 删除涉及已登记路线图地基（如 `pcid_is_enabled`）⇒ **必须上报，不自主决定**。⑧ **双向引用**：本条与 [syscall-followup.md](syscall-followup.md) 的 T5 台账（裁定一~七记录块 + A/B/C/D 桶明细 + 待裁三态归零表）**互为引用**。
>
> 状态（2026-09-18，**T5-B 待裁归零（六次修订）**）：**待裁 47 已全部归零为三态，无裸待裁**。依裁定二逐项落态并补**三字段**（等待原因 + 解锁条件 + 责任方）——**等路线图 13 项 ⇒ 转「未来功能」**（CET 5 `set_ssp`/`alloc_kernel_shadow_stack`/`configure_user_cet_msr`/`configure_interrupt_ssp_table`/`is_ssp_valid`、NUMA 5 `set_distance`/`best_alloc_node`/`nearest_free_node`/`contains_cpu`/`all_nodes`、PCID 1 `pcid_is_enabled`、IOMMU-DMAR 2 `get_dmar_drhd_list`/`get_dmar_host_addr_width`，**登记处均经核实**）；**判据待补 32 项 ⇒ 补齐后定桶「完整性保留」**（G1 aarch64 门控诊断 2 / G2 feature 门控 4 / G3 机制·安全·TCB 3 / G4 驱动·统计·诊断 API 面 4 / G5 FS mount-unmount 面 + Plan B FD 表 9 / G6 族残缺 6 / G7 无等价入口 3 / G8 无等价格式化入口 1）；**安全面待 T3 2 项 ⇒ 留待裁**（ramfs `split_path`/`validate_path`，裁定七证据链已判非安全缺陷、倾向冗余，**剩余唯一阻塞＝删除涉安全面须 reviewer 授权**，裁定六）。**桶数六次修订**：删候选 **0** / 完整性保留 43 → **75** / 待裁 47 → **2** / 未来功能 339 → **352** / 接线 8；**T5 内合计 90 → 77**，合计 **437**（**算术闭合**，转移可逐项追，见 [syscall-followup.md](syscall-followup.md) 的 **B-5 归零表**）。**裁定四验收**：① 删候选清零 ✅ / ② 待裁带三字段 ✅ / ③ 桶数闭合 + 转移可追 ✅ / ⑤ 双向引用 ✅ / **④ 审计噪音治理未落地** ⇒ 已单开 **B09-21**（工程计划 B），**T5 关闭挂起于此**。**台账事实订正两处**：① `kpti.rs:23` 所引 `engineering-progress.md` §五 **已失效**（该文件在现行 `docs/plan/` 不存在，`kernel-roadmap.md` 已归档且无 PCID 条目）⇒ PCID 登记处改以 `kpti.rs:23-31` 现状清单 + [kpti-complete-project.md](kpti-complete-project.md) 为准；② `services/fs/devpts.rs:241` `umount_devpts` **实现体误调 `mount_devpts`** ⇒ 登记为**已知缺陷**（无调用链且非安全面 ⇒ 不删，修正随 VFS mount 集成）。**按裁定六上报、未自主处置的潜在新增删候选**：`pci_scan`、aarch64 诊断 2 项、`format_duration`（若 reviewer 认定 `core::fmt` 等价成立）。
>
> 状态（2026-09-18，**第四轮裁定后 T5-C 关闭，七次修订**）：**T5 五条验收全部达成 ⇒ T5 置 `[X]`**（桶数不变 **0 / 75 / 2 / 8 / 352 ＝ 437**）。① **B09-21 审计噪音治理已落地**（裁定五）：台账新增 **B-6** 机器可读区块（`audit-classified-begin` / `audit-classified-end`，437 项），脚本 `load_classified_set()` 实现**未分类 ⇒ HIGH / 已分类 ⇒ INFO**（零引用事实保留，**只降噪不豁免**），fail-closed 六条失败路径实测全部回落空集，正向 **HIGH=0 / INFO=437**。② **ramfs 2 项试删已授权执行 → 门槛失败 → 回退退桶**：删除后 host-tests `td18_fs_kernel_error_test::usages_all_use_kernel_wrapper` 的**源码文本计数**断言（`min(FsError::Kernel( 计数, KernelError:: 计数) ≥ 10`）由 11 → 8 触发失败 ⇒ `git checkout` 回退 + 退桶（新解锁条件＝是否授权同步调整该脆弱断言）。③ **潜在新增删候选全部维持保留**：`pci_scan` / aarch64 诊断 2 项 / `format_duration` 定格完整性保留。④ **T5 关闭不消灭的遗留**：ramfs 2 项退桶 / `umount_devpts` 误调缺陷 / 接线 8 / 未来功能 352。逐项见 [syscall-followup.md](syscall-followup.md) **B-7**。
>
> 逐项台账、分组统计与判据见 [syscall-followup.md](syscall-followup.md) 的「T5 实施记录」+「T5 全量甄别台账」。

### D-5. TODO → 转正式（33 项，2026-09-11 复核：TRACK- 23 + 普通 10）

> 处置：TRACK- 23 项 + 普通 10 项 = **实现治理**（实装对应功能）；过期/无价值 TODO（如 mremap 90BFB0）→ 直接删（见 D-7）。
> 2026-09-11 全量复核：TRACK- 实测 24 处（mremap 90BFB0 过期 → 删除，转正式 23 项）；普通 TODO 实测 10 处（原登记 9 项笔误，实为 10 处——时间戳 3 处 + oomd/memfd×2/xhci/net-init/pidfd/overlayfs）。

**TRACK- 追踪规划（23 项）**：
- syscall/types.rs（7 项）：getitimer/setitimer/clone(线程创建)/hardlink/symlink/fchown/times——POSIX 兼容必需（mremap 90BFB0 已过期 → 删除，不入清单）
- iouring.rs（4 项）：8B9CBC(VFS fd 集成)/9CADCD(网络异步)/ADBECDE(超时)/BECFEF(缓冲/文件注册)
- ipc/signal.rs（4 项）：48CC21/614BD5/F806F4/3A9016（信号分发/blocked 位图）
- uefi.rs（2 项）：4D5E78(EFI 表解析)/5E6F89(SetTime)
- shadow_stack.rs（2 项）：4C9A12(PMM 物理页)/6E7C34(#GP 检测)
- idt/safety.rs（1 项）：2B3C56(CPUID 完整解析)
- power.rs（2 项）：6F7A9A(S3 挂起)/7A3B01(调频 MSR)
- tickless.rs（1 项）：3C4D67(hrtimer 集成)

**普通 TODO（9 项，内核需要的功能缺口）**：
- oomd.rs:94（OOM killer 实际发送 SIGKILL，安全关键）/ memfd.rs:60/77（per-process fd 表 + CLOEXEC）/ xhci.rs:670（Event Ring 处理）/ net/init.rs:607（skb 投递到 smoltcp，依赖 NAPI）/ pidfd.rs:172（依赖 Task 4 OpenFile 系统）/ overlayfs.rs:205（copy-up 写时复制 + 时间戳更新，overlayfs 核心语义）/ ext2·exfat·nestfs_inode 时间戳（3 处同类——**2026-09-09 判据确认内核需要**：POSIX stat mtime 语义完善项，低优先级）

> 已确认无价值/随手的 TODO 不入清单，直接删除。
>
> 状态（2026-09-18，**丙批/T6 首轮 C1 无阻塞切片 5 项已兑现**）：**4 实装 + 1 转正式任务**，五门槛 **5/5 全过**。
>
> ① **OOMD 真实发送 SIGKILL**（`TODO(TRACK-...)` oomd.rs，安全关键）——commit `da367a28`：`framework/proc/oomd.rs` Emergency 宽限期后遍历进程表选 RSS 最大用户进程并 `do_signal_send(victim, SIGKILL)`；配套新增 TCB 原语 `framework/mm/{vmm_x86_64,vmm_aarch64}.rs::count_present_user_pages`（页表 4 KiB 当量用户页计数，RSS 近似）。**锁序约束**：页表遍历置于 `process_for_each` 闭包内（只读，进程表锁保护页表根不被释放），信号发送置于闭包**之外**（`do_signal_send` 内部再次取进程表锁，闭包内调用自锁死；该路径运行于 scheduler tick 关中断上下文，不可恢复）⇒ 已由 `oomd_sigkill_sent_outside_process_table_iteration` 固化为回归不变量。
> ② **ext2 时间戳写回**——commit `97c03f52`：`Ext2Inode::set_times` 写 `i_atime`/`i_mtime`/`i_ctime` 并经 `save_inode` 落盘；`Ext2FileSystem::fs_utimensat` 按路径解析 inode 号后委托。
> ③ **exfat 时间戳写回 ⇒ 分类「需转正式任务」**（commit `97c03f52`，注释落盘）：三项前置基础设施均缺失，不属本批授权范围——无目录项定位（`lookup_path` 只返回首簇号，丢弃目录项扇区偏移）/ 无时间戳编解码（`ExfatDirEntry` 未解析 exFAT 时间戳与 SetChecksum）/ 无验证载体（`ExfatFileSystem` 全仓零实例化、无镜像 mock，写错目录项会破坏 FS 却不可测）。
> ④ **nestfs 时间戳写回**——commit `97c03f52`：新增 `NestfsData::set_times`，接线 `Inode::set_times` / `fs_utimensat` / `fs_stat`（stat 增补 `atime`/`mtime`/`ctime`），支持 `u64::MAX` = UTIME_OMIT。
> ⑤ **`idt/safety.rs::CpuFeatures::detect` CPUID 完整解析**——commit `d055ab58`：x86_64 经 CPUID leaf 0 取最大叶号、leaf 1 的 EDX bit9（APIC）/ ECX bit21（x2APIC）真实解析；aarch64 返回架构中性缺省值（GIC，无 APIC）；原无效断言（`has_apic || !has_apic`）改为双架构真实不变量。
>
> **测试**：`mm::vmm::count_present_user_pages`（QEMU 行为验证：空根=0 / 基线增量恒等）/ `oomd_emergency_actually_sends_sigkill` / `oomd_sigkill_sent_outside_process_table_iteration` / `nestfs_utimensat_writes_back_times`（端到端，含 UTIME_OMIT 与不存在路径）/ `test_ext2_utimensat_wires_to_disk_inode`（接线证据，ext2 需块设备）/ `idt::cpu_features_no_panic`（重写）。
>
> **上报项（按裁定六）**：① 新增 TCB 内存面原语 `count_present_user_pages`（`framework/mm`，含裸机页表遍历 `unsafe` + SAFETY 注释）；② exfat 偏离——本批未实装，转正式任务；③ **覆盖缺口**：OOMD 端到端不可行为测试（`register_pressure_classifier` 为 OnceLock 一次性注册，无法在测试注入 Emergency），故以「新原语行为测试 + 接线/锁序源码证据 + 明确标注缺口」替代；④ 观察项：`framework/idt/safety.rs:7` `#[allow(unused_imports)]` 属**预存** F9 豁免残留（非本批引入，未擅自处置，§12.2）。

### D-6. 兑现路径与验证

- 本清单条目按优先级渐进实装（POSIX 兼容 → 安全 → 驱动 → QX 独有）。
- 每批兑现后：双架构 0w0e + clippy 三线 + host-tests + QEMU + `audit_unwired_pub_fn.py` R2/R1 下降 + grep `TODO(TRACK` 复核 0。
- 状态：[D-5 丙批 C1 5 项已兑现（4 实装 + 1 转正式任务，2026-09-18）；其余 D-1~D-4 / D-5 余项仍为 []，兑现后逐条标记 [X]]

### D-7. "直接删"清单（2026-09-09 补充，集中登记）

> 处置方向：**直接删**——功能已存在/确认无价值，删除后无功能损失。核实后逐项删 + 跑验证门槛。

| 项 | 删除理由 | 状态 |
|---|---|---|
| **mremap TODO(TRACK-90BFB0)**（[syscall/types.rs:72](../../src/kernel/services/syscall/types.rs#L72)）| ✅ 已确认——dispatch 已实装 `mremap_syscall`（[dispatch.rs:228](../../src/kernel/framework/syscall/dispatch.rs#L228)），TODO 过期 | [] |
| **R4 DomainFlags**（[credo/types.rs:101](../../src/kernel/services/credo/types.rs#L101)）| 零引用，仅定义一处——按"内核需不需要"判据核实：credo domain 体系需要 → 转 D-1/D-4；不需要 → 删 | [] |
| **R3 trait 误报项**（D-3 核实为无用的）| 非架构预留接口，内核不需要 → 删 | [] |
| **R1 筛出的无用函数**（D-4 核实为无价值的）| 内核不需要 → 删（如部分 apic/xhci 只读操作）| []（批 1 完成 2026-09-18：删 9 项「重复能力/等价公共入口/废弃兼容壳」，R1 447 → 438；实测确认其余主体为 API 面预留，不再按"无价值"删——详见 [syscall-followup.md](syscall-followup.md) T5 实施记录）|
| **F9 豁免残留**（已激活代码上的 allow）| 对应代码已接线 → 删豁免（limits.rs 等）| [] |

> 2026-09-09 判据更新注记：**ext2/exfat/nestfs 时间戳 TODO 3 处经"内核需不需要"判据确认 = 内核需要**（POSIX stat mtime 语义完善项）→ **转 D-5 转正式**（实现治理，低优先级），不再列入直接删待核实。DomainFlags/R3/R1 待核实项按"内核需不需要"判据核实后回填本表。

### D-8. 丙批审查整改：时间戳写回属主判据（B1）

- 描述：丙批 `97c03f52` 的 ext2 时间戳写回只写字段不判权限，与 nestfs 同层实装（判据＝属主或特权级 0）分歧 —— 任意 pwm 可改他人文件时间戳。
- 方案：判据**留在 FS 实现层并与写回同锁域**（`Ext2Inode::set_times` 在 `EXT2_FS` 锁内比对磁盘 `i_uid` ↔ `pwm_get_uid(pwm)`，非属主且特权级非 0 ⇒ `PermissionDenied`）；一致性由**测试面收口**（`host-tests/tests/fs_permissions_regression_test.rs::set_times_of_persistent_fs_checks_owner_or_privilege` 扫描 framework/services 两侧 fs 源码，凡 `set_times` 函数体含落盘动作 `save_inode`/`update_obj` 者，必须同体出现判据谓词）。**否决「上提到 VFS 统一收口」**：① VFS 分发层（`vfs_utimensat`）无 owner 模型 —— ext2 的 `VfsStat.owner_pwm` 恒 `0`、uid 与 pwm 无映射 ⇒ 对 ext2 只会是空判据（不修本缺陷）；② 判据与写回会被拆成两次路径解析 ⇒ 引入 TOCTOU 窗口；③ 判据是纯策略（应落 services），上提到 framework VFS 属 §4.1 归属错位，且与 trait 契约文档「无权限…由实现者决定」冲突。
- 状态：[X]
- 详情：负向验证——临时移除 ext2 判据 ⇒ 门槛测试 FAILED（`set_times 落盘前必须判属主/特权级`），恢复后 PASS，证明门槛非空过；五门槛 5/5 全过（build.sh all 5/5、audit.sh quick 0 违规、host-tests 99 bin 0 failed、QEMU kernel_test exit 33、qemu_boot_test 1/1）。行为复查：`pwm == 0`（bootstrap/内核内部/WASI 调用）特权级为 0 ⇒ 行为不变；`pwm` 未注册时 `pwm_get_uid` 返 `u32::MAX` 且特权级 `0xFF` ⇒ fail-closed 拒绝。B1 本身提交 `e854398a`。丙批审查其余整改项（A1 页表遍历竞态 / A3 victim 状态过滤 / A4 `terminated_count` / B2 ext2 `i_ctime` / B4 换行 / exfat 撤覆写 / F9 豁免残留）另批处置，逐项状态见下。

- 余项状态（丙批审查其余 7 项，本批实施；每项独立提交、可独立回退）：

| # | 项 | 处置 | 提交 | 状态 |
|---|---|---|---|---|
| 1 | A1 页表遍历竞态根治 | 见「A1 前置闸门结论」：原案（`MmStruct.rss_pages` 记账）不可达，用户裁定走**方案 A** —— 遍历全程持 `VMM_LOCK`（公共 API 面保留、双架构同步）；结构门槛 `user_page_count_traversal_holds_vmm_lock` | `41003e87` | [X] |
| 2 | A3 OOMD victim 未过滤僵尸 | 抽出可单测判据 `better_oom_victim`（僵尸/`pid==0`/`cr3==0` 三类排除 + RSS 严格择优 + 延迟闭包） | `93568c05` | [X] |
| 3 | A4 `terminated_count` 无条件加一 | 改 `record_termination(delivered)`，仅 `do_signal_send` 返 `Ok` 才累加，三向日志分流 | `4614e707` | [X] |
| 4 | B2 ext2 `i_ctime` 直接抄 `i_mtime` | 改取当前秒值（`get_ticks()/get_frequency()`，与 `utimensat_syscall` 同源）；结构门槛 `test_ext2_ctime_source_independent_of_mtime` | `1b852e5d` | [X] |
| 5 | B4 `ext2_test.rs` 文件尾缺换行 | 仅补末行换行（1 行 diff） | `3541b63c` | [X] |
| 6 | exfat `set_times` 静默成功 | 删覆写，落回 trait 默认 `Err(KernelError::NotSupported)`，零新增代码；正式任务编号（C1/T6-③）保留于注释 | `7079a94e` | [X] |
| 7 | F9 `idt/safety.rs` 豁免残留 | `KERNEL_BASE` 单独按 `#[cfg(target_arch = "x86_64")]` 分组，删 `#[allow(unused_imports)]` | `29bee0a5` | [X] |

> 1/2/3/4/7 项的负向验证（临时拆判据看测试是否真红）均已实测：A1 把遍历的 `acquire_lock()` 行改注释 ⇒ `user_page_count_traversal_holds_vmm_lock` FAIL（`A1: …vmm_x86_64.rs 遍历必须获取 VMM_LOCK`）；A3 删僵尸过滤 ⇒ `Proc::oomd_victim_filter` FAIL；A4 改无条件 `fetch_add` ⇒ `Proc::oomd_terminated_count_only_on_delivery` FAIL；B2 恢复 `i_ctime = inode.i_mtime` ⇒ `test_ext2_ctime_source_independent_of_mtime` FAIL；F9 直测双架构强制重编 0 warning 行。各项五道门槛 5/5 全过（build.sh all `Passed: 5 Failed: 0`、audit.sh quick 全 passed、`audit_services_boundary.py` `>>> services 边界检查通过 <<<`、`make test-host` `RESULT: ALL 367 TESTS PASSED (9 skipped)`、`make test-unit` `QEMU exit: 33`、`qemu_boot_test.sh x86_64` 1/1）。R1 台账数字未变动，`CRITICAL=7 / HIGH=0 / WARN=0 / INFO=436` 保持算术闭合。

- A1 前置闸门结论（2026-09-19）：**闸门未过，停手上报后由用户裁定路径**。简报三要素已产出（记账点全集定位 / 口径草案 / 六处不能挂的理由），但核出**阻断性事实**：`MmStruct` 无 per-process 归属——`Process`（`proc/process.rs:133`）仅持 `cr3: AtomicU64`（L145）无 mm 字段；全仓无 `Arc<MmStruct>` / pid→mm 注册表；`CURRENT_MM`（`mm/vma.rs:1271`）为全局单例，唯一生产设置点是 exec（`proc/elf/mod.rs:297`）。故 OOMD 在 scheduler tick 遍历全部进程时**只能取到 `cr3`，取不到任何被遍历进程的 `&MmStruct`** ⇒ 原案「OOMD 改读 `MmStruct.rss_pages`」按现结构不可达。候选路径 ①（保留 cr3 遍历但持 `VMM_LOCK` 消竞态）经用户选为**方案 A** 并已落地（`41003e87`）；候选 ②（先补 per-process mm 归属再落计数器）属碰 TCB 核心/进程结构，未授权、不施工。
- A1 方案 A 实施记录（`41003e87`）：
  - **改动面**：`count_present_user_pages` 的 x86_64（[vmm_x86_64.rs:2051](../../src/kernel/framework/mm/vmm_x86_64.rs#L2051)）与 aarch64（[vmm_aarch64.rs:1147](../../src/kernel/framework/mm/vmm_aarch64.rs#L1147)）两个版本，`cr3 == 0` 早退之后 `acquire_lock()`、遍历之后 `release_lock(&flags)`；文档注释由「**不取 VMM 锁**…竞态允许近似」改为「**全程持 `VMM_LOCK`**…」，两处 `// SAFETY:` 补记「持锁期间中间页表不会被并发 unmap 递归释放，指针在遍历期间有效」。**公共 API 面未增删**（与用户原案「连同 aarch64 版本、顶层再导出、`test_mm.rs` 用例、`memory_pressure_extraction_test.rs` 文本断言一并删掉」不同，已随方案 A 选项明示并获授权）；`test_mm.rs` 基线增量恒等用例保留为行为验收。
  - **锁序核验**：`VMM_LOCK` 临界区不回调 `proc`/进程表（`grep proc::|process_|scheduler|PROCESS_TABLE` 于两个 vmm 文件**零命中**），故新增「进程表锁 → VMM_LOCK」边不构成 AB-BA；`mm/` 子树内 `numa.rs`/`uffd.rs` 的 proc 调用均在 VMM_LOCK 之外。TLB shootdown `broadcast_tlb_invalidate` → `send_broadcast_ipi` 为 fire-and-forget（[smp/mod.rs:78](../../src/kernel/framework/smp/mod.rs#L78)），不等待 ack，无反向等待。`acquire_lock` 自带 `IrqSaveFlags` 保存/恢复中断，OOMD 在 scheduler tick 已关中断的上下文里调用后仍保持关中断。
  - **证据**：host 结构门槛 `user_page_count_traversal_holds_vmm_lock`（双架构 `acquire_lock()` 先于 `release_lock(` 且函数体注释剔除后判定，防「注释里写着判据」假通过）**已负向验证**——临时把 x86_64 的 `acquire_lock()` 行改成注释 ⇒ `A1: …vmm_x86_64.rs 遍历必须获取 VMM_LOCK` FAILED，恢复后 PASS；QEMU 行为验证 `[150/503] mm::vmm::count_present_user_pages` PASS（持锁遍历无递归获取 panic，基线增量恒等不变）。
  - **偏差登记**：原案「删除 `count_present_user_pages` + aarch64 版本 + 顶层再导出 + `test_mm.rs` 用例 + `memory_pressure_extraction_test.rs` 文本断言」随方案 A **作废不执行**（该 API 面保留为门控项）。
  - **疑点（预存，不在本批授权内）**：x86_64 `acquire_lock`（[vmm_x86_64.rs:1887](../../src/kernel/framework/mm/vmm_x86_64.rs#L1887)）含「单核可重入」短路——`if VMM_LOCK.load(Acquire) { return flags; }`，判据是**全局**锁位而非持有者，故在他核持锁时本核会直接进入临界区，**跨核互斥在 SMP 下不成立**（aarch64 版本无此短路）。本批未触碰该锁实现（属 TCB 核心 + 安全面，四类禁区），仅登记留痕；另 `smp::register_cpu` 当前无 Rust 生产调用点（`SMP_ENABLED` 恒 false、`CPU_COUNT` 恒 1），故本竞态在当前单核配置下亦不显现。
