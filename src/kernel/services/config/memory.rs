#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 re-export。
//! 内存布局常量 — services 侧 re-export 兼容层
//!
//! ## DECISION-J 归属反转记录 (2026-09-12)
//!
//! 原常量定义 (页/栈/堆/用户地址空间/ASLR 基址) 按统一判据"机制持有的数据
//! 结构/常量归 framework"迁回 `framework/config/memory.rs` (被 framework
//! arch/mm/proc 机制直接消费)。framework/config 子模块为私有, 故经其顶层
//! re-export (`framework::config::PAGE_SIZE` 等) 显式转发, 保持 services 侧
//! API 兼容 (services→framework 合法方向)。

pub use crate::kernel::framework::config::{
    ASLR_HEAP_BITS, ASLR_MMAP_BITS, ASLR_PIE_BITS, ASLR_STACK_BITS, HUGE_PAGE_1G_SHIFT,
    HUGE_PAGE_1G_SIZE, HUGE_PAGE_2M_SHIFT, HUGE_PAGE_2M_SIZE, KERNEL_STACK_SIZE, PAGE_SHIFT,
    PAGE_SIZE, USER_CODE_BASE, USER_HEAP_BASE, USER_KSTACK_SIZE, USER_MMAP_BASE, USER_PIE_BASE,
    USER_STACK_GUARD, USER_STACK_MAX_SIZE, USER_STACK_SIZE, USER_STACK_TOP,
};
