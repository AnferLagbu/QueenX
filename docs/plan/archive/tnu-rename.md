# 项目命名迁移规划：TNU 声明式命名（代码/注释/文档）

> **状态：[X] 已否决（2026-09-15）**——命名迁移工程**取消**，保留 QueenX 意象体系（QueenX/NestFS/Chitin 不动）。否决原因：TNU 与 `github.com/tnuproject/tnu`（TNU/Tiramisù，独立类 Unix 内核，定位高度重合）实质性撞名；替代候选 INU（I am not Unix）曾探讨，最终用户决定保留 QX。本文档归档为历史探讨记录（含撞名调研证据，未来若重启命名讨论可参考）。

> 关联：HiveFS→NestFS 改名工程（`d8285613`，已归档）为同类型迁移的先例。
> 范围：**代码 + 注释 + 文档 + 工程脚本/CI**；**`docs/plan/archive/` 已归档内容不处理**（历史快照）；third-party（vendored smoltcp）/ `other/` / `target/` 排除。
> **边界**：README 中 Gitee/GitHub 仓库 URL（`gitee.com/AnferLagbu/QueenX` 等）**由用户自行修改**，委托不处理；仓库目录名/绝对路径（`/home/anfer/Code/QueenX`）保持稳定不改。
> 数据：2026-09-15 全仓扫描（src/kernel + src/rust + src/user + host-tests + scripts + ci + 非 archive 文档）。

## 描述

将项目命名元素从意象/缩写体系迁移至 **TNU 声明式命名**（This is Not Unix 家族）。内核名 QueenX → TNU；syscall 前缀 QX_* → TNU_*（已定）；文件系统/设备框架命名待决（见 §5）。

## 命名映射

| 元素 | 旧 | 新 | 状态 |
|---|---|---|---|
| 内核名 | QueenX / queenx / QUEENX | TNU / tnu / TNU | **已定** |
| syscall 前缀 | QX_*（32 个独有） | TNU_* | **已定**（编号不变，ABI 稳定） |
| 文件系统 | NestFS / nestfs | TAFS / tafs | **已定** |
| 设备框架 | Chitin / chitin | TDF / tdf（This is Drivers Framework） | **已定** |
| uname/version | "QueenX 0.1.0 (queenx)" | "TNU 0.1.0 (tnu)" | 已定 |
| crate 名 | queenx / queenx-host-tests / queenx-tests | tnu / tnu-host-tests / tnu-tests | 已定 |

## 迁移范围清单（全仓扫描数据）

### 1. 代码层（src/，含注释内标识符）

| 命名 | 计数 | 位置要点 |
|---|---|---|
| QueenX/queenx | 143 处 | crate 名、uname/version 字符串、boot banner、config 日志 |
| NestFS/nestfs | 350 处 | services/fs/nestfs/ 模块 + 类型/字符串 |
| Chitin/chitin | 326 处 | framework/chitin/ 模块 + ChitinDevice/ChitinOps/ChitinProto 类型 + 字符串 |
| **目录名** | nestfs/ → tafs/、chitin/ → tdf/ | 模块目录（git mv，参照 HiveFS 先例：hvfs/ → nestfs/） |
| QX_* 常量 | 32 个 | framework/syscall/types.rs:458-608（编号 730-898） |
| QX_* 引用 | dispatch 分支 + 文档注释 | framework/services syscall 层（用户态 0 引用） |
| 用户可见品牌 | ~10 处 | [info.rs:82](uname)、[procfs_core.rs:340](/proc/version)、[lib.rs:522/891](boot)、wizard 文案 ×3 |

**工程层（补充扫描 2026-09-15）**：

| 目录 | 计数 | 内容 |
|---|---|---|
| host-tests/ | 155 处 | crate 名 queenx-host-tests + 20 文件测试注释 |
| src/rust/ | 49 处 | crate 名 queenx/queenx-tests + 构建配置 |
| src/user/ | 27 处 | wizard + fbterm 终端 banner + httpsrv HTTP 页面/响应头 + **fstype 字符串 `b"nestfs"`（wizard/mod.rs:16，P3 ABI 同步点）** |
| scripts/ | 28 处 | 审计脚本 argparse 描述 + qemu_debug/requirements banner + "queenx crate" 引用 + `smoltcp-localization/apply.sh:31` 绝对路径 |
| ci/ | 4 处 | CI yml 引用 |

### 2. 文档层（非 archive）

