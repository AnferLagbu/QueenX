//! clone — 线程创建系统调用 (TCB)
//!
//! Linux `clone()` 是 `fork()` 的超集, 支持细粒度资源共享:
//! - `CLONE_VM`: 共享地址空间 (线程)
//! - `CLONE_FS`: 共享文件系统信息
//! - `CLONE_FILES`: 共享文件描述符表
//! - `CLONE_SIGHAND`: 共享信号处理
//! - `CLONE_THREAD`: 同一线程组
//! - `CLONE_PARENT`: 共享父进程
//!
//! ## 实现策略
//!
//! 当前简化实现:
//! - `CLONE_VM`: 子进程共享父进程的页表 (不 COW)
//! - 其他标志: 忽略 (子进程独立拷贝)
//! - 无 `CLONE_VM`: 等同于 fork (COW)
//!
//! # Safety
//!
//! - 页表操作需要 VMM 锁
//! - 进程表操作需要 `PROCESS_TABLE` 锁
//! - 栈指针必须指向用户空间

use crate::framework::proc::ProcessState;
use crate::framework::proc::api;
use crate::framework::proc::raw;
use crate::framework::syscall::Errno;

use core::sync::atomic::Ordering;

/// clone 标志位
pub const CLONE_VM: u64 = 0x00000100; // 共享地址空间
pub const CLONE_FS: u64 = 0x00000200; // 共享 fs 信息
pub const CLONE_FILES: u64 = 0x00000400; // 共享 fd 表
pub const CLONE_SIGHAND: u64 = 0x00000800; // 共享信号处理
pub const CLONE_PIDFD: u64 = 0x00001000; // 返回 pidfd
pub const CLONE_PARENT: u64 = 0x00008000; // 共享父进程
pub const CLONE_THREAD: u64 = 0x00010000; // 同一线程组
pub const CLONE_SYSVSEM: u64 = 0x00040000; // 共享 SysV 信号量
pub const CLONE_PARENT_SETTID: u64 = 0x00100000; // 写 TID 到 parent tidptr
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000; // 子进程退出时清 tidptr
pub const CLONE_CHILD_SETTID: u64 = 0x01000000; // 写 TID 到 child tidptr

/// Namespace 标志位 (D1)
pub const CLONE_NEWNS: u64 = 0x00020000; // Mount namespace
pub const CLONE_NEWUTS: u64 = 0x04000000; // UTS namespace
pub const CLONE_NEWIPC: u64 = 0x08000000; // IPC namespace
pub const CLONE_NEWUSER: u64 = 0x10000000; // User namespace
pub const CLONE_NEWPID: u64 = 0x20000000; // PID namespace
pub const CLONE_NEWNET: u64 = 0x40000000; // Network namespace
pub const CLONE_NEWCGROUP: u64 = 0x02000000; // Cgroup namespace
/// 所有 `CLONE_NEW`* 掩码
pub const CLONE_NEW_ALL: u64 = CLONE_NEWNS
    | CLONE_NEWUTS
    | CLONE_NEWIPC
    | CLONE_NEWUSER
    | CLONE_NEWPID
    | CLONE_NEWNET
    | CLONE_NEWCGROUP;

