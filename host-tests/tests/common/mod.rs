//! 集成测试共享 helper (Rust 官方测试组织标准 §tests/common/mod.rs)
//!
//! ## 设计目标
//! 集中放置被多个 `tests/*.rs` 共享的桩对象 / 测试装置, 避免在每个集成
//! 测试文件重复定义, 同时避免污染 `src/lib.rs` 的公共 API.
//!
//! ## 文件名约定
//! 集成测试文件中通过以下方式引用:
//! ```ignore
//! mod common;
//! use common::xxx;
//! ```
//!
//! ## 当前内容
//! 现阶段 `MockIoMem` / `EerdState` 仅 `e1000_eeprom.rs` 使用, 保留在各
//! 集成测试文件内. 当后续测试出现真正的跨文件共享需求时, 移入此处.
//!
//! ## 注意事项
//! 本文件整体在 `lib.rs` 的 release 编译中**不会**被编译, cargo 自动
//! 跳过 `tests/common/mod.rs` (不视作独立测试目标), 因此无需 `#[cfg(test)]`.
//!
//! B09-03 (2026-09-14): `#![allow(dead_code)]` (F9 违规) 已删除 — 本文件当前
//! 无共享 helper 代码, allow 为空属性; 未来加入 helper 时按 F9 走实现使用路径.

// 当前无共享 helper. 后续可加入:
// pub mod mock_iomem;    // 模拟 IoMem MMIO 读写
// pub mod buddy_helper;  // 伙伴分配器测试装置
