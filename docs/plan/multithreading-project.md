# 多线程模型工程（独立大工程）

> **定位**：独立工程，非分册 8 内容。因涉及线程模型深入设计（调度器统一 / 线程组结构 / 共享资源语义 / 同步原语迁移），需要专门的设计过程与演进路径。
>
> **目标**：QX 从"多进程单线程"演进为 **1:1 多线程模型**（每用户线程 = 一个内核线程，进程 = 线程组 tgid）。与 Linux NPTL 对齐。
>
> **来源**：分册 8 审查中识别（G-17 多线程失效条件 + 双轨调度器结构性事实），2026-09-08 用户决策独立成档。
>
> **关联**：G-17（Mutex 重入检测 owner PID → 线程指针，本工程子项）。

## 1. 决策记录（2026-09-08 用户确认 D1-D6）

| 决策点 | 选择 | 说明 |
|---|---|---|
| D1 调度统一路线 | **A 统一线程级** | SCHEDULER_EX 为唯一调度器，SCHEDULER（PID 级 DL/RT/CFS）逻辑迁移到线程维度 |
| D2 G-17 owner 迁移 | **线程指针** | Mutex owner = `*mut Thread`（唯一标识，TID 可复用）|
| D3 实施复杂度 | **循序渐进的相对完整** | 分阶段实施相对完整语义，每阶段验证 |
| D4 资源共享 | **CLONE_VM + FILES + SIGHAND + FS + CLONE_PARENT_SETTID 等** | 完整 Linux clone 共享语义（pthread 必需）|
| D5 tgid 结构 | **独立 ThreadGroup 结构** | 显式组对象，含组内线程列表 |
| D6 tick 错位 | **一并处理** | 随 D1 调度统一时理顺 tick 链路 |

**线程映射模型**（2026-09-08）：**1:1**（每用户线程 = 内核 Thread），与 Linux NPTL 对齐。超轻量并发留待用户态协程库（独立，不与内核线程模型耦合）。

## 2. 调研结论（2026-09-08，源码依据）

