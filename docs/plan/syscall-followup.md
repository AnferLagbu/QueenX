# syscall 后续工程总纲（编号治理闭环后的剩余任务）

> 关联：B09-17/18（编号归位）+ syscall-dispatch-cleanup（问题 4/5 收敛）均已完成（commit `aea73cd1`）。
> 本总纲规划**剩余任务**：功能实装（T1）、分层迁移（T2）、半成品清除（T3）、审计核实（T4/T5/T6）、预存登记（T7）。
> 数据来源：分册 9 工程计划 D + syscall-dispatch-cleanup B2 分类 + 2026-09-15 源码核实。

## 描述

syscall 编号空间治理已闭环（QX_* 归位 SYS_*、双 dispatch 重叠归零）。剩余为三类：
1. **功能实装**：Linux 有标准编号但未实装的 syscall（T1）。
2. **分层收尾**：framework 回退层 24 个"未迁移" syscall 转 services（T2），其中半成品先清除（T3）。
3. **审计核实**：R3/R1/TODO 遗留（T4/T5/T6）+ aarch64 编号预存（T7）。

## 任务方案

### T1：R2 未实装 SYS_* 功能实装（P0，POSIX 兼容）

**范围**：分册 9 D-2 清单（~33 项，**实施前重扫核实**——sendfile 已实装 services/fs/sendfile.rs，D-2 清单过时）。

**分组**（按 POSIX 兼容优先级）：

| 组 | 成员 | 优先级 |
|---|---|---|
| G1 文件 I/O 核心 | readv/writev/preadv/pwritev/statx/close_range/fchownat/utimensat/fallocate | P0 |
| G2 进程/信号 | prctl/arch_prctl/capget/capset/set_robust_list/get_robust_list/waitid | P0 |
| G3 网络 | recvmmsg/sendmmsg/socketpair | P1 |
| G4 内存 | mbind/userfaultfd | P1 |
| G5 多路复用 | inotify_init/epoll_pwait/ppoll | P1 |
| G6 时间 | clock_nanosleep/settimeofday/adjtimex | P1 |
| G7 文件系统 | pivot_root/chroot/setdomainname/execveat | P2 |

**方案**：每项 = services 实现 → dispatch 接线（services 层）→ host 单测 + 集成测试 → QEMU 冒烟（触路径）。
**验证**：每批双架构 0w0e + clippy 0 + host-tests + QEMU。
**待核实**：部分项可能已部分实装（如 sendfile）；process_vm_readv/writev 安全面特殊（跨进程读写，需权限校验设计）。

### T2：回退层 24 未迁移 syscall → services 迁移（P1，F/S 分层）

**范围**：syscall-cleanup B2 登记的 24 项（services 无对应、framework 回退真实执行）：
read/write/rt_sigreturn/seccomp/prctl/io_uring_setup/enter/register/unshare/setns/bpf/kexec_load/tcgetpgrp/tcsetpgrp/execve/setrlimit/tgkill/sendfile/splice/CREDO_DISK_INSTALL/CREDO_HOTPLUG_STATUS/FB_OPEN/MMAP/RELEASE。

**方案**：逐批迁移——每项：services 实现（或确认 services 已实现如 sendfile）→ services dispatch 接线 → 回退层删分支 + 契约注释更新 → 测试。
**分批**：
- 批 1 核心 I/O/进程：read/write/execve/rt_sigreturn/setrlimit/tgkill（用户态核心，优先级最高；实际迁移项为 read/write/execve/setrlimit——rt_sigreturn 已随 T3 清除，tgkill 归 T1 实装）
- 批 2 安全/机制：seccomp/prctl/tcgetpgrp/tcsetpgrp
- 批 3 网络/异步：sendfile/splice/io_uring×3
- 批 4 namespace/调试：unshare/setns/bpf/kexec_load
- 批 5 平台扩展：CREDO×2/FB×3

**验证**：每批双架构 0w0e + host-tests + QEMU（syscall 路径必跑）。
**注意**：迁移后 framework 回退层仅剩"机制独有 27 + 哨兵 14"（B2 契约允许面），F/S 分层收尾。

