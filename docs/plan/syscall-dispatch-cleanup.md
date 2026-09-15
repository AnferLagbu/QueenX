# syscall 分发收敛工程（B09-17/18 讨论点 4+5 根治）

> 关联：B09-17/18（QX_* 归位 SYS_*）审核讨论点 4（api.rs 双份 SYS_*）与 5（framework/services 双 dispatch 冗余）。
> 决策：纳入修复（2026-09-14 用户）——避免任务堆积/优先级抢占导致遗忘。
> 依据：以下全部为源码实测（2026-09-14）。

## 描述

- **问题 4**：`framework/syscall/api.rs` 双份 `SYS_*` 常量定义（与 `types.rs` 权威表重复），维护隐患（漂移风险）。
- **问题 5**：framework/services 双 dispatch 分层存在**重叠死分支**与编号维护风险（本次 B09-17 已暴露 QX_* 编号错误致 30 syscall 不可达）。

## 方案

### 部分 A：api.rs SYS_* 去重（问题 4）

**实测现状**：
- `api.rs` L46-88 定义 ~30 个 `SYS_*` 常量，**无任何代码引用**（全仓 grep `api::SYS_` 0 处；api.rs 内非定义行仅注释提及 L3/L7/L43）→ 纯死代码。
- `types.rs` 为权威表（236 个 Linux 编号，DECISION-J 迁回）。
- `MAX_SYSCALLS` 双份（api.rs:91 与 types.rs:37，值同为 900）。

**改动**：
| 项 | 处置 |
|---|---|
| api.rs L42-88 `SYS_*` 常量段 | **删除**（types.rs 唯一权威） |
| api.rs `MAX_SYSCALLS`（L91） | **删除**（types.rs 保留，值一致） |
| api.rs **函数**（validate_user_ptr / write_struct_to_user / sys_kill / sys_nanosleep / sys_rt_sigaction / mmap_get_mm_or_alloc 等） | **保留**（services 大量使用，framework 安全 API 面） |
| api.rs QX 独有 re-export（L40） | 保留 |

**验证**：编译兜底（若有遗漏引用编译报错）+ grep `api::SYS_` 清零。

### 部分 B：双 dispatch 分层收敛（问题 5）

**调用链（实测）**：中断入口 → services `dispatch_from_ctx` → framework `syscall_dispatch`（extern "C"）→ `syscall_dispatch_impl`（总入口）：
```
seccomp 检查 → current_syscall_dispatch().dispatch()（services 策略，225 分支，优先）
            → services 返回 ENOSYS_RET 时 fallthrough
            → framework 回退 match（65 分支，兜底）
```
**分层本质**：services 策略层 + framework 机制/未迁移回退层——**分层合理，不合并**。收敛为：

| # | 改动 | 依据（实测） |
|---|---|---|
| B1 | **删除回退层 4 个重叠死分支**：`SYS_socket / SYS_getpeername / SYS_getsockname / SYS_listen` | services 对应分支为**真实实现**（调 `services::net::syscall::*`，返回非 ENOSYS）→ services 优先命中 → framework 分支**永不执行**。删除行为不变 |
| B2 | **回退层 65 分支分类 audit** | 与 services 重叠 → 删（本次 4 个）；机制独有（FW/FTRACE/CGROUP/ROUTE/NF/PM/SECURE_BOOT/TPM/CET/TICKLESS/TIMESYNC/UEFI/KGDB/SNAPSHOT/GET_CANARY/IO_URING_SUBMIT 等）→ 保留 framework；未迁移待迁移 → 登记 |
| B3 | **分层契约文档化**（写于 syscall/mod.rs 或 dispatch.rs 头注释） | framework 回退层仅"机制 + 未迁移哨兵"；**禁止新增与 services 重叠分支**；新 syscall 归属规范：Linux 标准编号 → services；QX 独有机制 → framework |
| B4 | 回退层编号归位收尾确认 | B09-17 已转 SYS_*，grep 确认无 QX_* 残留（排除 QX 独有） |

### 验证门槛

1. 双架构 `cargo check --release` 0w0e + clippy 0
2. host-tests 全量（socket 相关测试——删除重叠分支后 services 路径生效，行为不变）
3. **QEMU boot**（网络/socket 路径必跑：删除后确认 services socket 分支实际执行）
4. grep 清零：`api::SYS_`（部分 A）+ 回退层无 services 重叠分支（部分 B）

### 风险

1. 删除重叠分支安全前提：services socket 实现完整（已实测真实实现非 ENOSYS）；QEMU 网络冒烟兜底。
2. api.rs SYS_* 删除：编译兜底（无引用已实测）。
3. B2 audit 需逐个分支人工判定（机制 vs 未迁移）——实施时逐分支登记。
4. 契约文档化不改变行为。

