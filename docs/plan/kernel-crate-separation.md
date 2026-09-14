# kernel 独立 crate 化工程（方案 D：RA `#[path]` 误报根治）

> 关联：`docs/plan/framekernel-paradigm-enforcement.md` §10 方案 D 登记。
> 参照：Asterinas workspace 多 crate 模型（kernel 壳 + kernel/core + comps + libs，正常 `use` 依赖，无 `#[path]`）。
> 现状：`src/rust/src/lib.rs:188` `#[path = "../../kernel/mod.rs"] pub mod kernel;` 使 rust-analyzer 报 `unresolved module`（跨 crate 目录 `#[path]` 是 RA 已知解析缺陷）；cargo 双架构 0 error（路径实际有效）。

## 描述

将 kernel 从 queenx crate 的 `#[path]` 内嵌模块改为 **workspace 独立 crate**，lib.rs 以正常 `use`/`pub use` 依赖，根治 RA `#[path]` 误报。最小可行版：kernel 整体一个独立 crate（framework/services 保持其内部模块树），**不**拆多 crate（Asterinas 式 60 crate 为后续演进方向，非本工程）。

## 方案

### 1. 结构设计（最小改动）

```
src/kernel/
├── Cargo.toml          # 新增：package "kernel"（edition 2024, MPL-2.0）
├── lib.rs              # mod.rs 改名 lib.rs（crate 根，内容不变）
├── framework/          # 不动
└── services/           # 不动

src/rust/
├── Cargo.toml          # queenx：+kernel path 依赖 + feature 转发；无 workspace（最小）
└── src/lib.rs          # 移除 #[path] pub mod kernel → pub use kernel; 顶层约束门控迁出
```

- **kernel Cargo.toml**：`[lib] name = "kernel", path = "lib.rs", crate-type = ["staticlib", "rlib"]`；`[features]` 从 queenx 迁移（host-test / kernel_test / fault_injection / net / alloc / smp / async / lock_stats / debug_mutex 等）；`[dependencies]` 迁移 kernel 侧依赖（spin 等 framework 依赖）。
- **queenx Cargo.toml**：`kernel = { path = "../kernel" }`；feature 显式转发（cargo 不自动继承）：`host-test = ["kernel/host-test"]`、`kernel_test = ["kernel/kernel_test"]` 等。
- **re-export 兼容**：queenx lib.rs `pub use kernel;` → host-tests 的 `queenx::kernel::`（实测 29 文件 / 88 处）**无需改动**（RA 对 re-export 正常解析）。

### 2. 顶层约束门控迁移（B08-12 产物，必须同步）

`src/rust/src/lib.rs` L4-6 现门控：`#![cfg_attr(not(feature = "host-test"), no_std/no_main/feature(alloc_error_handler))]` + `panic_handler`/`alloc_error_handler` 的 `cfg(not(host-test))` → 全部迁到 **kernel lib.rs**（以 kernel 的 host-test feature 门控）。queenx lib.rs 保留壳层所需。

### 3. 路径改写（核心，~3400 处）

| 位置 | 改写 | 量 | 备注 |
|---|---|---|---|
| src/kernel 内部 | `crate::kernel::` → `crate::` | **3300 处 / 501 文件** | 实测全部带 `::`（`crate::kernel` 裸引用 0 处），无边界问题；kernel 变 crate 根后 `crate::kernel::framework` → `crate::framework` |
| src/rust/src/lib.rs | `crate::kernel::` → `kernel::` | 106 处 | queenx 壳 + re-export |
| host-tests | 不改 | 0 | `queenx::kernel::` 经 re-export 兼容 |

### 4. 构建联动

- **Makefile**：`RUST_LIB` 现取 `src/rust/target/<triple>/release/libqueenx.a` → kernel 独立后 staticlib 由 kernel crate 产出（libkernel.a）。为保持 Makefile/CI 简单：**统一 target-dir**（`CARGO_TARGET_DIR` 指向 `src/rust/target`），Makefile 改取 libkernel.a。
- **build.rs**（src/rust/build.rs）：实测 0 kernel 引用，不动。
- **CI yml**：queenx manifest 路径不变（src/rust/Cargo.toml）；方案 B 的 `--config build-std` 注入作用于裸机 target，kernel 作为依赖自动编译 ✅。
- **host-tests**：path 依赖 queenx 不变 + re-export 兼容。

### 5. 批次计划

| 批次 | 内容 | 验收 |
|---|---|---|
| **D1 结构搭建** | kernel Cargo.toml + mod.rs→lib.rs + features/deps 迁移 + queenx path 依赖 + `pub use kernel` + 顶层约束门控迁移 | queenx 编译通过（kernel 作为依赖被编译，`#[path]` 暂留或先删） |
| **D2 路径改写** | kernel 内部 3300 处（`crate::kernel::`→`crate::`）+ lib.rs 106 处（→`kernel::`），移除 `#[path]` | 双架构 `cargo check --release` 0w0e |
| **D3 构建联动** | Makefile libkernel.a 路径 + 统一 target-dir + CI yml 确认 + host-tests re-export 验证 | Makefile/QEMU 链接链、host-tests 全量 |
| **D4 验证** | §2.3 全门槛 + RA 误报消失确认 | 见验证门槛 |

### 6. 验证门槛

