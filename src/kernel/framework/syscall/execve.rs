#![deny(unsafe_code)]
//! execve 结果类型 — framework 层权威定义
//!
//! ## 归属记录
//!
//! TCB 核心逻辑 (用户指针验证 / SUID 处理 / 进程替换) 在
//! `framework::syscall::dispatch::sys_execve` 机制内; 本类型为其返回值的
//! errno 解析包装 (机制持有). 第二十五批自 services/proc/execve.rs 迁回
//! (DECISION-O ② MemoryPressure 同判据: 机制类型归 framework).
//!
//! services 层 0 unsafe — 本文件不含 unsafe 代码.

use crate::framework::syscall::Errno;

/// execve 结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecveResult {
    /// 成功 (execve 成功时不返回, 此值仅用于类型完整性)
    Success,
    /// 失败, errno
    Err(Errno),
}

impl ExecveResult {
    /// 从 syscall 返回值解析
    pub fn from_ret(ret: i64) -> Self {
        if ret >= 0 {
            Self::Success
        } else {
            let errno = match -ret as i32 {
                2 => Errno::ENOENT,
                14 => Errno::EFAULT,
                13 => Errno::EACCES,
                8 => Errno::ENOEXEC,
                _ => Errno::EINVAL,
            };
            Self::Err(errno)
        }
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    /// 转换为 syscall 返回值
    pub fn as_ret(&self) -> i64 {
        match self {
            Self::Success => 0,
            Self::Err(e) => -(*e as i64),
        }
    }
}