| 命名 | 计数 | 位置 |
|---|---|---|
| QueenX | 180 处 | docs/plan/*.md + docs/explain/*.md + AGENTS.md + README |
| QX_ | 66 处 | 同上（syscall 文档引用） |
| NestFS | 113 处 | 同上 |
| Chitin | 73 处 | 同上 |

**排除**：`docs/plan/archive/`（历史快照，含 netops/kernel-crate-separation/hvfs-rename/syscall-dispatch-cleanup 等已归档文档）。

## 批次计划

| 批 | 内容 | 验证 |
|---|---|---|
| **P1 品牌元数据** | uname/version 字符串、boot banner、config 日志、wizard 文案、crate 名（queenx→tnu + 产物名）——**明细见 §6 用户可见位置裁决表** | 双架构 0w0e + QEMU boot（uname/日志冒烟） |
| **P2 syscall 前缀** | 32 个 QX_* 常量 → TNU_* + types.rs/dispatch 分支/文档注释同步（编号不变） | 双架构 0w0e + host-tests + QEMU（syscall 路径） |
| **P3 组件名** | NestFS→TAFS、Chitin→TDF（**已定**）——**目录名**（nestfs/→tafs/、chitin/→tdf/，git mv）+ 模块路径 + 类型前缀（ChitinDevice→TdfDevice 等）+ 字符串，参照 HiveFS 改名流程。**关键 ABI 同步**：fstype 字符串 `b"nestfs"`（内核 mount 注册 + [wizard/mod.rs:16](`fs_mount(b"nestfs")`)+ prepare.rs 文案）同 commit 改 `b"tafs"` | 双架构 0w0e + clippy + host-tests + **QEMU 挂载冒烟**（fstype 生效） + 残留清零 |
| **P4 文档** | docs/plan + docs/explain + AGENTS.md + README 品牌替换（archive 不动） | 文字核验 |
| **P5 注释清理** | 代码注释中残留品牌词（cpu/mod.rs、ipc/strategy.rs 等 QX 注释引用） | 残留扫描清零 |
| **P6 工程引用** | scripts/ci 中 queenx crate 名/品牌引用（audit 脚本 argparse 描述、qemu_debug/requirements banner、`audit_unwired_pub_fn.py` "queenx crate" staticlib 名）+ **审计脚本模块路径**（audit_services_boundary.py:129/441/442、audit_coupling.py:112 的 `framework::chitin` 白/黑名单——chitin→tdf 须同步否则边界审计失效）+ host-tests/src/rust crate 名同步 | 脚本语法 + 审计 selftest + 全验证链复跑 |

## 待决点

1. **节奏**：立即实施，或等 syscall 后续工程（T1-T7）完成后（避免与 dispatch 改动冲突）？

## 用户可见位置裁决表（已定）

| 位置 | 当前 | 裁决 |
|---|---|---|
| uname sysname（info.rs:79） | `"QueenX"` | → `"TNU"`（现状已品牌化，无 Linux 兼容取舍） |
| uname nodename（info.rs:80） | `"queenx-node"` | → `"tnu-node"` |
| uname release（info.rs:81） | `"0.1.0"` | 保留（版本号） |
| uname version（info.rs:82） | `"QueenX 0.1.0 (queenx)"` | → `"TNU 0.1.0 (tnu)"` |
| UtsNamespace 默认主机名（namespace.rs:143） | `"QueenX"` | → `"TNU"` |
| /proc/version（procfs_core.rs:340） | `"QueenX version 0.1.0 (queenx@build)..."` | → `"TNU version 0.1.0 (tnu@build)..."` |
| boot banner（lib.rs:522/891） | `"QueenX starting"` / `"QueenX initialized"` | → TNU 版 |
| config 日志（config/mod.rs:216） | `"==== QueenX Configuration ===="` | → TNU 版 |
| klog sinks（klog.rs:154） | `"QueenX klog sinks"` | → TNU 版 |
| 测试框架 banner（tests/mod.rs:406） | `"=== QueenX Test Framework ==="` | → TNU 版 |
| 向导标题（wizard/mod.rs:27/30） | `"QueenX Installation Wizard"` / `"Welcome to QueenX OS"` | → TNU 版 |
| 向导完成（finish.rs:23） | `"QueenX has been installed..."` | → TNU 版 |
| fbterm 终端 banner（src/user/fbterm/main.rs:148/196） | `"fbterm v0.2 \| QueenX User-Space Terminal"` / `"*** QueenX fbterm v0.2 ***"` | → TNU 版 |
| httpsrv 响应头（src/user/httpsrv/main.rs:54） | `"Server: QueenX-httpsrv/0.1"` | → `"Server: TNU-httpsrv/0.1"` |
| httpsrv HTML 页面（main.rs:119-142） | `"QueenX HTTP Server"` / `"About QueenX"` 等 | → TNU 版 |

## 方案

1. 按 P1→P5 批次顺序执行（每批独立验证，P3 依赖 P1 的品牌基调定稿）。
2. syscall 前缀改名**不改变编号**（730-898 区不动），纯常量名/引用源码级替换——B09-17 编号治理成果不受影响。
3. crate 名迁移需同步 Makefile/CI/脚本引用（queenx → tnu）。
4. 已归档文档（archive/）不改，仅未来归档时自然过时。

## 状态

[ ] 待决（§5 四项拍板后实施）

## 详情

### 源码核实（2026-09-15）

- QX_* 独有 syscall 32 个，编号 730-898（FW/GET_CANARY/FTRACE/KGDB/ROUTE/NF/CGROUP/PM/SECURE_BOOT/TPM/CET/TICKLESS/TIMESYNC/UEFI/IO_URING_SUBMIT/SNAPSHOT），定义集中在 `framework/syscall/types.rs:458-608`。
- 用户态（src/user）对 QX_* **0 引用**——迁移仅动内核定义 + dispatch + 文档。
- 蚂蚁意象元素全仓仅 3 个命名点（QueenX/NestFS/Chitin），无生态词（colony/pheromone 等 0 处）——意象体系稀疏，迁移影响面可控。
- HiveFS 改名先例：git mv 38 文件 + ~2050 处替换 + 全量验证，TNU 迁移规模相近（~1100 处 + crate 名 + 文档）。
