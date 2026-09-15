# HiveFS 全面改名 NestFS 工程

> 关联：`docs/explain/ref-naming.md` / `docs/explain/vision-hope.md`（命名与愿景依据）。
> 背景：HiveFS（超虚拟文件系统）命名源自"蜂巢（hive）"隐喻，与本内核"蚂蚁（QueenX）"隐喻冲突；定名 NestFS（巢，蚂蚁巢穴）。camelCase 简称 `HvFS` 弃用（`nestfs` 6 字符直接使用，无需简写，2026-09 用户裁决）。

## 描述

HiveFS 全面改名。按语义分两形：

- **`NestFS`** — 正式名称（camelCase）。用于注释、文档、日志、README 中的人类可读名称。
- **`nestfs`** — 标识符（全小写）。用于 Rust 模块名、目录/文件名、函数/类型名、fstype 字符串。

所有旧名变体（HiveFS / HvFS / Hivefs / hivefs / Hvfs / hvfs）全部消除（archive 除外）。

## 方案

### 1. 语义映射规则

| 旧 | 新 | 语义 | 示例 |
|---|---|---|---|
| `HiveFS` | `NestFS` | 正式名称（注释/文档/日志） | "Root filesystem: NestFS" |
| `HvFS` | `NestFS` | camelCase 简称 → 统一正式名 | "HvFS mount failed" → "NestFS mount failed" |
| `hivefs` | `nestfs` | 全小写标识符 | 目录/文件名 |
| `hvfs` | `nestfs` | 模块名/路径/字符串 | `mod nestfs`、`services::fs::nestfs`、`b"nestfs"` |
| `Hvfs`（前缀） | `Nestfs`（前缀） | camelCase 标识符 | `HvfsInode` → `NestfsInode` |
| `hvfs_xxx` | `nestfs_xxx` | snake_case 标识符 | `get_hvfs` → `get_nestfs`、`hvfs_data` → `nestfs_data` |

### 2. 改动范围

**A. 目录/文件重命名（git mv）**

| 旧 | 新 | 备注 |
|---|---|---|
| `src/kernel/services/fs/hvfs/`（~20 文件） | `src/kernel/services/fs/nestfs/` | 主实现 |
| `src/kernel/framework/fs/hvfs/`（mod.rs + arc_safe.rs） | `src/kernel/framework/fs/nestfs/` | framework 侧 |
| `src/kernel/framework/tests/test_hvfs.rs` / `test_hvfs_ext.rs` | `test_nestfs.rs` / `test_nestfs_ext.rs` | |
| `host-tests/tests/hvfs_test.rs` / `hvfs_e2e_test.rs` / `hvfs_stress_test.rs` / `hvfs_persist_test.rs` / `hvfs_trait_abstract_test.rs` | `nestfs_*.rs` | 5 文件 |
| 文件内 `hvfs.rs` / `hvfs_data.rs` / `hvfs_inode.rs` → `nestfs.rs` 等 | 同上 | services/fs/nestfs/ 内 |

**B. 内容替换（Rust 标识符 + 注释 + 字符串）**