#[expect(
    clippy::similar_names,
    reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分"
)]
#[expect(
    clippy::too_many_lines,
    reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
)]
#[expect(
    clippy::ref_as_ptr,
    reason = "ref_as_ptr: &T as *const T 是已知安全 (Rust 2024 可用 &raw const; 当前优先 expect"
)]
#[expect(
    clippy::ptr_cast_constness,
    reason = "ptr_cast_constness: *mut T as *const T 是已知安全 (Rust 2024 可用 ptr.cast_const 或 &raw const; 当前优先 expect"
)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
/// clone 系统调用实现
///
/// `flags`: 克隆标志 (`CLONE_VM` | `CLONE_FS` | ...)
/// `child_stack`: 子进程的用户栈地址 (0 = 与父进程相同)
/// `parent_tidptr`: 父进程 TID 指针
/// `child_tidptr`: 子进程 TID 指针
/// `tls`: TLS 地址
pub fn sys_clone(
    flags: u64,
    child_stack: u64,
    parent_tidptr: u64,
    child_tidptr: u64,
    tls: u64,
) -> i64 {
    let parent_pid = match api::process_get_current_pid() {
        0 => return Errno::ECHILD.as_ret(),
        p => p,
    };

    // 如果没有 CLONE_VM, 行为等同于 fork
    if flags & CLONE_VM == 0 {
        let child_pid = api::sys_fork();
        if child_pid == 0 {
            return Errno::ENOMEM.as_ret();
        }

        // 如果指定了 child_stack, 修改子进程的 RSP
        if child_stack != 0 {
            let _ = api::process_with_mut(child_pid, |p| {
                let mut ctx = p.context.lock();
                ctx.rsp = child_stack;
            });
        }

        // CLONE_PARENT_SETTID: 写 TID 到 parent_tidptr
        if flags & CLONE_PARENT_SETTID != 0 && parent_tidptr != 0 {
            // SAFETY: parent_tidptr 由 syscall 入口验证
            unsafe {
                core::ptr::write_volatile(parent_tidptr as *mut i32, child_pid as i32);
            }
        }

        // CLONE_CHILD_CLEARTID: 登记清除地址, 退出时写 0 并 futex 唤醒.
        // fork 路径不处理 CLONE_CHILD_SETTID (父上下文写入会落在父地址空间,
        // 子进程 COW 后不可见; Linux 同样仅在子上下文/共享地址空间写).
        if flags & CLONE_CHILD_CLEARTID != 0 && child_tidptr != 0 {
            let _ = api::process_with_mut(child_pid, |p| {
                p.clear_child_tid
                    .store(child_tidptr, core::sync::atomic::Ordering::Release);
            });
        }

        // D1: CLONE_NEW* — 为子进程创建新 namespace
        let new_ns_flags = flags & CLONE_NEW_ALL;
        if new_ns_flags != 0 {
            let _ = api::process_with_mut(child_pid, |p| {
                let parent_ns = {
                    // 子进程已通过 fork 继承了父进程的 namespace
                    // 现在根据 CLONE_NEW* 创建新实例
                    let current_ns = p.namespaces.lock();
                    crate::framework::proc::NamespaceSet::clone_from(&current_ns, new_ns_flags)
                };
                *p.namespaces.lock() = parent_ns;
            });
        }

        return i64::from(child_pid);
    }

    // CLONE_VM: 共享地址空间 (创建线程)
    let parent_cr3 = api::process_with(parent_pid, |p| p.cr3.load(Ordering::SeqCst)).unwrap_or(0);
    if parent_cr3 == 0 {
        return Errno::ENOMEM.as_ret();
    }

    // 分配子进程 PID
    let child_pid = api::proc_alloc_pid();
    if child_pid == 0 {
        return Errno::ENOMEM.as_ret();
    }

    // 克隆父进程名称
    let name_str = api::process_with(parent_pid, |p| {
        let name = p.name.lock();
        alloc::string::String::clone(&*name)
    })
    .unwrap_or_default();

    // 创建子进程 (共享 CR3, 不 COW)
    let child_ptr = raw::alloc_process(
        child_pid,
        name_str.as_str(),
        Some(crate::framework::proc::ProcessId(parent_pid)),
    );
    let child = raw::process_ref_mut(child_ptr);

    // 共享地址空间: 子进程使用父进程的 CR3
    child.cr3.store(parent_cr3, Ordering::SeqCst);

    // G1 修复: 共享父进程地址空间必须登记一个持有者.
    // 父子各持同一个 PML4 物理地址 ⇒ 两者的 Process::drop 都会释放同一张页表;
    // 无持有计数时先退出者会把另一方仍在使用的页表销毁 (UAF). 计数登记处只此一处
    // (`child_ctx.cr3` 是上下文快照, 不承担所有权, 不重复计数).
    if !crate::framework::mm::pmm::get_pmm().frame_inc(crate::framework::mm::PhysAddr(parent_cr3)) {
        // 登记失败 (父 cr3 未处于计数态, 契约违反): 回滚已写入的 cr3,
        // 避免子进程 drop 时误释放父进程地址空间.
        crate::klog_error!(
            "clone: CLONE_VM 无法登记 cr3 持有者 (cr3={:#X}), 拒绝共享地址空间",
            parent_cr3
        );
        child.cr3.store(0, Ordering::SeqCst);
        raw::drop_boxed_process(child_ptr);
        return Errno::ENOMEM.as_ret();
    }

    // 复制父进程属性
    let (parent_pwm, parent_sched_policy, parent_rt_priority) =
        api::process_with(parent_pid, |p| {
            (
                p.pwm.load(Ordering::SeqCst),
                p.sched_policy.load(Ordering::SeqCst),
                p.rt_priority.load(Ordering::SeqCst),
            )
        })
        .unwrap_or((0, 0, 0));
    child.pwm.store(parent_pwm, Ordering::SeqCst);
    child
        .sched_policy
        .store(parent_sched_policy, Ordering::SeqCst);
    child
        .rt_priority
        .store(parent_rt_priority, Ordering::SeqCst);

    // 添加到父进程的子进程列表
    api::process_with_mut(parent_pid, |p| {
        p.children
            .lock()
            .push(crate::framework::proc::ProcessId(child_pid));
    });

    // 分配内核栈
    if !child.allocate_kernel_stack() {
        raw::drop_boxed_process(child_ptr);
        return Errno::ENOMEM.as_ret();
    }

    // 复制父进程的内核栈
    {
        let parent_kstack =
            api::process_with(parent_pid, |p| p.kernel_stack.load(Ordering::SeqCst)).unwrap_or(0);
        let child_kstack = child.kernel_stack.load(Ordering::SeqCst);
        let stack_size: usize = 65536;
        raw::copy_kstack(child_kstack, parent_kstack, stack_size);
        crate::framework::proc::kernel_stack_write_canary(child_kstack);
    }

    // 上下文初始化 (分架构: x86_64 的 cr3/rax/rsp 与 aarch64 的 x29/x25/x27 复用
    // 同一偏移, 见 `arch/aarch64/context.rs` 头注释, 故必须按架构分别写)
    let parent_ctx = if let Some(ctx) = api::process_with(parent_pid, |p| *p.context.lock()) {
        ctx
    } else {
        crate::klog_error!("clone: 父进程 {} 在进程表中未找到", parent_pid);
        return Errno::ESRCH.as_ret();
    };
    {
        let mut child_ctx = child.context.lock();
        *child_ctx = parent_ctx;
        #[cfg(target_arch = "x86_64")]
        {
            child_ctx.cr3 = parent_cr3; // 共享 CR3
            child_ctx.rax = 0; // 子进程返回 0
            // 如果指定了 child_stack, 修改 RSP
            if child_stack != 0 {
                child_ctx.rsp = child_stack;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            // KPTI-17: 子线程 x0 = 0 (clone 返回值); 用户栈指针在 @128 (SP_EL0).
            child_ctx.extra_regs[0] = 0;
            if child_stack != 0 {
                child_ctx.ss = child_stack;
            }
            // SP_EL1 (@96) 必须是子线程**自己的**内核栈顶: 沿用父线程的值会
            // 让子线程首次陷入 EL1 时把异常帧压进父线程的栈页.
            child_ctx.ds = child.kernel_stack.load(Ordering::SeqCst) & !0xF;
            // `_fpu_pad`(@136, EL0 的 TTBR0) 与 `es`(@104, EL1 视图根) 随 ctx
            // 复制而来: 本路径恒共享父线程页表 (上文 cr3 = parent_cr3), 二者
            // 对父子线程同值, 无需改写.
        }

        // CLONE_SETTLS: 设置子进程 TLS 基址
        // 当前仅存储于 Process.tls_base, 切换恢复未实装; x86_64 需在切换时写
        // MSR_FS_BASE、aarch64 写 tpidr_el0 (无用户态依赖, 为 Linux 线程
        // 兼容面预留, 待用户态线程库出现时实装)
        if tls != 0 {
            child
                .tls_base
                .store(tls, core::sync::atomic::Ordering::Release);
        }
    }

    // 注册到进程表
    api::process_insert(
        child as *const crate::framework::proc::Process as *mut crate::framework::proc::Process,
    );

    // CLONE_PARENT_SETTID
    if flags & CLONE_PARENT_SETTID != 0 && parent_tidptr != 0 {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_volatile(parent_tidptr as *mut i32, child_pid as i32);
        }
    }

    // CLONE_CHILD_SETTID: 写子进程 TID 到 child_tidptr (共享地址空间, 子可直接读到)
    if flags & CLONE_CHILD_SETTID != 0 && child_tidptr != 0 {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文); CLONE_VM 下父子共享
        // 地址空间, 此写入对子进程可见
        unsafe {
            core::ptr::write_volatile(child_tidptr as *mut i32, child_pid as i32);
        }
    }

    // CLONE_CHILD_CLEARTID: 登记清除地址, 子进程退出时写 0 并 futex 唤醒
    if flags & CLONE_CHILD_CLEARTID != 0 && child_tidptr != 0 {
        child
            .clear_child_tid
            .store(child_tidptr, core::sync::atomic::Ordering::Release);
    }

    // 添加到调度器
    let _ = child.set_state_safe(ProcessState::Ready);
    api::scheduler_add_to_run_queue(child_pid);

    crate::klog_debug!(
        Process,
        "[clone] parent={} child={} flags=0x{:X} (CLONE_VM)",
        parent_pid,
        child_pid,
        flags
    );

    i64::from(child_pid)
}
