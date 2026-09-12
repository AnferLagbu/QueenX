//! IPC 策略 trait — 策略-机制分离接口
//!
//! 本模块落实「framekernel 范式落实」工程 DECISION-I: framework/ipc 的
//! `pipe.rs`/`shm.rs`/`msgq.rs` FFI 边界直接调用 `services::ipc::*_safe`
//! (framework→services 反向依赖 13 处), 改为经 `IpcStrategy` trait 注入:
//! framework 定义契约, services 实现并注册, FFI 边界经 `current_ipc_strategy()`
//! 调用策略方法。
//!
//! ## 设计 (对齐 PageFaultPolicy/SwapPolicy 既有模式)
//!
//! - trait 定义在 framework (引用 framework 类型 `IpcNamespace`/`IpcId`)
//! - 实现在 services (100% safe Rust, `#![deny(unsafe_code)]`)
//! - services 通过 `register_ipc_strategy()` 注册; FFI 边界经 `current_ipc_strategy()`
//!   获取策略
//! - **无内建回退**: 策略方法 (`pipe_create_safe` 等) 依赖 services 实现, framework
//!   无法安全回退 — 与 `services::ipc::global()` 的 `expect` 契约一致, 未注册即调用
//!   属编程错误 (IPC FFI 仅在 syscall 时触发, 注册必早于任何用户态使用)

use super::types::{IpcId, IpcNamespace};

/// IPC 策略接口 — services 实现, framework FFI 边界调用
///
/// 所有方法为纯策略逻辑 (参数校验 + 资源操作), 操作由调用方提供的
/// `&mut IpcNamespace` (framework 机制全局状态), 不涉及硬件或 unsafe.
pub trait IpcStrategy: Send + Sync {
    // ── Pipe ──

    /// 判断 fd 是否为 pipe fd (供 sendfile/splice 使用)
    fn is_pipe_fd(&self, fd: i32) -> bool;

    /// 创建管道 (策略: 分配槽位 + 初始化等待队列)
    fn pipe_create(
        &self,
        ns: &mut IpcNamespace,
        next_id: &mut IpcId,
        pid: u32,
    ) -> Result<(i32, i32), i32>;

    /// 管道读 (策略: 缓冲复制 + 唤醒写端)
    fn pipe_read(
        &self,
        ns: &mut IpcNamespace,
        fd: i32,
        buf: &mut [u8],
        count: u32,
    ) -> Result<u32, i32>;

    /// 管道写 (策略: 缓冲复制 + 唤醒读端)
    fn pipe_write(
        &self,
        ns: &mut IpcNamespace,
        fd: i32,
        buf: &[u8],
        count: u32,
    ) -> Result<u32, i32>;

    /// 关闭管道 (策略: 释放槽位)
    fn pipe_close(&self, ns: &mut IpcNamespace, fd: i32) -> Result<(), i32>;

    // ── SHM ──

    /// 创建共享内存段 (策略: 分配槽位 + 物理页)
    fn shm_create(
        &self,
        ns: &mut IpcNamespace,
        next_id: &mut IpcId,
        size: u64,
        perm: i32,
        pid: u32,
    ) -> Result<IpcId, i32>;

    /// 附加共享内存段 (策略: 页表映射, 返回物理地址)
    fn shm_attach(&self, ns: &mut IpcNamespace, id: IpcId, pid: u32) -> Result<u64, i32>;

    /// 分离共享内存段 (策略: 解除映射)
    fn shm_detach(&self, ns: &mut IpcNamespace, id: IpcId, pid: u32) -> Result<(), i32>;

    /// 销毁共享内存段 (策略: 释放资源)
    fn shm_destroy(&self, ns: &mut IpcNamespace, id: IpcId) -> Result<(), i32>;

    // ── MsgQ ──

    /// 创建消息队列 (策略: 分配槽位 + 初始化链表)
    fn msgq_create(
        &self,
        ns: &mut IpcNamespace,
        next_id: &mut IpcId,
        perm: i32,
        pid: u32,
    ) -> Result<IpcId, i32>;

    /// 发送消息 (策略: 参数校验 + 容量检查 + 入队 + 唤醒)
    fn msgq_send(
        &self,
        ns: &mut IpcNamespace,
        id: IpcId,
        type_: u64,
        data: Option<&[u8]>,
        size: usize,
        pid: u32,
    ) -> Result<(), i32>;

    /// 接收消息 (策略: 出队 + 数据复制)
    fn msgq_recv(
        &self,
        ns: &mut IpcNamespace,
        id: IpcId,
        type_out: Option<&mut u64>,
        data_out: Option<&mut [u8]>,
        size_out: Option<&mut u64>,
    ) -> Result<usize, i32>;

    /// 销毁消息队列 (策略: 释放消息链表 + 槽位)
    fn msgq_destroy(&self, ns: &mut IpcNamespace, id: IpcId) -> Result<(), i32>;
}

// ============================================================================
// 全局注册表
// ============================================================================

/// 全局 IPC 策略注册表 — services 通过 `register_ipc_strategy` 注册
static IPC_STRATEGY: crate::kernel::framework::sync::OnceLock<&'static dyn IpcStrategy> =
    crate::kernel::framework::sync::OnceLock::new();

/// 注册 IPC 策略 (由 `services::ipc::strategy` 调用)
///
/// 只能注册一次; 重复注册返回 `Err`.
///
/// # Errors
/// 当策略已注册时, 返回 `Err`, 其中携带已注册的旧策略指针.
pub fn register_ipc_strategy(
    strategy: &'static dyn IpcStrategy,
) -> Result<(), &'static dyn IpcStrategy> {
    match IPC_STRATEGY.set(strategy) {
        Ok(()) => Ok(()),
        Err(existing) => Err(existing),
    }
}

/// 获取当前注册的 IPC 策略.
///
/// # Panics
/// 当策略尚未注册时 panic — 与 `services::ipc::global()` 的 `expect` 契约一致:
/// IPC FFI 仅在 syscall 时触发, 注册必早于任何用户态使用.
pub fn current_ipc_strategy() -> &'static dyn IpcStrategy {
    match IPC_STRATEGY.get() {
        Some(&s) => s,
        None => panic!("ipc strategy not registered"),
    }
}
