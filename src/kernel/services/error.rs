// SPDX-License-Identifier: MPL-2.0
// TD-08: services 层统一错误类型 `KernelError` (Single Source of Truth).
//
// 验收:
//   - 字段数为子集枚举 (不变量: 跨服务共享字段 = 1 份)
//   - `services::net::socket::SocketError` 改为 `pub type SocketError = KernelError;` 零包装
//   - `services::net::unix::UnixSocketError` 仅保留子系统特有字段 (PathNotFound) + `Kernel(KernelError)` 包装
//   - `From<fw::UdsError>` / `From<i32>` / `to_errno` 单一来源

#![deny(unsafe_code)]

// B09-12/DECISION-H13 P0-2: KernelError 定义已迁回 framework (framework::error),
// 本处 re-export 保持调用方兼容 (services→framework 单向依赖).
pub use crate::framework::error::KernelError;
