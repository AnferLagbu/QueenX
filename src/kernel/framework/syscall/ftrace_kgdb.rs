//! ftrace / KGDB 系统调用实现
//!
//! ## 编号 (800-809: 内核调试 / 跟踪)
//!
//! - `QX_FTRACE_ENABLE`   (800): 启用 ftrace 全局开关
//! - `QX_FTRACE_DISABLE`  (801): 禁用 ftrace 全局开关
//! - `QX_FTRACE_READ`     (802): 从 ring buffer 弹出一条事件, 写入用户态缓冲
//! - `QX_FTRACE_STAT`     (803): 查询 (`event_count`, `overflow_count`) 到用户态
//! - `QX_KGDB_ENTER`      (804): 主动进入 KGDB 主循环 (等待外部 gdb)
//!
//! ## 用户态布局
//!
//! ```c
//! struct user_trace_event {
//!     uint64_t timestamp;
//!     uint64_t name_hash;
//!     uint64_t arg0;
//!     uint64_t arg1;
//!     uint64_t arg2;
//!     uint64_t arg3;
//! }; // 48 字节, 需 8 字节对齐
//! ```
//!
//! ## 安全
//!
//! - 用户态指针经 `check_user_ptr` / `check_user_buf` 校验
//! - `kgdb_enter` 要求串口已注册 (`kgdb_serial_ready`), 否则返回 ENODEV

use crate::framework::debug::TraceEvent;
use crate::framework::debug::api;
use crate::framework::syscall::raw as raw_sync;
use core::mem;
use core::ptr;

const EFAULT: i64 = -14;
const ENODEV: i64 = -19;

/// `sys_ftrace_enable`: 启用 ftrace 全局开关
pub fn sys_ftrace_enable() -> i64 {
    api::ftrace_enable();
    0
}

/// `sys_ftrace_disable`: 禁用 ftrace 全局开关
pub fn sys_ftrace_disable() -> i64 {
    api::ftrace_disable();
    0
}

/// `sys_ftrace_read`: 弹出一条事件, 写入用户态缓冲
///
/// - `a0`: 用户态指针 (`UserTraceEvent` 布局 48 字节, 8 字节对齐)
/// - 返回 0 = 成功, 1 = 缓冲区空 (无事件), 负数 = errno
pub fn sys_ftrace_read(a0: u64) -> i64 {
    if !raw_sync::check_user_buf(a0, mem::size_of::<TraceEvent>() as u64) {
        return EFAULT;
    }
    // SAFETY: check_user_buf 已校验 a0 指向大小足够且对齐的用户空间,
    // TraceEvent 是 POD 类型, 序列化安全
    api::ftrace_pop_event().map_or(1, |ev| {
        unsafe {
            ptr::write_unaligned(a0 as *mut TraceEvent, ev);
        }
        0
    })
}

/// `sys_ftrace_stat`: 拷贝 (`event_count`, `overflow_count`) 到用户态
///
/// - `a0`: 用户态指针 (16 字节, [u64; 2] 布局: `event_count`, `overflow_count`)
pub fn sys_ftrace_stat(a0: u64) -> i64 {
    if !raw_sync::check_user_buf(a0, 16) {
        return EFAULT;
    }
    let ec = api::ftrace_event_count();
    let oc = api::ftrace_overflow_count();
    // SAFETY: check_user_buf 已校验 a0 指向 16 字节对齐的用户空间
    unsafe {
        let p = a0 as *mut [u64; 2];
        ptr::write_unaligned(p, [ec, oc]);
    }
    0
}

/// `sys_kgdb_enter`: 主动进入 KGDB 主循环
///
/// - 串口未注册时返回 ENODEV
/// - 串口已注册时: 阻塞与外部 gdb 通信, 收到 c/s/k 后返回 0
pub fn sys_kgdb_enter() -> i64 {
    if !api::kgdb_serial_ready() {
        return ENODEV;
    }
    let mut regs = api::KgdbRegs::default();
    api::kgdb_breakpoint(&mut regs);
    0
}
