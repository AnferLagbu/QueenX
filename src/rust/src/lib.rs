// B08-12: 顶层约束按 host-test feature 门控 — host 编译 (std) 时剥离裸机专属约束:
// no_std → std (host-test 下启用 std 提供 panic/alloc handler); no_main → 测试 harness 提供 main;
// alloc_error_handler 为 no_std 专属 nightly feature, host-test 下 std 自带.
#![cfg_attr(not(feature = "host-test"), no_std)]
#![cfg_attr(not(feature = "host-test"), no_main)]
#![cfg_attr(not(feature = "host-test"), feature(alloc_error_handler))]
// I-09: 移除 `#![feature(asm)]`. nightly 1.97 中 `core::arch::asm!` 已稳定,
// 源码中所有 asm 调用已走 `core::arch::asm!`, 顶层 feature gate 不再需要.
// ============================================================================
// ✅ 全局警告抑制配置 (内核开发环境特有)
// ============================================================================

//! 允许的警告类别 (符合 OS 内核开发最佳实践)

// I-09: 移除 `#![allow(stable_features)]` — 该 allow 仅用于 asm feature
// 声明, 已一并移除, 不再有 unstable 特性走 stable 路径.

// 1. 全局单例模式 - 内核中常见且必要
#![allow(static_mut_refs)]
// 32个: TRUST_CHAIN, TOKEN_MANAGER 等全局可变静态

// 2. C 语言兼容性 - 与原有 C 代码保持一致的命名风格
#![allow(non_upper_case_globals)] // 函数名: kfree, kmalloc 等
#![allow(non_camel_case_types)] // 类型名: u8_t, u32_t 等
#![allow(non_snake_case)] // 变量名: io_port 等

// 4. FFI 边界 - C/Rust 互操作不可避免
#![allow(improper_ctypes)]
// 3个: IrqSaveFlags, FFI 类型

// 5. 安全相关 - 已通过代码审查确认安全
#![allow(unused_unsafe)]
// 16个: 过度保守的 unsafe 块

