#![deny(unsafe_code)]
//! SHA-256 哈希实现 — services 层 re-export 壳
//!
//! ## 归属记录
//!
//! 纯算法实现 (SHA-256 哈希, 安全原语) 的权威定义在
//! `framework/credo/sha256.rs` (第二十五批反转归位, DECISION-K 项 5 credo
//! 判据 — framework/credo/secure_boot 直接消费). 本文件仅 re-export 保持
//! services 内部消费者路径兼容.

pub use crate::kernel::framework::credo::sha256::*;
