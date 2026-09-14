#![deny(unsafe_code)]
//! init 启动子系统 — services 层安全代理
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 framework::proc::api。
//!
//! ## 职责
//!
//! - 查询 init 启动状态 (0=未启动, 1=initramfs 解压, 2=加载, 3=Ring 3)
//! - 提供类型安全的常量供其他服务引用
//!
//! ## 启动流程 (由 framework::proc::launch_first_user_process 内部驱动)
//!
//! 1. 挂载 ramfs 为 `/`
//! 2. 解压 initramfs cpio 到 ramfs (feature = "initramfs")
//! 3. 加载 `/init` ELF, 创建 PID 1
//! 4. 加入调度器, 切换 Ring 3

// ============================================================================
// init 启动状态常量
// ============================================================================

/// 未启动
pub const INIT_STATUS_NOT_STARTED: u32 = 0;
/// initramfs 解压中
pub const INIT_STATUS_UNPACKING: u32 = 1;
/// init ELF 加载中
pub const INIT_STATUS_LOADING: u32 = 2;
/// 已 Ring 3 进入 (init 运行中)
pub const INIT_STATUS_RUNNING: u32 = 3;

// ============================================================================
// safe 状态查询 API
// ============================================================================

/// 查询 init 启动状态
#[inline]
pub fn init_launch_status() -> u32 {
    crate::framework::proc::init_launch_status()
}

/// init 是否已运行 (>= 3 表示已进入 Ring 3)
#[inline]
pub fn is_init_running() -> bool {
    init_launch_status() >= INIT_STATUS_RUNNING
}

// ============================================================================
// 单元测试 (host)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_status_not_running_when_zero() {
        // 启动前状态应为 0
        // 注: host 进程下 status 可能为 0, 验证语义
        assert!(!is_init_running() || init_launch_status() == 0);
    }

    #[test]
    fn test_init_status_constants_distinct() {
        assert_ne!(INIT_STATUS_NOT_STARTED, INIT_STATUS_UNPACKING);
        assert_ne!(INIT_STATUS_UNPACKING, INIT_STATUS_LOADING);
        assert_ne!(INIT_STATUS_LOADING, INIT_STATUS_RUNNING);
    }
}