// 6. Clippy: 内核代码中原始指针解引用是固有操作，由调用者保证安全性
#![allow(clippy::not_unsafe_ptr_arg_deref)]
// 7. Clippy: 内核内部 &self → &mut T 模式（如 Mutex::get_mut、UnsafeCell 包装）
#![allow(clippy::mut_from_ref)]
// 8. Clippy: 内核 C 字符串字面量 — 多用于 FFI，接收方类型多样（*const u8/i8/c_char）
#![allow(clippy::manual_c_str_literals)]
// 9. Clippy: 内核文档注释风格 — 模块级 doc comment 后空行是既存惯例
#![allow(clippy::empty_line_after_doc_comments)]
// 10. Clippy: Result<_, ()> — 内核错误路径使用 () 作为错误值是有意设计
#![allow(clippy::result_unit_err)]
// rustfmt 整改后函数体行数普遍增长 10-20%, 原 100 行阈值过紧; 放宽至 200
#![allow(clippy::too_many_lines)]
// 11. Clippy: module_inception — 内核模块命名（如 fs/hvfs/hvfs.rs）是架构惯例
#![allow(clippy::module_inception)]
// 12. Clippy: new_without_default — 内核对象通常不应有无参默认构造
#![allow(clippy::new_without_default)]
// 13. Clippy: collapsible_if — 内核路径中的 if 嵌套有时是为了可读性
#![allow(clippy::collapsible_if)]
// 14. Clippy: single_match — match 单分支有时比 if-let 更清晰地表明穷尽性
#![allow(clippy::single_match)]
// 15. Clippy: too_many_arguments — 内核 API 参数数量由协议决定
#![allow(clippy::too_many_arguments)]
// 16. Clippy: type_complexity — 内核类型天然复杂（Box<dyn Fn> 等）
#![allow(clippy::type_complexity)]
// 17. Clippy: transmute_ptr_to_ptr — 内核 FFI 中的指针转换是显式约定
#![allow(clippy::transmute_ptr_to_ptr)]
// 18. Clippy: missing_transmute_annotations
#![allow(clippy::missing_transmute_annotations)]
// 19. Clippy: let_and_return — 内核错误路径中中间变量有助于可读性
#![allow(clippy::let_and_return)]
// 20. Clippy: wrong_self_convention — 内核 to_*/as_* 的 self 约定与 std 不同
#![allow(clippy::wrong_self_convention)]
// 21. Clippy: needless_range_loop — 内核中部分循环显式索引是有意可读性选择
#![allow(clippy::needless_range_loop)]
// 22. Clippy: manual_find — 显式循环比 .find() 在某些场景更清晰
#![allow(clippy::manual_find)]
// 23. Clippy: unnecessary_cast
#![allow(clippy::unnecessary_cast)]
// 24. Clippy: double_parens — 已迁出 (经脚本逐个审查确认无触发, 移除 allow)
// 25. Clippy: unnecessary_lazy_evaluations
#![allow(clippy::unnecessary_lazy_evaluations)]
// 26. Clippy: manual_div_ceil
#![allow(clippy::manual_div_ceil)]
// 27. Clippy: match_like_matches_macro — 已迁出 (经脚本逐个审查确认无触发, 移除 allow)
// 28. Clippy: manual_unwrap_or_default / manual_unwrap_or — 显式 match 更清晰
#![allow(clippy::manual_unwrap_or_default)]
#![allow(clippy::manual_unwrap_or)]
// 29. Clippy: unnecessary_map_or — 显式 map_or 可读性更好
#![allow(clippy::unnecessary_map_or)]
// 30. Clippy: derivable_impls — 部分内核 impl 有文档注释需要保留
#![allow(clippy::derivable_impls)]
// 31. Clippy: manual_checked_ops — 内核显式检查除法更直观
#![allow(clippy::manual_checked_ops)]
// 32. Clippy: question_mark — 内核错误路径保留显式 match 更清晰
#![allow(clippy::question_mark)]
// 33. Clippy: manual_range_patterns — 显式范围 vs range pattern 可读性各有优劣
#![allow(clippy::manual_range_patterns)]
// 34. Clippy: manual_flatten — 显式 if-let 比 .flatten() 更清晰
#![allow(clippy::manual_flatten)]
// 35. Clippy: collapsible_match — 保留嵌套 match 结构
#![allow(clippy::collapsible_match)]
// 36. Clippy: let_unit_value — 已迁出 (经脚本逐个审查确认无触发, 移除 allow)
// 37. Clippy: empty_loop — 内核自旋等待
#![allow(clippy::empty_loop)]
// 38. Clippy: explicit_counter_loop — 显式计数器在测试代码中更直观
#![allow(clippy::explicit_counter_loop)]
// 39. Clippy: pointers_in_nomem_asm_block — 内核 ASM 代码必须传指针
#![allow(clippy::pointers_in_nomem_asm_block)]
// 40. Clippy: empty_line_after_outer_attr — 已迁出 (经脚本逐个审查确认无触发, 移除 allow)
// 41. Clippy: doc_lazy_continuation / doc_overindented_list_items — 内核文档风格
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::doc_overindented_list_items)]
// 42. Clippy: must_use_candidate — 内核内部 API 大量返回 Result/Option, 全标注 #[must_use] 会增加 200+ 行噪音
//     重要公共 API 在函数定义处已加 #[must_use]; 内部 helper 不强制
#![allow(clippy::must_use_candidate)]
// 43. Clippy: unreadable_literal — 内核硬件常量 (MMIO 地址/位掩码/魔数) 经常是固定位模式, 加下划线分隔反而降低可读性
//     与硬件规范直接对齐 (如 0xDEADBEEF, 0x74726976 = "virt" 小端); 改下划线会影响阅读与 SPEC 比对
#![allow(clippy::unreadable_literal)]
// 44. Clippy: inline_always — 内核大量 #[inline(always)] 是性能关键 (中断处理/锁内热路径); 全局保持显式标注
#![allow(clippy::inline_always)]
// 45. Clippy: large_stack_arrays — 已迁出 (经脚本逐个审查确认无触发, 移除 allow)
// 46. Clippy: struct_field_names / pub_underscore_fields — 结构体字段命名约定是模块内风格 (如 page_state 含 page_* 字段); pub _xxx 是内核模块内 convention
#![allow(clippy::struct_field_names)]
#![allow(clippy::pub_underscore_fields)]
// 47. Clippy: struct_excessive_bools — 状态标志结构体用多个 bool 字段是常见模式; 当前实现无需重构
#![allow(clippy::struct_excessive_bools)]
// 48. Clippy: doc_markdown — 内核文档使用中文 + 硬件术语 (MMIO/MSI/APIC 等) 不加反引号是约定; 阶段 3 已处理部分
#![allow(clippy::doc_markdown)]
// 49. Clippy: ptr_as_ptr — 部分 macro (如 klog_fmt) 内部 ptr cast 无法 expect 兜底; 真实代码 expect 已兑底
#![allow(clippy::ptr_as_ptr)]
// 50. Clippy: cast_ptr_alignment — 部分 MMIO 寄存器地址已知对齐 (硬件规范); macro 内 cast_ptr_alignment 无法 expect
#![allow(clippy::cast_ptr_alignment)]
// 51. Clippy: zero_sized_map_values / missing_fields_in_debug — 内核 struct 字段设计选择; 当前实现合理
#![allow(clippy::zero_sized_map_values)]
#![allow(clippy::missing_fields_in_debug)]
// 52. Clippy: cast_lossless — 已迁出 (经脚本逐个审查确认无触发, 移除 allow)
// 53. Clippy: duplicated_attributes — 已迁出 (经脚本逐个审查确认无触发, 移除 allow)

// B08-12 注: host-test (std) 下 alloc 经 extern prelude 可见, 此处无条件声明不会产生
// 重复 lang item — 之前观察到的 E0152 duplicate lang item 是 src/rust/.cargo/config.toml
// 的 build-std 配置在 src/rust 目录内运行时的假象 (host-tests 从仓库根/host-tests 构建时
// 不加载该 config). 门控掉 extern crate alloc 反而导致 std 模式下 alloc:: 路径不可解析.
extern crate alloc;

// B08-12: 全局分配器 (KernelAllocator) 为裸机专属 — host-test (std) 下禁用,
// 避免 Rust 全局分配 (含 std 自身初始化) 走内核 kmalloc → IrqSpinLock → cli (SIGSEGV).
#[cfg(not(feature = "host-test"))]
mod memory_allocator;

// ============================================================================
// 内核模块 (统一从 kernel/ 入口)
// ============================================================================