- 模块声明：`pub mod hvfs` → `pub mod nestfs`（services/fs/mod.rs、framework/fs/mod.rs、hvfs/mod.rs）
- 引用路径：`crate::kernel::services::fs::hvfs::...` → `nestfs::...`（lib.rs、syscall/dispatch.rs、vfs/*、barrier/*、driver/*、mm/api.rs、proc/fd_alloc.rs、dma/api.rs、chitin/mod.rs、tests/* 等 54+ 文件）
- 函数/类型/常量：`get_hvfs` → `get_nestfs`、`HvfsInode` → `NestfsInode`、`HVFS_*` → `NESTFS_*`（若存在）
- 注释/文档/日志：`HvFS`/`HiveFS` → `NestFS`（98 处/27 文件）
- 测试：host-tests 5 文件 + framework/tests 2 文件（~370 处）

**C. fstype ABI 字符串（行为同步，关键）**

- 用户态挂载/格式化：`b"hvfs\0"` → `b"nestfs\0"`（src/user/install/src/wizard/mod.rs:16、prepare.rs；src/user/lib/src/sys.rs）
- **内核 mount 注册必须同步**：services/fs/mount.rs 注册的 fstype 名（当前 `hvfs`）→ `nestfs`，否则 mount 失败。内核 + 用户态 + 相关测试需同步验证。

**D. 文档**

- `docs/explain/ref-naming.md` / `vision-hope.md` / `guide-dev.md` / `ref-lock-order.md`：HvFS/HiveFS → NestFS，并登记改名决议
- `docs/plan/` 当前非 archive 文档（framekernel-paradigm-enforcement.md 48 处、eliminate-parallel-implementations.md 26 处、audit-fix-09 13 处等）：机械替换 + 标注改名
- `host-tests/README.md`、根 README（若有）

**E. 性能基线**

- `host-tests/benches/baseline.json`：`category: "hvfs"` → `"nestfs"`（8 处）。**注意**：改名后与历史基线失去可比性，baseline 数据需按新 category 重测/更新（AGENTS.md §8：性能基线每次 PR 更新）。

### 3. 排除项

- `docs/plan/archive/**`：历史快照，**不改**（AGENTS.md §6）
- `target/` / `build/`：构建产物
- `other/**`（asterinas / smoltcp 第三方）：无关
- `.git/`：历史 commit 承载旧名，不动

### 4. 执行顺序

1. 重命名目录/文件（git mv）
2. 更新模块声明 + 全部引用路径（编译驱动）
3. 替换名称类（HvFS/HiveFS → NestFS）+ fstype 字符串（内核 mount + 用户态同步）
4. 更新测试文件内容
5. 更新文档（explain/plan 非 archive）+ baseline.json
6. 全量验证（§2.3 门槛）

### 5. 验证门槛

- 双架构 `cargo check --release` 0 error / 0 warning
- clippy 0 warning
- 核心审计全通过
- host-tests 全量（hvfs 测试 5 文件 + 既有集成测试）
- **QEMU boot**：文件系统挂载路径（fstype `nestfs`）——必跑
- grep 残留清零：`(?i)hvfs|(?i)hivefs` 仅剩 `docs/plan/archive/**`（历史快照）

## 状态

[X] 实施完成（2026-09-14，AI 实施，用户审查）

### 实施记录

- **重命名**：git mv 38 文件（services/fs/hvfs/ → nestfs/ 目录含 hvfs.rs→nestfs.rs 等 3 文件级、framework/fs/hvfs/ → nestfs/、framework/tests 2 文件、host-tests 5 测试文件）。
- **内容替换**（批量脚本，三轮补全）：① 常规变体（HiveFS/HvFS/hivefs/hvfs/Hvfs/HvFs/HVFS）967 处/97 文件；② 补充大写常量 HVFS_/HvFs 34 处/12 文件；③ **Hv 前缀类型标识符**（HvZil/HvBlockPointer/HvDmuObject/HvArcKey/HvTxgState 等 30+ 类型）1047 处/39 文件。全变体残留清零（archive/元文档/产物除外）。
- **fstype ABI**：`b"hvfs"` → `b"nestfs"`（wizard/sys 用户态 + 内核 mount 注册同步）；QEMU boot 日志实证 `[NestFS] Initializing...` 生效。
- **baseline.json**：category `hvfs` → `nestfs`（用户裁决 A，重置基线）。
- **验证全绿**：双架构 `cargo check --release` 0w0e ✅ / clippy `-D warnings` 双架构 0 ✅ / host-tests 99 套件全过 ✅ / QEMU x86_64 冒烟 1/1（VFS ready + NestFS 初始化 + Ring 3 init）✅ / 残留 grep 清零 ✅。
- **flaky 观察**：host-tests 首次全量偶发 `nestfs_test` 2 用例失败（seek EINVAL/-22），重跑 + 单独跑 + stash 改名前基线对比均通过——判定与改名无关的偶发 flaky（项目有 fsx flaky 预存先例），未引入回归。
- **排除确认**：docs/plan/archive/**（历史快照）、target/产物、other/**（第三方）未改；规划文档自身保留旧名作对照。

## 详情

### 位置清单（2026-09-14 全仓统计，排除 archive/target/other）

| 范围 | 处数 | 文件数 | 说明 |
|---|---|---|---|
| `src/`（含 kernel/services/framework + rust/lib.rs + user） | 413 | 64 | 其中 `HvFS`/`HiveFS` 名称 98 处/27 文件；`hvfs` 标识符 288 处/54 文件 |
| `host-tests/` | 422 | 15 | 含 5 个 hvfs 测试文件 + baseline.json + README + Cargo.toml 注释 |
| `docs/`（非 archive） | ~170 | ~25 | archive 417 处/40 文件排除（历史快照） |
| `scripts/` / `ci/` / README 根 | 待实施时补充扫描 | — | 含挂载冒烟断言等 |

### 关键风险

1. **fstype ABI**：`hvfs` → `nestfs` 是用户可见行为变更（mount/format 系统调用参数），内核注册与用户态调用点必须**同一 commit 同步**，否则运行期挂载失败。QEMU 冒烟兜底。
2. **baseline.json**：category 改名使历史性能基线不可比，需重置基线数据（或用户裁决保留旧 category 名）。
3. **host-tests 引用量**：hvfs_test.rs 289 处等大文件机械替换，依赖编译 + 测试兜底。
4. **module_inception 注释**（lib.rs:47）：`fs/hvfs/hvfs.rs` 惯例注释同步为 `fs/nestfs/nestfs.rs`。