1. 双架构 `cargo check --release` 0 error / 0 warning
2. clippy `-D warnings` 双架构 0
3. 核心审计全过（boundary / coupling / safety / deadlock / tcb 等）
4. host-tests 全量（re-export 兼容）
5. **QEMU boot**（静态链接路径变化，必跑）
6. **RA 误报消失确认**：IDE 打开 src/rust/src/lib.rs，`unresolved module` 不再报（`#[path]` 已移除）

## 状态

[X] 实施完成（2026-09-14，实施：AI）

### 实施记录（D1→D4 全绿）

- **D1 结构搭建** ✅：kernel Cargo.toml（deps: spin/bitflags/zerocopy/ed25519-dalek/smoltcp path + features 全量迁移）；mod.rs→lib.rs（doc→attributes→items 重组）；queenx 壳 lib.rs 重写为 `pub use kernel` + re-export；顶层约束/panic_handler/kernel_init/alloc_error_handler/memory_allocator 迁 kernel。
- **D2 路径改写** ✅：kernel 内部 `crate::kernel::`→`crate::` 共 3370 处（3262+108 两轮，含重组引入），残留 0；`#[path = "../../kernel/mod.rs"]` 移除（RA 误报根因消除）。
- **D3 构建联动** ✅：Makefile RUST_LIB→`libkernel.a`（4 处 + 统一 target-dir `--target-dir ../rust/target`）；build.rs 迁 src/kernel（产物检查逻辑不变，manifest 相对路径天然正确）；build.sh/audit.sh/CI yml（ci-x86/aarch64/bench/lint）裸机检查点全部指向 kernel manifest；clippy.toml/rustfmt.toml 复制到 src/kernel（clippy 配置基于 manifest 目录查找，缺失导致 284 个 pedantic expect unfulfilled——实测发现）；kernel `.cargo/config.toml`（target-dir 统一 + curve25519 serial 后端，缺失导致 SIGILL）。
- **D4 验证** ✅：双架构 build 0w0e / clippy 双架构 0 / audit.sh quick 全绿 / host-tests 755/0（re-export 兼容 + 静态契约测试同步：`crate::kernel::`→`crate::` 55 处、`src/rust/src/lib.rs`→`src/kernel/lib.rs` 5 处、smoltcp 依赖检查改指 kernel manifest）/ QEMU x86_64 Ring 3 + aarch64 virt 挂网卡冒烟 / kernel_test.bin 链接通过 / RA 误报根因（跨 crate #[path]）移除。
- **实施期实测修正**（对照原方案）：
  1. `crate::kernel::` 实际 3370 处（原估 3300）；host-tests 静态断言 55 处 `crate::kernel::` 需同步（原方案仅说 re-export 兼容，未含静态契约测试）。
  2. kernel 直接依赖仅 spin/bitflags/zerocopy/ed25519-dalek/smoltcp（byteorder/heapleapless/managed/rstest 在 smoltcp vendored 内部；x86_64 是 arch 模块名）。
  3. clippy.toml/rustfmt.toml/.cargo/config（target-dir + serial env）须随 kernel 独立复制——原方案未列。
  4. Makefile RUST_LIB_TEST 系列缺 STAGE1_BIN 依赖（aarch64 切换后直接构建 kernel_test 暴露，build.rs 产物检查兜底）。

## 详情

### 影响面（2026-09-14 实测）

| 项 | 规模 |
|---|---|
| `crate::kernel::`（src/kernel 内部） | 3300 处 / 501 文件 |
| `crate::kernel::`（src/rust/src） | 106 处 |
| `queenx::kernel::`（host-tests 源码） | 88 处 / 29 文件（re-export 兼容，不改） |
| kernel 顶层约束门控 | lib.rs L4-6 + panic/alloc_error_handler |
| features 迁移 | host-test/kernel_test/fault_injection/net/alloc/smp 等 |
| 构建联动 | Makefile RUST_LIB、target-dir、CI manifest |

### Asterinas 参照要点

- workspace 多 crate：kernel 壳（`#![deny(unsafe_code)]` + main 入口）+ kernel/core + comps + libs，全部正常 `use` 依赖。
- 本工程**不照搬 60 crate**：仅 kernel 独立单 crate（最小可行），为后续拆 services 独立 crate（获编译器级 F1 services 0 unsafe / F3 无环依赖）铺路。

### 风险

1. **feature 转发遗漏**：kernel 的 host-test/kernel_test 若未从 queenx 正确转发，QEMU 测试/host 编译语义破坏（编译+测试兜底）。
2. **顶层约束迁移遗漏**：no_std/no_main/panic_handler 泄漏（编译兜底）。
3. **staticlib 产物路径**：Makefile 链接链（QEMU boot 兜底）。
4. **依赖迁移遗漏**：kernel 的 deps 未从 queenx 移出（编译兜底）。
5. **3300 处替换遗漏**：编译兜底（未替换处报 unresolved `kernel`）。
6. **re-export 兼容层**：`queenx::kernel::` 保留非纯根治，但 RA 正常解析（目标达成），且为 host-tests 零改动。

### 收益

- RA `#[path]` 误报根治（IDE 体验）。
- 对齐 Asterinas crate 化模型，为后续编译器级 F1/F3 保证铺路。
- cargo/CI 行为不变（现状 0 error 保持）。