/// QueenX 内核 - 所有子系统的统一入口
///
/// ## 模块结构
///
/// ```text
/// kernel/
/// ├── framework/   # 特权 TCB (唯一允许 unsafe, 硬件/MMU/中断/上下文切换)
/// │   ├── arch/     # 架构相关 (x86_64 + aarch64: gdt/idt/tss/apic/mmu/gic/...)
/// │   ├── boot/     # 引导协议 (multiboot2 / aarch64 entry)
/// │   ├── cpu/      # CPU 探测 (cpuid/msr/tsc/拓扑)
/// │   ├── mm/       # 内存管理 (PMM/VMM/Kmalloc/KPTI)
/// │   ├── proc/     # 进程/线程 TCB (user_proc/switch.asm)
/// │   ├── idt/irq/  # 中断与异常处理
/// │   ├── sync/     # 同步原语 (spinlock/mutex/rwlock/rcu)
/// │   ├── driver/   # 原生硬件驱动 (display/net/input/char/bus)
/// │   ├── net/      # 网络硬件 + 协议栈
/// │   ├── fs/       # 文件系统底层 (VFS 抽象)
/// │   ├── dma/      # DMA 引擎
/// │   ├── credo/    # 身份/密码学 (能力矩阵/secure_boot)
/// │   ├── chitin/   # 设备驱动框架 (user_driver/composite/devtree)
/// │   ├── barrier/  # 弹性恢复底层
/// │   ├── wasm/     # WASM 沙箱
/// │   ├── syscall/  # 系统调用入口 (TCB 侧)
/// │   └── ...       # alloc/console/klog/config/smp/io/ipc/pci/timer 等
/// └── services/     # 去特权业务层 (100% safe Rust, #![deny(unsafe_code)])
///     ├── syscall/  # 系统调用分发策略 (完整 syscall→handler 映射)
///     ├── proc/     # 进程管理策略 (CFS 调度/进程表安全代理)
///     ├── fs/       # VFS + ramfs + HvFS + devfs + procfs
///     ├── net/      # 网络栈 (smoltcp) + socket 层
///     ├── ipc/      # 管道/共享内存/消息队列/信号
///     ├── mm/       # Page Cache/Swap/mmap 安全代理
///     ├── credo/    # 身份与权限策略
///     ├── barrier/  # 栏栈恢复策略
///     ├── chitin/   # 设备驱动框架
///     ├── driver/   # 设备驱动业务 (HDMI 时序等)
///     ├── io/       # io_uring 异步 I/O
///     ├── timer/    # 定时器子系统
///     ├── wasm/     # WASM 沙箱
///     └── ...       # config/console/klog/sync/init/debug/userctx 等
/// ```
#[path = "../../kernel/mod.rs"]
pub mod kernel;

// 重新导出常用类型 (方便直接使用 crate::xxx 而非 crate::kernel::xxx)
pub use kernel::framework::cpu::CpuInfo;
pub use kernel::framework::klog::LogLevel;

// B08-12: 以下符号仅 panic_handler/kernel_test 路径使用, host-test 下为死代码
#[cfg(not(feature = "host-test"))]
use core::panic::PanicInfo;
#[cfg(not(feature = "host-test"))]
use core::sync::atomic::Ordering;

