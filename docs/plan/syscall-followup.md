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
- [X] T2：回退层保留项 → services 迁移（批 1-5 全部完成，见下方 T2 实施记录）
- [X] T1：R2 未实装 SYS_* 实装（G1-G7 全部完成，见下方 T1 实施记录）
- [X] T4：R3 零引用 pub mod 7 项核实（2026-09-18 完成，处置＝删除，见下方 T4 实施记录）
- [ ] T5：R1 甄别（批 1 完成：R1 447 → **438**，处置＝只删确证无用 9 项；批 2 完成：438 项全量扫描已登记，**经 reviewer 四轮复核后按修订口径重划**（删候选 **2** / 硬件原语完整性保留 **43** / 接线 8 / 未来功能 339 / 待裁 **46**，合计 438；**均为暂定值**），未改代码；**A-2 已退回重做并重新定型**（判据升格三合一；原 23 项＝11 留 + 4 安全面 + 8 退桶；**第三轮**：`ct_eq_salt`/`ct_eq_password` ⇒ 族残缺入完整性保留 41→43，ramfs `split_path`/`validate_path` ⇒ 挂起待 T3 结论入 B-4 待裁 35→37，安全面桶清零），**施工方式＝逐项试删 + 既有五条门槛（不立项新工具）**，任一维硬失败即回退；**A-2 11 项试删已由 reviewer 第三轮授予开工**（解锁五条 ①②③④⑤ 已逐条核销），执行约束＝逐项独立提交 / 每项跑五门槛全量 / **QEMU boot 硬闸门** / ramfs 2 项不入本批；**第四轮开出后逐项复核实测发现 9 项判据不成立**（6 项「同族兄弟在用」＝**族残缺**形态、2 项台账 ② 判据事实错误、1 项无等价公共入口）⇒ 按裁定**退桶 9 项入待裁**（删候选 11 → **2**，待裁 37 → **46**），本轮仅 `write_log_line` / `format_duration` 2 项进入试删。见「T5 实施记录」「T5 全量甄别台账」）
- [ ] T6-T7：登记排后（TODO 33 项 / aarch64 编号）

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

### T2 实施记录（批 3）

**迁移项**（4 项，services 0 unsafe）：

| 项 | services 落点 | 要点 |
|---|---|---|
| sendfile | `services/fs/sendfile.rs`（`sys_sendfile`，既有封装，本次接线） | 委托 framework 机制 `framework::syscall::sendfile::sys_sendfile`（VFS/IPC/pipe 访问 + unsafe 用户 offset 指针，机制留 framework） |
| splice | `services/fs/sendfile.rs`（`sys_splice`，既有封装，本次接线） | 同上，委托 `framework::syscall::sendfile::sys_splice` |
| io_uring_setup | `services/io/iouring.rs`（`io_uring_setup_syscall`） | 委托 framework 机制 `io_uring_setup`（全局实例表 + ID 分配），services 仅参数转换 + errno 映射 |
| io_uring_enter | `services/io/iouring.rs`（`io_uring_enter_syscall`） | 委托 framework 机制 `io_uring_enter`（SQE→CQE 处理） |

**机制保留（framework）**：`framework/syscall/sendfile.rs`（sys_sendfile/sys_splice 完整实现 + SPLICE_F_* 常量，services 经 `framework::syscall` 顶层 re-export 消费）；`framework/io/iouring.rs`（IoUring/RingBuffer/Sqe/Cqe + io_uring_setup/enter/destroy/submit/reap 机制函数 + `sys_io_uring_submit_sqe`——QX_IO_URING_SUBMIT 机制独有，留在回退层）。

**framework 回退层清理**：删 SYS_sendfile/SYS_splice/SYS_io_uring_setup/SYS_io_uring_enter 4 分支 + `sys_io_uring_setup`/`sys_io_uring_enter` 薄策略入口（参数转换类，迁 services 后无调用方）；sendfile/splice 实现在 framework 保留为机制库（services 委托调用）。

**验证**：build.sh all 5/5、clippy 3 维 0 warning、核心审计通过、host-tests 全量、QEMU boot（Ring 3/init）通过。

### T2 实施记录（批 4）

**迁移项**（4 项，services 0 unsafe）：

| 项 | services 落点 | 要点 |
|---|---|---|
| unshare | `services/proc/namespace.rs`（`unshare_syscall`） | 委托 `NamespaceSet::unshare`（机制：按 flags 创建新 ns 实例）；services 仅 errno 映射 |
| setns | `services/proc/namespace.rs`（`setns_syscall`） | 参数解析（CLONE_NEW_* 标志位与简化枚举双语义，B06-18 语义保留）+ CAP_SYS_ADMIN 特权判定（`credo::pwm_has_capability`，经顶层 re-export）+ 委托 `setns_by_type` |
| bpf | `services/debug/ebpf.rs`（`bpf_syscall`，既有安全代理接线） | 委托 framework `debug::sys_bpf`（机制） |
| kexec_load | `services/driver/kexec.rs`（`kexec_syscall`，既有安全代理接线） | 委托 framework `driver::sys_kexec`（extern "C" 机制函数） |

**机制保留（framework）**：`framework/proc/namespace.rs`（NamespaceSet/各 ns 实例/NsType/CLONE_NEW_*/NsRegistry/ns_register + unshare/setns_by_type 机制语义）；`framework/debug`（sys_bpf + BPF 子系统）；`framework/driver/kexec.rs`（sys_kexec + KexecSubsystem）。

**framework 回退层清理**：删 SYS_unshare/SYS_setns/SYS_bpf/SYS_kexec_load 4 分支 + `sys_unshare`/`sys_setns` 策略入口；proc/mod.rs namespace re-export 收口（NamespaceSet 保留，sys_setns/sys_unshare 移除）；bpf/kexec 机制函数保留（services 代理委托调用）。

**验证**：build.sh all 5/5、clippy 3 维 0 warning、核心审计通过（TD-22 注释中文化 100%）、host-tests 全量、QEMU boot（Ring 3/init）通过。

### T2 实施记录（批 5，收官）

**迁移项**（5 项，services 0 unsafe）：

| 项 | services 落点 | 要点 |
|---|---|---|
| CREDO_DISK_INSTALL | `services/credo/storage/disk.rs`（`boot_install_syscall`） | cfg 门控镜像原回退层（x86_64 生产委托 `sys_boot_install`；kernel_test ENOSYS；aarch64 生产分支不编译走 `_ =>` 兜底）；机制含磁盘扇区写 + credo 权限检查留 framework |
| CREDO_HOTPLUG_STATUS | 同上（`hotplug_status_syscall`） | 委托 `sys_hotplug_status`（unsafe 用户 buffer 写入 + 驱动状态读取）；用户态补 `hotplug_status` wrapper（T3 登记项） |
| FB_OPEN | `services/driver/fb.rs`（`fb_open_syscall`，新建模块） | 委托 `sys_fb_open`（FB 驱动读取 + 用户 FbInfo 写入） |
| FB_MMAP | 同上（`fb_mmap_syscall`） | 委托 `sys_fb_mmap`（页表映射）；机制层新增 `FB_MAP_RECORD` 单槽记录映射区间供 release 解除 |
| FB_RELEASE | 同上（`fb_release_syscall`） | **自空 stub 实装**：依 `FB_MAP_RECORD` unmap 页表并清记录（SIMPLIFIED：单槽，多映射需扩展 per-process 表） |

**framework 回退层清理**：删 SYS_CREDO_DISK_INSTALL/SYS_CREDO_HOTPLUG_STATUS/SYS_FB_OPEN/SYS_FB_MMAP/SYS_FB_RELEASE 5 分支；5 个机制函数 pub 化（经 `framework::syscall` 顶层 `pub use dispatch::*` re-export 供 services 委托）；`sys_fb_release` 空 stub 实装（FB_MAP_RECORD 记录 + unmap）。

**验证**：build.sh all 5/5、clippy 3 维 0 warning、核心审计通过、host-tests 全量、QEMU boot（Ring 3/init）通过。

**T2 收官结论**：21 保留项全部迁移（批 1-5）；framework 回退层收敛为机制独有 + ENOSYS 哨兵，与分层契约一致（B2 契约允许面）。

### T1 实施记录（G1 文件 I/O 核心）

**重扫核实**（2026-09-15）：D-2 清单中 sendfile 已实装过时（T3 已核实）；除 inotify_init1 外，G1 全部 9 项在 services/framework dispatch 均无分支，确认未实装。

**实装项**（9 项，services 0 unsafe）：

| 项 | services 落点 | 机制/要点 |
|---|---|---|
| readv/writev | `services/fs/io.rs`（`readv_syscall`/`writev_syscall`） | 逐 iovec 段委托 `read_syscall`/`write_syscall`（复用 fd 路由 + 校验）；iovec 数组经 `api::read_struct_from_user` safe 逐条解析（IOV_MAX=1024） |
| preadv/pwritev | 同上（`preadv_syscall`/`pwritev_syscall`） | **framework 新增 `vfs_pread`/`vfs_pwrite`**（显式 offset 读/写，不更新 fd 当前偏移，SIMPLIFIED 不走 pcache 快路径）；services 逐段委托 + pos<0 → EINVAL |
| statx | `services/fs/stat.rs`（`statx_syscall` + `Statx` 结构） | 组装 Linux `struct statx`（256 字节）；SIMPLIFIED 仅填基础字段（mode/uid/gid/size/nlink/ino/时间戳），dev/btime 置 0 |
| close_range | `services/fs/io.rs`（`close_range_syscall`） | 遍历 VFS 全局 fd 表 `[first,last]` 占用条目，逐个 `vfs_close`（先收集再关，避免表锁重入死锁）；SIMPLIFIED 仅 flags=0（UNSHARE/CLOEXEC → ENOSYS） |
| fchownat | `services/fs/file_ops.rs`（`fchownat_syscall`） | SIMPLIFIED 仅 `dirfd==AT_FDCWD`（复用 `chown_syscall` UID/GID→PWM 查表 + `vfs_chown_ext`）；非 AT_FDCWD → ENOTSUP |
| utimensat | `services/fs/stat.rs`（`utimensat_syscall`） | NULL times → 当前时间（tick/frequency 换算秒）；非 NULL 读用户 `timespec[2]`，sec==-1 表示不修改（u64::MAX）；委托 `vfs_utimensat_safe`（顶层 re-export 补 vfs_utimensat_safe） |
| fallocate | `services/fs/file_ops.rs`（`fallocate_syscall`） | SIMPLIFIED 仅 mode=0（扩展文件大小到 offset+len，仅扩展不缩小）；基于 `vfs_fstat_safe` + `vfs_truncate_internal` 近似 |

**framework 机制扩展**：`vfs_pread`/`vfs_pwrite`（handle.rs，显式 offset 不更新 fd 偏移）+ 顶层 re-export（vfs/mod.rs 补 vfs_pread/vfs_pwrite/vfs_utimensat_safe）。

**验证**：build.sh all 5/5、clippy 3 维 0 warning、核心审计通过、host-tests 全量、QEMU boot（Ring 3/init）通过。

### T1 实施记录（G2 进程/信号）

**实现路径裁定**（AskUserQuestion，用户授权）：tgkill / waitid / set+get_robust_list / prctl(PR_SET_NAME/PR_GET_NAME) = 相对完整设计实装；capget/capset / arch_prctl = **ENOSYS 保留**（无凭证能力模型 / 无 arch 相关用户态需求，登记不实装）。

**实装项**（5 项 + framework 退出路径机制，services 0 unsafe）：

| 项 | services 落点 | 机制/要点 |
|---|---|---|
| tgkill | `services/proc/signal.rs`（`tgkill_syscall`） | 校验链：tgid<=0/tid<=0 → EINVAL；sig 范围 0..=63（0=存在性探测）→ EINVAL；目标不存在或 `target.pid != tgid` → ESRCH；委托 `api::sys_kill`。SIMPLIFIED：tgid==tid 等价校验替代"tid ∈ tgid 线程组"判定（当前 tid≡pid 无线程组模型；K-06 引入线程组后改查 tid 所属进程 tgid） |
| waitid | `services/proc/wait4.rs`（`waitid_syscall`） | idtype P_ALL/P_PID/P_PGID（P_PGID 转负值复用统一收集）；options 合法位校验 + 必须含 WEXITED/WSTOPPED/WCONTINUED 之一；委托 framework `wait_reap`；Reaped 且 infop 非 0 → 组装 128B `SiginfoChld`（si_signo=SIGCHLD、si_code=CLD_EXITED）经 `api::write_struct_to_user` 写回；Running → Ok(0)（WNOHANG）；NoChild → ECHILD。SIMPLIFIED：拒绝 WSTOPPED/WCONTINUED（无 stop/continue 状态跟踪）、si_code 恒 CLD_EXITED、si_uid 恒 0 |
| set_robust_list | `services/proc/clone.rs`（`set_robust_list_syscall`） | len != 24（`struct robust_list_head` ABI 大小）→ EINVAL；`process_with` 登记 robust_head/robust_len |
| get_robust_list | 同上（`get_robust_list_syscall`） | pid==0 → 当前进程；head/len 指针非 0 时经 `api::write_struct_to_user` 写回（失败 EFAULT）。SIMPLIFIED：不做 PTRACE_MODE_READ 权限校验（无 ptrace/uid 模型） |
| prctl(PR_SET_NAME/PR_GET_NAME) | `services/proc/seccomp.rs`（prctl_syscall 补两 arm） | SET_NAME：`copy_string_from_user` 读 16B → truncate(15)+NUL → UTF-8 lossy 写 comm；GET_NAME：comm 16B NUL 结尾写回用户 `[u8;16]` |

**framework 机制扩展**：

- **Process 机制字段**（process.rs）：`clear_child_tid: AtomicU64`（CLONE_CHILD_CLEARTID 清除地址）、`robust_head: AtomicU64` + `robust_len: AtomicU32`（robust list 登记项）。
- **`framework/proc/robust.rs`（新建，退出清理机制）**：`exit_cleanup(pid)` 退出路径执行——clear_child_tid 非 0 → 写 0 + `futex_wake` + 清字段；robust_head 非 0 → 遍历 robust list（先读 next 再处理当前，防链表破坏；`ROBUST_LIST_LIMIT=2048` 防环形链表），futex 字 `(word & FUTEX_TID_MASK) == pid` 时置 `FUTEX_OWNER_DIED` 并唤醒。SIMPLIFIED：list_op_pending 不区分 get_robust_list 与 set_robust_list 两种 pending 语义（直接按 uaddr 处理）。内核测试：`robust::futex_word_masks` / `robust::robust_head_layout`（24B ABI 验证）。
- **退出路径挂钩**（proc_ops.rs `process_exit`）：flock 释放之后、切换内核页表/销毁用户地址空间之前调用 `robust::exit_cleanup`（用户内存仍可访问）。
- **clone.rs 消费**：CLEARTID 登记（fork 与 CLONE_VM 双路径均登记 child_tidptr）；CHILD_SETTID 仅 CLONE_VM 路径写 child tid（fork 路径父上下文写入落父地址空间，COW 后子不可见——Linux 同语义）；`_child_tidptr` 改名 `child_tidptr`。
- **wait4.rs 统一收集机制重构**：新增 `WaitInfo{pid, exit_code}` + `WaitOutcome{Reaped/Running/NoChild}` + `wait_reap(target_pid, non_blocking, keep_zombie)`，wait4 与 waitid 共用；`keep_zombie` 实现 WNOWAIT；sys_wait4 重写为 `wait_reap` 委托（行为等价）；wait4 的 target_pid < -1（P_PGID 匹配）从 ENOSYS 桩实装（匹配 Process.pgid）。
- **futex.rs**：`futex_wake` pub 化（robust 退出路径唤醒消费端）。

**ENOSYS 保留登记**：capget/capset（无凭证能力模型）、arch_prctl（无 arch 相关用户态需求）——不实装，保留回退层 ENOSYS 哨兵。

**验证**：build.sh all 5/5、clippy 3 维 0 warning、核心审计通过、host-tests 全量、QEMU boot（Ring 3/init）通过、QEMU kernel_test 470/470（较前批 +2：robust 两测试）。

### T1 实施记录（G3 网络）

**实现路径裁定**（AskUserQuestion，用户授权）：socketpair / sendmmsg / recvmmsg = **B 相对完整设计实装**——socketpair 支持 AF_UNIX Stream+Dgram 双类型，recvmmsg/sendmmsg 循环复用既有收发原语，recvmmsg 完整 timeout 语义（get_ticks deadline + scheduler_yield 重试），mmsghdr msg_flags 逐条透传。

**实装项**（3 项，services 0 unsafe）：

| 项 | services 落点 | 机制/要点 |
|---|---|---|
| socketpair | `services/net/syscall.rs`（`socketpair_syscall`） | sv 缓冲校验（EFAULT）；domain≠AF_UNIX → ENOTSUP、protocol≠0 → EPROTONOSUPPORT、type 非 Stream/Dgram → ENOTSUP；委托 `uds_socketpair`；sv 写回 packed u64（低 32 位 fd0 / 高 32 位 fd1，低 4 字节落 sv[0]），写回失败 close 两端 → EFAULT |
| sendmmsg | 同上（`sendmmsg_syscall`） | vlen==0 → Ok(0)、vlen>UIO_MAXIOV(1024) → EINVAL；逐 64B mmsghdr entry 校验（EFAULT）；UDS 分流 `sendmmsg_uds_entry`（`gather_entry_iov` 收集 iov；msg_name 非空走路径发送（与 sendto 一致）、空走 `uds_send_connected`，超 UNIX_DGRAM_MAX → EMSGSIZE）/ 非 UDS 委托 `sendmsg_syscall`；msg_len 经 `raw_copy_out(entry+56, 4, …)` 写回（4 字节粒度避免越界下条 entry）；出错且已发送 ≥1 条 → break 返回已发条数（Linux 语义） |
| recvmmsg | 同上（`recvmmsg_syscall`） | timeout timespec 解析（sec<0 或 nsec 越界 → EINVAL；rel_ms checked_mul/add 溢出饱和 u64::MAX）；blocking = timeout_ptr≠0 且无 MSG_DONTWAIT；EAGAIN 时满足（非 blocking / deadline 已过 / MSG_WAITFORONE 且 received>0）其一 → break（received==0 的 EAGAIN 终态返 EAGAIN），否则 `scheduler_yield()` 重试；成功逐条补写 msg_len（recvmsg_syscall 不写 msg_len，mmsghdr 语义须补）；UDS 分流 `recvmmsg_uds_entry`（`uds_sock_type` 分流 Stream recv / Dgram recvfrom，单缓冲 cap=min(iov 总容量, 类型缓冲上限)，逐段 `copy_out_to_entry_iov` 写回，`cap>0 && n>=cap` → MSG_TRUNC 透传 msg_flags） |

**unix.rs 机制扩展**（services 内纯策略）：