### T3：半成品清除 + 用户态调用 audit（P1，安全面）

**前置**：用户态调用 audit——grep src/user + src/userland 对 24 项的调用（已初步：用户态暂不用 sendfile/readv 等高级项；核心 read/write/execve 必然被用）。
**方案**：
1. audit 产出"保留（有用户）/清除（无用户半成品）"清单。
2. 半成品判定：参数忽略（如 sys_tgkill 的 `_tgid` 未使用）、stub/占位返回、未完整实现。
3. **清除**：无用户的半成品回退分支 → 删除 → 恢复 ENOSYS（安全态，QEMU 兜底确认用户态不依赖）。
4. **保留**：有用户的核心（read/write/execve 等）→ 直接并入 T2 迁移（framework 已实装则迁 services）。
**验证**：双架构 0w0e + host-tests + QEMU boot（确认清除后用户态程序不回归）。
**注意**：T3 与 T2 联动（清除 = 迁移的前置筛除），可合并为"回退层收敛"工程，T3 先行 audit。

### T4：R3 零引用 pub mod 7 项核实（P2）

**范围**：dmu_trait/raidz_trait/spa_trait/txg_trait/zap_trait/zil_persist_trait/zil_trait（nestfs 的 trait 抽象层）。
**方案**：逐个核实（接线使用 → 保留；纯预留 → 删除或合并）。并入 NestFS 收尾工程。

### T5：R1 pub fn 445 项甄别（P2，大）

**方案**：逐项分拣三态（已接线保留 / 转正式实装 / 删除）。审计工具驱动（audit_unwired_pub_fn），分批。
**与 T1 联动**：R1 中"应实现未实现"的归 T1 功能实装；纯死代码归删除。

### T6：TODO 转正式 33 项（P2）

**范围**：TRACK- 23 + 普通 10（分册 9 D-5）。
**方案**：实现治理（实装对应功能）；过期/无价值 TODO 直接删。独立工程。

### T7：aarch64 syscall 编号空间（P2，预存登记）

**问题**：Linux aarch64 编号 ≠ x86_64（如 aarch64 read=63 vs x86_64 read=0、bpf aarch64=280 vs x86_64=321）；dispatch 当前单编号表（x86_64 基准）。
**处置**：登记预存——若 aarch64 需跑 Linux 用户态二进制，需架构相关编号表（dispatch 按 target_arch 分派）。当前 aarch64 无用户态 syscall 场景（QEMU 冒烟仅内核），暂不阻塞。

## 优先级与依赖

```
T3 (audit 前置) → T2 (回退层收敛) ─┐
                                   ├→ 完成后 framework 回退层 = 机制 27 + 哨兵 14（分层收尾）
T1 (功能实装, 独立并行) ───────────┘
T4/T5/T6 (审计核实, 低优先)
T7 (预存登记)
```

- **依赖**：T3 audit 是 T2 迁移范围的前置筛除；T1 独立可并行。
- **建议执行序**：T3 audit → T2 批 1-2 → T1 G1-G2（核心组）并行；T4-T6 排后；T7 仅登记。

## 验证门槛（每任务）

1. 双架构 `cargo check --release` 0w0e + clippy 0
2. host-tests 全量（新增 syscall 必有单测/集成测试）
3. **QEMU boot**（dispatch 改动必跑）
4. 核心审计（boundary/coupling——迁移 services 后 F1 services 0 unsafe 保持）

## 状态

- [X] T3：半成品清除 + 用户态调用 audit（2026-09-15 完成，见下方 T3 实施记录）
- [ ] T2：回退层 21 保留项 → services 迁移（批 1 完成，见下方 T2 实施记录；批 2-5 待做）
- [ ] T1：R2 未实装 SYS_* 实装（先重扫核实清单，与 T2 并行）
- [ ] T4-T7：登记排后（R3 pub mod / R1 445 项 / TODO 33 项 / aarch64 编号）

### T3 实施记录（2026-09-15）

**用户态调用 audit 结论**（src/user 存在；src/userland 不存在，AGENTS.md 记载过时）：

