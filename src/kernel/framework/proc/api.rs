//! 进程管理子系统 API 层
//!
//! 为内核其它模块提供进程/线程/调度的统一入口。
//!
//! ## 模块结构
//! - `proc_ops` — 进程创建/销毁/查询/操作 (`CProcess` + process_* 函数)
//! - `sched_ops` — 调度器操作 (scheduler_* 函数)
//! - `raw` — 裸指针/FFI 桥接
//!
//! ## 调用方契约
//! - `syscall::mod` — fork/execve/exit/wait4/kill/getpid 等系统调用
//! - `syscall::mmap` — mmap 通过 `process_get_current_pid` 获取当前进程
//! - `ipc::pipe/shm/signal` — IPC 操作需关联当前进程 PID 和 PWM
//! - `barrier::recovery` — 进程域纳入栏栈恢复
//! - `credo::session` — 会话管理器注册/注销进程
//! - `fs::procfs` — `/proc` 文件系统读取进程列表
//!
//! ## 安全约束
//! - `CURRENT_PROCESS_PTR` 用 `AtomicU64` 无锁读写,但 `C_CURRENT_PROCESS` 是 unsafe static mut
//! - `process_get_current()` 懒初始化 init 进程 (pid=1)
//! - `process_exit()` 必须在内核态调用,退出前切换到内核 CR3
//! - `PROCESS_TABLE` / `SCHEDULER` 均为全局单例,内部有锁保护
//!
//! ## 性能特征
//! - 进程查找: O(1) 哈希表
//! - 进程创建: O(N) PID 扫描 (N ≤ 65536)
//! - 上下文切换: asm stub, ~200 CPU cycles

use core::sync::atomic::Ordering;

use super::proc_ops;
use super::sched_ops;
use super::scheduler::SCHEDULER;
use super::user_proc::USER_PROC_MANAGER;
use crate::framework::mm::{PAGE_SIZE, pmm_alloc_pages, pmm_free_pages};

// 向后兼容 re-export: 将已拆分至 proc_ops / sched_ops 的函数重新导出
// 使外部代码仍可通过 `proc::api::*` 路径访问
pub use proc_ops::*;
pub use sched_ops::*;

// ============================================================================
// wait_queue 桩函数
// ============================================================================

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn wait_queue_init(_wq: *mut u8) {}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn wait_queue_add(_wq: *mut u8, _thread: u64) {}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn wait_queue_wake_one(_wq: *mut u8) {}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn wait_queue_wake_all(_wq: *mut u8) {}