- `uds_socketpair(sock_type)`：双端 `uds_create` + UDS_STATE 互设 peer + 双方 Connected（参照 uds_connect Dgram 双向 peer 模式）；第二端创建失败回滚 close 第一端，槽位关联异常回滚两端。
- `uds_send_connected(fd, data)`：Stream 委托 `uds_send`；Dgram 校验 Connected/peer/peer_closed（→ NotFound）+ len 超 `UNIX_DGRAM_MAX`（→ Invalid）+ 对端在途数据报（→ Again），写入对端 dgram 缓冲 + passcred 12B 凭据（同 uds_sendto）。SIMPLIFIED：Dgram 已连接发送为单条在途非排队（Linux 为可靠排队缓冲），dgram 缓冲队列化时改写。
- `uds_sock_type(fd)`：套接字类型查询（recvmmsg 分流接收原语）。

**syscall.rs 常量区**：`MMSGHDR_SIZE=64` + msghdr 字段偏移（msg_name@0 / msg_namelen@8 / msg_iov@16 / msg_iovlen@24 / msg_flags@48 / msg_len@56）+ `UIO_MAXIOV=1024` + MSG_TRUNC=0x20 / MSG_DONTWAIT=0x40 / MSG_WAITFORONE=0x10000 + NSEC_PER_SEC。

**dispatch 接线**（`services/syscall/dispatch.rs` dispatch_net）：SYS_socketpair / SYS_sendmmsg / SYS_recvmmsg 3 分支（sendmmsg 4 参、recvmmsg 5 参含 timeout 指针）。

**kernel_test 扩展**（framework/tests/test_uds.rs，批实施 +3 / 审查处置 +2）：socketpair_stream（双向收发）/ socketpair_dgram（Connected 发送 + 二次发送 Again 断言）/ socketpair_rollback（资源耗尽回滚验证）/ close_releases_bitmap（close 位图回收回归）/ sendto_oversize（超限拒绝回归）——后两项见批审查处置；`release_all_uds_bitmap_bits()` 保留为失败用例兜底清理（位图泄漏修复后正常路径位图应自空）。

**批审查处置**（AskUserQuestion 用户裁决，3 项预存）：

- **uds_close 补 free_fd（本批修复）**：close 回收 fd_alloc 位图位（对齐 pidfd close 释放模式；含 listener 关闭时 pending client 位同步回收）；回归测试 close_releases_bitmap（17 次 create+close 循环，泄漏行为下第 17 次即 NoMem）。
- **uds_sendto 长度校验（本批修复）**：`data.len() > UNIX_DGRAM_MAX → Invalid`（与 uds_send_connected 同款防护）；回归测试 sendto_oversize（超限拒绝 + 正常路径不受影响）。
- **recvmsg/sendmsg_syscall UDS 数据面分流缺失（专项登记）**：登记下方预存表，后续工程处置。

**验证**：双架构 check 0w0e、clippy 3 维 0 warning、核心审计通过、build.sh all 5/5、host-tests 全量、QEMU boot（Ring 3/init）通过、QEMU kernel_test 475/475（批实施 +3 / 处置 +2）。

### T1 实施记录（G4 内存）

**实现路径裁定**（AskUserQuestion，用户授权）：mbind = **B 路径**（VMA 增策略字段 + 两级查询，不伪造 PMM per-node 分配）；userfaultfd = **B 真阻塞语义**（相对完整），且 #PF 阻塞落点采 **B′ 标记阻塞 + tick 抢占**（#PF 走 IST 栈不能就地切换上下文）。

**实装项**（2 项，services 0 unsafe）：

| 项 | services 落点 | 机制/要点 |
|---|---|---|
| mbind | `services/mm/numa.rs`（`mbind_syscall`） | 参数校验：`len == 0` / 非页对齐 → EINVAL；`flags` 保留位（`MPOL_MF_VALID` = 0xF）→ EINVAL；`MPOL_*` → `NumaPolicy` 映射（`from_linux_mode`，ABI 编码与枚举判别值不同）失败 → EINVAL；`addr + len` 溢出 → ENOMEM；`MPOL_DEFAULT` 要求 `nodemask == NULL`、Bind/Interleave 要求非空位掩码；委托 `MmStruct::set_numa_policy_range` 写 VMA 级策略 |
| userfaultfd | `services/mm/uffd.rs`（`userfaultfd_syscall` / `ioctl_uffd` / `read_event`） | `userfaultfd(flags)`：仅接受 `O_CLOEXEC`/`O_NONBLOCK`，创建实例（失败 EMFILE）；`UFFDIO_*`：结构体 read/write_struct_to_user + 参数校验（COPY 校验 `len == PAGE_SIZE` + `check_user_buf` + `copy_from_user` 到内核暂存；ZEROPAGE 校验页对齐）；`read()`：无事件则 `scheduler_block` + `scheduler_yield_ex` 等待，`fault_notify` 入队后由 framework 唤醒 |

**framework 机制扩展**：

- **`framework/mm/uffd.rs`（新建，ABI 常量 + 实例表 + #PF 拦截 + 页填充）**：Linux 真实 ABI 值（`UFFDIO_API` = 0xC018AA3F / `REGISTER` = 0xC020AA00 / `UNREGISTER` / `WAKE` / `COPY` / `ZEROPAGE`、`UFFD_API` = 0xAA、`UFFD_EVENT_PAGEFAULT` = 0x12、`struct uffd_msg` = 32B）；实例表 `UFFD_TABLE: IrqSpinLock<[UffdInstance; 16]>`（中断安全，`#PF` 路径读取）；`fault_notify(page, flags)` → `NotRegistered` / `Waiting`（入队事件 + 唤醒 `read()` 线程）/ `Ready`；`fill_provided_page` 在 `#PF` 重入路径把暂存数据写入 PMM 新页；`create`/`release`（唤醒等待者）/`api_negotiate`/`register`/`unregister`/`wake`/`provide_page`/`pop_event`/`set_reader`/`clear_reader`/`is_open`/`is_active`。
- **`framework/mm/page_fault.rs`**：`PfResult` 增 `UffdWait = 5`；`handle_vma_fault_with_mm` 在匿名页缺页前拦截（仅 `user && !present`）——`Waiting` → `PfResult::UffdWait`，`Ready` → 新增 `handle_uffd_provided`（alloc_page → 填充暂存 → 映射）。
- **`framework/idt/handlers.rs`**：`PageFaultHandler` 对 `UffdWait` 仅 `scheduler_block(WaitingForIo)` + 返回 `Recovered`（iretq 回用户态，由 tick 抢占切走；`#PF` 走 IST=4 专用栈，禁止就地切换上下文）。
- **`framework/mm/vma.rs`**：`range_is_mapped(start, end)`（注册区间全覆盖校验，允许 VMA 真子集/跨相邻 VMA，无空洞）；`Vma.numa` 字段 + `numa_policy_at` + `set_numa_policy_range`（G4①）。
- **`framework/mm/numa.rs`**：`MPOL_*` 常量 + `NumaPolicy::from_linux_mode` + `NumaRangePolicy` + `effective_policy_for`（G4①）。
- **`framework/proc/fd_alloc.rs`**：`FdSubsystem::UserFaultFd = 7`（`COUNT` 7 → 8）+ `FdPlan::USERFAULT_FD`（1200, 16）。
- **服务侧接线**：`services/fs/file_ops.rs`（ioctl 路由）、`services/fs/io.rs`（read 路由）、`services/fs/open.rs`（close 回收）、`services/syscall/dispatch.rs`（`SYS_mbind` + `SYS_userfaultfd`）。
- **kernel_test 扩展**（`framework/tests/test_mm.rs`，+3 并注册为 `mm::uffd` 组）：`lifecycle`（创建/握手/版本校验/ioctls 位图/释放）、`arg_validation`（对齐/模式/fd 校验分支）、`fault_flow`（注册 → 缺页入队 → 事件字段 → 提供页 → Ready → 物理页填充校验 → 注销回落 demand paging；临时安装测试地址空间）。

**SIMPLIFIED 清单**：

| 位置 | 简化点 / 影响面 / 扩展时机 |
|---|---|
| uffd 单挂起页模型 | 一个实例同时只跟踪一页；COPY/ZEROPAGE 必须命中该页否则 EINVAL；改 per-page 挂起表后可支持多页并发 |
| uffd 事件队列定长（16）| 队列满则丢弃拦截（`NotRegistered`，交回内核 demand paging），不永久挂起缺页进程；扩容时可改动态队列 |
| uffd 仅 MISSING 模式 | WP/MINOR 与 fork/remap/remove 事件未实装（`UFFDIO_API.features` 回填 0）|
| uffd `O_NONBLOCK` | 接受但无行为差异（`read()` 无事件即阻塞）；引入 fd 级状态标志后按 `O_NONBLOCK` 返 EAGAIN |
| mbind `MPOL_MF_MOVE` | 不迁移已触达页（无页迁移机制），不报错；PMM per-node 分区 + 页迁移后由分配路径消费策略 |
| mbind 范围粒度 | 要求范围被 VMA 整体对齐覆盖（无 split_vma → 部分生效会被整体拒绝，见 `set_numa_policy_range` 文档）|

**验证**：双架构 check 0w0e、build.sh all 5/5、clippy 3 维 0 warning、11 个核心/扩展审计脚本通过（`audit_comment_language` 修 1 处纯英文段落注释）、host-tests 全量、QEMU kernel_test 480/480（G4 新增 5 项：`mm::numa` 的 `vma_policy`/`linux_mode` + `mm::uffd` 的 `lifecycle`/`arg_validation`/`fault_flow`，475 → 480）。

**批审查处置**（静态契约测试同步，本批修复）：`FdSubsystem::COUNT` 新增 UserFaultFd 后，`td15_fd_idx_of_test::test_timerfd_subsystem_in_fd_plan`（断言 COUNT == 7）与 `fd_allocator_unified_test::test_subsystem_count_is_five`（仅统计 0..4 变体，PidFd 起已过时）同步为 COUNT == 8 / 8 个变体，并对 `test_mm.rs` 早前遗留的未命中 `#[expect(unreadable_literal)]` 清理。

### T1 实施记录（G5 多路复用）

**实装项**（3 项，services 0 unsafe）：

| 项 | 落点 | 机制/要点 |
|---|---|---|
| inotify_init | `services/syscall/dispatch.rs`（`SYS_inotify_init` 分支） | Linux 遗留接口 = `inotify_init1(0)`，直接委托既有 `sys_inotify_init1`（无新增实现，仅接线） |
| ppoll | `services/fs/file_ops.rs`（`ppoll_syscall`） | `struct timespec` 解析（`tv_sec`/`tv_nsec` 范围校验 → EINVAL，越界指针 → EFAULT）+ 临时信号屏蔽字 + 委托 `poll_syscall` |
| epoll_pwait | `services/sync/epoll.rs`（`epoll_pwait_syscall`） | 临时信号屏蔽字 + 委托 `epoll_wait_syscall`（校验与阻塞语义继承，timeout == -1 真阻塞） |

**framework/services 机制扩展**：

- **`framework/proc/signal.rs`**：新增 `SIGKILL`/`SIGSTOP` 编号常量 + `sanitize_blocked_mask(mask)`（剔除不可屏蔽信号位的**单点权威**实现）。`signal_pick_next` 判据为 `pending & !blocked`，屏蔽字含 SIGKILL/SIGSTOP 位会导致进程永久不可终止 — 任何写 `blocked_mask` 的路径必须过此函数。
- **`framework/syscall/dispatch.rs`**：`sys_rt_sigprocmask` 原内联位运算 `& !((1u64 << 9) | (1u64 << 19))` 改为调用 `sanitize_blocked_mask`（消除与 services 新调用点的并行实现，符合"内核内部并行实现必须收敛为单一权威"硬约束）。
- **`services/proc/signal.rs`**：新增 `with_temporary_sigmask(ptr, sigsetsize, f)`（ppoll / epoll_pwait 共用）——`sigmask == NULL` 透传；否则校验 `sigsetsize == 8`（EINVAL）、读用户掩码（EFAULT）、记录旧掩码 → `set_blocked_mask`（经 sanitize）→ 执行闭包 → 恢复旧掩码（错误路径同样恢复）。
- **`services/syscall/dispatch.rs`**：`SYS_ppoll`/`SYS_inotify_init` 接入 `dispatch_fs`，`SYS_epoll_pwait` 接入 `dispatch_sync`（`dispatch_sync` 形参 `_a5` → `a5` 以取 `sigsetsize`）。

**SIMPLIFIED 清单**：

| 位置 | 简化点 / 影响面 / 扩展时机 |
|---|---|
| `ppoll_syscall` 超时 | 复用 `poll_syscall` 的单次扫描语义（既有 `SYS_poll` 现状：事件就绪即返回，否则立即 0，timeout 不生效）；解析出的毫秒仅下传。影响：无事件时不阻塞（与 `epoll_wait` 的 timeout > 0 同缺口）。扩展：hrtimer 定时唤醒接入 poll 等待队列后由 `poll_syscall` 消费 |
| `with_temporary_sigmask` 原子性 | "设置 → 执行 → 恢复"顺序等效（非 Linux 的原子 `set_user_sigmask`）；等待期间屏蔽字保持替换值，故唤醒判定按新掩码。扩展：引入信号等待队列原子重挂时改为真原子替换 |

**验证**：双架构 check 0w0e（x86_64/aarch64，含 kernel_test 维度）、clippy 3 维 0 warning、11 个核心审计脚本 rc=0（含 `audit_reverse_deps` 生产反向依赖 0/0）、host-tests 全量、`./ci/build.sh all` 5/5、QEMU kernel_test 484/484（G5 新增 `fs::multiplex` 4 项：`inotify_init_legacy`/`temporary_sigmask_swap`/`ppoll_arg_validation`/`epoll_pwait_validation`，480 → 484）。

### T1 实施记录（G6 时间）

**实装项**（3 项，services 0 unsafe）：

| 项 | 落点 | 机制/要点 |
|---|---|---|
| settimeofday | `services/timer/clock.rs`（`settimeofday_syscall` + 策略核心 `apply_settimeofday`） | `tv == NULL` 按 Linux 语义返回 0（只设时区，本实装忽略时区）；`tv` 读取失败 / `tz` 非空不可读 → EFAULT；`tv_sec < 0` 或 `tv_usec` 越界 → EINVAL；euid != 0 → EPERM；委托 `framework::timer::TimeSyncSubsystem::set_time` 写墙钟基准 |
| adjtimex | `services/timer/clock.rs`（`adjtimex_syscall` + 策略核心 `apply_adjtimex`） | 读入 `struct timex`（x86_64 ABI 208B，host gcc 实测 `sizeof`/`offsetof` 定为权威布局）；支持 `ADJ_OFFSET`/`ADJ_FREQUENCY`/`ADJ_SETOFFSET`/`ADJ_NANO`，其余 mode 位 → EINVAL；非特权 → EPERM；恒回填状态快照（`status` 按 `synced` 置 `STA_UNSYNC`）并返回 `TIME_OK` |
| clock_nanosleep | `services/timer/clock.rs`（`clock_nanosleep_syscall` + 策略核心 `clock_nanosleep_wait_ns`） | `flags` 仅 `TIMER_ABSTIME`（否则 EINVAL）；`req` 为空 → EFAULT；`TIMER_ABSTIME` 下目标已过则等待 0；`rem` 按 Linux 语义不回写；睡眠委托 `framework::timer::sleep_ns` |

**framework/services 机制扩展**：

- **`framework/timer/sleep.rs`**：新增 `sleep_ns(total_ns)` — 睡眠策略的**单点权威**机制（`< 1ms` 走 hrtimer 时钟源忙等；`>= 1ms` 委托 `timer_sleep`，毫秒换算由原截断改为 `div_ceil` 向上取整以符合 POSIX「不低于请求时长」）。`framework/syscall/dispatch.rs::sys_nanosleep` 原内联的忙等/换算分支改为调用本函数（消除与 `SYS_clock_nanosleep` 的并行实现）。
- **`framework/timer/time_sync.rs`**：`set_time` / `adj_freq` 补 `last_sync_time` 基准重置 — 原实装不更新基准，`get_adjusted_time_ns` 的 `elapsed` 会以启动时刻起算，频率/跳变调整后墙钟按全部 uptime 累积补偿（秒级偏离）。本批使该路径经 `settimeofday`/`adjtimex` 可达，故属必须修复项（回归 `time::clock::freq_baseline` 以 500ppm 上界覆盖）。
- **`framework/timer/tick.rs`**：`on_timer_interrupt` 末尾接线 `timesync_subsystem().tick_adjust()` — 原 `tick_adjust` 全项目零调用 → `ADJ_OFFSET` 登记的渐进偏移（`offset_remaining`）永不被消耗，`adjtimex` 会是半成品。仅原子操作，无锁/无分配，可在 hardirq 上下文调用。
- **`framework/syscall/info.rs`**：删除 `sys_gettimeofday` — 墙钟查询改为 services 策略后失去唯一调用方（避免死代码）。
- **`services/timer/clock.rs`**：读路径统一为「原始 tick + timesync 机制偏移」（偏移初值 0，行为与旧实现一致）；`services/syscall/dispatch.rs` 的 `dispatch_proc` 接入 `SYS_settimeofday` / `SYS_adjtimex` / `SYS_clock_nanosleep` 三个分支。

**SIMPLIFIED 清单**：

| 位置 | 简化点 / 影响面 / 扩展时机 |
|---|---|
| `settimeofday_syscall` 的 `tz` | 仅做可读性校验后忽略（Linux 已废弃 timezone 语义，仅首次设置时消费）。影响：依赖时区偏移反推本地时间的旧程序拿不到时区信息。扩展：引入时区表后按 Linux 首次设置语义消费 |
| `adjtimex_syscall` 字段面 | 不支持 `ADJ_MAXERROR`/`ADJ_ESTERROR`/`ADJ_STATUS`/`ADJ_TIMECONST`/`ADJ_TICK`（→ EINVAL），误差估计类回填字段恒 0，`precision` 固定 1us。影响：ntpd/chronyd 类调优与误差统计不可用。扩展：引入时钟误差估计（PLL/FLL 二阶环路）后按 Linux 语义补齐 |
| `clock_nanosleep_syscall` 中断语义 | 不支持信号中断提前返回 `EINTR`（与 `SYS_nanosleep` 基线一致）。影响：等待期间收到信号不提前唤醒。扩展：睡眠机制接入可中断等待（信号投递唤醒阻塞队列）后统一补充 |
| 特权判定 | `has_time_privilege()` 用 `euid == 0` 等价 `CAP_SYS_TIME`（与 `sethostname` 的 PWM 能力判定路径不同）。影响：细粒度 capability 场景下语义偏粗。扩展：PWM capability 全覆盖后改判 `CAP_SYS_TIME` |

**验证**：双架构 check 0w0e（x86_64/aarch64，含 kernel_test 维度）、clippy 3 维 0 warning、13 个核心/扩展审计脚本 rc=0、host-tests 全量（99 个 test bin 全 ok）、`./ci/build.sh all` 5/5、QEMU kernel_test 490/490（G6 新增 `time::clock` 6 项：`gettimeofday_validation`/`settimeofday_apply`/`clock_nanosleep_policy`/`adjtimex_policy`/`freq_baseline`/`gradual_adjust`，484 → 490）。