| 分类 | 项 | 依据 |
|---|---|---|
| 保留（有用户，T2 迁移） | read、write、execve | io.rs/eash/install/fbterm 全量使用 |
| 保留（有用户，T2 批 5） | CREDO_DISK_INSTALL | install/wizard/prepare.rs `boot_install` |
| 保留（有用户，T2 批 5） | FB_OPEN/MMAP/RELEASE | fbterm main.rs:179/184/271 直用 |
| 保留（无用户但真实实现，T2 迁移） | seccomp、prctl、tcgetpgrp、tcsetpgrp、setrlimit、unshare、setns、bpf、kexec_load、sendfile、splice、io_uring_setup、io_uring_enter、CREDO_HOTPLUG_STATUS | 实现均非 stub；用户态无常量/wrapper/调用 |
| **清除（无用户半成品/死分支）** | **tgkill、rt_sigreturn（回退分支）、io_uring_register** | 见下 |

**清除明细**（恢复 ENOSYS 安全态，types.rs 权威编号常量保留）：

1. **SYS_tgkill**：半成品——`_tgid` 参数忽略 + 语义错位（将 tid 当 pid 走 sys_kill）；用户态 wrapper 0 调用方。删 dispatch 分支 + `sys_tgkill` + 用户态 `tgkill` wrapper/常量。
2. **SYS_rt_sigreturn 回退分支**：死分支——pre-dispatch 特殊路径（dispatch.rs `is_rt_sigreturn`）无条件 `return` 拦截，回退层永不可达；`sys_rt_sigreturn` 残缺（仅清标志不恢复上下文）。删分支 + 函数；**SS_ONSTACK 清除逻辑（P1-I-45）迁移至可达的 pre-dispatch 路径**（顺带修复潜在缺口：原可达 sigreturn 路径从未清标志）；host-tests sigaltstack_test 源码断言同步指向新位置。
3. **SYS_io_uring_register**：恒 ENOSYS 桩，删除后落 `_ =>` 兜底 ENOSYS 行为不变。删分支 + iouring.rs 函数。

**半成品补充发现（不清除，登记 T2 处置）**：`sys_fb_release` 空 stub（恒 0 无释放动作，有用户 → T2 批 5 实装）；`sys_tcgetpgrp/sys_tcsetpgrp` `_fd` 参数忽略（POSIX 简化语义，T2 批 2 迁移时确认）；用户态 sys.rs `SYS_CREDO_HOTPLUG_STATUS` 常量无 wrapper 无调用（T2 批 5 补 wrapper）。

**验证**：build.sh all 5/5（双架构 0w0e + host-tests 全量 + asm 门控 + link）、clippy 双架构 0 warning、核心审计 8 项通过、QEMU boot（Ring 3/init）通过。

### T2 实施记录（批 1）

**迁移项**（4 项，services 0 unsafe）：

| 项 | services 落点 | 要点 |
|---|---|---|
| read/write | `services/fs/io.rs`（`read_syscall`/`write_syscall`） | fd 路由策略（stdin/stdout/eventfd/signalfd/timerfd/inotify/VFS）整体迁入；fd 严格转换（try_from，失败 -EINVAL）随分支迁至 services dispatch |
| execve | `services/proc/exec.rs`（`execve_syscall`） | 指针校验 + argv 扫描（`api::read_u64_from_user` safe 读取）+ SUID 判定 + 委托 `proc_exec_replace`；`ExecveResult` 包装类型删除（唯一消费者是被删回退分支），services 直接返回精确 Errno（EFAULT/ENOENT，与原 from_ret 映射一致） |
| setrlimit | `services/proc/sysinfo.rs`（`setrlimit_syscall`，与 getrlimit 同落点） | RlimitTable 机制字段留在 framework（DECISION-J 第十九批判据不变）；services 做策略：校验 + `read_struct_from_user` 读取 + 特权判定（pid==1）+ 委托表更新 |

**机制统一**：write 路径的用户数据拷贝由 framework dispatch 私有的 `copy_from_user_buf`（CR3 页表走查）统一到 `framework::mm::copy_user`（`copy_from_user`/`copy_to_user`，异常表兜底，services 已有使用先例）；并行实现收敛为单一权威。stdin 单字节写入改 `copy_to_user`（替代 unsafe `raw::write_u8`，后者随迁移失去唯一调用方删除）。

