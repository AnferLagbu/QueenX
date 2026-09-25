# framekernel-bench 度量口径修复（measure 0ns 折叠）

> 0-1 句话说清"为什么有这个计划": [framekernel_bench.rs](file:///home/anfer/Code/QueenX/host-tests/src/framekernel_bench.rs) 的 `measure()` 与各 bench 函数返回值语义错配（双重归一化 + 自适应放大从未生效），致 [baseline.json](file:///home/anfer/Code/QueenX/host-tests/benches/baseline.json) 23 条中 5 条 `ps_per_op=0`、其余整体低估约 10^4 倍，`check_bench_regression.py` 失去回归检出能力。
> 来源: G-07 平行实现消除收尾轮（[eliminate-parallel-implementations.md](./eliminate-parallel-implementations.md)）验收时发现。用户裁定：baseline 先按现状重录，口径修复登记为本独立任务。

## 工程计划 A: measure 与 bench 函数返回值语义收敛

### 背景

- **双重归一化（核心缺陷）**
  - 描述: `measure()` 的注释声明契约「`f()` 返回总耗时 (ns)」，但全部 bench 函数实际返回**已归一化的 ps_per_op**（如 `zap_dispatch_bench` 尾部 `elapsed.saturating_mul(1_000) / iters`、`pte_set_flags_bench` 尾部 `elapsed.saturating_mul(1_000) / (iters * BATCH)`）。`measure()` 把该 ps_per_op 当作 `total_ns`，随后又做一次 `total_ns.saturating_mul(1_000) / iters` —— 同一份耗时被归一化两次。
  - 方案: 二选一。(A) bench 函数统一改为返回总耗时（`elapsed.as_nanos()`），归一化只在 `measure()` 中做一次 —— 与 `measure()` 现有注释契约一致，且 `iterations` / `total_ns` / `ops_per_sec` 字段语义自洽。(B) 废弃 `measure()` 的归一化与自适应放大，直接透传 bench 返回的 `ps_per_op` —— 改动更小，但 `iterations` 字段失去意义。推荐 A。
  - 状态: []

- **自适应放大从未生效**
  - 描述: `measure()` 以 `iters = (iters * 10).min(10_000_000)` 放大取样规模，但 `run_all()` 传入的闭包捕获的是**字面量**（如 `|| zap_dispatch_bench(10_000)`），放大值从未传给 `f()`；放大循环只是重复调用同一个固定工作量的 `f()`，并按放大后的 `iters` 归一化。
  - 方案: `measure()` 签名改为 `Fn(u64) -> u128`，循环内把放大后的 `iters` 传入闭包；`run_all()` 各闭包由 `|| xxx_bench(N)` 改为 `|iters| xxx_bench(iters)` —— bench 函数本就以 `iters` 为形参，23 处重复字面量随之消除。
  - 状态: []

- **现状量化（重录后的 baseline 留档）**
  - 描述: 因 `iters` 恒被放大到 10_000_000，记录值 = `floor(实际 ps_per_op / 10_000)`。重录后 baseline 23 条**全部** `iterations=10000000`；`ps_per_op=0` 者 5 条（`page_flags_bits` / `capability_check` / `dma_state_machine` / `vfs_poll_dispatch` / `raidz_dispatch`），其真实单操作耗时均 < 10ns 故被整数截断折叠；`zil_persist_dispatch` 记录 729 ps，对应实际约 7.29 µs/op（与逐位 CRC32 处理 4096B 块的量级吻合）。
  - 方案: 本任务修复轮一并重录 baseline，故当前值仅作对照留档，不作为长期基线。
  - 状态: []

- **回归检出能力失效（连带影响）**
  - 描述: `check_bench_regression.py` 对 `baseline=0` 条目走「至少为 0, 跳过」分支，另对 `|diff| < 1.0ns` 一律按噪声跳过。当前 23 条中仅 `zil_persist_dispatch` 可能触发告警 —— 与 AGENTS.md §8「性能基线每次 PR 更新」及 §12.4 目标驱动验证的意图不符。
  - 方案: 修复度量口径后在同一轮重录 baseline，并复核噪声阈值口径（`1.0ns` 绝对阈值对修复后的亚纳秒快路径是否仍合适，或改为「相对百分比 + 绝对下限」双条件）。
  - 状态: []

### 验证门槛

- **度量口径自洽**
  - 描述: 每条 bench 的 `iterations` 与实际工作量一致，`total_ns` 与 `ps_per_op × iterations / 1000` 自洽，无条目因整数截断折叠为 0（亚纳秒条目应给出真实小数值而非 0）。
  - 方案: 修复后运行 `cargo run --release --bin framekernel-bench` 逐条核对上述两项。
  - 状态: []

- **回归门禁有效**
  - 描述: baseline 重录后 `python3 scripts/check_bench_regression.py` PASS；反向验证 —— 人为放大某条 bench 工作量约 20% 时能被检出为回归。
  - 方案: 重录 + 临时注入退化的反向验证（验证后回滚）。
  - 状态: []

### 范围

- **不在本任务内**
  - 描述: bench 集合的增删与内核实现迁移（属 G-07 平行实现消除，已收尾）；bench 函数算法本体与其测量对象改动。
  - 方案: 仅动 `measure()` 与 `run_all()` 闭包签名 + baseline 重录 + `check_bench_regression.py` 阈值口径评估。
  - 状态: []