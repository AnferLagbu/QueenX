//! sigaltstack 替代栈信号投递契约测试 (P1-I-45)
//!
//! 验证信号投递路径对 sigaltstack 的支持:
//! 1. 进程设置 sigaltstack (addr!=0, size>=frame) 后,
//!    do_signal_deliver 必须使用替代栈顶部, 不写主栈 (主栈溢出场景不死锁)
//! 2. SS_ONSTACK 标记位: 进入信号 handler 前置位, 防止重入信号再次落回替代栈
//! 3. SS_DISABLE 标记位: 用户禁用替代栈时, 投递回退到主栈
//! 4. 替代栈容量不足时, 回退到主栈
//! 5. sigreturn 时清除 SS_ONSTACK 标记 (允许下一次信号再次落回替代栈)
//!
//! ## B08-20 处置 (2026-09-06): 算法镜像移除, 静态契约保留
//!
//! 原 `pick_frame_rsp` 镜像 `do_signal_deliver` 的 use_alternate 决策
//! (framework/proc/signal.rs:552-567). 评估结论: **该决策 host 不可直接测** —
//! 它内联于 `do_signal_deliver` 函数体, 依赖全局 PROCESS_TABLE (当前进程
//! `sigaltstack_*` 字段) + `InterruptFrame` 指针 + `do_signal_default_action`
//! (可能终止进程), 无法在 host 环境以函数形式调用.
//!
//! 按 B08-20/21 消并规则: 本地 `MockSigaltstack` / `pick_frame_rsp` / `make_ss`
//! 平行实现与对应用例已删除. **保留**已有效的 include_str 静态契约 (源码文本
//! 扫描, 验证 signal.rs 实现替代栈字段读取/决策/sigreturn 清位), 真实投递
//! 语义由 QEMU 集成测试覆盖.
//!
//! 待内核将 use_alternate 决策提炼为 pub 纯函数后可恢复 host 侧算法验证 (记录待办).

#[test]
fn source_signal_uses_sigaltstack() {
    // P1-I-45 源码静态扫描: signal.rs 必须实现替代栈判定
    let source = include_str!("../../src/kernel/framework/proc/signal.rs");
    assert!(
        source.contains("sigaltstack_addr")
            && source.contains("sigaltstack_size")
            && source.contains("sigaltstack_flags"),
        "P1-I-45: signal.rs 必须读取 sigaltstack 字段"
    );
    assert!(
        source.contains("use_alternate")
            && source.contains("SS_DISABLE")
            && source.contains("SS_ONSTACK"),
        "P1-I-45: signal.rs 必须实现 SS_DISABLE / SS_ONSTACK 决策"
    );
    assert!(
        source.contains("ss_addr + ss_size - total as u64"),
        "P1-I-45: 替代栈顶部必须为 ss_addr + ss_size - total"
    );
}

#[test]
fn source_syscall_clears_onstack_on_sigreturn() {
    // P1-I-45 源码静态扫描: syscall/dispatch.rs::sys_rt_sigreturn 必须清 SS_ONSTACK
    let source = include_str!("../../src/kernel/framework/syscall/dispatch.rs");
    let rt_sigreturn_start = source
        .find("fn sys_rt_sigreturn() -> i64 {")
        .expect("必须存在 sys_rt_sigreturn");
    let rt_sigreturn_body = &source[rt_sigreturn_start..];
    // 必须清 SS_ONSTACK
    assert!(
        rt_sigreturn_body.contains("!crate::kernel::framework::proc::signal::SS_ONSTACK")
            || rt_sigreturn_body.contains("!crate::kernel::framework::proc::SS_ONSTACK")
            || rt_sigreturn_body.contains("!SS_ONSTACK"),
        "P1-I-45: sys_rt_sigreturn 必须清除 SS_ONSTACK 标记"
    );
}