- **双轨调度器**：SCHEDULER（[scheduler.rs:1451](../../src/kernel/framework/proc/scheduler.rs#L1451)，PID 级 DL/RT/CFS 生产主路径）+ SCHEDULER_EX（[scheduler_ex.rs:900](../../src/kernel/framework/proc/scheduler_ex.rs#L900)，TID/Thread 级，生产下仅 idle 孤岛）；[scheduler.rs:590-592](../../src/kernel/framework/proc/scheduler.rs#L590-L592) 把 PID 单向写入 SCHEDULER_EX.current（伪同步）。
- **Thread 基础设施孤岛**：[thread.rs:240](../../src/kernel/framework/proc/thread.rs#L240) `create_thread` 全仓库零调用；`THREAD_MANAGER.set_current/exit_current` 零调用；Thread 已具 tid/pid/context_ptr/独立栈/环形队列骨架。
- **CLONE_THREAD 未处理**：[clone.rs:38](../../src/kernel/framework/syscall/clone.rs#L38) 只定义常量；CLONE_VM 分支创建独立 PID 的共享 CR3 进程（[clone.rs:150,163](../../src/kernel/framework/syscall/clone.rs#L150-L163)）。
- **无 tgid/线程组**：[process.rs:107-255](../../src/kernel/framework/proc/process.rs#L107-L255) 无 tgid/thread_group/线程列表；`sys_gettid` = getpid（[info.rs:22-24](../../src/kernel/framework/syscall/info.rs#L22-L24)）。
- **资源不共享**：fd_table/sigaction_table 挂 Process（[process.rs:167,189](../../src/kernel/framework/proc/process.rs#L167-L189)）。
- **tick 错位**：PID 级 SCHEDULER.tick()（CFS vruntime/睡眠/zombie）未接入生产 timer，实际 timer 驱动线程级 SCHEDULER_EX.tick()（[sched_ops.rs:94-96](../../src/kernel/framework/proc/sched_ops.rs#L94-L96)）。
- **已登记线索**：code-audit:3826-3848 P1-A exit_group 线程组方案（DECISION-H21 → P1-L 未实施）。

## 3. 深入设计主题（独立设计过程，先设计后实施）

> 本工程核心价值在于**深入设计**，以下主题需在实施前完成设计论证（设计文档 / 决策记录 / 方案对比），不直接进入施工。

### 3.1 线程组模型设计（D5）

- ThreadGroup 结构：tgid、组内线程列表、共享资源引用（fd_table/sigaction_table/fs）
- Process 与 ThreadGroup 关系：进程 = 组长线程 + ThreadGroup？
- tgid 分配：复用 PID 空间（组长 PID = tgid）还是独立？
- 生命周期：组内最后一个线程退出 → 组销毁；孤儿收养语义
- 设计待办：ThreadGroup 与 Process 的边界（Process 是否退化为纯容器？）

### 3.2 调度器统一设计（D1=A）

- SCHEDULER（DL/RT/CFS）迁移到线程维度：调度字段（cfs_vruntime/rt_priority/dl_*）从 Process 迁 Thread
- SCHEDULER_EX 合并/废弃策略：并入 SCHEDULER 还是 SCHEDULER 重写于 SCHEDULER_EX 之上？
- current 统一：线程为唯一调度单位，进程 = 单线程线程组特例
- tick 链路理顺（D6）：timer IRQ → 唯一调度器 tick（含 CFS vruntime/睡眠/zombie）
- 负载均衡：现 SCHEDULER 负载均衡迁移到线程维度
- 设计待办：策略-机制分离（SchedDecision trait）如何与线程维度融合

### 3.3 共享资源语义设计（D4）

- CLONE_FILES：fd 表引用计数共享；FD 分配/关闭/dup 的组内可见性
- CLONE_SIGHAND：sigaction 表共享；信号投递到组内线程 vs 特定线程
- CLONE_FS：cwd/umask 共享；chdir 组内可见性
- CLONE_PARENT_SETTID/CHILD_SETTID/CLEARTID：glibc pthread 栈管理依赖
- 设计待办：共享资源的引用计数 + 并发访问；与 fd_alloc/信号子系统交互

### 3.4 同步原语迁移（D2）

- Mutex owner：PID → `*mut Thread`（线程指针）
- 其他基于 PID 的同步（pi_mutex、rwlock owner 等）审计
- 线程级 current 的获取路径（依赖 K-04 调度统一）
- 设计待办：全同步原语 owner 标识审计清单

### 3.5 线程组系统调用语义

- exit_group：组内全部线程终结（P1-A 方案实装）
- 信号组广播：kill 到 tgid；tgkill/pthread_kill 到特定线程
- waitpid：线程组语义
- gettid 修正：返回线程 TID（当前 = getpid）
- 设计待办：信号投递模型（组投递 vs 线程投递）

## 4. 实施阶段（循序渐进，每阶段 = 状态 [X] + 验证门槛）

- **K-01. 线程组基础结构（tgid/ThreadGroup）**
  - 描述：独立 ThreadGroup 结构（D5），含 tgid、组内线程列表、共享资源引用（fd_table/sigaction_table/fs）。
  - 方案：Process 关联 ThreadGroup；Thread 挂组；tgid 分配；`sys_gettid` 返回线程 TID（修正当前 = getpid）。
  - 状态：[]
- **K-02. CLONE_THREAD 语义实现（线程创建接线）**
  - 描述：clone.rs 处理 CLONE_THREAD——共享 tgid、加入线程组、独立 TID/内核栈/context；`create_thread`（[thread.rs:240](../../src/kernel/framework/proc/thread.rs#L240)）接线进 clone 路径（当前零调用孤岛）。
  - 方案：clone(CLONE_THREAD|CLONE_VM|...) 创建真实线程而非"共享 CR3 的进程"；CLONE_PARENT_SETTID 等标志处理（D4）。
  - 状态：[]
- **K-03. 共享资源语义（CLONE_FILES/SIGHAND/FS）**
  - 描述：线程共享 fd 表/信号处理/fs 信息（D4，pthread 必需）。当前 fd_table/sigaction_table 挂 Process（[process.rs:167,189](../../src/kernel/framework/proc/process.rs#L167-L189)），线程无法共享。
  - 方案：引用计数共享（Linux clone 语义）；线程组内共享访问。
  - 状态：[]
- **K-04. 调度统一（D1=A：统一线程级）**
  - 描述：SCHEDULER（PID 级 DL/RT/CFS）迁移到线程维度——调度字段（cfs_vruntime/rt_priority/dl_*）从 Process 迁 Thread；SCHEDULER_EX 合并/废弃；current 统一为线程；**tick 错位一并理顺（D6）**。
  - 方案：进程 = 单线程线程组特例；Linux CFS/EEVDF 线程级调度语义。
  - 状态：[]
- **K-05. G-17 迁移：Mutex owner → 线程指针（D2）**
  - 描述：Mutex 重入检测 owner 从 PID 迁移 `*mut Thread`（线程指针，唯一标识，TID 可复用）。依赖 K-04（线程 current 就绪）。
  - 方案：`raw_lock` owner 比较改线程指针；mutex_reentrant 测试更新；消除"PID 重入误判"架构假设（单线程模型依赖解除）。
  - 状态：[]
- **K-06. 线程组语义系统调用**
  - 描述：exit_group（组内全部线程终结，非仅当前进程，[dispatch.rs:375-378](../../src/kernel/services/syscall/dispatch.rs#L375-L378) SIMPLIFIED 修正）；信号组广播（kill 到 tgid）；tgkill/pthread_kill；waitpid 线程组语义；gettid 修正（K-01）。
  - 方案：按 P1-A（code-audit:3826）方案实装。
  - 状态：[]
- **K-07. 测试与验证收口**
  - 描述：kernel_test 多线程用例（创建/共享/exit_group/信号广播）+ host-tests 共享套件 + 多线程压力测试 + G-17 mutex_reentrant 多线程场景。
  - 方案：§2.3 五条门槛 + 专项多线程压力；QEMU 双架构回归。
  - 状态：[]

### 依赖关系

```
K-01 → K-02 → K-03 → K-04 → K-05
              ↘      └────→ K-06 → K-07
```

- K-01/02 可先行（不依赖调度统一）；K-05 依赖 K-04
- K-03 依赖 K-02（共享需先有线程组）；K-06 依赖 K-01~04

## 5. 验证门槛（每阶段不可豁免）

- 双架构 `./ci/build.sh all` 0w0e + clippy 0 warning + 核心审计 F1-F9（含 audit_deadlock_matrix 锁顺序）
- host-tests 全量 + QEMU kernel_test 回归
- K-04 调度统一后：进程调度语义不回归（现有单进程用例全绿）
- K-05 G-17 迁移：mutex_reentrant + 多线程并发互斥用例通过

## 6. 关联与风险

- **G-17**：本工程子项（K-05），owner PID → 线程指针，解除单线程模型架构假设（当前模型下现有修复正确，无需回退）
- **风险**：TCB 内调度器/同步原语重构，需完整回归 + 压力测试；分阶段降险（K-01~03 先做线程功能，K-04 再动调度）
- **依赖**：无外部依赖；可独立启动

## 7. 变更历史

- **2026-09-08**：创建本工程（从分册 8 独立成档）。用户决策 D1-D6 确认；线程映射模型定为 1:1；调研结论登记；阶段拆分 K-01~K-07 建立。