// B08-12: host-test 下 panic 由 std 提供, 内核 panic_handler 仅裸机/内核测试模式生效
#[cfg(not(feature = "host-test"))]
#[panic_handler]
#[expect(
    clippy::too_many_lines,
    reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
)]
fn panic(info: &PanicInfo) -> ! {
    crate::kernel::framework::barrier::PANIC_FLAG.store(true, Ordering::SeqCst);

    // 诊断 (TRACK-INIT-RING3-PANIC): 先直接输出 panic location (file:line),
    // 绕过 PanicInfo::Display 格式化 (其内部 slice 索引在栈/数据被破坏时
    // 可能递归 panic → 无法看到原始 panic 点). location 是静态字符串, 不分配.
    if crate::kernel::framework::klog::KLOG_INIT.load(Ordering::Acquire) {
        if let Some(loc) = info.location() {
            crate::kernel::framework::klog::serial_write_bytes(b"\n[PANIC LOC] ");
            crate::kernel::framework::klog::serial_write_bytes(loc.file().as_bytes());
            crate::kernel::framework::klog::serial_write_bytes(b":");
            let mut line_buf = [0u8; 16];
            let mut idx = 16usize;
            let mut n = u64::from(loc.line());
            if n == 0 {
                line_buf[0] = b'0';
                idx = 0;
            } else {
                while n > 0 && idx > 0 {
                    idx -= 1;
                    line_buf[idx] = b'0' + (n % 10) as u8;
                    n /= 10;
                }
            }
            crate::kernel::framework::klog::serial_write_bytes(&line_buf[idx..]);
            crate::kernel::framework::klog::serial_write_bytes(b"\n");
        }
    }

    // 修复 (TRACK-INIT-RING3-PANIC): 中断上下文 panic 时禁止分配内存.
    // 原实现 `alloc::format!` 分配 String → k_malloc → KernelHeap IrqSpinLock,
    // 若 panic 发生在 IRQ 上下文 (如中断路径内存分配) 会递归 panic → 跳 0x0 #UD.
    // 改用栈缓冲 CursorWriter (framework::klog, 纯 core::fmt 不分配) 格式化消息.
    let mut msg_buf = [0u8; 256];
    let mut msg_cursor: usize = 0;
    let _ = core::fmt::write(
        &mut crate::kernel::framework::klog::CursorWriter::new(
            &mut msg_buf,
            &mut msg_cursor,
        ),
        format_args!("{info}"),
    );
    let msg: &str = core::str::from_utf8(&msg_buf[..msg_cursor]).unwrap_or("PANIC (fmt failed)");
    let bytes = msg.as_bytes();
    let len = bytes.len().min(127);
    {
        let mut panic_msg = crate::kernel::framework::barrier::PANIC_MSG.lock();
        panic_msg[..len].copy_from_slice(&bytes[..len]);
        panic_msg[len] = 0;
    }

    // 先捕获寄存器状态 — 在所有架构都需要
    #[allow(unused_mut)]
    let mut regs: [u64; 16] = [0; 16];
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "mov {0}, rax", "mov {1}, rbx", "mov {2}, rcx",
            "mov {3}, rdx", "mov {4}, rsi", "mov {5}, rdi",
            out(reg) regs[0], out(reg) regs[1], out(reg) regs[2],
            out(reg) regs[3], out(reg) regs[4], out(reg) regs[5],
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "mov {0}, rbp", "mov {1}, rsp", "mov {2}, r8",
            "mov {3}, r9",  "mov {4}, r10", "mov {5}, r11",
            out(reg) regs[6], out(reg) regs[7], out(reg) regs[8],
            out(reg) regs[9], out(reg) regs[10], out(reg) regs[11],
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "mov {0}, r12", "mov {1}, r13", "mov {2}, r14", "mov {3}, r15",
            out(reg) regs[12], out(reg) regs[13], out(reg) regs[14], out(reg) regs[15],
            options(nostack, preserves_flags)
        );
    }
    let reg_names: [[u8; 4]; 16] = [
        *b"RAX ", *b"RBX ", *b"RCX ", *b"RDX ", *b"RSI ", *b"RDI ", *b"RBP ", *b"RSP ", *b"R8  ",
        *b"R9  ", *b"R10 ", *b"R11 ", *b"R12 ", *b"R13 ", *b"R14 ", *b"R15 ",
    ];
    #[allow(unused_mut, unused_assignments)]
    let mut cr2: u64 = 0;
    #[allow(unused_mut, unused_assignments)]
    let mut cr3_val: u64 = 0;
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
        core::arch::asm!("mov {}, cr3", out(reg) cr3_val);
    }

    // 1. 串口输出崩溃信息
    if crate::kernel::framework::klog::KLOG_INIT.load(Ordering::Acquire) {
        crate::kernel::framework::klog::serial_write_bytes(
            b"\n========== KERNEL PANIC ==========\n",
        );
        crate::kernel::framework::klog::serial_write_bytes(msg.as_bytes());
        crate::kernel::framework::klog::serial_write_bytes(b"\n--- Register Dump ---\n");
        for i in 0..16 {
            crate::kernel::framework::klog::serial_write_bytes(b"  ");
            crate::kernel::framework::klog::serial_write_bytes(&reg_names[i]);
            crate::kernel::framework::klog::serial_write_bytes(b"= 0x");
            let mut hex_buf = [0u8; 16];
            let v = regs[i];
            for (d, item) in hex_buf.iter_mut().enumerate() {
                let nibble = ((v >> (60 - d * 4)) & 0xF) as u8;
                *item = if nibble < 10 {
                    b'0' + nibble
                } else {
                    b'a' + nibble - 10
                };
            }
            crate::kernel::framework::klog::serial_write_bytes(&hex_buf);
            if i % 4 == 3 {
                crate::kernel::framework::klog::serial_write_bytes(b"\n");
            }
        }
        crate::kernel::framework::klog::serial_write_bytes(b"  CR2= 0x");
        for d in 0..16 {
            let nibble = ((cr2 >> (60 - d * 4)) & 0xF) as u8;
            crate::kernel::framework::klog::serial_write_bytes(&[if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + nibble - 10
            }]);
        }
        crate::kernel::framework::klog::serial_write_bytes(b"  CR3= 0x");
        for d in 0..16 {
            let nibble = ((cr3_val >> (60 - d * 4)) & 0xF) as u8;
            crate::kernel::framework::klog::serial_write_bytes(&[if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + nibble - 10
            }]);
        }
        crate::kernel::framework::klog::serial_write_bytes(
            b"\n===================================\n",
        );
    }

    // 2. 图形控制台输出崩溃信息
    crate::kernel::framework::console::gfx_console_panic_reclaim(msg);
    crate::kernel::framework::console::gfx_console_panic_write("\n--- Register Dump ---\n");
    for i in 0..16 {
        let mut buf = [0u8; 64];
        let mut cursor: usize = 0;
        write_hex_to_buf(&mut buf, &mut cursor, regs[i]);
        // 修复 (TRACK-INIT-RING3-PANIC): 原 `alloc::format!` 在中断上下文 panic
        // 时分配内存 → 递归 panic → 栈破坏. 改用栈缓冲 CursorWriter (不分配).
        let mut line_buf = [0u8; 64];
        let mut line_cur = 0usize;
        let _ = core::fmt::write(
            &mut crate::kernel::framework::klog::CursorWriter::new(
                &mut line_buf,
                &mut line_cur,
            ),
            format_args!(
                "  {} = {}\n",
                core::str::from_utf8(&reg_names[i]).unwrap_or("?? "),
                core::str::from_utf8(&buf[..cursor]).unwrap_or("?")
            ),
        );
        let line = core::str::from_utf8(&line_buf[..line_cur]).unwrap_or("?\n");
        crate::kernel::framework::console::gfx_console_panic_write(line);
    }
    {
        let mut cr2_str = [0u8; 32];
        let mut cur: usize = 0;
        write_hex_to_buf(&mut cr2_str, &mut cur, cr2);
        let mut line_buf = [0u8; 40];
        let mut line_cur = 0usize;
        let _ = core::fmt::write(
            &mut crate::kernel::framework::klog::CursorWriter::new(
                &mut line_buf,
                &mut line_cur,
            ),
            format_args!(
                "  CR2= {}\n",
                core::str::from_utf8(&cr2_str[..cur]).unwrap_or("?")
            ),
        );
        let line = core::str::from_utf8(&line_buf[..line_cur]).unwrap_or("?\n");
        crate::kernel::framework::console::gfx_console_panic_write(line);
        let mut cr3_str = [0u8; 32];
        let mut c3: usize = 0;
        write_hex_to_buf(&mut cr3_str, &mut c3, cr3_val);
        let mut line_buf2 = [0u8; 40];
        let mut line_cur2 = 0usize;
        let _ = core::fmt::write(
            &mut crate::kernel::framework::klog::CursorWriter::new(
                &mut line_buf2,
                &mut line_cur2,
            ),
            format_args!(
                "  CR3= {}\n",
                core::str::from_utf8(&cr3_str[..c3]).unwrap_or("?")
            ),
        );
        let line2 = core::str::from_utf8(&line_buf2[..line_cur2]).unwrap_or("?\n");
        crate::kernel::framework::console::gfx_console_panic_write(line2);
    }

    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("int 0x82", options(noreturn));
    }
    #[cfg(target_arch = "aarch64")]
    {
        // AArch64 栏栈恢复: 直接调用恢复逻辑进行域回滚
        unsafe extern "C" {
            fn recovery_try_recover_from_idt() -> i32;
        }
        let result = unsafe { recovery_try_recover_from_idt() };
        if result >= 0 {
            // 域状态已回滚到一致快照, 记录恢复事件
            crate::kernel::framework::klog::serial_write_bytes(
                b"\n[RECOVERY] Barrier-stack: domain rolled back\n",
            );
            crate::kernel::framework::barrier::PANIC_FLAG
                .store(false, core::sync::atomic::Ordering::SeqCst);
        } else {
            crate::kernel::framework::klog::serial_write_bytes(
                b"\n[RECOVERY] Barrier-stack: recovery failed, halting\n",
            );
        }
        loop {
            unsafe {
                core::arch::asm!("wfi");
            }
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

// B08-12: 仅 panic_handler 使用, host-test 下为死代码
#[cfg(not(feature = "host-test"))]
fn write_hex_to_buf(buf: &mut [u8], cursor: &mut usize, value: u64) {
    for d in 0..16 {
        if *cursor >= buf.len() {
            break;
        }
        let nibble = ((value >> (60 - d * 4)) & 0xF) as u8;
        buf[*cursor] = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
        *cursor += 1;
    }
}

#[cfg(not(test))]
// B08-12: host-test 下 std 自带 alloc error handler, 内核版仅裸机/内核测试模式生效
#[cfg(not(feature = "host-test"))]
#[alloc_error_handler]
fn alloc_error(layout: alloc::alloc::Layout) -> ! {
    panic!("Allocation error: {layout:?}");
}

/// 内核启动入口 (引导跳转目标).
///
/// 按依赖顺序初始化各子系统: `KLog` → Boot 栈 canary 校验 → 配置校验 →
/// 架构初始化 → 内存/进程/网络等子系统 → 进入用户态.
///
/// # Panics
/// Boot 栈 canary 校验失败 (栈溢出至栈底) 时立即 panic, 断言内核状态不可信.
#[unsafe(no_mangle)]
// J-01 (2026-09-08): used_underscore_binding expect 仅裸机分支 (not kernel_test) 生效 —
// kernel_test 分支无 `_xxx` 绑定使用, expect 在 feature 下 unfulfilled
#[cfg_attr(
    not(feature = "kernel_test"),
    expect(
        clippy::used_underscore_binding,
        reason = "下划线前缀表示私有约定或局部清理; 重命名需追改所有访问点, 风险高"
    )
)]
#[expect(
    clippy::too_many_lines,
    reason = "函数体超 100 行 (复杂度阈值); 拆分需追改调用链且增加间接层, 当前任务优先 expect 兑底"
)]
#[expect(
    clippy::unreadable_literal,
    reason = "unreadable_literal: 长数字常量无下划线分隔; 内核硬件常量 (MMIO 地址/位掩码) 已知精确值, 当前优先 expect"
)]
pub extern "C" fn kernel_init() {
    // 0. KLog — 自举串口驱动, 必须先于所有子系统
    unsafe {
        crate::kernel::framework::klog::klog_init();
    }
    crate::klog_boot_info!("QueenX starting");

    // 0.05. Boot 栈 canary 验证 — 检测 trampoline → kernel_init 路径上的栈溢出.
    // canary 在 boot.asm trampoline64_high (x86_64) 或 entry.rs (aarch64) 写入 stack_bottom,
    // 若被覆盖则说明 boot 栈已溢出至栈底, 内核状态不可信, 立即 panic.
    assert!(
        crate::kernel::framework::proc::check_boot_stack_canary(),
        "[BOOT] stack canary corrupted! Boot stack overflow detected. \
         Stack size=256KB, canary at stack_bottom was overwritten \
         during trampoline→kernel_init transition."
    );
    crate::klog_boot_info!("Boot stack canary verified");

    // 0.1. Config validation — 验证系统配置一致性
    // Must be called after klog_init for error reporting
    crate::kernel::framework::config::init();
    crate::klog_boot_info!("Configuration validated");

    // Test mode: skip normal init, run unit tests
    #[cfg(feature = "kernel_test")]
    {
        // J-01 (2026-09-08): 三个 const 集中块首 (items_after_statements 清理) —
        // kmalloc 堆大小与 PMM bitmap 间隙布局常量, 供下方初始化步骤使用
        const KMALLOC_HEAP_SIZE: u64 = 16 * 1024 * 1024;
        const GAP_SIZE: u64 = 0x200000;
        const BITMAP_GAP_SIZE: u64 = 0x200000;

        // Validate configuration even in test mode
        crate::kernel::framework::config::init();

        <crate::kernel::framework::arch::CurrentArch as crate::kernel::framework::arch::InterruptArch>::interrupt_disable(
        );

        let boot_info = crate::kernel::framework::boot::init();
        crate::kernel::framework::mm::pmm::pmm_init(boot_info.mem_size, boot_info.kernel_end);
        crate::kernel::framework::mm::vmm::vmm_init();
        let heap_start = crate::kernel::framework::mm::VirtAddr(
            crate::kernel::framework::mm::KERNEL_BASE + boot_info.kernel_end + 0x200000,
        );
        unsafe {
            crate::kernel::framework::mm::kmalloc::get_kmalloc_mut()
                .init(heap_start, KMALLOC_HEAP_SIZE);
        }
        // 诊断: kmalloc init 后检查页表
        {
            let read_u64 = |phys: u64, idx: usize| -> u64 {
                let va = phys + crate::kernel::framework::mm::KERNEL_BASE + idx as u64 * 8;
                unsafe { core::ptr::read_volatile(va as *const u64) }
            };
            let pd24 = read_u64(0x109000, 24);
            let pd63 = read_u64(0x109000, 63);
            crate::klog_boot_info!(
                "[PAGETABLE] after kmalloc: pd[24]=0x{:016X} pd[63]=0x{:016X}",
                pd24,
                pd63
            );
        }
        // 必须包含 kernel_end 到 heap_start 之间的 2MB 间隙，
        // 否则 PMM bitmap 会放在 kmalloc 堆内部，导致 bitmap 与堆数据互相覆盖。
        // 必须包含 heap_end 到 bitmap 之间的 2MB 间隙，
        // 否则 bitmap 与 heap 共享同一个 2MB 块，heap 扩展拆分 2MB 巨页时会覆盖 bitmap 的 PTE。
        // GAP_SIZE + KMALLOC_HEAP_SIZE + BITMAP_GAP_SIZE = 0x200000 + 16MB + 0x200000 = 20MB
        crate::kernel::framework::mm::pmm::pmm_init_bitmap(
            GAP_SIZE + KMALLOC_HEAP_SIZE + BITMAP_GAP_SIZE,
        );

        // 诊断: dump 页表关键条目 (PML4[256]→pdpt_high[0]→pd[24]/[63])
        {
            let read_u64 = |phys: u64, idx: usize| -> u64 {
                let va = phys + crate::kernel::framework::mm::KERNEL_BASE + idx as u64 * 8;
                unsafe { core::ptr::read_volatile(va as *const u64) }
            };
            let pml4_256 = read_u64(0x102000, 256);
            let pdpt_0 = read_u64(0x104000, 0);
            let pd24 = read_u64(0x109000, 24);
            let pd63 = read_u64(0x109000, 63);
            let pd0 = read_u64(0x109000, 0);
            crate::klog_boot_info!(
                "[PAGETABLE] pml4[256]=0x{:016X} pdpt[0]=0x{:016X} pd[0]=0x{:016X} pd[24]=0x{:016X} pd[63]=0x{:016X}",
                pml4_256,
                pdpt_0,
                pd0,
                pd24,
                pd63
            );
        }

        // I-24 启动顺序契约: GDT/TSS init (set_ist[0..4]) 必须在 IDT init 之前.
        // cpu_init() 在 gdt_init() 之前调用 (kpti_init 依赖 has_invpcid() → get_cpu_info() → cpu_init).
        // 正常路径由 interrupt_late_init 处理; 测试模式需显式调用.
        #[cfg(target_arch = "x86_64")]
        {
            crate::kernel::framework::cpu::cpu_init();
            crate::kernel::framework::arch::x86_64::gdt::gdt_init();
        }

        <crate::kernel::framework::arch::CurrentArch as crate::kernel::framework::arch::Arch>::interrupt_early_init();
        crate::klog_boot_info!("Test mode: interrupt early init done");

        crate::kernel::framework::smp::init();
        crate::klog_boot_info!("Test mode: SMP BSP registered");
        crate::kernel::framework::proc::scheduler::init();
        crate::klog_boot_info!("Test mode: Scheduler ready");

        #[cfg(feature = "fault_injection")]
        {
            let rate = option_env!("FAULT_RATE")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(50);
            crate::kernel::framework::barrier::fault_inject::FAULT_INJECTION_RATE
                .store(rate, core::sync::atomic::Ordering::Relaxed);
            crate::klog_boot_info!("[CHAOS] Fault injection enabled, rate={}/1000", rate);
        }

        crate::kernel::framework::tests::test_runner_init();
        crate::klog_boot_info!("Tests complete");

        let r = crate::kernel::framework::tests::runner();
        let failed = r.failed.load(Ordering::SeqCst);
        crate::kernel::framework::tests::qemu_exit(failed == 0);
    }

    // 1. Boot Info — 获取内存布局
    #[cfg(not(feature = "kernel_test"))]
    {
        let boot_info = crate::kernel::framework::boot::init();
        crate::klog_boot_info!(
            "Boot info: mem={} MB, kernel_end=0x{:X}",
            boot_info.mem_size / (1024 * 1024),
            boot_info.kernel_end
        );

        // 2. PMM — 物理内存管理器初始化
        crate::kernel::framework::mm::pmm::pmm_init(boot_info.mem_size, boot_info.kernel_end);
        crate::klog_boot_info!("PMM initialized");

        // 3. VMM — 虚拟内存管理器初始化 (必须在PMM之后)
        crate::kernel::framework::mm::vmm::vmm_init();
        crate::klog_boot_info!("VMM initialized");

        // 3.1 VMM 初始化后验证: 确保 GLOBAL_VMM OnceLock 已正确完成初始化.
        // 若 VMM init 内部静默失败 (如页错误导致 OnceLock 状态停留在 IN_PROGRESS),
        // 此处提前 panic 并给出明确诊断信息, 避免后续 get_vmm() 时信息不足.
        let vmm_state = crate::kernel::framework::mm::vmm::vmm_debug_state();
        assert!(
            vmm_state == 2,
            "[VMM] initialization verification failed: OnceLock state={vmm_state} (expected 2=DONE). \
             VMM init may have panicked or been interrupted."
        );

        // 4. kmalloc — 内核堆初始化
        #[expect(
            clippy::items_after_statements,
            reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
        )]
        const KMALLOC_HEAP_SIZE: u64 = 16 * 1024 * 1024; // 16 MB
        #[cfg(target_arch = "x86_64")]
        let heap_start = crate::kernel::framework::mm::VirtAddr(
            crate::kernel::framework::mm::KERNEL_BASE + boot_info.kernel_end + 0x200000,
        );
        #[cfg(target_arch = "aarch64")]
        let heap_start = crate::kernel::framework::mm::VirtAddr(boot_info.kernel_end + 0x200000);
        unsafe {
            crate::kernel::framework::mm::kmalloc::get_kmalloc_mut()
                .init(heap_start, KMALLOC_HEAP_SIZE);
        }
        crate::klog_boot_info!(
            "kmalloc initialized at 0x{:X}, size={} MB",
            heap_start.0,
            KMALLOC_HEAP_SIZE / (1024 * 1024)
        );

        // 5. PMM Bitmap — 初始化位图分配器
        // Must include the 2MB gap between kernel_end and heap_start,
        // otherwise PMM bitmap will allocate from within the kmalloc heap
        // (pages 7165+), causing heap corruption when alloc_table() zeros
        // newly allocated page table pages.
        // Must also include 2MB gap between heap_end and bitmap,
        // otherwise bitmap shares a 2MB huge page with heap, and heap
        // expansion's 2MB huge split will overwrite the bitmap PTE.
        // GAP_SIZE + KMALLOC_HEAP_SIZE + BITMAP_GAP_SIZE = 0x200000 + 16MB + 0x200000 = 20MB
        #[expect(
            clippy::items_after_statements,
            reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
        )]
        const GAP_SIZE: u64 = 0x200000;
        #[expect(
            clippy::items_after_statements,
            reason = "item 紧邻使用点声明以便阅读上下文; 移至 scope 顶部会割裂逻辑块, 必要时手动重构"
        )]
        const BITMAP_GAP_SIZE: u64 = 0x200000;
        let reserved_after_kernel = GAP_SIZE + KMALLOC_HEAP_SIZE + BITMAP_GAP_SIZE;
        crate::kernel::framework::mm::pmm::pmm_init_bitmap(reserved_after_kernel);
        crate::klog_boot_info!("PMM bitmap initialized");

        // --- Barrier-stack recovery domains (moved before interrupts to avoid race) ---
        // Register PMM + PROC domains before interrupts are enabled so timer IRQ
        // won't race with domain registration on the RECOVERY_MANAGER spinlock
        crate::kernel::framework::mm::pmm::pmm_register_barrier_domain();
        crate::kernel::framework::proc::process::proc_register_barrier_domain();
        #[cfg(target_arch = "aarch64")]
        unsafe {
            crate::kernel::framework::arch::aarch64::barrier::enable_barrier_sgi();
        }
        crate::klog_boot_info!("Barrier-stack recovery domains registered (PMM=3, PROC=4)");

        // 5.5. Swap — 物理内存回收/换出 (B3 完整实现)
        // 必须在 PMM + VMM + kmalloc 初始化之后 (使用 pmm/vmm 接口)
        // 必须在 interrupt_late_init 之前 (softirq 注册依赖 IRQ 子系统)
        // 实际上 softirq 是 static handler 表, 不强制 init 顺序, 但保持 init 流程清晰:
        //   swap_init (PMM 之后) → kswapd_init (interrupt_late_init 之后, scheduler tick 之前)
        if crate::kernel::services::mm::swap::swap_init() {
            crate::klog_boot_info!("Swap subsystem initialized");
        } else {
            crate::klog_boot_info!("Swap subsystem init FAILED (degraded mode)");
        }

        // 6. 中断/异常设置
        <crate::kernel::framework::arch::CurrentArch as crate::kernel::framework::arch::Arch>::interrupt_late_init();
        crate::klog_boot_info!("Interrupt subsystem ready");

        // 6.5. kswapd softirq 注册 (依赖 IRQ 子系统, scheduler tick 触发 wakeup)
        crate::kernel::services::mm::swap::kswapd_init();

        // 7. Timer 初始化 (中断延后到网络就绪后启用)
        match crate::kernel::framework::timer::timer_init(1000) {
            Ok(_freq) => {
                crate::klog_boot_info!("Timer configured");
                #[cfg(target_arch = "x86_64")]
                let _ = crate::kernel::framework::timer::irq::register_timer_irq();
            }
            Err(_msg) => {
                let _ = _msg;
            }
        }

        // 8. Scheduler
        crate::kernel::framework::proc::scheduler::init();
        crate::kernel::framework::proc::scheduler_ex::init();
        crate::klog_boot_info!("Scheduler ready");

        // 9. VFS
        crate::kernel::framework::fs::vfs::init();
        crate::klog_boot_info!("VFS ready");

        // 9-1. UDS (AF_UNIX) — Phase C.3
        crate::kernel::services::net::unix::uds_init();
        crate::klog_boot_info!("UDS subsystem initialized");

        // 10. Network (smoltcp + 网卡驱动)
        {
            // SAFETY: qx_net_init 签名是 `pub extern "C" fn`, 函数本身非 unsafe,
            // 但 Rust 调用任何 extern "C" 函数必须包 unsafe 块 (FFI 调用约定: 调用方
            // 负责确保跨边界 ABI 兼容性). 此处由启动流程串行调用 (BSP 单线程阶段),
            // 满足 extern "C" 调用语义: 无 panic 跨边界传播、无不变量跨边界依赖.
            unsafe {
                crate::kernel::framework::net::init::qx_net_init();
            }
            crate::klog_boot_info!("Network subsystem initialized");
        }

        // 10-10.6. Driver subsystem init
        crate::kernel::framework::driver::init_all();
        // §6.4 直接方案 B: x86_64 字符设备 (vga/serial) 权威迁 services,
        // 由 crate root (合法双向编排者) 调用 services char_init 注册进 Chitin.
        #[cfg(target_arch = "x86_64")]
        crate::kernel::services::driver::char::char_init();
        // §6.4 直接方案 B: virtio-blk 权威迁 services (aarch64 QEMU -M virt 主战场;
        // x86_64 走 PCI AHCI/NVMe, 此调用探测 virtio-mmio 无设备即跳过)
        crate::kernel::services::driver::virtio::blk_init();
        crate::klog_boot_info!("Driver subsystem initialized");
        {
            let chitin_count = crate::kernel::framework::chitin::chitin_count() as u64;
            let block_count = crate::kernel::framework::chitin::chitin_count_by_proto(
                crate::kernel::framework::chitin::ChitinProto::Block,
            ) as u64;
            let net_count = crate::kernel::framework::chitin::chitin_count_by_proto(
                crate::kernel::framework::chitin::ChitinProto::Net,
            ) as u64;
            let input_count = crate::kernel::framework::chitin::chitin_count_by_proto(
                crate::kernel::framework::chitin::ChitinProto::Input,
            ) as u64;
            crate::klog_boot_info!(
                "Chitin: {} device(s) [blk={} net={} input={}]",
                chitin_count,
                block_count,
                net_count,
                input_count
            );
        }

        // HvFS + 磁盘挂载 — BlockDevice 注册表自动发现多块磁盘 (支持 ATA/NVMe/virtio-blk)
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        {
            let hvfs = crate::kernel::framework::fs::hvfs::hvfs::get_hvfs();
            // init() 会自动扫描所有块设备, 发现 QueenX 签名的磁盘并挂载
            hvfs.init();

            if hvfs.is_disk_mode() {
                crate::kernel::framework::fs::hvfs::hvfs::get_hvfs()
                    .spa
                    .disk_present
                    .store(true, core::sync::atomic::Ordering::Release);
                let r = crate::kernel::framework::fs::vfs::api::vfs_mount_internal(
                    b"/".as_ptr(),
                    b"hvfs".as_ptr(),
                );
                if r == 0 {
                    let n_drives = crate::kernel::framework::fs::hvfs::hvfs::get_hvfs()
                        .drives_discovered
                        .lock()
                        .len() as u64;
                    if n_drives > 1 {
                        crate::klog_boot_info!("Root filesystem: HvFS ({} drives)", n_drives);
                    } else {
                        crate::klog_boot_info!("Root filesystem: HvFS (disk)");
                    }
                } else {
                    crate::klog_boot_info!("HvFS mount failed");
                }
            } else {
                crate::klog_boot_info!("HvFS: running in memory mode (no disk)");
            }
        }

        // 启动定时器 (延迟到所有子系统初始化完成后)
        #[cfg(target_arch = "aarch64")]
        {
            let interval = crate::kernel::framework::arch::aarch64::exception::TIMER_INTERVAL_TICKS
                .load(core::sync::atomic::Ordering::Relaxed);
            crate::kernel::framework::arch::aarch64::timer::start_interval(interval);
        }

        crate::klog_boot_info!("QueenX initialized, entering user mode...");

        // 11. Syscall 子系统初始化 (必须在 interrupt_late_init 之后, launch_first_user_process 之前)
        // 11a. framework 层: MSR/STAR/LSTAR 配置 + epoll 回调注册
        crate::kernel::framework::syscall_init::syscall_init();
        // 11b. services 层: 系统调用分发策略注册
        crate::kernel::services::syscall::init();
        crate::klog_boot_info!("Syscall subsystem ready");

        // 11.5. 进入 Ring 3 前最终 boot 栈 canary 验证.
        // 内核初始化全程 (PMM→VMM→kmalloc→中断→调度→网络→VFS→驱动→syscall)
        // 均在 boot 栈上运行, 此处做最终溢出检测, 确保进入用户态前栈完整性.
        assert!(
            crate::kernel::framework::proc::check_boot_stack_canary(),
            "[BOOT] stack canary corrupted before Ring 3 entry! \
             Boot stack overflow during kernel init sequence. \
             Stack size=128KB."
        );
        crate::klog_boot_info!("Boot stack canary verified (pre-Ring3)");

        // 12. Launch first user process
        unsafe {
            crate::kernel::framework::proc::api::launch_first_user_process();
        }
        // unreachable: launch_first_user_process is noreturn
    } // end #[cfg(not(feature = "kernel_test"))]
}