> 测试策略：kernel_test/host 双端无用户可写内存（低地址为内核恒等映射，真解引用会踩内核数据），故成功路径测**策略核心函数**（`apply_settimeofday`/`apply_adjtimex`/`clock_nanosleep_wait_ns`）+ **机制往返**（`set_time`/`adj_freq`/`adj_time`/`tick_adjust`），用户指针路径以越界地址（`>= USER_ADDR_MAX`）触发确定性 EFAULT。

### T1 实施记录（G7 文件系统）

**实现路径裁定**（AskUserQuestion，用户授权）：

- **chroot / pivot_root = A 完整**——VFS 路径解析引入根前缀 + 单一权威路径归一化；`chroot` 校验目录/特权后设根；`pivot_root` 校验 `new_root`/`put_old` 关系后切根；**默认根 "/" 时行为与改造前逐字节等价**。
- **setdomainname = 统一路线**——`sethostname` / `gethostname` / `uname` / `setdomainname` 四处主机名/域名全部收敛到 framework `UtsNamespace`（单一权威），不再各自硬编码。

**实装项**（4 项，services 0 unsafe）：

| 项 | services 落点 | 机制/要点 |
|---|---|---|
| chroot | `services/fs/path.rs`（`chroot_syscall`） | 指针校验（EFAULT）→ `CAP_SYS_ADMIN`（EACCES，SYSTEM 域 bit0，与 `mount`/`umount2` 先例一致）→ `vfs_stat_safe` 校验存在且为目录（ENOENT/ENOTDIR）→ `resolve_user_path` 归一化（超长 ENAMETOOLONG）→ `VFS_MANAGER.set_root(real_root)`（切根 + cwd 重置为视图根 "/"） |
| pivot_root | 同上（`pivot_root_syscall` + `pivot_root_plan` 纯逻辑） | 两路径同前置校验；关系校验：`new_root` == 当前根 → EBUSY；`put_old` 非严格位于 `new_root` 之下 → EINVAL（`is_strictly_under` 路径边界感知，`"/"` 情形单独处理）；通过后 `set_root(new_root)`。`pivot_root_plan` 抽为纯函数便于直接验证 |
| setdomainname | `services/proc/sysinfo.rs`（`setdomainname_syscall` + 共用核心 `set_uts_name_syscall`） | 与 sethostname 同构：`len == 0 || len > 63` → EINVAL；SYSTEM 域 UTS 名称设置位 → EACCES；读入 64B 缓冲（EFAULT）→ `uts_current().set_domainname(...)` |
| execveat | `services/proc/exec.rs`（`execveat_syscall`） | `flags` 白名单（`AT_EMPTY_PATH` / `AT_SYMLINK_NOFOLLOW`，其余 EINVAL）；`dirfd != AT_FDCWD` → ENOTSUP；`AT_EMPTY_PATH` + 空 pathname → ENOTSUP（`fexecve` 语义未支持）；其余委托 `execve_syscall`（ABI 与执行语义单点复用） |

**framework/services 机制扩展**：

- **`framework/fs/vfs/vfs.rs`**：`VfsManager.root` 根前缀字段（`IrqSpinLock<[u8; VFS_MAX_PATH]>`，默认 "/"）+ `get_root`/`set_root`（切根并重置 cwd）+ **`normalize_view_path_into`（单一权威路径归一化）**——绝对路径以视图根为起点、相对路径以视图 cwd 为起点、`.` 忽略、`..` 上溯一级但**钳制在视图根内**（chroot 逃逸防护关键）、结果恒以 '/' 开头且无尾随 '/'；零堆分配（栈上 `[u8; VFS_MAX_PATH]`）；+ `resolve_view_path`（仅归一化, 供 `chdir` 保存视图路径 cwd）+ `resolve_user_path`（归一化 + 根前缀拼接, 默认根时逐字节等价旧行为）+ `truncate_to_parent`；`VfsSnapshot.root` 纳入快照。
- **`framework/fs/vfs/path.rs`**：16 个接受用户路径的 VFS 入口统一改经 `resolve_user_path`；`vfs_set_cwd_internal` 改用 `resolve_view_path`（cwd 语义为视图路径——相对解析须以视图为基准, 且 `..` 已在此钳制）。
- **`framework/fs/vfs/handle.rs`**：`vfs_open_internal` 入口归一化。
- **`services/fs/file_handle.rs`**：`name_to_handle_at` 先 `resolve_user_path` 归一化再 `resolve_mount_fs`（host 源检查回归 `name_to_handle_at_uses_path_resolution` 同步断言两步入径）。
- **`framework/proc/namespace.rs`**：`UtsNamespace.set_domainname`/`get_domainname`（65B 缓冲，未设置为空串）+ `UTS_DEFAULT_NODENAME` 常量（与 `UtsNamespace::new` 初值同源）+ `uts_current()`（**单一权威读取入口**：进程表 → `NamespaceSet` → uts；仅暂持 namespaces 锁取 Arc，UTS 字段锁在调用方按需获取以避免锁嵌套）。
- **`framework/syscall/info.rs`**：`sys_uname` 的 nodename/domainname 改取 `uts_current()`（无进程上下文回退 `default_nodename()`），删除原硬编码 `"queenx-node"` / `"(none)"`。
- **`services/proc/sysinfo.rs`**：`gethostname_syscall` 由恒返回字面量 `"localhost"` 改为读 UTS nodename（≤64B + NUL，按 `size` 截断）；`sethostname_syscall` 由"仅校验不存储"改为写入 UTS nodename。
- **`framework/credo/capability.rs`**：新增命名常量 `SYSTEM_CAP_UTS_SETNAME = 1 << 9`（提取自原 sethostname 字面量 `9`）+ `credo::mod` 顶层 re-export；`sysinfo.rs` 特权判定由 `(pwm, 0, 9)` 改为 `(pwm, CAP_DOMAIN_SYSTEM, SYSTEM_CAP_UTS_SETNAME)`。
- **`services/syscall/dispatch.rs`**：`SYS_chroot` / `SYS_pivot_root`（dispatch_fs）、`SYS_setdomainname` / `SYS_execveat`（dispatch_proc）接线。

**SIMPLIFIED 清单**：

| 位置 | 简化点 / 影响面 / 扩展时机 |
|---|---|
| `VfsManager.root` 为全局单例状态（非 per-process root） | 影响面：`chroot` 影响全系统视图（其他进程亦受新根约束）。扩展：引入 per-process root 或 mount namespace 时改进程级根字段 |
| `pivot_root_syscall` 仅切根前缀 | 影响面：不摘除旧根挂载点（无 per-process mount namespace 可摘），`put_old` 仅参与关系校验、旧根经绝对路径仍可达。扩展：需要 Linux"旧根不可达"强语义时在 `VfsManager` 摘除旧根挂载点 |
| `vfs_set_cwd_internal` 归一化失败静默保持原 cwd | 影响面：FFI 无返回值可上报，`chdir` 失败（超长/非 UTF-8）对用户不可见。扩展：需要 ENAMETOOLONG 上报时改签名为 `i32` |
| `execveat_syscall` 不支持目录 fd 相对解析与空路径执行 | 影响面：依赖 `AT_EMPTY_PATH` 的程序（部分动态加载器 / 容器运行时）不可用。扩展：VFS 提供"目录 fd + 相对路径"解析机制后按 Linux 语义补齐 |

**kernel_test 扩展**（+8，490 → 498）：`framework/tests/test_vfs.rs` 4 项（`test_resolve_default_root` / `test_resolve_dot_components` / `test_resolve_relative_to_cwd` / `test_resolve_with_root_prefix`，经 `resolve_is` 辅助断言归一化结果）+ `framework/tests/sys.rs` 4 项（`execveat_validation` / `setdomainname_validation` / `chroot_validation` / `pivot_root_validation`，注册于 `register_fs_tests`）。

**host 链接修复（本批引入，非预存）**：

- **现象**：`host-tests` 的 E-04 共享测试集（`e04_shared_runner_test`，debug 与 release 皆然）链接失败 —— `rust-lld: error: undefined symbol: _kernel_text_start / _kernel_text_end / USER_CR3_SAVE`。
- **根因定位**：HEAD（88382323）release 链接正常 → 本批引入。逐文件回退二分：`framework/tests/` 三个文件单独回退均正常，回拷 `sys.rs` 即失败。`ar x` + `nm -C` 分析 release rlib 各 CGU：`cgu.05` 定义 `setrlimit_syscall` 且带 `U create_user_page_table` / `U USER_CR3_SAVE` / `U _kernel_text_*`（与 `user_proc::raw::*`、`mm::kpti`、`mm::vmm_x86_64` **CGU 共置**），`cgu.09` 定义 `execveat_syscall` 且引用 `kpti::KPTI_READY`/`kpti_init`。即：新增测试引用 `services::proc::sysinfo` / `services::proc::exec` 处理器 → rustc 把 arch 目标文件合并进同一 CGU → host 测试二进制需解析仅由链接脚本（`x86_64.ld`）与汇编（`isr.asm`）提供的符号。
- **修复方案（用户裁定「还有更优方案吗」后选定）**：在 host-only 壳 crate `src/rust/src/lib.rs` 以 `#[cfg(feature = "host-test")] #[unsafe(no_mangle)]` 提供 4 个零值占位符号（`_kernel_text_start` / `_kpti_trampoline_end` / `_kernel_text_end`: `u8 = 0`；`USER_CR3_SAVE`: `AtomicU64::new(0)`）。**TCB（framework）零改动**、无需新增构建配置、对全部 host 测试二进制一次性生效，与"壳仅供 host 链接（裸机直接走 kernel crate + 链接脚本）"的既有设计意图一致。
- **候选对比**：候选 A（framework 内 `#[cfg(feature = "host-test")]` 占位）需在 TCB 内混入 host 分支；候选 B（`host-tests` 用 `--defsym`）需逐测试目标配置链接参数、且 `--defsym` 无法表达 `AtomicU64` 类型的原子变量；候选 C（相关测试降级 QEMU-only）以损失 host 回归覆盖为代价。三者均劣于所选方案。**2026-09-17 审查修正**：对候选 A 的排除理由**不成立**——`framework/arch/x86_64/mod.rs:49-67`（E-04 `cpu_id`）早有同类 host 桩分支，且这正是"同源双编译"的实现方式；本批选占位壳的理由是"TCB 零改动 + 对全部 host 二进制一次生效"的**成本考量**，而非架构约束。
- **残留风险（非结构根治）**：本方案在**符号级**确定性收敛（不再依赖 CGU 布局），但**类级根因未消除**——host 仍编译引用汇编/链接脚本符号的裸机 arch 模块、两侧无同步守卫、且"链接期硬失败"退化为"运行期静默错值"。三项残留 + 候选处置（审计守卫 / 结构根治 / 维持现状）见下方「预存登记（host 链接占位符号残留风险）」，**2026-09-17 审查裁定：结构根治（符号使用点级 host 桩化），壳占位随之删除**。
- **附带影响**：`host-tests/tests/plan_b_inode_test.rs::name_to_handle_at_uses_path_resolution` 的源文本断言随 `file_handle.rs` 入径变化同步更新（`VFS_MANAGER.resolve_mount_fs(path)` → `resolve_mount_fs(` + 新增 `resolve_user_path(` 断言，契约强度不降）。

**验证**：双架构 check 0w0e（x86_64/aarch64，含 `kernel_test` 维度）、clippy 3 维（pedantic lib / kernel_test / host-test）0 warning、`./ci/audit.sh quick` 全绿（含注释中文化 100%、services 0 unsafe、边界黑名单、双子树 deadlock 矩阵等；`audit_implicit_deps` 151 → 159，详见下方预存登记）、`./ci/build.sh all` 5/5、host-tests 全量（99 个 test bin 全 `ok` / 0 failed，含 E-04 共享测试集 371 项）、QEMU kernel_test 498/498（0 skipped）。

### T4 实施记录（2026-09-18）

**核实结论**（工具 `scripts/audit_unwired_pub_fn.py` R3，7 项逐一核实）：

| 模块 | trait | 包装器 | 生产引用 | 内联测试 | host-tests 平行实现 |
|---|---|---|---|---|---|
| `dmu_trait` | `DmuManager` | `StandardDmu` | 0 | 11 个，门禁中从不编译 | `HostDmuManager` / `StandardHostDmu` |
| `raidz_trait` | `RaidzEngine` | `StandardRaidz` | 0 | 11 个 | `HostRaidzEngine` / `StandardHostRaidz` |
| `spa_trait` | `SpaManager` | `StandardSpa` | 0 | 11 个 | `HostSpaManager` / `StandardHostSpa` |
| `txg_trait` | `TxgManager` | `StandardTxg` | 0 | 9 个 | `HostTxgManager` / `StandardHostTxg` |
| `zap_trait` | `ZapStore` | `StandardZap` | 0 | 11 个 | `HostZapStore` / `StandardHostZap` |
| `zil_trait` | `ZilLog` | `StandardZil` | 0 | 13 个 | `HostZilLog` / `StandardHostZil` |
| `zil_persist_trait` | `ZilPersist` | `StandardZilPersist` | 0 | 11 个 | `HostZilPersist` / `StandardHostZilPersist` |

7 项**全部为纯预留抽象**（零生产引用）。判据：

1. trait 自述目的「便于单元测试注入 mock」未落地——全仓无任何生产/测试调用方依赖该抽象。
2. 具体实现的测试由 `framework/tests/test_nestfs.rs` 直接针对 `NestZap` / `NestZil` / `NestSpa` / `NestObjSet` / `NestTxgGroup` 等**具体类型**完成，未走 trait。
3. 7 文件内的 `#[cfg(test)] mod tests` 在 `make test-unit`（构造为 `--features kernel_test`，非 `cargo test`）与 `make test-host`（依赖编译不含 `cfg(test)`）下**均不编译**——从不执行。本仓 `cargo test` 入口仅为 host-tests（Makefile L419 / ci/build.sh L58）。
4. host-tests 已走「直接引用内核真实源码」的源共享路线（DECISION-052 路线 C），取代 trait 注入式 mock。
5. 项目既有教训明载：「Trait injection for empty forwarding logic creates unnecessary abstraction overhead」。

**ZFS 归属性判定**：这层 trait 壳**不承载任何 ZFS 语义**——纯方法签名镜像 + 1:1 转发（如 `StandardZap::insert` 仅 `self.0.insert(...)`；`StandardSpa::name()` 是返回 `""` 的 stub）。类 ZFS 关键在具体实现子系统（`spa.rs` / `dmu.rs` / `zap.rs` / `txg.rs` / `zil.rs` / `zil_persist.rs` / `raidz.rs` / `arc.rs` / `vdev.rs` / `metaslab.rs` / `bp.rs` / `dedup.rs` / `snapshot.rs` / `dataset.rs` 等，约 7100 行），与本次删除零交集。

**处置裁定**（AskUserQuestion，用户授权）：**删除 7 模块**。

**改动清单**：

| 文件 | 改动 |
|---|---|
| `services/fs/nestfs/{dmu,raidz,spa,txg,zap,zil,zil_persist}_trait.rs` | 删除 7 文件（含从不运行的内联测试） |
| `services/fs/nestfs/mod.rs` | 删除 7 行 `pub mod` 声明 |
| `scripts/audit_invariants.py` | I2 检测 docstring 中指向已删文件 `raidz_trait.rs:304` 的历史注释改写为通用表述（本批改动直接导致的 stale 引用，§9.3 本轮修复） |

**验证**：

- `audit_unwired_pub_fn.py` R3：**7 → 0**；
- `./ci/build.sh all` 5/5（双架构 0w0e + host-tests + link）；
- `./ci/audit.sh quick` rc=0（双架构 check、clippy 3 维 0 warning、services 0 unsafe、注释中文化 100% 全绿）；
- `make test-host` 全量 0 failed；
- 未跑 QEMU（未触及 boot/架构路径）。

**附带观测（归 T5）**：R1 由 438 → 447（+9）——删除包装器后，原先仅被 `Standard*` 一次性转发调用而「看似被引用」的具体方法失去该跨文件引用，真实零引用状态随之暴露。属审计信号变得更诚实（非本批引入的缺陷），纳入 T5 甄别一并处置。

### T5 实施记录（批 1，2026-09-18）

**路径裁定（用户）**：R1 甄别采用**方案 A「分批只删确证无用」**——仅对「**重复能力 / 有等价公共入口 / 确证废弃**」三类施工删除；「**能力预留 / 转正式**」类**不改代码**，仅登记入台账（沿用 DECISION-052 第三层 + 分册 9 D-4 判据）。T5 声明**必须分批**。

**调研发现（关键，改变 T5 预期）**：R1 447 项经逐文件抽查后确认**主体为有意保留的 API 面，而非缺陷**：

| 类别 | 实例 | 为何零调用但保留 |
|---|---|---|
| services 的 framework 安全代理壳 | `driver/acpi.rs`（`get_lapic_base`/`get_ioapic_*`/`hpet_info`）、`credo/secure_boot.rs`（8 项）、`credo/crypto.rs`、`driver/char/serial.rs`、`driver/storage/ahci.rs`、`fs/nestfs/dedup.rs` | services 经顶层 re-export 暴露 safe API（F2 边界要求）；「零调用」≠「无用」 |
| 文件系统 mount/umount API 面 | `fs/{sysfs,cgroupfs,configfs,virtiofs,systree,devpts}.rs` 的 `mount_*`/`umount_*` | 完整实现，待 VFS mount 集成接线 |
| 调试/统计查询面 | `idt/statistics.rs`、`idt/handlers.rs`、`mm/slab.rs`、`mm/page_fault.rs`、`sync/atomic.rs` 计数、`net/route.rs`、`net/netfilter.rs` | 诊断/可观测能力预留 |
| 同步/跨架构原语预留 | `sync/{rwlock,seqlock,pi_mutex,atomic}`、`arch/aarch64/{mmu,gic,psci,vmm_aarch64}`、`arch/x86_64/{apic,ioapic}` | D-4 已登记「能力预留」 |
| 驱动硬件操作 | `driver/{display,xhci,e1000,dma,uefi,power}` | D-4 已登记「部分应接线，部分转正式」 |

→ 结论：**T5 的可删面远小于 R1 计数**；粗暴删除会移除有意保留的 API 面（D-4 明载「转正式比接线更合理，避免为消除而接线」）。

**批 1 删除清单（9 项 R1 项，均为「重复能力 / 等价公共入口 / 废弃兼容壳」）**：

