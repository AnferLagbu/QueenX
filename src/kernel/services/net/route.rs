#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! 路由表管理 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-12)
//!
//! 原路由表 CRUD/CIDR 匹配/syscall 按统一判据"机制持有的数据结构/常量归
//! framework"迁回 `framework/net/route.rs` (`RouteEntry` 被 framework smoltcp
//! 同步机制消费、`sys_route_*` 被 framework syscall dispatch 调用, 依赖闭包
//! 全在 framework 内)。framework/net 的 route 为公开子模块 (`pub mod route`),
//! 可直接 glob re-export, 保持 services 侧 API 兼容 (services→framework 合法方向)。

pub use crate::kernel::framework::net::route::*;
