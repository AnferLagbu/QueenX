#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 时钟查询 syscall 策略 — services 层统一时间模块 (B05-26 归位)
//!
//! 集中 `clock_gettime` / `gettimeofday`, 消除此前分散在 fs/file_ops 与
//! proc/info 的职责错位.

use crate::framework::syscall::Errno;

/// POSIX 时钟 ID
const CLOCK_REALTIME: i32 = 0;
const CLOCK_MONOTONIC: i32 = 1;

/// `clock_gettime(clk_id, tp)` 策略
///
/// 仅支持 `CLOCK_REALTIME` 与 `CLOCK_MONOTONIC`; 其余时钟 ID 返回 `EINVAL`.
///
/// # Errors
///
/// - `tp` 为空指针 → `EINVAL`
/// - 时钟 ID 不受支持 → `EINVAL`
/// - 用户缓冲写入失败 → `EFAULT`
pub fn clock_gettime_syscall(clk_id: i32, tp_ptr: u64) -> i64 {
    if tp_ptr == 0 {
        return Errno::EINVAL.as_ret();
    }
    if clk_id != CLOCK_REALTIME && clk_id != CLOCK_MONOTONIC {
        return Errno::EINVAL.as_ret();
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[expect(
        clippy::items_after_statements,
        reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
    )]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }

    let ticks = crate::framework::syscall::api::get_ticks();
    let t = Timespec {
        tv_sec: (ticks / 1000) as i64,
        tv_nsec: ((ticks % 1000) * 1000000) as i64,
    };

    if !crate::framework::syscall::api::write_struct_to_user(tp_ptr, &t) {
        return Errno::EFAULT.as_ret();
    }
    0
}

/// `gettimeofday(tv)` 策略
///
/// `tv` 指向 struct timeval (`tv_sec` + `tv_usec`, 16 字节)
///
/// # Errors
///
/// 当 `tv == 0` 时返回 `EFAULT`.
pub fn gettimeofday_syscall(tv: u64) -> Result<usize, Errno> {
    if tv == 0 {
        return Err(Errno::EFAULT);
    }
    let ret = crate::framework::syscall::info::sys_gettimeofday(tv);
    if ret < 0 {
        Err(Errno::from_ret(ret))
    } else {
        Ok(ret as usize)
    }
}