| # | 文件 | 项 | 判据 |
|---|---|---|---|
| 1 | `framework/error.rs` | `io_error()` / `out_of_memory()` / `read_only()` | 「向后兼容别名（fs 层旧变体名 → 统一变体名）」块内三项，全仓 0 调用 → 废弃兼容壳 |
| 1b | 同上 | `not_found()` | 同块第四项，`.not_found()` 全仓 0 调用（审计 `rg -w` 把测试局部变量 `not_found` 计入引用，故未进 R1 计数）→ 连同 #1 整块 impl 删除 |
| 2 | `framework/config/boot_image.rs` | `read_boot_image()` | doc 自述「供测试/调试」，全仓（含 host-tests）0 引用；同文件 `encoded_len()` 有调用方 |
| 3 | `framework/driver/bus/pci.rs` | `pci_device_count()` | 纯转发 `crate::framework::pci::device_count()`，后者已被 `pci_get_device_count` FFI 使用 → 等价公共入口 |
| 4 | `framework/mm/vma.rs` | `mm_struct_new()` | 纯别名 `MmStruct::new()`（测试已在用），且未进 `mm/mod.rs` re-export |
| 5 | `framework/mm/swap.rs` | `pte_to_swap_entry()` | 薄包装 `SwapEntry::from_pte()`，后者已有调用方；未进 `mm/mod.rs` re-export |
| 6 | `services/fs/inode.rs` | `new_legacy_inode()` | 与 `LegacyInode::from_fs_result()`（`fs/file_handle.rs:203` 在用）重复的构造入口 |
| 7 | `services/proc/table.rs` | `allocate_reserved_pid()` | 函数体即 `allocate_pid()`（doc 亦述「普通进程用 allocate_pid」）→ 重复入口且名实不符 |

**批 1 保留（登记「待裁」，非删除）**：

| 项 | 保留理由 | 建议 |
|---|---|---|
| `framework/driver/bus/pci.rs::pci_scan()` | 能力与 `pci::scan_all_buses()`（已被 `services/driver/storage/mod.rs:182` 等使用）重叠，但含设备日志输出，属驱动**诊断面** | 待裁（接线 vs 删） |
| `fs/{sysfs,cgroupfs,configfs,virtiofs}::umount_*` 等占位实现 | `umount_sysfs` 恒 `Ok(())`、`umount_devpts` 误调 `mount_devpts` —— 属 FS API 面**半成品**，删除会移除 API 面 | 登记预存缺陷，待 VFS mount 集成时修实装 |
| `sync/atomic.rs` `record_*` 四项 | 计数本应被原子操作调用（`dump_stats` 已在用），当前计数恒 0 —— 属**半接线缺陷** | 待裁（接线会增热路径开销 vs 删计数面） |

**R1 台账（批 1 后 438 项，按子系统分类）**：

| 项数 | 子系统 | 类别 |
|---|---|---|
| 60 | `framework/` 其他（idt/timer/net/cpu/dma/pci/io/debug/console/chitin/config/ipc/credo/error/vmspace/page_table/irqline） | 预留（查询/诊断/原语） |
| 58 | `framework/arch/*`（apic/ioapic/gic/mmu/psci/gdt/uart/acpi/timer/shadow_stack） | 预留（跨架构原语，D-4） |
| 43 | `framework/proc/*`（进程/调度/命名空间/信号/fd_table/rlimit/cgroup/session/cfs） | 预留（策略查询面） |
| 43 | `services/driver/*`（usb/acpi/char/storage/display/virtio/firmware/uefi） | 预留（safe 代理壳 + 硬件面） |
| 42 | `framework/driver/*`（display/usb/net/power/uefi/hotplug/pci/input） | 预留（D-4） |
| 41 | `services/fs/*`（systree/devpts/exfat/ext2/tmpfs/virtiofs/sysfs/cgroupfs/configfs/ramfs/inode） | 预留（mount API 面）+ 少量待裁 |
| 34 | `services/fs/nestfs/*`（dedup/bp/arc/raidz/txg/spa/zil/dmu…） | 预留（NestFS 子系统） |
| 30 | `services/` 其他（ipc/proc/mm/net/wasm/barrier/timer/config） | 预留 + 少量待裁 |
| 22 | `framework/mm/*`（vmm_aarch64/kpti/numa/swap/slab/vma/page_fault/frame） | 预留（D-4） |
| 21 | `services/credo/*`（identity/secure_boot/crypto） | 预留（D-4） |
| 17 | `framework/sync/*`（rwlock/seqlock/pi_mutex/atomic/mutex/rcu/spinlock） | 预留（D-4） |
| 14 | `framework/barrier/*`（domain/recovery/reset/recoverable/snapshot） | 预留（BCB 策略面） |
| 13 | `framework/fs/vfs/*`（flock/handle/inotify/dcache/vfs） | 预留（VFS 内部） |

**验证**：`audit_unwired_pub_fn.py` R1 **447 → 438**（−9，与删除项精确对应；R3 仍 0）；`./ci/build.sh all` 5/5（双架构 0w0e + host-tests + link）；`./ci/audit.sh quick` rc=0；`make test-host` 全量 0 failed；未跑 QEMU（未触及 boot/架构路径）。

**后续批次判据（沿用批 1）**：先查「是否存在等价公共入口 / 是否有使用者」，再看是否属 D-4 预留类别；仅前者成立才删，后者一律登记入台账。

### T5 全量甄别台账（批 2，2026-09-18）

**性质**：批 2 为**只读扫描 + 登记**，未改任何源码（用户裁定「主项扫，登记后我和审核进行讨论」）。扫描后逐项四分类结果如下，供用户与 reviewer 讨论后决定「删 / 接线 / 预留 / 待裁」的最终处置。

**扫描区间**（批 1 后 R1 余 438 项，按区间切三块）：

| 区间 | 覆盖 | 项数 |
|---|---|---|
| g1 | `framework/{arch,mm,sync,barrier}` | 111 |
| g2 | `framework/` 其余（driver/fs/proc/idt/timer/net/cpu/dma/pci/io/debug/console/chitin/credo/ipc/…） | 158 |
| g3 | `services/` 全量 | 169 |

**方法与限制（复核前置，reviewer 反馈后补）**：