**framework 回退层清理**：删 SYS_read/SYS_write/SYS_execve/SYS_setrlimit 4 分支 + `sys_read`/`sys_write`/`sys_execve`/`copy_from_user_buf`/`try_fd` + `framework/syscall/execve.rs` 模块；types.rs 编号常量保留（编号空间权威不受迁移影响）。getrlimit 早于本批已迁 services，故 framework `proc/rlimit.rs` 的 `sys_getrlimit`/`sys_setrlimit` 两个策略入口函数在迁移后失去全部调用方（仅 re-export 自引用），随本批删除，re-export 收口为机制字段与查询辅助（`RLIM_INFINITY`/`RLIMIT_CORE`/`get_memlock_limit`）；`RlimitTable` 机制字段保留（DECISION-J 第十九批判据）。

**验证**：build.sh all 5/5、clippy 3 维（pedantic x86_64 + kernel_test + host-test）0 warning、核心审计 8 项通过（deadlock 唯一 HIGH 为预存 AP_STARTUP_LOCK 人工审查项，非本次引入）、QEMU boot（Ring 3/init）通过、QEMU kernel_test 468/468 全过（含用户态 init/eash 真实执行 read/write 路径）。

### T2 实施记录（批 2）

**迁移项**（4 项，services 0 unsafe）：

| 项 | services 落点 | 要点 |
|---|---|---|
| seccomp | `services/proc/seccomp.rs`（`seccomp_syscall`） | 策略：operation 校验（STRICT/FILTER）+ no_new_privs 特权判定 + 委托机制状态变更（mode/filters 经 `process_with` + `SeccompState` 公开字段）；`MAX_FILTERS`/`DEFAULT_ACTION` 转 pub 留在 framework（seccomp_check 机制共用权威），services 引用 |
| prctl | `services/proc/seccomp.rs`（`prctl_syscall`） | 仅实装 PR_SET/GET_SECCOMP + PR_SET/GET_NO_NEW_PRIVS 四 option，其余 ENOSYS（与原行为一致）；PR_* 常量随迁 |
| tcgetpgrp | `services/proc/session.rs`（`tcgetpgrp_syscall`） | 委托 framework `get_foreground_pgid`（机制查询辅助）；POSIX 简化 `_fd` 忽略（T3 登记项随本批确认） |
| tcsetpgrp | `services/proc/session.rs`（`tcsetpgrp_syscall`） | 校验 pgid 属当前会话进程组（`process_for_each` 遍历）+ 委托 `SESSION_MANAGER.set_foreground_pgid` |

**机制保留（framework）**：`SeccompState`/`SeccompFilter`/`SeccompRule`/`SeccompAction`/`SeccompMode`/`seccomp_check`/`add_rule`（seccomp_check 被 framework 分发前置消费）；`SessionManager`/`SESSION_MANAGER`/`get_foreground_pgid`/`proc_setsid` 等会话机制与进程组策略。

**framework 回退层清理**：删 SYS_seccomp/SYS_prctl/SYS_tcgetpgrp/SYS_tcsetpgrp 4 分支 + `sys_seccomp`/`sys_prctl_prctl`/`sys_tcgetpgrp`/`sys_tcsetpgrp` + PR_* 私有常量；proc/mod.rs re-export 收口（seccomp 移除策略入口、补充机制类型/常量导出；session `pub use session::*` 自动收口）。

**验证**：build.sh all 5/5、clippy 3 维 0 warning、核心审计通过、host-tests 全量、QEMU boot（Ring 3/init）通过。

## 详情

### 源码核实（2026-09-15）

- sendfile 已实装：`services/fs/sendfile.rs:13`（包装 `framework::syscall::sys_sendfile`）→ **D-2 清单含 sendfile 属过时**，实施前重扫。
- services dispatch 无 read/write/execve/prctl 分支（framework 回退层执行）→ B2 24 未迁移分类准确。
- 用户态暂不用 sendfile/readv/writev/preadv（src/user+userland grep 空）→ 高级项实装优先级可据用户态需求排后。