// ============================================================================
// 会话/用户进程初始化
// ============================================================================

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn session_init() {
    super::session::SESSION_MANAGER.init();
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn user_proc_init() {
    USER_PROC_MANAGER.init();
}

// ============================================================================
// init 启动状态查询 (供 services 包装)
// ============================================================================

/// init 启动状态: 0=未启动, 1=initramfs 解压中, 2=init ELF 加载中, 3=已 Ring 3 进入
static INIT_STATUS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// 获取 init 启动状态
pub fn init_launch_status() -> u32 {
    INIT_STATUS.load(core::sync::atomic::Ordering::Acquire)
}

/// 由 `launch_first_user_process` 内部设置
fn set_init_status(s: u32) {
    INIT_STATUS.store(s, core::sync::atomic::Ordering::Release);
}

// ============================================================================
// 用户进程加载与进入
// ============================================================================

const ELF_MAX_SIZE: usize = 1024 * 1024;

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::borrow_as_ptr,
    reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
)]
#[expect(
    clippy::ptr_cast_constness,
    reason = "ptr_cast_constness: *mut T as *const T 是已知安全 (Rust 2024 可用 ptr.cast_const 或 &raw const; 当前优先 expect"
)]
pub extern "C" fn user_proc_load_elf(path: *const u8, pwm: u64) -> i32 {
    if path.is_null() {
        return -1;
    }

    let mut st: crate::framework::fs::VfsStat = crate::framework::fs::VfsStat::default();
    let stat_result = crate::framework::fs::vfs_stat(path, &mut st, pwm);
    if stat_result < 0 {
        return -1;
    }

    let file_size = u64::from(st.size);
    if file_size == 0 || file_size > ELF_MAX_SIZE as u64 {
        return -1;
    }

    let fd = crate::framework::fs::vfs_open(path, 0, pwm);
    if fd < 0 {
        return -1;
    }

    let pages = file_size.div_ceil(PAGE_SIZE) as usize;
    let buffer = pmm_alloc_pages(pages);
    if buffer.is_null() {
        crate::framework::fs::vfs_close(fd as u32);
        return -1;
    }

    let bytes_read = crate::framework::fs::vfs_read(fd as u32, buffer as *mut u8, file_size as u32);

    crate::framework::fs::vfs_close(fd as u32);

    if bytes_read <= 0 {
        pmm_free_pages(buffer, pages);
        return -1;
    }

    // PT_INTERP 改写: 检测 Linux 二进制, 将动态链接器路径改为 queenx elfld.so
    if super::elf::needs_interp_rewrite(buffer as *const u8, bytes_read as u64) {
        // SAFETY: buffer 是刚分配的内核页, bytes_read 有效范围内可写
        unsafe {
            super::elf::rewrite_interp_path(buffer as *mut u8, bytes_read as u64);
        }
    }

    let result =
        USER_PROC_MANAGER.load_elf_from_memory(buffer as *const u8, bytes_read as u64, pwm);

    pmm_free_pages(buffer, pages);

    result
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn user_proc_load_elf_from_memory(
    elf_data: *const u8,
    elf_size: u64,
    pwm: u64,
) -> i32 {
    USER_PROC_MANAGER.load_elf_from_memory(elf_data, elf_size, pwm)
}

/// 在 ELF 加载完成后, 在用户栈上建立 argv/envp (供 exec 系统调用使用)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
#[expect(
    clippy::similar_names,
    reason = "similar_names: 变量名相似表达同族概念; 当前优先 expect"
)]
///
/// # Safety
///
/// `name` 是合法的 C 字符串 (以 NUL 结尾). 进程表已初始化.
pub unsafe extern "C" fn user_proc_setup_argv(
    pid: u32,
    argv: *const *const u8,
    argc: u32,
    envp: *const *const u8,
    envc: u32,
) -> i32 {
    // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
    unsafe {
        let proc = match USER_PROC_MANAGER.get(pid) {
            Some(p) => p,
            None => return -1,
        };

        let sp = USER_PROC_MANAGER.setup_user_stack(proc, argv, argc as usize, envp, envc as usize);

        if sp == 0 { -1 } else { 0 }
    }
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn user_proc_enter_by_pid(pid: u32) -> i32 {
    crate::klog_boot_info!("[USER] user_proc_enter_by_pid: pid={}", pid);

    let (pid_val, pwm_val, state_val) = USER_PROC_MANAGER
        .with_process(pid, |proc| {
            (
                u64::from(proc.process().pid.0),
                proc.process().pwm.load(Ordering::SeqCst),
                proc.process().state.load(Ordering::SeqCst),
            )
        })
        .unwrap_or((0, 0, 0));

    crate::klog_boot_info!(
        "[USER] with_process result: pid_val={:#X} pwm_val={:#X} state_val={}",
        pid_val,
        pwm_val,
        state_val
    );

    if pid_val == 0 {
        crate::klog_boot_info!("[USER] pid_val is 0, returning -1");
        return -1;
    }

    proc_ops::C_CURRENT_PROCESS.map_mut(|p| {
        p.pid = pid_val;
        p.pwm = pwm_val;
        p.state = state_val;
        p.parent_pid = 1;
    });

    SCHEDULER.set_current(pid);

    crate::klog_boot_info!("[USER] calling USER_PROC_MANAGER.get({})", pid);
    USER_PROC_MANAGER.get(pid).map_or(-1, |proc| {
        // 诊断：打印从 Process 读取的 kernel_stack 值
        // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
        let p_kstack = unsafe {
            (*proc)
                .process()
                .kernel_stack
                .load(core::sync::atomic::Ordering::SeqCst)
        };
        crate::klog_boot_info!(
            "[USER] got proc={:#X}, Process.kstack={:#X}",
            proc as u64,
            p_kstack
        );
        crate::klog_boot_info!("[USER] calling enter()");
        USER_PROC_MANAGER.enter(proc);
        0
    })
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
#[expect(
    clippy::too_many_lines,
    reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
)]
pub extern "C" fn launch_first_user_process() -> ! {
    crate::klog_boot_info!("[USER] Launching init process...");

    // 1. 挂载 ramfs 为根文件系统
    crate::klog_boot_info!("[USER] Mounting ramfs...");
    let mount_result = crate::framework::fs::vfs_mount(b"/\0".as_ptr(), b"ramfs\0".as_ptr());
    crate::klog_boot_info!("[USER] ramfs mount result={}", mount_result);

    if mount_result < 0 {
        crate::klog_boot_info!("[USER] Warning: ramfs mount on / failed ({})", mount_result);
    }

    crate::klog_boot_info!("[USER] ramfs mount done, entering aarch64 path...");

    // 2. 解压 initramfs (如果启用 feature "initramfs")
    set_init_status(1);

    #[cfg(all(target_arch = "x86_64", feature = "initramfs"))]
    {
        let initramfs = include_bytes!("../../../../build/user/initramfs.cpio");
        if initramfs.len() > 0 {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            let result = unsafe {
                // DECOUPL-4: 使用 framework::fs::unpack 顶层路径
                crate::framework::fs::unpack(initramfs.as_ptr(), initramfs.len())
            };
            match result {
                Ok(count) => {
                    crate::klog_boot_info!("[USER] initramfs: {} files unpacked", count);
                    set_init_status(2);
                    let pid = user_proc_load_elf(b"/init\0".as_ptr(), 0);
                    if pid > 0 {
                        let pid_u32 = pid as u32;
                        proc_ops::C_CURRENT_PROCESS.map_mut(|p| {
                            p.pid = u64::from(pid_u32);
                            p.pwm = 0;
                            p.state = 2;
                            p.parent_pid = 1;
                        });
                        SCHEDULER.add(pid_u32);
                        set_init_status(3);
                        crate::klog_boot_info!(
                            "[USER] Entering Ring 3 (init from /init, pid={})...",
                            pid_u32
                        );
                        user_proc_enter_by_pid(pid_u32);
                    }
                    crate::klog_boot_info!("[USER] /init not found, falling back to init.bin");
                }
                Err(e) => {
                    crate::klog_boot_info!("[USER] initramfs unpack failed: {}", e);
                }
            }
        }

        // 回退: 直接加载内嵌的 init.bin
        let bin = include_bytes!("../../../../build/user/init.bin");
        let bin_ptr = bin.as_ptr();
        let bin_size = bin.len() as u64;

        if bin_size == 0 {
            crate::klog_err!(Boot, "[USER] init binary is empty");
            crate::framework::tests::qemu_exit(false);
        }

        let pid = USER_PROC_MANAGER.load_elf_from_memory(bin_ptr, bin_size, 0);
        if pid <= 0 {
            crate::klog_err!(Boot, "[USER] Failed to load init ELF, pid={}", pid);
            crate::framework::tests::qemu_exit(false);
        }

        let pid_u32 = pid as u32;

        proc_ops::C_CURRENT_PROCESS.map_mut(|p| {
            p.pid = u64::from(pid_u32);
            p.pwm = 0;
            p.state = 2;
            p.parent_pid = 1;
        });

        SCHEDULER.add(pid_u32);

        crate::klog_boot_info!("[USER] Entering Ring 3 (init pid={})...", pid_u32);
        user_proc_enter_by_pid(pid_u32);
    }

    #[cfg(all(target_arch = "x86_64", not(feature = "initramfs")))]
    {
        // 产物桩化: init.bin 是裸机产物 (build/ 被 gitignore, 仅由 make 生成).
        // 判据用 target_os 而非 feature —— 本块 gate 为 all(target_arch = "x86_64",
        // not(feature = "initramfs")), host target 的 host-test / kernel_test 两个
        // lint 维度与 host-tests 都会编译它, 不桩化即让 host 侧编译硬依赖裸机产物
        // (build.rs G-06「消除隐式 make 耦合」的漏项). 本函数为裸机 init 入口
        // (extern "C"), host 不可达, 取空切片 ⇒ 走下方 bin_size == 0 报错退出分支.
        #[cfg(target_os = "none")]
        let bin = include_bytes!("../../../../build/user/init.bin");
        #[cfg(not(target_os = "none"))]
        let bin: &[u8] = &[];
        let bin_ptr = bin.as_ptr();
        let bin_size = bin.len() as u64;

        if bin_size == 0 {
            crate::klog_err!(Boot, "[USER] init binary is empty");
            crate::framework::tests::qemu_exit(false);
        }

        let pid = USER_PROC_MANAGER.load_elf_from_memory(bin_ptr, bin_size, 0);
        if pid <= 0 {
            crate::klog_err!(Boot, "[USER] Failed to load init ELF, pid={}", pid);
            crate::framework::tests::qemu_exit(false);
        }

        let pid_u32 = pid as u32;

        proc_ops::C_CURRENT_PROCESS.map_mut(|p| {
            p.pid = u64::from(pid_u32);
            p.pwm = 0;
            p.state = 2;
            p.parent_pid = 1;
        });

        SCHEDULER.add(pid_u32);

        crate::klog_boot_info!("[USER] Entering Ring 3 (init pid={})...", pid_u32);
        user_proc_enter_by_pid(pid_u32);
    }

    #[cfg(target_arch = "aarch64")]
    {
        // 禁用 IRQ 以防止 timer 中断在 ELF 加载/进程创建期间干扰,
        // 导致非确定性挂起 (PMM 分配/页表操作/调度器状态不一致).
        // SAFETY: 后续 enter_user 设置 SPSR_EL1 时 DAIF 全屏蔽, EL0 init
        // 进程自行启用中断, 此处无需恢复.
        let _saved = crate::arch!(interrupt_disable());
        crate::klog_boot_info!("[USER] aarch64: starting init ELF load...");

        let bin = include_bytes!("../../../../build/user/init.bin");

        let bin_ptr = bin.as_ptr();

        let bin_size = bin.len() as u64;

        crate::klog_boot_info!("[USER] aarch64: init ELF size={}", bin_size);

        if bin_size == 0 {
            crate::klog_boot_info!("[USER] init ELF is empty");
            loop {
                crate::arch!(halt());
            }
        }

        crate::klog_boot_info!("[USER] aarch64: bin_size check passed");

        crate::klog_boot_info!("[USER] aarch64: bin_ptr={:#X}", bin_ptr as u64);

        crate::klog_boot_info!("[USER] aarch64: calling load_elf_from_memory...");
        let pid = USER_PROC_MANAGER.load_elf_from_memory(bin_ptr, bin_size, 0);
        crate::klog_boot_info!("[USER] aarch64: load_elf_from_memory returned pid={}", pid);
        if pid <= 0 {
            crate::klog_boot_info!("[USER] Failed to load init ELF");
            loop {
                crate::arch!(halt());
            }
        }

        let pid_u32 = pid as u32;

        proc_ops::C_CURRENT_PROCESS.map_mut(|p| {
            p.pid = u64::from(pid_u32);
            p.pwm = 0;
            p.state = 2;
            p.parent_pid = 1;
        });

        crate::klog_boot_info!("[USER] aarch64: adding to scheduler...");
        SCHEDULER.add(pid_u32);

        crate::klog_boot_info!("[USER] Entering EL0 (init pid={})...", pid_u32);
        user_proc_enter_by_pid(pid_u32);
    }

    loop {
        crate::arch!(halt());
    }
}