## 状态

- [X] 部分 A：api.rs SYS_* 常量段（43 个 = 42 SYS_* + SYS_CREDO_BASE）+ MAX_SYSCALLS 删除，types.rs 唯一权威
- [X] B1：回退层网络重叠分支删除（2 net 臂 + 2 包装函数；socket/listen 仅在哨兵臂，见 B1 实施记录）
- [X] B2：65 编号分类 audit（27 机制独有 / 24 未迁移登记 / 14 哨兵，真实重叠归零）
- [X] B3：分层契约文档化（mod.rs "双层分发契约" + dispatch.rs 哨兵区注释）
- [X] B4：全量验证通过（build all 5/5 / clippy 双架构 0w / 核心审计 8 项 / host-tests 全量 / QEMU boot Ring3 / QEMU kernel_test 468 全过含 UDS 套件），commit aea73cd1

## 详情

### 实测数据（2026-09-14）

| 项 | 数值 |
|---|---|
| api.rs SYS_* 常量 | ~30 个（死代码，0 引用） |
| framework 回退层 match 分支 | 65 个（含 B09-17 归位后 SYS_*） |
| services dispatch 分支 | 225 个 |
| **重叠分支** | **4 个**：SYS_socket / SYS_getpeername / SYS_getsockname / SYS_listen（services 真实实现 → framework 侧死分支） |
| MAX_SYSCALLS 双份 | api.rs:91 + types.rs:37（值同 900） |

### 关联

- B09-17/18 主任务（QX_* 归位）：本工程为其收尾（讨论点 4/5）。
- DECISION-J（types.rs 迁回）：api.rs 双份是迁移遗留，本工程消除。
- framework 回退层机制独有项（FW/CGROUP 等）为 QX 扩展 syscall，保留。

### B1 实施记录（2026-09-14）

实测修正：回退层网络区实际结构为 **2 个 net 使能真实臂**（`SYS_getsockname` / `SYS_getpeername`，包装 `framework::net::syscall`）+ **1 个 `not(net)` ENOSYS 哨兵臂**（含 14 编号，socket/listen 仅在此臂中，无独立真实分支）。故"4 个重叠分支"落地为：

- 删除 2 个 net 使能真实臂（services `dispatch_net` 无 cfg 门控、所有构建配置下优先命中且返回非 ENOSYS → framework 侧死分支）。
- 连带删除失去引用的包装函数 `sys_getsockname` / `sys_getpeername`（F9 零容忍）。
- socket / listen 无需单独处理（仅存在于哨兵臂，非真实实现分支）。
- 哨兵臂保留并加分层契约注释（B3 落地于 dispatch.rs + mod.rs）。

验证：x86_64 release check 0w0e（default features，同时覆盖 not(net) 哨兵路径编译）。

### B2 分类 audit 登记（2026-09-14）

回退层实测 **53 个 match 臂 / 65 个 syscall 编号**（含 `_` 兜底）。逐编号与 services dispatch（209 个编号，`dispatch_fs/proc/net/mm/sync/credo/other` 七路）求交分类：

| 分类 | 数量 | 编号清单 | 处置 |
|---|---|---|---|
| 机制独有（QX_* 扩展） | 27 | FW_LOAD/GET/GET_INFO/DETACH(4)、FTRACE_ENABLE/DISABLE/READ/STAT(4)、KGDB_ENTER、ROUTE_ADD/DEL/QUERY(3)、NF_ADD/DEL_RULE(2)、IO_URING_SUBMIT、CGROUP_CREATE/DESTROY/ATTACH/SET_LIMIT/GET_STAT(5)、PM、SECURE_BOOT、TPM、CET、TICKLESS、TIMESYNC、UEFI | 保留 framework（QX 独有机制） |
| 未迁移（services 无对应，回退真实执行） | 24 | read、write、rt_sigreturn、seccomp、prctl、io_uring_setup/enter/register(3)、unshare、setns、bpf、kexec_load、tcgetpgrp、tcsetpgrp、execve、setrlimit、tgkill、sendfile、splice、CREDO_DISK_INSTALL、CREDO_HOTPLUG_STATUS、FB_OPEN/MMAP/RELEASE(3) | 登记，后续按迁移工程逐批转 services |
| 哨兵（services 真实实现，回退仅 ENOSYS） | 14 | socket、connect、accept、sendto、recvfrom、shutdown、bind、listen、sendmsg、recvmsg、setsockopt、getsockopt、getsockname、getpeername（单一 `not(net)` 臂） | 保留哨兵（B3 契约允许项） |

**结论**：B1 后回退层与 services 的真实实现重叠为 **0**；唯一交集即 14 编号哨兵臂（ENOSYS，兜底日志 `net_nosys`）。
