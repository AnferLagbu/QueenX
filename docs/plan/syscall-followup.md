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
- [ ] T1：R2 未实装 SYS_* 实装（G1/G2/G3 完成，见下方 T1 实施记录；G4-G7 待做）
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
7. **QEMU 三门槛**：`make test-unit` → **ALL 498 TESTS PASSED (0 skipped)**；`./scripts/qemu_boot_test.sh x86_64` → **1/1 通过**（进入 Ring 3 启动 init）；**真机路径证据（非仅编译通过）**——引导日志实测 `Boot stack canary verified`、`kernel_end=0x3E60000`（非零 → 簇 3 真机分支生效）、`[KPTI] text region: start=0x12B000 end=0x2805E9 (342 pages)`、`[KPTI] map_text_region: …`、`[KPTI] data pages mapped: USER_CR3_SAVE=0x23FD000`、`[KPTI] kpti_init: kernel_pml4=0x102000, user_pml4_phys=0x3e60000` —— 簇 1/2/3/5 的真机分支均**实际执行**，桩化未侵蚀裸机语义。（**P2′ 后复跑同绿**：498/498 + boot 1/1，日志中 `_kernel_text_*` / `_kernel_end` 取值点均正常输出 → 声明收拢未影响真机构建。）

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