**① 判定工具**：`scripts/audit_unwired_pub_fn.py` 的 **R1 规则**（`pub fn` 跨文件零引用 WARN）。引用计数实现为 `rg -c -w <name> src/ host-tests/`（[脚本 L247-255](file:///home/anfer/Code/QueenX/scripts/audit_unwired_pub_fn.py#L247-L255)），**纯文本词边界计数**；脚本自述「基于 .rs 文件源码 AST 分析, **不依赖 cargo build**」（[L69](file:///home/anfer/Code/QueenX/scripts/audit_unwired_pub_fn.py#L69)）。**声明侧无 cfg 感知**——`EXEMPT_CFG_ATTR` 只豁免「引用侧被 cfg 门控的引用」，不识别「声明本身被 cfg 门控」。

**② 判定构建维**：**不存在单一构建维**。名义上是「`src/` + `host-tests/` 文本并集」，既非某一构建维，也未做「逐维取交集」。⇒ 台账中的「零引用」**不等于**「任何构建维下都无调用者」。

**③ 已知系统性误判源**（计数由此失真，非个案）：

| 误判源 | 实例 | 后果 |
|---|---|---|
| 声明侧 `#[cfg(target_arch = "…")]` | [mm/mod.rs:51-53](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/mod.rs#L51-L53) 以 `#[path = "vmm_aarch64.rs"]` 门控 `pub mod vmm`；`arch/aarch64/**` 同理 | 非本维编译的模块被判「无调用者」= **构造性结果**，非死代码证据 |
| 声明侧 `#[cfg(feature = "…")]` | [sync/atomic.rs:182](file:///home/anfer/Code/QueenX/src/kernel/framework/sync/atomic.rs#L182) `#[cfg(feature = "atomic_stats")] mod stats` | feature 门控代码被判「死」，实为「默认维未启用」 |
| `#[cfg(feature = "kernel_test")]` 测试模块 | `barrier/snapshot.rs` / `barrier/reset/layered.rs` 的 `pub mod tests` | 测试用例被判「未接线」 |

③ 表补充：**单维甄别的结果仅在该维有效**，跨维完备性需逐维复核（遗留 1 裁定：属**漏项风险**，非误删风险——甄别所在维中 x86_64 专属项*有*引用，不会被误列删候选；误删方向已由「aarch64 门控」属性隔离）。

**④ 复核判据（正确口径）**：**逐构建维取交集**——x86_64 裸机 / aarch64 裸机 / kernel_test / host-test **四维皆零引用**，才可判「真无引用」；且**无调用者 ≠ 死代码**（F9 要求「硬件规范常量须通过实现使用路径消除」，但其**反向不成立**：汇编/外部调用、硬件规范要求、跨架构对称性都会使「无 Rust 调用者」合法）。**删候选判据进一步升格为三合一**（reviewer 第二轮）：零引用 ＋ 等价公共入口 ＋ 非 API/FFI/feature/硬件原语面，见「分类语义」。

**⑤ 本台账定位**：以下全部计数为**暂定值**，**不得作为施工依据**（§12.4）；处置路径见 ⑥（试删流程），不另造静态分析器。

**⑥ 施工落地方式（遗留 2 裁定：不立项新工具）**：删候选**不用静态分析器**，直接**逐项试删 → 跑既有五条门槛 → 任一维硬失败即回退并归入「硬件原语保留 / 待裁」**。四维判据由既有门槛原地覆盖，零新增基础设施：

| 维 | 既有门槛 |
|---|---|
| x86_64 裸机 | `./ci/build.sh x86_64`（`./ci/build.sh all` 第一阶段） |
| aarch64 裸机 | `./ci/build.sh all` 第二阶段 |
| `kernel_test` | clippy `kernel_test` 维 + QEMU `kernel_test` |
| `host-test` | clippy `host-test` 维 + `make test-host` + QEMU boot |

> 理由：**编译/链接器即权威判据**（工具是近似、编译器是真相）；且与上一轮「host 链接守卫交还链接器、否决审计守卫脚本」的裁定同构——近似的静态分析恰是「黑名单不全 → 虚假信心」那类危险机制（§12.3 简约准则）。低风险项可批量，`kernel_test` 与 QEMU boot 为硬闸门。若日后仍要立项该工具，应**单开任务且低优先级**，并明确**不是 T5 的前置**。

**⑦ 试删的原理性盲区（reviewer 第二轮，必须记住）**：`试删 + 五条门槛` **在原理上发现不了「安全校验被移除」**——删掉一个零调用的校验函数后，编译/链接全过；测试若未覆盖该路径，也全绿，但校验已经没了。⇒ 安全敏感项（**路径校验 / 常数时间比较 / CET-SSP / CR4 权限谓词**）**不能靠试删兜底**，必须先经安全面确认（见 A-3 与「分类语义」安全面行）。

**⑧ 顺序约束与解锁条件（reviewer 第二 / 三轮裁定）**：

- **不得并行 T1 收尾与 T5 施工**：两批都动 `framework`，QEMU 一旦挂掉无法归因。**文件级冲突已确认**：`pcid_is_enabled` 位于 `mm/kpti.rs`，正是 T1 P2′ 声明收拢所改文件。T5 的**文档修订**不占代码，可与 T1 收尾并行。
- **A-2 开工解锁条件（五条，缺一不可；reviewer 第三轮已逐条核销）**：

| 条件 | 状态 | 依据 |
|---|---|---|
| ① 7 项「零消费」判据补齐证据或退桶 | ✅ 完成 | 7 项全退（3 项同文件整组统一、1 项已登记路线图项、3 项统计/诊断 API 面） ⇒ A-4 退桶 8 |
| ② 安全敏感 4 项完成安全面二次分桶 | ✅ 本次裁定完成 | `ct_eq_salt`/`ct_eq_password` ⇒ 族残缺入 A-1；`split_path`/`validate_path` ⇒ B-4 待 T3 结论（A-3） |
| ③ `gdt.rs` 注释原文核实 | ✅ 已完成 | `get_gdt_table` 注释原文＝`/// 获取 GDT 表的引用 (调试用途)` ⇒ 自述调试用途，非死代码，退桶（A-4） |
| ④ 两组同文件分裂按路线图统一 | ✅ 已完成 | `shadow_stack.rs`（`is_ssp_valid` 随 4 项 CET 项）/ `numa.rs`（`contains_cpu` `all_nodes` 随 3 项 NUMA 项）整组归入待裁（B-3 注） |
| ⑤ T1 收尾完成（串行闸门） | ✅ 已解除 | T1 收尾三门槛复跑全 RC=0（QEMU boot 1/1 命中 `VFS ready`） |

- **A-2 开工执行约束（reviewer 第三轮，区别于原方案）**：① **逐项试删**，每项**独立提交、可单独回退**（便于归因）；② 每项试删后跑**五条门槛全量**（非只编译），其中 **QEMU boot 为硬闸门**；③ **B-4 的 ramfs 2 项不在本次试删范围内**（等 T3 结论）；④ **`pcid_is_enabled` 已移出删候选**（属已登记路线图地基，A-4），确认**不在 11 项内**——A-2 11 项清单为 `get_ap` / `tss_64bit` / `consume_quota_tick` / `is_quota_exceeded` / `write_log_line` / `vfs_close_safe` / `vfs_seek_safe` / `vfs_readdir_safe` / `get_fs_name` / `pipe_exists` / `format_duration`。

**分类语义（修订版，登记口径）**：

| 分类 | 含义（可复核标准） | 处置路径 |
|---|---|---|
| 删候选 | **收窄为三合一判据（reviewer 第二轮升格）**：① **该构建维零引用** ＋ ② **存在能力等价的公共入口**（或重复实现）＋ ③ **非 API/FFI/feature/硬件原语面**。三者**同时成立**方可判删——「有等价入口」单独不成立（该函数可能正属 API 面 / FFI 面 / feature 面，判删后即删掉对外能力） | 须先过「硬件原语完整性」二次分类 + **安全面确认** + ⑥ 试删 |
| 安全面待确认 | 安全敏感面：路径校验 / 常数时间比较 / CET-SSP / CR4 权限谓词 等。**试删在原理上发现不了「安全校验被移除」**（删掉零调用校验函数后编译链接全过、测试若无该路径覆盖亦全绿） | **不走试删**；**产出规格＝三档（冗余 / 缺陷 / 族残缺，见 B-0）**，逐项定型后分别入 A-1 / B / C-1。本轮 4 项已分流完毕 |
| 硬件原语完整性保留 | 硬件原语/规范常量（内省、兼容包装、常量 getter 属其典型形态）；「无 Rust 调用者」可由汇编/规范/对称性解释 | 记预留，**不删** |
| 接线 | **仅限**「现有调用链缺此半」（小改即可挂上） | 可施工子清单 |
| 未来功能 | 所属**子系统未集成** + 原「预留」API 面（DECISION-052 第三层） | 记录，不接线、不删除 |
| 待裁 | 半接线缺陷 / 依赖路线图 / 受构建维或 feature 门控影响 | 需用户 + reviewer 裁定 |

**分组统计（原始扫描输出，保留可追溯；口径已作废）**：

| 区间 | 项数 | 删 | 接线 | 预留 | 待裁 |
|---|---|---|---|---|---|
| g1 `framework/{arch,mm,sync,barrier}` | 111 | 59 | 34 | 8 | 10 |
| g2 `framework/` 其余 | 158 | 7 | 102 | 47 | 2 |
| g3 `services/` | 169 | 4 | 6 | 150 | 9 |
| **合计** | **438** | **70** | **142** | **205** | **21** |

**修订后桶数（四次修订，按 ④ + 三合一判据 + 安全面三档分流 + 试删前逐项复核重划；不得作为施工依据）**：

| 桶 | 项数 | 构成（含来源标记） |
|---|---|---|
| 删候选 | **2** | 三次修订 11 − **试删前复核退桶 9（A-5）**。**逐项判据（三合一）成立者**（安全面分流不回填本桶） |
| 硬件原语完整性保留 | **43** | 41（x86_64 侧 31 + aarch64 侧 10，优先级裁定见 A-1）+ **安全面「族残缺」档 2**（`ct_eq_salt` `ct_eq_password`，reviewer 第三轮裁定一） |
| 接线 | 8 | `fs/vfs/handle.rs` 1 + `fs/vfs/vfs.rs` 1 + `irqline.rs` 1 + `frame.rs` 1 + `proc/fd_table.rs` 4（**逐项独立判定**） |
| 未来功能（DECISION-052 第三层） | 339 | **来源合成**：原「接线」剩余 134（**＝142−8，排除法所得，非实测**）+ 原「预留」205 |
| 待裁 | **46** | 三次修订 37 + **A-5 退桶 9**（族残缺 6 + 台账 ② 事实错误 / 无等价入口 3） |
| **合计** | **438** | 算术闭合已核对（2 + 43 + 8 + 339 + 46 = 438）；原「安全面待确认 4」桶第三轮清零（三档分流完毕） |

> 已识别 **aarch64 专属 31 项**（`arch/aarch64/**` 8+9+3+2+1 与 `mm/vmm_aarch64.rs` 6、`mm/kpti_aarch64.rs` 2），**x86_64 专属项面未量化**（`arch/x86_64/**`、`mm/kpti.rs`、`idt/**`、`cpu/**`、`arch/shadow_stack.rs` 等），两者均须按 ⑥ 试删复核。

#### A. 删候选（四次修订：70 → 11 → 2）+ 硬件原语完整性保留（三次修订：41 → 43）

**A-1 硬件原语完整性保留（43 项；记预留，不删）**

| 文件 | 项 | 理由 |
|---|---|---|
| `framework/arch/x86_64/apic.rs` | 15：`get_version` `get_timer_count` `is_timer_calibrated` `configure_lint0` `configure_lint1` `apic_read_isr` `apic_read_tmr` `apic_read_irr` `apic_is_in_isr` `apic_is_in_irr` `apic_is_level_triggered` `send_ipi_level` `broadcast_ipi_level` `icr_level` `icr_broadcast` | APIC 原语完整性；须在**裸机 + kernel_test 维**逐一验证是否被中断/启动路径调用——**误删直接破坏真机中断** |
| `framework/arch/x86_64/ioapic.rs` | 6：`get_max_irq` `set_irq_level` `set_id` `get_arbitration_id` `delivery_lowest` `delivery_init` | IOAPIC 原语完整性，同上 |
| `framework/sync/pi_mutex.rs` | 2：`get_ceiling` `get_protocol` | 同步原语族完整性（PI 协议查询属规范形态） |
| `framework/sync/rwlock.rs` | 5：`raw_read_unlock` `raw_write_unlock` `read_irqsave` `write_irqsave` `pending_writer_count` | 原语层入口完整性 |
| `framework/sync/seqlock.rs` | 2：`current_sequence` `get_valid` | 原语层入口完整性 |
| `framework/sync/spinlock.rs` | 1：`lock_irq` | 规范要求的原语形态 |
| `framework/arch/aarch64/gic.rs` | 3：`is_ppi` `is_valid_irq` `is_spi_pending` | 架构规范范围谓词 + GIC 寄存器状态查询（aarch64 原语面） |
| `framework/arch/aarch64/mmu.rs` | 1：`allows_el0_access` | 页表描述符权限谓词（硬件内省） |
| `framework/arch/aarch64/timer.rs` | 1：`read_control` | 定时器控制寄存器读取（硬件内省） |
| `framework/mm/vmm_aarch64.rs` | 5：`is_desc_table` `is_desc_block` `is_desc_page` `is_desc_device_memory` `is_desc_non_cacheable` | 页表描述符类型谓词（硬件内省） |
| `services/credo/crypto.rs` | 2：`ct_eq_salt`(210) `ct_eq_password`(216) | **族残缺档（reviewer 第三轮裁定一）**：与 `ct_eq_hash`(204) 形状完全相同，同为 `Salt` / `PasswordHash` / `Sha256Hash` 的**类型化比较同族**；`ct_eq_hash` 有调用者、未入删候选 ⇒ 删另两者则**族残缺且不对称**。真正作用不是「提供能力」（`ct_eq` 已提供），而是**阻止调用方拆字段**（写 `ct_eq(&a.0, &b.0)`）——删掉等于**诱导密码学代码绕过类型包装**、降低抽象层级。与 A-1 保留 `sync/*` 原语族入口**判据完全同构** |

> **安全面「族残缺」档并入（reviewer 第三轮裁定一：41 → 43）**：`ct_eq_salt` / `ct_eq_password` 由「安全面待确认」定型为**族残缺** ⇒ 按裁定只保留、不删、不停留于待裁。**附核实（裁定一要求）**：`grep -rn` 全仓 `src/` + `host-tests/` 实测两者**零引用**（仅自身定义行，无任何调用）；二者为 services 侧普通 `pub fn`，**无 `#[no_mangle]` / 无 `extern "C"` / 无 FFI 导出**，且零引用即排除跨 crate / 用户态 API 面按名调用 ⇒ **保留不引入新暴露面**。

> **桶间优先级裁定（三处核对 ①）**：aarch64 12 项中 **10 项**同时符合「硬件原语」与「aarch64 门控」两种属性 ⇒ 按 **硬件原语保留 > aarch64 门控 > 删候选** 归入本桶（本桶 31 → **41**）；余 **2 项**（`arch/aarch64/mmu.rs::diagnose_permission`、`mm/vmm_aarch64.rs::diagnose_descriptor`）为**诊断输出**非原语，留「待裁」并保留 aarch64 门控标记。

**A-2 删候选（2 项；三合一判据逐项成立，须过 ⑥ 试删验证后方可施工）**

| 文件 | 项（行号） | 判据（① 零引用 / ② 等价公共入口 / ③ 非 API-FFI-feature-原语面） |
|---|---|---|
| `framework/console/gfx_console.rs` | `write_log_line`(286) | ② 与在用 `write_str`(280) **逐字节相同**（均 `for ch in s.chars() { self.putchar(ch) }`）⇒ 纯重复实现；③ 非 API/FFI/feature/原语面 |
| `framework/timer/tick.rs` | `format_duration`(355) | ② `#[cfg(feature = "alloc")]` 纯格式化工具，`core::fmt` 可等价；③ 非对外 API 面 |

> **本桶三次修订的 11 项中，9 项于试删前逐项复核退回待裁**（实测判据不成立，见 **A-5**）——本桶 2 项为**唯一进入试删者**。

**A-3 安全面二次分桶（reviewer 第三轮裁定，4 项已全部分流完毕；本桶清零）**

| 文件 | 项（行号） | 定型档 | 去向 |
|---|---|---|---|
| `services/credo/crypto.rs` | `ct_eq_salt`(210) `ct_eq_password`(216) | **族残缺** | ⇒ **A-1 硬件原语完整性保留（43）**。与 `ct_eq_hash`(204) 同族，删则族残缺且不对称，且诱导调用方绕过类型包装（裁定一） |
| `services/fs/ramfs.rs` | `split_path`(524) `validate_path`(544) | **待定型（冗余 / 缺陷 二选一）** | ⇒ **B-4 待裁**。**不得进试删队列**，须先由 T3 安全面给出「VFS 是否已提供等价校验」的结论（裁定二） |

> **事实更正（裁定二，威胁模型修正）**：上轮 A-3 记述的「路径穿越防护」**不成立**——实读 [ramfs.rs:544-562](file:///home/anfer/Code/QueenX/src/kernel/services/fs/ramfs.rs#L544-L562) `validate_path` 只做**空 / 长度(`VFS_MAX_PATH`) / NUL** 三项检查，**不含 `..` 穿越检查**；穿越防护由 VFS 侧 `resolve_path`（[ramfs.rs:515](file:///home/anfer/Code/QueenX/src/kernel/services/fs/ramfs.rs#L515) `global().resolve_path(path)`）负责。故「删掉即移除穿越防护」的担心不成立。
>
> **零调用的两种含义（裁定二给出，必须二选一）**：① **VFS 层已做等价（空/长度/NUL）校验** ⇒ 这两项**冗余**，可删，但须**登记契约**「ramfs 依赖 VFS 前置校验」（否则将来 VFS 改动会无声破坏 ramfs 的假设）；② **VFS 未做等价校验** ⇒ 零调用说明 ramfs 路径入口**缺基础校验**，是**安全缺陷**而非死代码 ⇒ **不得删**，登记缺陷移 T3 并按 C-1 接线（ramfs 路径入口补调用）。此项待 T3 结论后定型。

**A-4 退桶（8 项；判据不成立 ⇒ 并入「待裁」桶）**

| 文件 | 项（行号） | 判据不成立之处 |
|---|---|---|
| `framework/arch/shadow_stack.rs` | `is_ssp_valid`(129) | **同文件整组统一**（`set_ssp` 等 4 项在待裁，取决于 CET 路线图）；且属安全敏感面（CET/SSP）。② 未取得等价入口证据 |
| `framework/mm/numa.rs` | `contains_cpu`(195) `all_nodes`(334) | **同文件整组统一**（`set_distance` 等 3 项在待裁，取决于 NUMA 路线图）。② 未取得等价入口证据 |
| `framework/mm/kpti.rs` | `pcid_is_enabled`(149) | **已登记的未完成路线图项**：文件头 doc（`kpti.rs:23-31`）明载「未完成 … **PCID/INVPCID 优化**：当前每次切换 CR3 都 TLB 全清，高频 syscall 性能损失 5-15%」⇒ 该函数是这条已登记项的地基，删它＝删掉登记过的路线图基础设施 |
| `framework/barrier/reset/audit.rs` | `count_by_result`(101) | ② 未取得等价入口（同文件 `count_by_layer` 是另一维度，非等价）；③ 属诊断查询 API 面 |
| `framework/mm/page_fault.rs` | `page_fault_count`(497) | ② 未取得等价**封装**入口（`PAGE_FAULT_COUNT` 为 `pub static`，是并行读法而非等价替代）；③ 属统计 API 面 |
| `framework/mm/slab.rs` | `utilization`(921) | ② 仅部分等价（`get_stats() -> CacheStats{total_objects, active_objects}` 可推导，但为使能等价能力需调用方自行除法）；③ 属统计 API 面 ⇒ 证据不足以支撑删除 |
| `framework/arch/x86_64/gdt.rs` | `get_gdt_table`(701) | **注释原文核对**：`/// 获取 GDT 表的引用 (调试用途)` ⇒ 自述调试用途，**非死代码**，退桶 |

> **退 A-1 还是 B**：上表 8 项**均非硬件原语 / 规范常量**（A-1 的准入条件），故按 reviewer「补不出即退 A-1 或 B」统一退 **B（待裁）**，不走 A-1。

**A-5 试删前逐项复核退桶（9 项；reviewer 第四轮裁定 + 本轮实测；退入「待裁」）**

> **触发**：A-2 11 项试删开工后，逐项复核「零调用 ＝ 冗余 / 缺陷 / 族残缺」定型（B-0 三档规格）时，实测发现 9 项判据不成立。**判定依据为全仓 `grep -rn` 实测引用计数**，非静态推断。

**A-5.1 族残缺档（6 项；同族兄弟在用 ⇒ 不删，保对称）**

| 文件 | 项（行号） | 同族兄弟实测引用 | 族残缺证据 |
|---|---|---|---|
| `framework/arch/x86_64/gdt.rs` | `tss_64bit`(173) | `code_64bit` **4** / `data_32bit` **4** / `tss_64bit` **0** | `Granularity` 构造器族 **2/3 在用**。字段为 `Granularity(pub(crate) u8)` ⇒ 删命名构造器后调用方只能写 `Granularity(Granularity::LONG_MODE)` **绕过构造器族**（与裁定一 `ct_eq(&a.0, &b.0)` **判据同构**） |
| `framework/fs/vfs/handle.rs` | `vfs_close_safe`(542) `vfs_seek_safe`(547) `vfs_readdir_safe`(556) | `vfs_*_safe` 全族 **17 员 / 14 员在用** | 在用者：`vfs_open_safe` `vfs_read_safe` `vfs_write_safe` `vfs_fstat_safe` `vfs_stat_safe` `vfs_mount_safe` + `path.rs` 8 员 + `vfs_utimensat_safe` ⇒ 14/17 在用，删 3 员则族残缺不对称 |
| `framework/arch/x86_64/acpi.rs` | `get_ap`(457) | `get_ap_list` **1** / `get_ap_count` **3** / `has_madt` **6** / `parse_madt` **5** | AP 信息访问器族在用 |
| `framework/fs/vfs/vfs.rs` | `get_fs_name`(75) | `get_fs_type` **2** / `get_fs` **13** / `set_fs` **1** | VFS 挂载点元信息访问器族在用 |

> **`vfs_*_safe` 三项的档位争议（如实登记）**：实测 [services/fs/dir_ops.rs:14-28](file:///home/anfer/Code/QueenX/src/kernel/services/fs/dir_ops.rs#L14-L28) 已**直接调用裸 `extern "C"`** `vfs_seek` / `vfs_readdir`（不经 `_safe` 壳）⇒ 该族**本就不是封装边界**，删 3 员**不改变抽象层级**（弱于 `tss_64bit` 的族残缺强度）。仍按「同族部分在用 ⇒ 保对称」退回待裁，档位由 reviewer 复核确定。

> **`get_ap` 的试删门槛已实测通过（如实登记）**：该项试删后 `./ci/build.sh all` / `./ci/audit.sh quick` / `make test-host` / `make test-unit` / `./scripts/qemu_boot_test.sh x86_64` **5/5 全过**（QEMU boot 硬闸门通过）⇒ **门槛在原理上无法识别族残缺**（与 ⑦ 试删盲区同源），故按裁定退回待裁，代码已 `git checkout` 回退。

**A-5.2 台账 ② 判据事实错误 / 无等价入口档（3 项）**

| 文件 | 项（行号） | 原台账记述 | 实测更正 |
|---|---|---|---|
| `framework/barrier/domain.rs` | `consume_quota_tick`(213) `is_quota_exceeded`(307) | 「② 与**在用** `check_quota` 语义重复」 | **`check_quota` 自身零引用**（仅 [domain.rs:278](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/domain.rs#L278) 定义行；全仓其余命中仅在 `docs/` 与 `build/*.map`）⇒「在用」**不成立**；且 `check_quota` 本身已列在同台账待裁表（B-2）。② 判据落空 ⇒ 退桶 |
| `framework/ipc/dynamic.rs` | `pipe_exists`(118) | 「② 可由**在用** `get_pipe` / `pipe_count` 等价判定」 | 全仓**无 `get_pipe` 符号**；`pipe_count()` 只返回数量、**不判定指定 `IpcId` 是否存在** ⇒ **无能力等价的公共入口**，② 判据落空 ⇒ 按三合一判据它本不该入删候选，退桶 |

> **负债如实登记**：上表 2 处错误系「二次修订」时未经全仓实测即写入 ② 判据所致——已于本轮就地订正，并据此退桶。

#### B. 待裁（四次修订：21 → 37 → 46）

**B-0 二次分桶产出规格（三档形态；reviewer 第三轮裁定三，本桶的定型口径）**

> 「安全面二次分桶」的产物**固定为三档**，不再以「三合一判据」代偿——三合一判据只判「**是否依赖等价入口**」，**不判「零调用是冗余还是缺陷」**，故不足以区分下列三档：

| 定型档 | 含义 | 处置 |
|---|---|---|
| **冗余** | 能力已被**上层 / 等价入口**覆盖（零调用＝上层已代劳） | **可删**，但须**登记契约**（写明依赖哪一层的前置能力），否则上层将来改动会无声破坏本层假设 |
| **缺陷** | 本该被调用却零调用（零调用＝**校验/能力缺失**） | **不能删**；登记为缺陷移 T3，并按 C-1 **接线**（补调用） |
| **族残缺** | 同族中**部分在用**（删之则族残缺且不对称） | **不删**，保对称 ⇒ 并入 A-1 完整性保留 |

> 三档与包装层的关系：**可删的前提是「零调用＝冗余」**，而非「零调用」本身；**零调用本身不构成删的判据**（第二轮已裁定）。本轮 4 项安全面按此规格分流结果见 A-3。

**B-1 新增（构建维 / feature 门控，reviewer 反馈）**

| 文件 | 项 | 待裁点 |
|---|---|---|
| `framework/arch/aarch64/mmu.rs` + `framework/mm/vmm_aarch64.rs` | 2：`diagnose_permission` / `diagnose_descriptor` | **aarch64 专属 + 诊断类**（[mm/mod.rs:51-53](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/mod.rs#L51-L53) 门控）⇒ x86_64 维下整模块不编译，「零引用」为**构造性结果**；另 10 项 aarch64 项因「硬件原语」优先级已归 A-1（见该桶优先级裁定） |
| `framework/sync/atomic.rs` | 4：`record_inc` `record_dec` `record_cmpxchg_success` `record_cmpxchg_fail` | 位于 `#[cfg(feature = "atomic_stats")]`（[atomic.rs:182](file:///home/anfer/Code/QueenX/src/kernel/framework/sync/atomic.rs#L182)），**feature 门控代码非死代码**；既有登记 [subsystem-sync.md §9.3 [P2]](archive/audit-2026-08-14/subsystem-sync.md#L1075-L1092)「`atomic_stats` 引用 `println!` — no_std 不支持」⇒ 处置＝**修 feature 或删 feature（用户决策）** |

**B-2 原有（21 项）**

| 文件 | 项（行号） | 待裁点 |
|---|---|---|
| `framework/arch/shadow_stack.rs` | `set_ssp`(124) `alloc_kernel_shadow_stack`(302) `configure_user_cet_msr`(394) `configure_interrupt_ssp_table`(445) | 去留取决于 CET 子系统路线图 |
| `framework/arch/x86_64/acpi.rs` | `get_dmar_drhd_list`(878) `get_dmar_host_addr_width`(883) | 取决于 IOMMU/DMAR 路线图 |
| `framework/barrier/recoverable.rs` | `lock_fast`(75) | 免 checkpoint 快速路径，属设计决策 |
| `framework/mm/numa.rs` | `set_distance`(291) `best_alloc_node`(339) `nearest_free_node`(346) | 取决于 NUMA 子系统路线图 |
| `framework/cpu/cpuid.rs` | `cpuid_checked`(60) | 叶范围校验变体，保留安全变体 vs 删 |
| `framework/driver/bus/pci.rs` | `pci_scan`(75) | 与在用 `scan_all_buses` 能力重叠且带日志（批 1 已登记） |
| `services/fs/sysfs.rs` / `cgroupfs.rs` / `configfs.rs` / `virtiofs.rs` / `systree.rs` | `umount_sysfs`(189) `umount_cgroupfs`(389) `umount_configfs`(361) `umount_virtiofs`(314) `umount_systree`(505) | 占位半成品（恒 `Ok(())` / 常量清零）；删则移除 FS API 面 |
| `services/fs/devpts.rs` | `umount_devpts`(241) | 误调 `mount_devpts` 的半成品 |
| `services/fs/process_fd_table.rs` | `get_fd`(96) `close_cloexec_fds`(174) `clear_non_cloexec`(186) | Plan B 并行 FD 表整体未采用，删/接线待裁 |

**B-3 由 A-2 退桶并入（8 项；reviewer 第二轮裁定）**

| 文件 | 项 | 待裁点 |
|---|---|---|
| `framework/arch/shadow_stack.rs` | `is_ssp_valid`(129) | 与同文件 4 项（`set_ssp` 等）**整组统一**，取决于 CET 路线图；安全敏感（CET/SSP） |
| `framework/mm/numa.rs` | `contains_cpu`(195) `all_nodes`(334) | 与同文件 3 项（`set_distance` 等）**整组统一**，取决于 NUMA 路线图 |
| `framework/mm/kpti.rs` | `pcid_is_enabled`(149) | 已登记路线图项（`kpti.rs:23-31` PCID/INVPCID 优化）的地基 |
| `framework/barrier/reset/audit.rs` | `count_by_result`(101) | 诊断查询 API 面；② 未取得等价入口 |
| `framework/mm/page_fault.rs` | `page_fault_count`(497) | 统计 API 面；② 未取得等价封装入口 |
| `framework/mm/slab.rs` | `utilization`(921) | 统计 API 面；② 仅部分等价（`CacheStats` 可推导） |
| `framework/arch/x86_64/gdt.rs` | `get_gdt_table`(701) | 注释自述调试用途 ⇒ 非死代码 |

> **同文件分裂已收敛（reviewer 第二轮结构性问题 1）**：`shadow_stack.rs` / `numa.rs` 两处「同文件内部分裂成两桶」已按**路线图整组统一**处理——判删理由（零消费）对同文件「待裁」项同样成立，反证「零消费」不足以作为删的判据。故该 3 项随同文件整组归入待裁，不再单列删候选。

**B-4 由安全面分流并入（2 项；reviewer 第三轮裁定二）**

| 文件 | 项（行号） | 待裁点 |
|---|---|---|
| `services/fs/ramfs.rs` | `split_path`(524) `validate_path`(544) | **待定型（冗余 / 缺陷 二选一）**：须先由 **T3 安全面**给出「VFS 是否已提供等价（空/长度/NUL）校验」的结论。**结论出来前不进试删队列**。若定型为**冗余** ⇒ 可删，但须登记契约「ramfs 依赖 VFS 前置校验」；若定型为**缺陷** ⇒ 不得删，登记缺陷移 T3 并按 C-1 接线 |

> **本批唯一挂起项**：B-4 是 A-2 之外唯一的"待 T3"项——与 C-1 接线子清单联动（若定型为缺陷，C-1 新增 1 项「ramfs 路径入口补 `validate_path` 调用」；若定型为冗余，则 C-1 不动、本项转为可删候选）。

#### C. 原「接线」142 项（重划：仅 8 项留「接线」，其余 134 项入「未来功能」）

**C-1 接线（8 项；判据＝同族入口已在调用链中使用，仅缺此半 —— 可施工子清单）**

| 文件 | 项 | 缺的半 |
|---|---|---|
| `framework/fs/vfs/handle.rs` | `vfs_get_fd_handle` | FD→句柄访问器，同文件 `vfs_*` 入口已在用 |
| `framework/fs/vfs/vfs.rs` | `set_fd` | FD 表写入口，与在用 `get_fd` 成对 |
| `framework/irqline.rs` | `is_registered` | IRQ 线注册状态查询，与 `register_irq` 成对 |
| `framework/frame.rs` | `set_meta` | 页帧元数据写入口，与 `get_meta` 成对 |
| `framework/proc/fd_table.rs` | `get_handle_id` `is_cloexec` `set_cloexec` `get_cloexec_fds`（4） | FD 表 cloexec 面，exec 路径需用 |

**C-2 原 142 项全量（逐项保留可追溯；除 C-1 外均归「未来功能」）**

| 文件 | 数 | 项 |
|---|---|---|
| `framework/arch/aarch64/mmu.rs` | 6 | `write_ttbr0` `tlbi_vaae1` `tlbi_vmalle1` `make_user_rw_entry` `make_kernel_rw_entry` `make_user_ro_entry` |
| `framework/arch/aarch64/psci.rs` | 2 | `system_off` `system_reset` |
| `framework/arch/aarch64/timer.rs` | 1 | `set_timeout_ms` |
| `framework/arch/aarch64/uart.rs` | 1 | `switch_to_high_half` |
| `framework/arch/x86_64/apic.rs` | 4 | `mask_lint0` `mask_lint1` `unmask_lint0` `unmask_lint1` |
| `framework/barrier/domain.rs` | 2 | `check_quota` `check_proc_limit` |
| `framework/barrier/recovery.rs` | 4 | `recovery_registry_init` `recovery_subdomain_save_checkpoint` `cascade_recover` `hard_reset_domain` |
| `framework/barrier/{reset/layered,snapshot}.rs` | 4 | `test_recovery_status` `test_snapshot_basic` `test_registry_register` `test_registry_priority_order`（kernel_test 用例未挂测试运行器） |
| `framework/mm/kpti.rs` | 3 | `invpcid_flush_single` `kpti_kernel_pml4` `kpti_user_pml4_or_kernel` |
| `framework/mm/kpti_aarch64.rs` | 2 | `kpti_trampoline_ttbr1` `kpti_kernel_ttbr1` |
| `framework/mm/swap.rs` | 1 | `swap_free` |
| `framework/mm/vma.rs` | 1 | `with_offset` |
| `framework/sync/mutex.rs` / `pi_mutex.rs` / `rcu.rs` | 3 | `wait_timeout` / `set_ceiling` / `rcu_process_all_callbacks` |
| `framework/chitin/user_driver.rs` | 3 | `devtree_map_user_device` `devtree_unmap_user_device` `chitin_forward_irq` |
| `framework/cpu/tsc.rs` | 1 | `nanoseconds_to_cycles` |
| `framework/credo/{audit,identity,secure_boot}.rs` | 3 | `get_entries` `find_mut` `add_trust_entry` |
| `framework/debug/ebpf.rs` | 3 | `prog_run` `get_map` `get_prog` |
| `framework/dma/engine.rs` | 5 | `unmap_single` `sync_both` `sg_init` `sg_add_entry` `sg_total_length` |
| `framework/driver/block.rs` | 1 | `mark_removed` |
| `framework/driver/hotplug.rs` | 1 | `hotplug_poll` |
| `framework/driver/input/keyboard.rs` | 1 | `get_modifiers` |
| `framework/driver/net/e1000_io.rs` | 5 | `set_ctrl` `set_rx_ctl` `set_tx_ctl` `set_ipg` `install_rings` |
| `framework/driver/power.rs` | 4 | `ondemand_check` `register_notifier` `pm_subsystem` `pm_is_initialized` |
| `framework/driver/usb/mass_storage.rs` | 2 | `build_read_capacity_10_cbw` `build_request_sense_cbw` |
| `framework/driver/usb/usb_core.rs` | 3 | `register_controller` `find_device_by_class` `find_device_by_vid_pid` |
| `framework/driver/usb/xhci.rs` | 2 | `init_command_ring` `recover_endpoint` |
| `framework/frame.rs` | 1 | `set_meta` |
| `framework/fs/vfs/handle.rs` | 1 | `vfs_get_fd_handle` |
| `framework/fs/vfs/inotify.rs` | 1 | `inotify_fd_readable` |
| `framework/fs/vfs/vfs.rs` | 1 | `set_fd` |
| `framework/idt/idt.rs` | 1 | `set_exception_handler` |
| `framework/io/iouring.rs` | 2 | `io_uring_destroy` `io_uring_reap` |
| `framework/irqline.rs` | 1 | `is_registered` |
| `framework/net/netfilter.rs` | 2 | `hook_count` `list_rules` |
| `framework/net/route.rs` | 2 | `route_list` `default_route` |
| `framework/page_table.rs` | 1 | `verify_kernel_code_protection` |
| `framework/pci/msi.rs` | 5 | `msi_enable` `msi_disable` `msix_disable` `msix_mask_vector` `msix_unmask_vector` |
| `framework/proc/canary.rs` | 1 | `set_per_proc_seed` |
| `framework/proc/cfs.rs` | 3 | `get_weighted_load` `steal_highest_vruntime` `get_load` |
| `framework/proc/cgroup.rs` | 8 | `check_budget` `period_reset` `try_charge` `uncharge` `is_over_limit` `account_read` `account_write` `cgroup_of` |
| `framework/proc/cpu_queue.rs` | 1 | `register_sched_softirq` |
| `framework/proc/fd_table.rs` | 4 | `get_handle_id` `is_cloexec` `set_cloexec` `get_cloexec_fds` |
| `framework/proc/namespace.rs` | 3 | `to_clone_flag` `map_uid` `map_gid` |
| `framework/proc/posix_timer.rs` | 1 | `posix_timer_release_pid` |
| `framework/proc/process.rs` | 2 | `kernel_stack_check_canary` `allocate_user_space` |
| `framework/proc/rlimit.rs` | 5 | `check_nofile_exceeded` `check_as_exceeded` `check_nproc_exceeded` `get_stack_limit` `get_nofile_limit` |
| `framework/proc/scheduler.rs` | 2 | `set_deadline_params` `get_current_process` |
| `framework/proc/scheduler_ex.rs` | 3 | `freeze_all` `thaw_all` `exit_thread` |
| `framework/proc/seccomp.rs` | 2 | `from_linux` `to_linux` |
| `framework/proc/session.rs` | 3 | `get_session` `sys_tiocsctty` `signal_foreground_pgid` |
| `framework/proc/signal.rs` | 1 | `has_deliverable_signal` |
| `framework/proc/thread.rs` | 2 | `create_thread` `get_thread` |
| `framework/proc/user_proc.rs` | 1 | `create_from_binary` |
| `framework/syscall/epoll.rs` | 1 | `epoll_destroy` |
| `framework/timer/hrtimer.rs` | 1 | `hrtimer_ns_to_cycles` |
| `framework/timer/tick.rs` | 2 | `reset_ticks` `get_uptime_tsc` |
| `framework/timer/tickless.rs` | 2 | `enter_tickless` `exit_tickless` |
| `framework/timer/time_sync.rs` | 1 | `client_request` |
| `framework/vmspace.rs` | 1 | `map_huge` |
| `services/fs/{sysfs,cgroupfs,configfs,virtiofs,systree}.rs` | 5 | `mount_sysfs` `mount_cgroupfs` `mount_configfs` `mount_virtiofs` `mount_systree`（实现完整，待 VFS mount 集成） |
| `services/net/unix.rs` | 1 | `uds_recv_with_creds`（recvmsg UDS 凭据分流点缺失） |

#### D. 原「预留」205 项（按子系统计数；已并入「未来功能」桶）

| 子系统/文件 | 数 | 预留性质 |
|---|---|---|
| `framework/arch/aarch64/gic.rs` | 5 | SPI 使能/禁用/pending/触发配置（SPI bring-up 前预留） |
| `framework/arch/aarch64/mmu.rs` | 1 | `alloc_user_page_table`（Phase 6 每进程独立页表） |
| `framework/arch/aarch64/timer.rs` | 1 | `set_compare`（oneshot 高精度定时） |
| `framework/mm/swap.rs` | 1 | `swap_deinit`（host-tests 配对 + 将来热卸载） |
| `framework/chitin/{composite,devtree}.rs` | 2 | 设备树匹配/属性解析 API 面 |
| `framework/console/gfx_console.rs` | 2 | `set_margin` `set_colors`（控制台配置面） |
| `framework/cpu/feature.rs` | 6 | CPU 能力/厂商查询面（D-4） |
| `framework/dma/engine.rs` | 1 | `submit_transfer_async`（异步 DMA 面） |
| `framework/driver/display/controller.rs` | 11 | 多显示器管理 API 面（热拔管理未启用） |
| `framework/driver/display/framebuffer.rs` | 1 | `intersection`（绘制辅助） |
| `framework/driver/framework.rs` | 2 | `outw` `inw`（16 位 PIO 原语族） |
| `framework/driver/power.rs` | 2 | `latency_us` `power_saving`（电源约束查询面） |
| `framework/driver/uefi.rs` | 4 | UEFI 运行期服务 API 面 |
| `framework/driver/usb/ring.rs` | 2 | 环入队/出队指针访问器 |
| `framework/fs/vfs/dcache.rs` / `flock.rs` / `inotify.rs` | 6 | VFS 内部诊断面 + flock 子系统查询面 + inotify 统计 |
| `framework/idt/{handlers,idt,safety,statistics}.rs` | 7 | 诊断计数/历史查询面 + 屏障原语（D-4） |
| `framework/proc/scheduler_ex.rs` | 1 | `thread_dump_info`（线程信息转储） |
| `services/credo/{identity,secure_boot,crypto}.rs` | 19 | 身份/凭据/安全启动能力预留（D-4）+ safe 代理壳 |
| `services/driver/{acpi,char,display,firmware,storage,uefi,usb,virtio}` | 41 | safe 代理壳 + 硬件操作面（D-4） |
| `services/fs/nestfs/*` | 34 | NestFS 子系统预留（D-4） |
| `services/fs/{exfat,ext2,tmpfs,devpts,sysfs,systree,ramfs,cgroupfs,configfs}` | 21 | FS 内部访问器 / mount-配套面 / 空间统计面 |
| `services/ipc/{async_ipc,sem,signal}.rs` | 12 | IPC API/FFI 导出面（待用户态接线） |
| `services/mm/{memory_pressure,swap}.rs` | 3 | 内存压力策略 + SwapInfo 查询面 |
| `services/barrier/audit_export.rs` / `config/sysctl.rs` | 3 | 审计导出统计 + sysctl 序列化面 |
| `services/proc/{canary,elf,shadow_stack,signal}.rs` / `timer/*` / `net/unix.rs` / `wasm/*` | 12 | 查询面 + safe 代理壳 + wasm 运行时 API 面 |

#### 登记结论（修订版：E.1-E.4 + 第二 / 三 / 四轮裁定）

1. **元信息已补（E.1）**：① 判定工具＝`scripts/audit_unwired_pub_fn.py` R1（`rg -c -w` 文本并集，声明侧无 cfg 感知）；② 判定构建维＝**无单一构建维**，未做逐维交集 ⇒ 原「零引用」口径不可复核。③④⑤ 见上「方法与限制」。
2. **桶边界已重划并重算（E.2 + 三处核对① + 第二轮二次修订 + 第三轮三次修订 + 第四轮四次修订，暂定值）**：删候选 70 → 23 → **11** → **2**（二次修订：安全面 4 转 A-3、判据不成立 8 退桶入 B-3；**四次修订：试删前逐项复核退桶 9 入待裁，见 A-5 / 12**）；**硬件原语完整性保留 41 → 43**（x86_64 侧 31 + aarch64 侧 10，优先级＝**硬件原语保留 > aarch64 门控 > 删候选**；三次修订并入安全面**族残缺**档 2 ＝`ct_eq_salt`/`ct_eq_password`）；接线 142 → **8**（逐项独立判定）；未来功能 **339**（**来源合成，非实测**：原接线剩余 134 ＝142−8 排除法 + 原预留 205）；待裁 21 → 27 → 35 → **37** → **46**（三次修订并入安全面**待 T3 结论**档 2 ＝`split_path`/`validate_path`，B-4；**四次修订并入 A-5 退桶 9**）。**原「安全面待确认 4」桶三次修订后清零**（三档分流完毕）。合计 438（算术已核对闭合：2+43+8+339+46）。三处核对 ②③ 的来源标记已就地标注。
3. **`atomic_stats` 已退回待裁（E.3）**：`sync/atomic.rs` 4 项 `record_*` 属 `#[cfg(feature = "atomic_stats")]` 门控，**非死代码**；援引既有登记 [subsystem-sync.md §9.3 [P2]](archive/audit-2026-08-14/subsystem-sync.md#L1075-L1092)。
4. **删候选施工＝试删（E.4 + 遗留 2 裁定，不立项新工具）**：二次分类已移出 43 项硬件原语（含族残缺 2）、**且已移出安全面 4 项（不走试删）**；余 **11 项**（A-2；**四次修订后为 2 项**，见 12）按 ⑥「**逐项试删 → 跑既有五条门槛 → 任一维硬失败即回退**」推进（`build.sh all` / clippy `kernel_test` 维 / clippy `host-test` 维 / `make test-host` / QEMU `kernel_test`+boot），**编译/链接器即权威判据**，不新建调用图分析器。**第三轮已授予开工**（解锁五条逐条核销见 ⑧），执行约束：逐项独立提交 / 每项全量五门槛 / QEMU boot 硬闸门 / ramfs 2 项不入本批。
5. **遗留 1 已降级为方法学注释（不专项量化）**：x86_64 专属项面未量化属**漏项风险（完备性）而非误删风险（正确性）**——已在 ③ 补注「单维甄别的结果仅在该维有效，跨维完备性需逐维复核」。
6. **第二轮复核：A-2 已退回重做（reviewer，12/23 项判据站不住）**——原 23 项按四档处置，逐档有据：① **判据仅「零消费」7 项** → 补齐证据或退桶，结果 **7 项全退**（3 项同文件整组统一、1 项已登记路线图项、3 项属统计/诊断 API 面且未取得等价入口证据）；② **安全敏感 4 项** → 转 A-3 安全面二次分桶，**不走试删**；③ **注释待核 1 项**（`get_gdt_table`）→ 引注释原文核对＝`/// 获取 GDT 表的引用 (调试用途)`，**非死代码**，退桶；④ **判据较强 11 项** → 保留为 A-2。算术：23 − 4 − 8 = **11**。
7. **判据升格为三合一（reviewer 第二轮，根本性修正）**：「存在等价公共入口」**单独不成立**——有等价入口 ≠ 该函数无人用（可能正属 API 面 / FFI 面 / feature 面）。删候选判据＝**该构建维零引用 ＋ 存在能力等价的公共入口 ＋ 非 API/FFI/feature/硬件原语面**，三者须同时成立。
8. **试删的原理性盲区 + 顺序约束（reviewer 第二轮）**：`试删 + 五条门槛`**在原理上发现不了「安全校验被移除」**（删零调用校验后编译链接全过、无覆盖即全绿）⇒ 安全敏感项必须先经安全面确认；**不得并行 T1 收尾与 T5 施工**（同动 `framework`，且 `pcid_is_enabled` 与 T1 P2′ 所改 `kpti.rs` **文件级冲突**），T5 文档修订可并行。A-2 开工解锁条件五条见 ⑧。
9. **第三轮裁定一：`ct_eq_salt` / `ct_eq_password` → 不删，移入完整性保留（族残缺档）**——实读 [crypto.rs:198-217](file:///home/anfer/Code/QueenX/src/kernel/services/credo/crypto.rs#L198-L217)，四者（`ct_eq` / `ct_eq_hash` / `ct_eq_salt` / `ct_eq_password`）**形状完全相同**，是 `Salt` / `PasswordHash` / `Sha256Hash` 的**类型化比较同族**；`ct_eq_hash` 有调用者、未入删候选 ⇒ 删另两者则**族残缺且不对称**。真正作用不是「提供能力」（`ct_eq` 已提供），而是**阻止调用方拆字段**（写 `ct_eq(&a.0, &b.0)`）——删掉等于**诱导密码学代码绕过类型包装**、降低抽象层级，与 A-1 保留 `sync/*` 原语族入口**判据同构**。**附核实已完成**：全仓 `src/` + `host-tests/` 零引用、无 `#[no_mangle]` / 无 `extern "C"` / 无 FFI、超出零引用即无跨 crate 与用户态 API 面按名调用 ⇒ 保留不引入新暴露面。桶效应：A-1 41 → **43**。
10. **第三轮裁定二 + 裁定三：ramfs 2 项挂起待 T3；二次分桶产出规格固定为三档**——（二）`split_path` / `validate_path` **不得按「零调用」删**，须先由 T3 给出「VFS 是否已提供等价（空/长度/NUL）校验」结论再二选一（冗余 ⇒ 可删 + 登记契约；缺陷 ⇒ 不删 + 登记缺陷 + 按 C-1 接线）；**威胁模型已更正**：`validate_path` **不含 `..` 穿越检查**，穿越由 VFS `resolve_path` 负责，上轮「删即移除穿越防护」不成立（实况见 A-3）。桶效应：并入 B-4，待裁 35 → **37**。（三）**「二次分桶」产物固定为三档＝冗余 / 缺陷 / 族残缺**，写入 **B-0** 作为本桶定型口径——**三合一判据不足以区分三档**（它只判「是否依赖等价入口」，不判「零调用是冗余还是缺陷」）。
11. **第三轮开工裁定：A-2 11 项试删已授予开工**——解锁五条逐条核销（①②③④⑤ 全 ✅，见 ⑧）；执行约束：**逐项试删 / 每项独立提交可单独回退 / 每项跑五条门槛全量 / QEMU boot 为硬闸门 / ramfs 2 项不入本批 / `pcid_is_enabled` 已确认不在 11 项内**。**QEMU 日志已留存归档**：boot 日志 `build/log/qemu_boot_x86_64_t1wrap_20260918.log`（md5 `ad749d876db2808d053e219b5f91523a`，240 行）、kernel_test 日志 `tests/reports/unit_test_20260918_165355.log`（md5 `b1cf091bf44d3e6419fcd8f6c75f4d17`）。注：二者所在目录（`build/` / `tests/reports/`）**按 B08-10 既有裁定为 gitignore**（禁止日志入仓），故**可追溯性＝时间戳/固定名归档文件 + 关键行逐字入档**，非 git 跟踪。
12. **第四轮（试删开工后逐项复核）：A-2 退桶 9 项入待裁 ⇒ 删候选 11 → 2**——试删启动后按 **B-0 三档规格**逐项复核「零调用 ＝ 冗余 / 缺陷 / 族残缺」，以**全仓 `grep -rn` 实测引用计数**为判据，发现 9 项判据不成立（明细见 **A-5**）：
    - **族残缺 6 项**（同族兄弟在用 ⇒ 不删，保对称）：`tss_64bit`（`Granularity` 构造器族 `code_64bit` **4** / `data_32bit` **4** / `tss_64bit` **0** ⇒ 2/3 在用；字段 `Granularity(pub(crate) u8)` ⇒ 删后调用方须写 `Granularity(Granularity::LONG_MODE)` **绕过构造器族**，**与裁定一 `ct_eq(&a.0, &b.0)` 判据同构**）、`vfs_close_safe` / `vfs_seek_safe` / `vfs_readdir_safe`（`vfs_*_safe` 全族 **17 员 / 14 员在用**）、`get_ap`（`get_ap_list`/`get_ap_count`/`has_madt`/`parse_madt` 在用）、`get_fs_name`（`get_fs_type`/`get_fs`/`set_fs` 在用）。
    - **台账 ② 判据事实错误 / 无等价入口 3 项**：`consume_quota_tick` / `is_quota_exceeded`（原记「与**在用** `check_quota` 语义重复」——实测 `check_quota` **自身零引用**且已列 B-2 待裁 ⇒「在用」不成立）、`pipe_exists`（原记「可由**在用** `get_pipe` / `pipe_count` 等价判定」——全仓**无 `get_pipe` 符号**，`pipe_count()` 不判定指定 `IpcId` 是否存在 ⇒ **无等价公共入口**）。两处错误已就地订正。
    - **新增原理性证据（重要）**：`get_ap` 试删后五门槛 **5/5 全过**（含 QEMU boot 硬闸门）却仍被判族残缺退回 ⇒ **试删 + 五条门槛在原理上无法识别族残缺**，与 ⑧「发现不了安全校验被移除」**同源**——**门槛通过 ≠ 判据成立**，试删只证「无构建/链接/回归破坏」，不证「零调用属冗余」。
    - **档位争议如实登记**：`vfs_*_safe` 三项**弱于** `tss_64bit`——实测 [dir_ops.rs:14-28](file:///home/anfer/Code/QueenX/src/kernel/services/fs/dir_ops.rs#L14-L28) 已直接调用裸 `extern "C"` `vfs_seek` / `vfs_readdir`（不经 `_safe` 壳）⇒ 该族**本就不是封装边界**，删 3 员不改变抽象层级。仍按「同族部分在用 ⇒ 保对称」退桶，最终档位由 reviewer 复核确定。
    - **桶效应**：删候选 11 → **2**（仅 `write_log_line` / `format_duration`，为**唯一进入试删者**）；待裁 37 → **46**。合计 438（算术已核对闭合：2+43+8+339+46）。

## 详情

### 预存登记（T1 G2 审查处置，2026-09-16）

| 项 | 裁决 | 说明 |
|---|---|---|
| CLONE_SETTLS 切换恢复实装 | 待办（用户态线程库出现时） | `Process.tls_base` 保留不删（POSIX 线程兼容面预留）；x86_64 需在切换时写 MSR_FS_BASE、aarch64 写 tpidr_el0。注释已如实修正（clone.rs / process.rs 字段文档），原"上下文切换时恢复"陈述与事实不符 |
| sched_ops.rs 整批 FFI 面 0 引用核实 | 待办（后续专项） | scheduler_set_quota/remove_quota/set_proc_limit/proc_get_current_pid_internal/proc_yield_internal 全仓（Rust+C+asm）0 引用；`no_mangle` 屏蔽编译器告警。同批已删 proc_exit_internal（同性质，exit 实际走 proc_ops.rs `SCHEDULER.exit` 直调），整批删除待专项核实确认 |

### 预存登记（T1 G3 批报告，裁决处置）

| 项 | 裁决 | 说明 |
|---|---|---|
| uds_close 不释放 fd_alloc 位图 | 已修（本批） | 生产级 FD 泄漏：uds_close 仅清 UDS socket 槽位，从未调用 `fd_alloc::free_fd(Uds, fd)`（对照 pidfd close 释放模式）；UDS 容量 16，累计创建 16 次后位图永久耗尽 → uds_create 恒 NoMem。修复：uds_close 补 free_fd + listener 关闭时 pending client 位同步回收；kernel_test 回归 close_releases_bitmap，socketpair_rollback 显式补偿逻辑随修复移除 |
| recvmsg/sendmsg_syscall 无 UDS 数据面分流 | 专项登记（后续工程） | 旧 recvmsg/sendmsg_syscall 处理完 cmsg 后直调 fw::recvmsg/sendmsg，fw 层 fd_type 只认 smoltcp 1/2 → UDS fd 返 EBADF（当前仅 cmsg 凭据回传逻辑可达 UDS）。G3 recvmmsg/sendmmsg 新路径已内建 UDS 分流；旧路径分流待单独工程处置（本批 §12.2 未顺手扩大） |
| uds_sendto 无消息长度校验 | 已修（本批） | `uds_sendto` 未校验 `data.len() > UNIX_DGRAM_MAX`，超限直接 copy_from_slice 写 dgram_buf → 越界 panic 隐患。修复：入口补 `data.len() > UNIX_DGRAM_MAX → Invalid` 防护（与 uds_send_connected 同款）+ kernel_test 回归 sendto_oversize（超限拒绝 + 正常路径不受影响） |

### 预存登记（T1 G4 批报告）

| 项 | 裁决 | 说明 |
|---|---|---|
| `sys_set_mempolicy` 直接强转 Linux MPOL 编码 | 登记（后续专项，本批 §12.2 未扩大） | `framework/mm/numa.rs` 的 `sys_set_mempolicy(mode, nodemask)` 用 `NumaPolicy::from_u8(mode as u8)` 直接强转，而 Linux MPOL 编码与枚举判别值**不同**（`MPOL_PREFERRED = 1` vs `NumaPolicy::Bind = 1`、`MPOL_BIND = 2` vs `Interleave = 2`、`MPOL_INTERLEAVE = 3` vs `Preferred = 3`）→ 用户态传 1/2/3 全部错位。同批新增的 mbind 已走 `from_linux_mode` 正确映射（`NumaPolicy::from_u8` 文档已标注"禁止直接 from_u8"），两根路径编码处理不一致。同时 `sys_set_mempolicy` 的 `nodemask` 按值接收（Linux ABI 为指针 + maxnode），且无 `flags` 参数。修复需连同 G4 一词核对该 syscall 的 dispatch 调用形态（当前回退层调用点）。 |
| `range_is_mapped` 允许 VMA 真子集（本批语义修正） | 已修（本批） | 原实现要求目标区间被**单个** VMA 完整包含（`vma.start > start` 或 `vma.end < end` 即 false）→ uffd 注册落在 VMA 内部的区间被拒（kernel_test `mm::uffd::fault_flow` 实测暴露）。修正为交叠长度累加（允许真子集 / 跨相邻 VMA，仅拒绝空洞），与 `set_numa_policy_range` 的"整 VMA 对齐"要求差异在文档注明（uffd 注册区间不携带 VMA 策略字段，无策略越界问题）。 |

### 预存登记（T1 G7 批报告）

| 项 | 裁决 | 说明 |
|---|---|---|
| `uname` 默认 nodename `"queenx-node"` → `"QueenX"`，domainname `"(none)"` → 空串 | 已随本批变更（行为变化登记） | UTS 收敛的直接后果：`uname`（硬编码 `"queenx-node"`）/ `gethostname`（硬编码 `"localhost"`）/ `sethostname`（仅校验不存储）三处各说各话，统一后以 `UtsNamespace` 初值 `UTS_DEFAULT_NODENAME = b"QueenX"` 为准。原 `"(none)"` 为 Linux 未设域名的显示占位，本批按"未设置即空串"处理。回退路径：需对齐 Linux `(none)` 字面量时在 `sys_uname` 回填占位串 |
| `VfsManager.root` 全局单例根 + `pivot_root` 不摘除旧根 + `vfs_set_cwd_internal` 静默失败 | 登记（本批 SIMPLIFIED，见实施记录 SIMPLIFIED 清单） | 三项均为随 chroot/pivot_root 完整路径解析引入的显式简化，已在代码内以 `// SIMPLIFIED:` 标注具体简化点/影响面/扩展时机；待 per-process root 或 mount namespace 机制出现时统一升级 |
| `audit_implicit_deps` 151 → 159（+8） | 登记（本批引入，扩展审计维度） | 增量全部来自用户路径归一化的必要调用点：`services/fs/path.rs` +5（chroot/pivot_root 的 `VFS_MANAGER.set_root`/`get_root`/`resolve_user_path`）、`services/fs/file_handle.rs` +2（`name_to_handle_at` 归一化）、`services/fs/file_ops.rs` +1（`resolve_user_path`）。该维度为**扩展审计**（非 CI 硬门槛），HEAD 基线已有 151 处预存 backlog，本批未做清减（§12.2 不顺手扩大） |
| `audit_unwired_pub_fn` / `public_api_docs` / `implicit_deps` 大额预存 backlog | 登记（预存，非本批引入） | 经 HEAD worktree 对比确认与 T1 G4-G7 批次无关；待专项工程处置 |

### 预存登记（host 链接占位符号残留风险，T1 G7 批引入）

描述：T1 G7 批为修复 host 测试链接失败，在 host-only 壳 crate `src/rust/src/lib.rs` 提供 4 个零值占位符号（`_kernel_text_start` / `_kpti_trampoline_end` / `_kernel_text_end` / `USER_CR3_SAVE`）。该修复**不是结构根治**，残留风险登记如下，待与审查讨论处置。

方案（已定，2026-09-17 审查裁定）：**符号使用点级 host 桩化**——沿用 E-04 既有 idiom（`#[cfg(feature = "host-test")]` 桩分支 + `#[cfg(not(feature = "host-test"))]` 真机分支，先例见 `framework/arch/x86_64/mod.rs:49-67` 的 `cpu_id`），把"引用链接脚本/汇编符号的语句"收进 cfg 分支，host 侧取常量中性返回或整段跳过；随后**删除壳 crate 的 4 个占位**。目标是**消灭 undefined symbol 这一类**，并把"违反即链接期硬失败"的响亮守卫交还给链接器——无需新增审计脚本，无需豁免表。

**否决的候选**：① 审计守卫（保留静默零值语义 + 长期维护门槛）；② 模块级 `cfg` 排除（整块排除会牵连 `vmm`/`proc` 的纯逻辑退出 host 编译，且 stub 面积膨胀）；③ 维持现状（保留对 CGU 布局的依赖，本批已实际付出一次逐文件二分排查成本）。原候选对比中"候选 A（framework 内 `cfg(host-test)` 占位）需在 TCB 内混入 host 分支"的排除理由**不成立**——`framework/arch/x86_64/mod.rs` 早有同类 host 桩分支，且这正是"同源双编译"的实现方式。

**可达性矩阵（2026-09-17 调研）**

调研口径：逐个符号访问点向上追溯调用链至入口，判定 host 测试能否到达；旁证为 `host-tests/` 全线零**代码**引用（`boot_install` / `CREDO_DISK_INSTALL` / `create_user_page_table` / `kpti_init` / `boot::init` 无任何命中；`handle_user_page_fault` / `read_user_cr3_asm` 仅命中文档注释，无调用）。

| 簇 | 符号 | 访问点 | 调用链顶端 | host 可达性 |
|---|---|---|---|---|
| 1 | `USER_CR3_SAVE` | ① `mm::read_user_cr3_asm`（`framework/mm/mod.rs:26`）② `kpti::map_kpti_data_pages`（`framework/mm/kpti.rs:718-720`） | ① `page_fault.rs:114` `handle_user_page_fault` ← `#PF` 入口 ② `kpti_init`（`kpti.rs:389`）/ `create_user_page_table`（`vmm_x86_64.rs:651`） | **不可达**（①`demand_paging_test.rs:10-13` 已载明"内核 mm 层 host 不可测，已移除 `handle_user_page_fault`/`handle_page_fault` 依赖"；②host 无 MMU，不可构造页表上下文） |
| 2 | `_kernel_text_start` / `_kernel_text_end` | ① `kpti::kpti_init`（`framework/mm/kpti.rs:366-380` step 4.5）② `vmm::create_user_page_table`（`framework/mm/vmm_x86_64.rs:630-635`） | `vmm_init` / 进程地址空间创建 | **不可达**（host 无 MMU，不可构造页表上下文） |
| 2b | `_kpti_trampoline_end` | **零引用**（仅 `kpti.rs:161` extern 声明 + `x86_64.ld:51` 定义） | — | 无引用即无 undefined symbol；G7 实际报错清单亦不含它 → **连声明一并删除** |
| 3 | `_kernel_end` | `boot::init`（`framework/boot/mod.rs:285`） | `kernel_init`（`src/kernel/lib.rs:562` / `:654`） | **不可达**（裸机引导入口） |
| 4 | `_kernel_start` / `_kernel_end` | `raw::kernel_start_ptr`（`framework/syscall/mod.rs:318`）/ `raw::kernel_end_phys`（`:331`） | `sys_boot_install`（`framework/syscall/dispatch.rs:876`，gate 为 `all(not(kernel_test), x86_64)`） | **不可达**（host-tests 零引用 boot_install / CREDO_DISK_INSTALL） |
| 5 | `stack_bottom` | ① `proc::check_boot_stack_canary`（`framework/proc/process.rs:50`）② `proc::write_boot_stack_canary`（`framework/proc/process.rs:69`） | `kernel_init`（`src/kernel/lib.rs:528` / `:904`）+ aarch64 引导入口（`boot/aarch64/entry.rs:40` / `:52`） | **不可达**（裸机引导入口） |

**aarch64 侧顺带项**：`_kernel_end` 另有 `framework/boot/aarch64/entry.rs:101` 访问点（该模块受 `target_arch = "aarch64"` 门控，host 构建不编译，不构成 host 链接风险）；列此仅为"符号语义桩化原则在双架构一致"的完整性记录。

**矩阵结论**：5 簇全部 host 不可达 → **全部桩化，无"必须保留占位"的残余项**。存量漏项（`_kernel_start` / `_kernel_end` / `stack_bottom`——G7 未覆盖、但已被 host 编译路径引用，只因 CGU 未共置而未爆）随本方案一并消灭，不需要"补占位"也不需要"豁免表"。净效果：链接脚本/汇编符号契约 **7 → 0**（P2′ 后**声明亦全部收拢**，host 下误引用为**编译期**失败——见裁决表「守卫两级化」条）。

**实施形态与纪律**

> **关键约束（实施前必读）**：cfg 必须落在**符号引用所在的语句/函数体内部**，**不能只 gate 调用点**。rustc 对 crate 内所有可达 item 一律生成代码，即使某函数在 host 构建下已无任何调用者，其函数体内的 `extern` 符号引用仍会进入目标文件并产生 undefined symbol。因此"gate 掉调用方"这种做法对消灭 undefined symbol **无效**（G7 的簇 4 判断即由此而来）。

| 簇 | 改法 | host 桩 |
|---|---|---|
| 1 | 两个访问点各自**函数体内** cfg 分叉：`read_user_cr3_asm`、`kpti::map_kpti_data_pages` | `return 0` / 整段跳过 |
| 2 | 2 处访问点各自把"符号取值 + 页表映射"收进 cfg 块（**函数本体保留编译，签名仍受 host 检查**） | 整段跳过 |
| 2 补注 | `kpti_init` / `create_user_page_table` 为**体内 cfg 分叉**（符合上表原文）；但其叶子函数 `map_text_region_in_user_pml4` / `map_text_page` 实施为**整函数 cfg**（实施后补注） | 理由：F9 禁 `#[allow(dead_code)]`，host 下调用点已整段 cfg 致**无调用者**，整函数消除是唯一合规路径；两者为 `pub(super)` 叶子、全为符号取值 + 页表操作，**无纯逻辑可分离**（不存在"保留编译有价值"的部分） |
| 2b | 删除 `kpti.rs` 的 `_kpti_trampoline_end` extern 声明（零引用） | — |
| 3 | `boot::init` 内符号取值收进 cfg 块 | `kernel_end = 0` |
| 4 | 两个访问器函数**内** cfg 分叉（`kernel_start_ptr` / `kernel_end_phys`；**调用链零改动**——若改为 gate 函数本体，`sys_boot_install` 与 services 侧 `boot_install_syscall` 的 cfg 需同链上推，且按上条约束仍不解决问题） | `null` / `0` |
| 5 | 两个访问点各自**函数体内** cfg 分叉：`check_boot_stack_canary`、`write_boot_stack_canary` | `return true` / 整段跳过 |

**纪律（维持"host-test 不平行实现"的关键）**：桩体**只允许两种形态**——常量中性返回、或整段不执行；**禁止**在桩体内出现任何条件分支、状态读写或业务判断。桩内无"实现"即无"两份实现"，平行实现的滋生根被切断。

**门控语义推广**：真机路径门控由 `not(feature = "kernel_test")` 推广为 `not(any(feature = "kernel_test", feature = "host-test"))`（kt/ht 同属"测试环境"，与 `audit_feature_semantics.py` 既有 `any(kernel_test, host_test)` 用法闭合）。属**既有约定表述扩展**，实施时须同步该脚本规则文本并确认无既有用例被推翻。

**验证**

1. 删除壳 4 占位后 `./ci/build.sh all` 5/5（host-tests 99 bin 全链接成功）；
2. **负向验证（必做，P1 改写版）**：临时去掉一处 host 桩（使 host 可达路径引用真符号），确认构建**硬失败**——P2′ 全声明收拢后报错层级为**编译期** E0425（`cannot find value … in module`）；仅当符号声明未随引用收拢时才为链接期 `undefined symbol`。两级均为「违反即硬失败」守卫（不承诺"编译进对象即报错"：引用不可达时不报错），证明守卫未换处藏问题；
3. 双架构 check 0w0e + clippy 3 维（pedantic lib / kernel_test / host-test）0 warning；QEMU kernel_test 498/498 + QEMU boot（Ring 3/init）；
4. `audit_feature_semantics.py` 的 HIGH 清单与 HEAD 基线**逐项一致**（不得新增/推翻；当前 7 项 HIGH 全为预存，与本批文件集零交集，详见实施记录第 6 条）；
5. **覆盖无损失断言**：门控后 host-tests 通过数不得下降。

状态：[X]
详情：已于 2026-09-17 按上述方案实施完成（符号使用点级 host 桩化 + 删除壳 4 占位），5 簇全部落地；P1-P4 收尾专项（负向验证 / extern 声明一致性 / 文档同步 / 全门槛回归含 QEMU boot）+ P2′（裁定 2：三处残余声明一并收拢）已全部完成，验证实测见下方「实施记录（2026-09-17）」。符号契约 **7 → 0** 达成（含声明侧），守卫为**两级**（编译期 / 链接期，均 fail-closed，见裁决表条与 P1 条）。

| 项 | 裁决 | 说明 |
|---|---|---|
| 占位仅为**当前枚举子集**，新符号会再次炸 | **已裁定：随本方案消灭**（存量漏项 3 个实测确认） | 底层条件（host 编译引用汇编/链接脚本符号）由 5 簇使用点桩化直接消除；G7 未覆盖、已被 host 编译路径引用却因 CGU 未共置而未爆的 3 个存量漏项（`_kernel_start` / `_kernel_end` / `stack_bottom`）同批消灭。**新增符号不会再炸**——host 可达路径一旦引用此类符号即**硬失败**（具体层级见下行），不需要枚举清单与豁免表 |
| 两侧无同步机制 | **已裁定：守卫两级化（编译期 / 链接期，均硬失败）；原「链接器即为守卫」表述修正** | 删除壳占位 + P2′ 全声明收拢后，「framework 引用的汇编/链接脚本符号 ↔ host 侧定义」这一原本无人校验的映射不再存在。守卫形态取决于该符号的**声明是否随引用一并 cfg 收拢**：**① 编译期**（声明已收拢 → item 在 host 下不存在，本项目现状：`USER_CR3_SAVE_ASM`、`stack_bottom`、syscall 的 `_kernel_start` / `_kernel_end`、`kpti` 的 `_kernel_text_*`、`boot` 的 `_kernel_end`）报 `error[E0425]: cannot find value … in module`；**② 链接期**（声明保留 → 无定义）报 `rust-lld: error: undefined symbol`。两级均为 fail-closed，且**编译期级更强**（更早暴露）。守卫从"需新写审计脚本枚举两侧"降级为"编译器/链接器内建"，无需维护成本 |
| 语义退化：链接期硬失败 → 运行期静默错值 | **已裁定：消除** | 静默错值的前提是"host 侧存在零值定义"；删除壳 4 占位后该前提不存在，退化路径随之消失，不变量回到"违反即**硬失败**"（编译期优先，层级见上行） |
| 可选收益（已发生，非风险） | **必要非充分（表述修正）** | 占位仅解除了链接障碍，**不解除**语义障碍（host 无 MMU / 无中断状态 / 无 CR3，不可构造页表上下文）。故 [demand_paging_test.rs](host-tests/tests/demand_paging_test.rs) 与 [td22_sigill_delivery_test.rs](host-tests/tests/td22_sigill_delivery_test.rs) 的源码扫描降级在本质因上仍然成立；本方案后 host 侧不再有 `USER_CR3_SAVE`，两处维持降级。恢复真实链接级覆盖属独立工程，**不据此扩大本方案范围** |

### 实施记录（host 链接占位符号残留，2026-09-17）

**改动清单**（与上方 5 簇改法逐条对应）

| 文件 | 改动 |
|---|---|
| [mm/mod.rs](src/kernel/framework/mm/mod.rs) | 簇 1①：`USER_CR3_SAVE_ASM` extern 声明 `all(x86_64, not(host-test))` 门控（连声明排除）；`read_user_cr3_asm` 函数体内三分支（真机 `load` / host `0` / aarch64 回退） |
| [mm/kpti.rs](src/kernel/framework/mm/kpti.rs) | 簇 1②+2+2b：`map_kpti_data_pages` 函数体内整段 cfg（host 分支消费参数）；`kpti_init` step 4.5 与 `map_text_region_in_user_pml4` / `map_text_page` 一并 `not(host-test)` 门控；删除 `_kpti_trampoline_end` 声明；`KERNEL_BASE` 导入按 host-test 分叉；P2′：`_kernel_text_start` / `_kernel_text_end` 声明补 `not(host-test)` 门控 |
| [mm/vmm_x86_64.rs](src/kernel/framework/mm/vmm_x86_64.rs) | 簇 2②：`create_user_page_table` 内 KPTI 同步段整段 cfg（该段即 `_kernel_text_*` 的跨模块引用点，P1 第二处负向验证即临时去此 cfg） |
| [boot/mod.rs](src/kernel/framework/boot/mod.rs) | 簇 3：`boot::init` 的 `kernel_end` 双分支（真机取符号地址 / host `0`）；P2′：`_kernel_end` 声明（块级，块内仅此一项）补 `not(host-test)` 门控 |
| [syscall/mod.rs](src/kernel/framework/syscall/mod.rs) | 簇 4：`kernel_start_ptr` / `kernel_end_phys` 函数体内分叉（host `null` / `0`），调用链零改动；P2 收尾：`_kernel_start` / `_kernel_end` 声明补 `not(host-test)` 门控（cfg 加在**单个 item** 上，同块 `timer_get_ticks` 不受影响） |
| [proc/process.rs](src/kernel/framework/proc/process.rs) | 簇 5：`stack_bottom` extern 声明门控；两个 canary 函数体内分叉（host `true` / 整段跳过） |
| [lib.rs](src/kernel/lib.rs) | 门控语义推广：两处真机引导块 `not(kernel_test)` → `not(any(kernel_test, host-test))`；`kernel_init` 的 `used_underscore_binding` / `unreadable_literal` expect 条件同步推广 |
| [src/rust/src/lib.rs](src/rust/src/lib.rs) | 删除壳 crate 4 个零值占位符号（回退到原 19 行内容，git diff 归零） |
| [scripts/audit_feature_semantics.py](scripts/audit_feature_semantics.py) | 规则文本同步（仅 docstring 追加约定推广说明；判定逻辑未动，检查面仍为 services/ + framework/tests/） |

**实施中发现的附带项（本批内一并处置，属桩化的直接后果）**：host-test 维 clippy 报 10 处 error —— 9 处 `unfulfilled_lint_expectations`（被 cfg 排除的代码不再触发 lint，但 item 级 `#[expect]` 仍注册）+ 1 处 `kpti.rs` 的 `KERNEL_BASE` unused import。修复手段与 J-01 先例一致：`#[expect(...)]` 收窄为 `#[cfg_attr(not(feature = "host-test"), expect(...))]`，import 按 host-test 分叉。**未使用任何 `#[allow(dead_code)]` / 豁免表**（F9）。

**验证实测（P1-P4 收尾专项 + P2′ 收拢后复跑）**

1. **P1 负向验证（改写版：全声明收拢后预期为编译期失败，报错逐字留存）**：
   - 做法：临时移除 [syscall/mod.rs](src/kernel/framework/syscall/mod.rs) `kernel_start_ptr` 的 host 桩分支（真机分支转为无条件），使 host 构建重新引用已收拢的真符号 `_kernel_start`。
   - 实测：`cargo test --manifest-path host-tests/Cargo.toml --test e04_shared_runner_test --no-run` → **exit=101**，报错逐字：
     ```
     error[E0425]: cannot find value `_kernel_start` in this scope
        --> /home/anfer/Code/QueenX/src/kernel/framework/syscall/mod.rs:338:19
         |
     338 |         unsafe { &_kernel_start as *const u8 }
         |                   ^^^^^^^^^^^^^ not found in this scope
     ```
   - **第二处（针对本批新收拢声明的定向验证，证明跨模块引用同样 fail-closed）**：临时移除 [vmm_x86_64.rs](src/kernel/framework/mm/vmm_x86_64.rs) `create_user_page_table` KPTI 同步段的**块级 cfg**，使跨模块引用（`crate::framework::mm::kpti::_kernel_text_*`）在 host 下转 live → **exit=101**，报错逐字（共 3 项，前两项即新收拢符号；rustc 并指认门控位置）：
     ```
     error[E0425]: cannot find value `_kernel_text_start` in module `crate::framework::mm::kpti`
        --> /home/anfer/Code/QueenX/src/kernel/framework/mm/vmm_x86_64.rs:633:69
     error[E0425]: cannot find value `_kernel_text_end` in module `crate::framework::mm::kpti`
        --> /home/anfer/Code/QueenX/src/kernel/framework/mm/vmm_x86_64.rs:636:69
     note: found an item that was configured out
        --> /home/anfer/Code/QueenX/src/kernel/framework/mm/kpti.rs:516:22
         |
     493 | #[cfg(not(feature = "host-test"))]
         |          ----------------------- the item is gated here
     ```
   - **守卫性质（P2′ 后最终表述）**：**两级守卫**——声明随引用收拢的符号在 host 下误引用即**编译期** E0425（早于链接）；仅当存在未收拢声明时才退为链接期 undefined symbol。原 P1 预期（"仅去桩即链接失败"）默认了两个前提（对象必被拉入 + 声明保留），P2′ 后两前提均不成立，故 P1 验收标准同步改写为「**编译期失败**」。**保留的语义精确化**：引用不可达（承载对象未被拉入二进制）时**不报错**——守卫是「违反即硬失败」，不承诺"编译进对象即报错"（该结论来自 P2′ 前的 `nm` 实测：rlib 内 `U _kernel_start` 计 1 处而 e04 二进制零命中）。
   - 撤销：两处临时改动全部回退（`kernel_start_ptr` 恢复双分支、`vmm_x86_64.rs` 恢复块级 cfg）→ 复跑 e04 `--no-run` **exit=0，恢复绿**；全仓 `P1 临时负向验证` 临时标记零残留。
2. **P2 择一结论 + P2′（裁定 2）**：**采纳「加 cfg」并一并收拢三处残余** ——
   - `syscall/mod.rs` 的 `_kernel_start` / `_kernel_end` 声明补 `not(feature = "host-test")` 门控；cfg 加在**单个 item** 上，同块 `timer_get_ticks` 不受影响。
   - **门控判据：按"符号存在性"而非"引用侧函数 gate"**（收尾复核后修正）——这 6 个符号全部由 `x86_64.ld` / `aarch64.ld` 定义（`_kernel_text_*` 亦两架构皆有；`_kpti_trampoline_end` 为 x86_64 独有且已随声明删除），**唯一不存在这些符号的构建是 host-test**（无 ld 脚本产物），故统一取 `not(feature = "host-test")`。此前一度写的 `all(not(kernel_test), x86_64, not(host-test))`（镜像引用侧函数的 gate）已弃用：它把"引用侧函数恰好被 gate 掉"误当作"符号不存在"，方向是把声明收得比事实更窄——在 aarch64 / kernel_test 维该符号**真实存在**，未来若把引用放宽到这两维会得到一次**误报**。现写法与 `stack_bottom`（仅 `not(host-test)`）、`USER_CR3_SAVE_ASM`（arch 项编码"只此架构存在"的事实）同构，符合裁定 2 理由 1（一致性）与理由 2（语义自洽）。
   - P2′：`kpti.rs` 的 `_kernel_text_start` / `_kernel_text_end` 与 `boot/mod.rs` 的 `_kernel_end` 声明亦补 `not(feature = "host-test")` 门控（`kpti.rs` 整模块已受 `target_arch = "x86_64"` 门控，不重复 arch 条件；`boot/mod.rs` 唯一引用点在 `init` 真机分支）。
   - 裁定依据：一致性（与 `mm/mod.rs` / `process.rs` 对齐）+ 语义自洽（声明保留意味着 host 构建仍"宣称"依赖链接脚本符号，与「符号契约 7 → 0」相斥）+ 守卫前移（编译期早于链接期，且条件写错只会是编译期报错，无实质风险）。
   - 原「残余一致性观察（未改）」一项随之**关闭**：三处声明已全部收拢，本方案符号集合内**不再存在保留声明的链接期守卫样本**（此即 P1 验收标准改写的原因）。
   - 复跑结果（P2′ 落地 + 判据修正为"存在性"后）：E-04 `--no-run` **exit=0**；`./ci/audit.sh quick` **exit=0**（双架构 check + clippy lib / kernel_test / host-test 三维全 passed，F4 SAFETY 覆盖缺漏 0；**aarch64 / kernel_test 两维的"未引用声明"零告警已实测**）；`./ci/build.sh all` **Passed 5 / Failed 0**（含 aarch64 release 构建）；`make test-unit` **498/498**（kernel_test 维链接正常，未引用声明不引入符号引用）；`qemu_boot_test.sh x86_64` **1/1**。注：x86_64 真机维在两种判据下声明均存在，真机产物等价，故 boot 语义无差异。
3. **门控语义推广落地**：[lib.rs](src/kernel/lib.rs) **3 处**（`:505` `kernel_init` 的 `used_underscore_binding` cfg_attr 条件、`:661` Boot Info 真机块、`:861` NestFS 挂载块）由 `not(kernel_test)` 推广为 `not(any(kernel_test, host-test))`；[audit_feature_semantics.py](scripts/audit_feature_semantics.py) 仅注释同步，**判定规则未动**（实测 HIGH 清单与基线一致，见第 6 条）。
4. `./ci/build.sh all` → **Passed 5 / Failed 0**（双架构 0w0e + host-tests + forbidden-patterns + x86_64 link）；`./ci/audit.sh quick` → **exit 0**，含 `framework SAFETY 覆盖 (缺漏 0 ≤ 0 基线)`（F4 100%）、`audit_safety_coverage` 之外全部子审计绿。（**P2′ 收拢后复跑，结果一致**——声明收拢动了 TCB，故全门槛重跑。）
5. **覆盖无损失断言** → `make test-host` **99 个 test bin 全 ok、755 tests passed / 0 FAILED**（与门控前基线一致，无下降；**P2′ 后复跑同值**）。
6. **HIGH 清单与 HEAD 基线逐项一致（不得新增/推翻）**：当前基线 **7 项 HIGH**（`services/credo/storage/disk.rs` ×2、`services/fs/io.rs` ×1、`services/net/unix.rs` ×1、`services/syscall/dispatch.rs` ×3）+ 20 项 INFO（`framework/tests/net.rs`、`framework/tests/test_config.rs`、`services/net/mod.rs`）——**与本批桩化文件集（`framework/mm/*`、`framework/boot/mod.rs`、`framework/proc/process.rs`、`framework/syscall/mod.rs`、`src/kernel/lib.rs`）零交集**；实测与 HEAD 影子树基线逐项一致，差异仅 `services/syscall/dispatch.rs` 3 处行号平移（785/898/902 → 826/939/943）。原文「rc=0」系裁定方笔误（未核 HEAD 基线），已按实测修正。
7. **QEMU 三门槛**：`make test-unit` → **ALL 498 TESTS PASSED (0 skipped)**；`./scripts/qemu_boot_test.sh x86_64` → **1/1 通过**（进入 Ring 3 启动 init）；**真机路径证据（非仅编译通过）**——引导日志实测 `Boot stack canary verified`、`Boot info: mem=128 MB, kernel_end=0x3E60000`（非零 → 簇 3 真机分支生效）、`[KPTI] text region: start=0x12B000 end=0x2801B9 (342 pages)`、`[KPTI] map_text_region: …`、`[KPTI] data pages mapped: USER_CR3_SAVE=0x23FD000, …`、`[KPTI] kpti_init: kernel_pml4=0x102000, user_pml4_phys=0x3e60000, …` —— 簇 1/2/3/5 的真机分支均**实际执行**，桩化未侵蚀裸机语义。（**P2′ 后复跑同绿**：498/498 + boot 1/1，日志中 `_kernel_text_*` / `_kernel_end` 取值点均正常输出 → 声明收拢未影响真机构建。）
   - **日志留存归档（reviewer 第三轮要求：可追溯）**：boot 日志已从会被下次运行覆盖的 `build/log/qemu_boot_x86_64.log` 归档为固定名 **`build/log/qemu_boot_x86_64_t1wrap_20260918.log`**（240 行，md5 `ad749d876db2808d053e219b5f91523a`，与归档前逐字节一致）；kernel_test 日志 **`tests/reports/unit_test_20260918_165355.log`**（带时间戳，md5 `b1cf091bf44d3e6419fcd8f6c75f4d17`）。上列逐字证据行**均取自该归档文件**（原引用 `end=0x2805E9` 与本轮归档实况 `end=0x2801B9` 不符，已按归档更正）。注：两目录按 B08-10 既有裁定为 gitignore，日志不入仓 ⇒ 可追溯性＝归档文件 + 逐字行入档。
8. **本轮复核复跑（T1 收尾复核：纯复核，未改任何代码）**：
   - **P2′ 现状核实**：三处残余声明收拢在源码中在位——[mm/kpti.rs](src/kernel/framework/mm/kpti.rs#L167-L171)（`_kernel_text_start` / `_kernel_text_end`，`not(feature = "host-test")`，模块已受 `target_arch = "x86_64"` 门控）、[boot/mod.rs](src/kernel/framework/boot/mod.rs#L114-L117)（`_kernel_end`，块级）、[syscall/mod.rs](src/kernel/framework/syscall/mod.rs#L124-L127)（`_kernel_start` / `_kernel_end`，单 item 级）；判据与 [mm/mod.rs](src/kernel/framework/mm/mod.rs#L17-L22) / [proc/process.rs](src/kernel/framework/proc/process.rs#L38-L41) 同构（符号存在性）。
   - **P1 负向验证复跑**：临时将 `syscall/mod.rs` 的 `kernel_start_ptr` 真机分支转为无条件（去掉 host 桩分支）→ `cargo test --manifest-path host-tests/Cargo.toml --test e04_shared_runner_test --no-run` **exit=101**，逐字报错：
     ```
     error[E0425]: cannot find value `_kernel_start` in this scope
        --> /home/anfer/Code/QueenX/src/kernel/framework/syscall/mod.rs:336:23
     ```
     ⇒ **编译期**失败（非链接期 `undefined symbol`），与改写后的验收标准一致；改动已回退（`git diff --stat` 仅两份文档、临时标记全仓零残留）。
   - **P4 全门槛回归（本轮实测）**：`./ci/build.sh all` → **Passed 5 / Failed 0**（RC=0）；`./ci/audit.sh quick` → **RC=0**（**F4 SAFETY 覆盖 1924 / 1924 = 100%，缺 SAFETY 0**；clippy lib / `kernel_test` / `host-test` **三维全 passed**）；`make test-host` → **RC=0**（101 个测试二进制全 `ok`、合计 **766 passed / 0 FAILED**，对基线 755 **无下降**）；`make test-unit` → **RC=0**（QEMU `kernel_test` **ALL 498 TESTS PASSED**）；`./scripts/qemu_boot_test.sh x86_64` → **RC=0，1/1 通过**（串口 240 行，命中里程碑 `VFS ready`，进入 Ring 3 启动 init）。
   - **P3 文档同步**：本条即同步结果；「守卫两级化（编译期 / 链接期，均 fail-closed）」表述与本轮复跑实测一致，无待改项。

**审核已确认的四项静态/链接证据（裁定方复核结论，本批复核后保持一致）**

- E-04 共享测试集 `e04_shared_runner_test` 链接成功（壳 4 占位删除后仍成功）；
- 该二进制符号表 `_kernel_text*` / `_kpti_trampoline_end` / `USER_CR3_SAVE` / `stack_bottom` / `_kernel_start` **零命中**；
- clippy `kernel_test` 维 + `host-test` 维均 **RC=0**（`host-test` 维含本批新增的 10 处 expect 收窄 + `KERNEL_BASE` import 分叉）；
- **F9**：无 `#[allow(dead_code)]` / 任何死代码豁免；桩体仅三种形态（常量中性返回 / 整段跳过 / 参数消费），无逻辑承载。
- **P2′（声明收拢）复核**：六处符号声明（`USER_CR3_SAVE_ASM`、`stack_bottom`、syscall 的 `_kernel_start` / `_kernel_end`、`kpti` 的 `_kernel_text_*`、`boot` 的 `_kernel_end`）门控判据统一为**符号存在性**（唯一无 ld 脚本产物的 host-test 维排除；`USER_CR3_SAVE_ASM` 另含 `x86_64` 因为该 asm 符号仅 x86_64 有；`kpti.rs` 模块本身即 x86_64 门控）；收拢后 P1 第二处负向验证在**编译期**即失败（报错逐字见第 1 条），证明"声明随引用收拢"未造成声明/引用条件错位。

### 源码核实（2026-09-15）

- sendfile 已实装：`services/fs/sendfile.rs:13`（包装 `framework::syscall::sys_sendfile`）→ **D-2 清单含 sendfile 属过时**，实施前重扫。
- services dispatch 无 read/write/execve/prctl 分支（framework 回退层执行）→ B2 24 未迁移分类准确。
- 用户态暂不用 sendfile/readv/writev/preadv（src/user+userland grep 空）→ 高级项实装优先级可据用户态需求排后。
