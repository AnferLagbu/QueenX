//! Core Dump 生成器
//!
//! 当进程收到 Core 类信号 (SIGQUIT/SIGILL/SIGABRT/SIGBUS/SIGFPE/SIGSEGV 等)
//! 时, 生成 ELF 格式的 core 文件, 包含进程的寄存器状态和内存映射。
//!
//! ## ELF Core 文件格式
//!
//! ```text
//! ELF Header (ET_CORE)
//! ├── PT_NOTE 段: NT_PRSTATUS (寄存器) + NT_SIGINFO (信号信息)
//! ├── PT_LOAD 段: 可读内存区域 (每个 VMA 一个)
//! └── ...
//! ```
//!
//! ## 限制
//!
//! - `RLIMIT_CORE`: core 文件大小上限 (0 = 禁止)
//! - 仅转储可读 VMA (跳过只执行/不可读)
//! - 最大转储 64 个内存段
//!
//! ## 安全
//!
//! - 本模块属于 framework (TCB), 允许 unsafe
//! - 内存读取通过物理页映射, 需确保 CR3 有效

use crate::framework::mm;
use crate::framework::mm::{PAGE_SIZE, PageFlags};
use crate::framework::proc::{RLIM_INFINITY, RLIMIT_CORE, process_get_current_pid, process_with};
// B08-22: ELF64 头统一引用 elf/mod.rs 单一定义, 消除 coredump 侧重复布局
use crate::framework::proc::elf::{Elf64Header, Elf64Phdr};
use core::sync::atomic::Ordering;

// SAFETY: C ABI 互操作，函数签名与外部代码约定一致
unsafe extern "C" {
    fn klog_ffi_info(msg: *const u8);
}

fn log(s: &str) {
    // SAFETY: klog_ffi_info 接受有效 *const u8 指针
    unsafe {
        klog_ffi_info(s.as_ptr());
    }
}

fn log_num(n: u64) {
    if n == 0 {
        log("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut num = n;
    let mut i = 19;
    while num > 0 {
        buf[i] = (num % 10) as u8 + b'0';
        num /= 10;
        i -= 1;
    }
    let s = core::str::from_utf8(&buf[i + 1..]).unwrap_or("?");
    log(s);
}

// ============================================================================
// ELF Core 常量
// ============================================================================

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_CORE: u16 = 4;
const EV_CURRENT: u32 = 1;

#[cfg(target_arch = "x86_64")]
const EM_MACHINE: u16 = 62; // EM_X86_64
#[cfg(target_arch = "aarch64")]
const EM_MACHINE: u16 = 183; // EM_AARCH64

const PT_NOTE: u32 = 4;
const PT_LOAD: u32 = 1;

const NT_PRSTATUS: u32 = 1;
const NT_SIGINFO: u32 = 0x53494749;

const PF_R: u32 = 4;
const PF_W: u32 = 2;
const PF_X: u32 = 1;

/// 最大转储内存段数
const MAX_CORE_SEGMENTS: usize = 64;

// ============================================================================
// ELF Core 结构体 (引用 elf/mod.rs 单一定义, B08-22)
// ============================================================================

/// `siginfo_t` 简化 (仅用于 core dump note)
#[repr(C)]
struct CoreSiginfo {
    si_signo: i32,
    si_code: i32,
    si_errno: i32,
}

// ============================================================================
// x86_64 寄存器 note
// ============================================================================

#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct PrStatus {
    siginfo: CoreSiginfo,
    _pad0: u16,
    pr_cursig: u16,
    _pad1: u32,
    pr_sigpend: u64,
    pr_sighold: u64,
    pr_pid: i32,
    pr_ppid: i32,
    pr_pgrp: i32,
    pr_sid: i32,
    pr_utime: u64,
    pr_stime: u64,
    pr_cutime: u64,
    pr_cstime: u64,
    // regset: 27 个 u64 (按 Linux prstatus 顺序)
    regs: [u64; 27],
}

#[cfg(target_arch = "aarch64")]
#[repr(C)]
struct PrStatus {
    siginfo: CoreSiginfo,
    _pad0: u16,
    pr_cursig: u16,
    _pad1: u32,
    pr_sigpend: u64,
    pr_sighold: u64,
    pr_pid: i32,
    pr_ppid: i32,
    pr_pgrp: i32,
    pr_sid: i32,
    pr_utime: u64,
    pr_stime: u64,
    pr_cutime: u64,
    pr_cstime: u64,
    // aarch64: x0-x30 + sp + pc + pstate = 34 个 u64
    regs: [u64; 34],
}

// ============================================================================
// Note 段布局
// ============================================================================

/// 单个 ELF Note 头
#[repr(C)]
struct Elf64Note {
    namesz: u32,
    descsz: u32,
    r#type: u32,
}

const NOTE_NAME: &[u8] = b"CORE\0"; // 5 bytes + null

/// 计算对齐后的 note 大小
fn note_size(namesz: u32, descsz: u32) -> u64 {
    let name_aligned = (u64::from(namesz) + 3) & !3;
    let desc_aligned = (u64::from(descsz) + 3) & !3;
    12 + name_aligned + desc_aligned // 12 = sizeof(Elf64Note)
}

// ============================================================================
// Core Dump 写入器
// ============================================================================

/// 内存段信息 (用于构建 `PT_LOAD`)
struct CoreSegment {
    start: u64,
    end: u64,
    flags: u32,     // 权限标志: PF_R | PF_W | PF_X
    file_size: u64, // 实际写入大小 (可能截断)
}

/// 生成 core dump
///
/// 调用时机: 进程收到 Core 类信号, 在 `do_signal_default_action` 中调用。
///
/// # 参数
/// - `pid`: 目标进程 PID
/// - `sig`: 导致 core dump 的信号编号
/// - `frame`: 中断帧指针 (寄存器快照)
///
/// # 返回
/// - `true`: core dump 成功写入
/// - `false`: core dump 失败 (`RLIMIT_CORE=0`, 磁盘满等)
pub fn do_coredump(pid: u32, sig: u8, frame: u64) -> bool {
    // 1. 检查 RLIMIT_CORE
    let core_limit = process_with(pid, |p| {
        let table = p.rlimit_table.lock();
        table.get(RLIMIT_CORE).map_or(0, |r| r.cur)
    })
    .unwrap_or(0);

    if core_limit == 0 {
        log("coredump: RLIMIT_CORE=0, skipped\n");
        return false;
    }

    // 2. 收集内存段
    let segments = collect_segments(pid);
    if segments.is_empty() {
        log("coredump: no segments to dump\n");
        return false;
    }

    // 3. 计算 note 段大小
    let prstatus_size = core::mem::size_of::<PrStatus>() as u32;
    let siginfo_size = core::mem::size_of::<CoreSiginfo>() as u32;

    let note_total = note_size(NOTE_NAME.len() as u32, prstatus_size)
        + note_size(NOTE_NAME.len() as u32, siginfo_size);

    // 4. 计算 program header 数量和偏移
    let phnum = 1 + segments.len() as u16; // PT_NOTE + PT_LOADs
    let ehdr_size = core::mem::size_of::<Elf64Header>() as u64;
    let phdr_size = core::mem::size_of::<Elf64Phdr>() as u64;
    let phdr_total = phdr_size * u64::from(phnum);
    let note_offset = ehdr_size + phdr_total;

    // 5. 计算总大小并检查 RLIMIT_CORE
    let mut total_size = note_offset + note_total;
    for seg in &segments {
        total_size += seg.file_size;
    }

    if core_limit != RLIM_INFINITY && total_size > core_limit {
        // 截断: 只写 RLIMIT_CORE 允许的大小
        log("coredump: truncated by RLIMIT_CORE\n");
        // 截断: 后续写循环应在写入达到 core_limit 字节后停止
        let _ = core_limit;
    }

    // 6. 打开 core 文件
    let core_path = build_core_path(pid);
    let open_flags = 0x0002 /* O_WRONLY */ | 0x0100 /* O_CREAT */ | 0x0200 /* O_TRUNC */;
    let fd = crate::framework::fs::vfs_open(
        core_path.as_ptr(),
        open_flags,
        0, // pwm = 0 (内核权限)
    );

    if fd < 0 {
        log("coredump: failed to open core file\n");
        return false;
    }

    let fd = fd as u32;

    // 7. 写入 ELF header
    let ehdr = build_ehdr(phnum, note_offset);
    let mut offset = 0u64;
    write_bytes(fd, &ehdr, &mut offset);

    // 8. 写入 program headers
    // PT_NOTE
    let note_phdr = build_note_phdr(note_offset, note_total);
    write_bytes(fd, &note_phdr, &mut offset);

    // PT_LOAD headers
    let mut data_offset = note_offset + note_total;
    for seg in &segments {
        let phdr = build_load_phdr(seg, data_offset);
        write_bytes(fd, &phdr, &mut offset);
        data_offset += seg.file_size;
    }

    // 9. 写入 note 段
    write_note_prstatus(fd, pid, sig, frame, &mut offset);
    write_note_siginfo(fd, sig, &mut offset);
    // 对齐 note 段
    let note_end = note_offset + note_total;
    if offset < note_end {
        let pad = note_end - offset;
        let zeros = [0u8; 16];
        let mut zoff = 0u64;
        while zoff < pad {
            let n = core::cmp::min(pad - zoff, 16);
            crate::framework::fs::vfs_write(fd, zeros.as_ptr(), n as u32);
            zoff += n;
        }
        offset = note_end;
    }

    // 10. 写入内存段数据
    for seg in &segments {
        write_segment_data(fd, pid, seg, &mut offset, core_limit);
    }

    // 11. 关闭文件
    crate::framework::fs::vfs_close(fd);

    log("coredump: written ");
    log_num(offset);
    log(" bytes for pid=");
    log_num(u64::from(pid));
    log("\n");

    true
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 构建 core 文件路径: "core.<pid>"
fn build_core_path(pid: u32) -> alloc::vec::Vec<u8> {
    let mut path = b"core.".to_vec();
    let mut num = pid;
    if num == 0 {
        path.push(b'0');
    } else {
        let mut digits = alloc::vec::Vec::new();
        while num > 0 {
            digits.push(b'0' + (num % 10) as u8);
            num /= 10;
        }
        digits.reverse();
        path.extend_from_slice(&digits);
    }
    path.push(0); // null terminator
    path
}

/// 收集进程的可读内存段
fn collect_segments(pid: u32) -> alloc::vec::Vec<CoreSegment> {
    let mut segments = alloc::vec::Vec::new();

    // 获取进程的 CR3 (页表基址)
    let cr3 = process_with(pid, |p| p.cr3.load(Ordering::SeqCst)).unwrap_or(0);

    if cr3 == 0 {
        return segments;
    }

    // 获取当前进程的 VMA 列表
    // SAFETY: get_current_mm 返回的 MmStruct 指针在进程存活期间有效
    let mm = mm::vma_get_current_mm();
    if let Some(mm) = mm {
        let vmas = mm.vmas.lock();
        for vma in vmas.iter() {
            if segments.len() >= MAX_CORE_SEGMENTS {
                break;
            }

            // 只转储可读 VMA
            let flags = vma.flags;
            let pf_r: u32 = if flags.contains(PageFlags::PRESENT) {
                PF_R
            } else {
                0u32
            };
            let pf_w: u32 = if flags.contains(PageFlags::WRITABLE) {
                PF_W
            } else {
                0u32
            };
            let pf_x: u32 = if flags.contains(PageFlags::NX) {
                0u32
            } else {
                PF_X
            };

            // 跳过不可读段
            if pf_r == 0 {
                continue;
            }

            let start = vma.start as u64;
            let end = vma.end as u64;
            let size = end - start;

            segments.push(CoreSegment {
                start,
                end,
                flags: pf_r | pf_w | pf_x,
                file_size: size,
            });
        }
    }

    segments
}

/// 构建 ELF header
fn build_ehdr(phnum: u16, _phoff: u64) -> Elf64Header {
    let mut e_ident = [0u8; 16];
    e_ident[0..4].copy_from_slice(&ELFMAG);
    e_ident[4] = ELFCLASS64;
    e_ident[5] = ELFDATA2LSB;
    e_ident[6] = EV_CURRENT as u8;

    Elf64Header {
        e_ident,
        e_type: ET_CORE,
        e_machine: EM_MACHINE,
        e_version: EV_CURRENT,
        e_entry: 0,
        e_phoff: core::mem::size_of::<Elf64Header>() as u64,
        e_shoff: 0,
        e_flags: 0,
        e_ehsize: core::mem::size_of::<Elf64Header>() as u16,
        e_phentsize: core::mem::size_of::<Elf64Phdr>() as u16,
        e_phnum: phnum,
        e_shentsize: 0,
        e_shnum: 0,
        e_shstrndx: 0,
    }
}

/// 构建 `PT_NOTE` program header
fn build_note_phdr(offset: u64, size: u64) -> Elf64Phdr {
    Elf64Phdr {
        p_type: PT_NOTE,
        p_flags: 0,
        p_offset: offset,
        p_vaddr: 0,
        p_paddr: 0,
        p_filesz: size,
        p_memsz: size,
        p_align: 4,
    }
}

/// 构建 `PT_LOAD` program header
fn build_load_phdr(seg: &CoreSegment, offset: u64) -> Elf64Phdr {
    Elf64Phdr {
        p_type: PT_LOAD,
        p_flags: seg.flags,
        p_offset: offset,
        p_vaddr: seg.start,
        p_paddr: 0,
        p_filesz: seg.file_size,
        p_memsz: seg.end - seg.start,
        p_align: PAGE_SIZE as u64,
    }
}

#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::borrow_as_ptr,
    reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
)]
/// 写入 `NT_PRSTATUS` note
fn write_note_prstatus(fd: u32, pid: u32, sig: u8, frame_addr: u64, offset: &mut u64) {
    let prstatus_size = core::mem::size_of::<PrStatus>() as u32;

    // Note header
    let note = Elf64Note {
        namesz: NOTE_NAME.len() as u32,
        descsz: prstatus_size,
        r#type: NT_PRSTATUS,
    };
    write_bytes(fd, &note, offset);

    // Note name (对齐到 4 字节)
    let name_aligned = (NOTE_NAME.len() as u64 + 3) & !3;
    crate::framework::fs::vfs_write(fd, NOTE_NAME.as_ptr(), name_aligned as u32);
    *offset += name_aligned;

    // 构建 PrStatus
    // SAFETY: PrStatus 是 POD 结构体, zeroed 后逐字段填充, 所有未设置字段已由 zeroed 初始化为零
    let mut prstatus: PrStatus = unsafe { core::mem::zeroed() };
    prstatus.siginfo.si_signo = i32::from(sig);
    prstatus.pr_cursig = u16::from(sig);
    prstatus.pr_pid = pid as i32;

    // 填充进程信息
    process_with(pid, |p| {
        prstatus.pr_ppid = p.parent.map_or(0, |pp| pp.0 as i32);
        prstatus.pr_pgrp = p.pgid.load(Ordering::SeqCst) as i32;
        prstatus.pr_sid = p.session_id.load(Ordering::SeqCst) as i32;
        prstatus.pr_sigpend = p.signal_pending_get();
        prstatus.pr_utime = p.user_time.load(Ordering::SeqCst);
        prstatus.pr_stime = p.sys_time.load(Ordering::SeqCst);
    });

    // 从中断帧填充寄存器
    fill_regs_from_frame(&mut prstatus, frame_addr);

    // 写入 PrStatus (对齐到 4 字节)
    let desc_aligned = (u64::from(prstatus_size) + 3) & !3;

    crate::framework::fs::vfs_write(fd, &prstatus as *const PrStatus as *const u8, prstatus_size);

    *offset += desc_aligned;
}

#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
#[expect(
    clippy::borrow_as_ptr,
    reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
)]
/// 写入 `NT_SIGINFO` note
fn write_note_siginfo(fd: u32, sig: u8, offset: &mut u64) {
    let siginfo_size = core::mem::size_of::<CoreSiginfo>() as u32;

    let note = Elf64Note {
        namesz: NOTE_NAME.len() as u32,
        descsz: siginfo_size,
        r#type: NT_SIGINFO,
    };
    write_bytes(fd, &note, offset);

    let name_aligned = (NOTE_NAME.len() as u64 + 3) & !3;
    crate::framework::fs::vfs_write(fd, NOTE_NAME.as_ptr(), name_aligned as u32);
    *offset += name_aligned;

    let si = CoreSiginfo {
        si_signo: i32::from(sig),
        si_code: 0,
        si_errno: 0,
    };

    let desc_aligned = (u64::from(siginfo_size) + 3) & !3;

    crate::framework::fs::vfs_write(fd, &si as *const CoreSiginfo as *const u8, siginfo_size);

    *offset += desc_aligned;
}

/// 从中断帧填充寄存器
#[cfg(target_arch = "x86_64")]
fn fill_regs_from_frame(prstatus: &mut PrStatus, frame_addr: u64) {
    if frame_addr == 0 {
        return;
    }
    // SAFETY: frame_addr 由调用方保证为有效的 InterruptFrame 指针
    let frame = unsafe { &*(frame_addr as *const crate::framework::idt::InterruptFrame) };

    // Linux x86_64 prstatus regset 顺序 (27 个):
    // r8 r9 r10 r11 r12 r13 r14 r15 rdi rsi rbp rbx rdx rax rcx rsp rip rflags
    // cs ss gs fs ... (简化: 填 0)
    // 但实际 Linux 只用 27 个 u64, 顺序如下:
    // 0: r15, 1: r14, 2: r13, 3: r12, 4: rbp, 5: rbx, 6: r11, 7: r10,
    // 8: r9, 9: r8, 10: rax, 11: rcx, 12: rdx, 13: rsi, 14: rdi,  // GPR 序号与名称
    // 15: orig_rax, 16: rip, 17: cs, 18: rflags, 19: rsp, 20: ss,  // 段/CS/RIP 序号
    // 21: fs_base, 22: gs_base, 23: ds, 24: es, 25: fs, 26: gs  // 段基址/段选择子
    let regs = &mut prstatus.regs;
    regs[0] = frame.r15;
    regs[1] = frame.r14;
    regs[2] = frame.r13;
    regs[3] = frame.r12;
    regs[4] = frame.rbp;
    regs[5] = frame.rbx;
    regs[6] = frame.r11;
    regs[7] = frame.r10;
    regs[8] = frame.r9;
    regs[9] = frame.r8;
    regs[10] = frame.rax;
    regs[11] = frame.rcx;
    regs[12] = frame.rdx;
    regs[13] = frame.rsi;
    regs[14] = frame.rdi;
    regs[15] = frame.rax; // orig_rax (简化: = rax)
    regs[16] = frame.rip;
    regs[17] = frame.cs;
    regs[18] = frame.rflags;
    regs[19] = frame.rsp;
    regs[20] = frame.ss;
    // fs_base, gs_base, ds, es, fs, gs = 0 (简化)
}

/// 从异常帧填充寄存器 (aarch64)
#[cfg(target_arch = "aarch64")]
fn fill_regs_from_frame(prstatus: &mut PrStatus, frame_addr: u64) {
    if frame_addr == 0 {
        return;
    }
    // SAFETY: frame_addr 由调用方保证为有效的 ExceptionFrame 指针
    let frame =
        unsafe { &*(frame_addr as *const crate::framework::arch::exception::ExceptionFrame) };

    let regs = &mut prstatus.regs;
    regs[0] = frame.x0;
    regs[1] = frame.x1;
    regs[2] = frame.x2;
    regs[3] = frame.x3;
    regs[4] = frame.x4;
    regs[5] = frame.x5;
    regs[6] = frame.x6;
    regs[7] = frame.x7;
    regs[8] = frame.x8;
    regs[9] = frame.x9;
    regs[10] = frame.x10;
    regs[11] = frame.x11;
    regs[12] = frame.x12;
    regs[13] = frame.x13;
    regs[14] = frame.x14;
    regs[15] = frame.x15;
    regs[16] = frame.x16;
    regs[17] = frame.x17;
    regs[18] = frame.x18;
    regs[19] = frame.x19;
    regs[20] = frame.x20;
    regs[21] = frame.x21;
    regs[22] = frame.x22;
    regs[23] = frame.x23;
    regs[24] = frame.x24;
    regs[25] = frame.x25;
    regs[26] = frame.x26;
    regs[27] = frame.x27;
    regs[28] = frame.x28;
    regs[29] = frame.x29; // FP
    regs[30] = frame.x30; // LR
    regs[31] = frame.sp;
    regs[32] = frame.elr; // PC
    regs[33] = frame.spsr as u64; // PSTATE
}

/// 写入内存段数据
fn write_segment_data(fd: u32, _pid: u32, seg: &CoreSegment, offset: &mut u64, core_limit: u64) {
    let size = seg.file_size;
    let mut written = 0u64;

    // 逐页读取并写入
    let mut addr = seg.start;
    while written < size {
        // 检查 RLIMIT_CORE
        if core_limit != RLIM_INFINITY && *offset >= core_limit {
            break;
        }

        let chunk_size = core::cmp::min(size - written, PAGE_SIZE as u64);

        // SAFETY: 从用户空间地址读取数据。需要切换 CR3 到目标进程。
        // 简化实现: 使用当前 CR3 (假设是目标进程, 因为 coredump 在信号投递时调用)
        let src = addr as *const u8;
        // 尝试读取, 如果页不存在则写零
        let mut buf = [0u8; PAGE_SIZE as usize];
        let readable = copy_from_user_safe(src, chunk_size as usize, &mut buf);

        if readable > 0 {
            crate::framework::fs::vfs_write(fd, buf.as_ptr(), readable as u32);
        } else {
            // 页不存在, 写零
            let zeros = [0u8; PAGE_SIZE as usize];
            crate::framework::fs::vfs_write(fd, zeros.as_ptr(), chunk_size as u32);
        }

        written += chunk_size;
        addr += chunk_size;
        *offset += chunk_size;
    }
}

/// 安全地从用户空间拷贝数据
///
/// 返回实际拷贝的字节数 (0 表示页不存在)
///
/// P0-I-36 修复: 改用 `framework/mm/copy_user` 的异常表安全 `copy_from_user`.
fn copy_from_user_safe(src: *const u8, len: usize, dst: &mut [u8]) -> usize {
    if src.is_null() || len == 0 || len > dst.len() {
        return 0;
    }
    // SAFETY: src 来自内核代码构造的进程 VMA 地址, 长度由调用方保证.
    //          委托给异常表保护版 copy_from_user, 缺页时返回 Err 而非 panic.
    let user_addr = src as u64;
    match crate::framework::mm::copy_from_user(dst, user_addr, len) {
        Ok(n) => n,
        Err(()) => 0,
    }
}

/// 写入字节到 fd
fn write_bytes<T>(fd: u32, data: &T, offset: &mut u64) {
    let size = core::mem::size_of::<T>();

    crate::framework::fs::vfs_write(fd, core::ptr::from_ref::<T>(data).cast::<u8>(), size as u32);

    *offset += size as u64;
}

// ============================================================================
// 公共 API
// ============================================================================

/// 检查当前进程是否允许生成 core dump
pub fn coredump_allowed() -> bool {
    let pid = process_get_current_pid();
    if pid == 0 {
        return false;
    }
    process_with(pid, |p| {
        let table = p.rlimit_table.lock();
        table.get(RLIMIT_CORE).is_some_and(|r| r.cur > 0)
    })
    .unwrap_or(false)
}

/// 获取 core dump 大小限制
pub fn coredump_limit() -> u64 {
    let pid = process_get_current_pid();
    if pid == 0 {
        return 0;
    }
    process_with(pid, |p| {
        let table = p.rlimit_table.lock();
        table.get(RLIMIT_CORE).map_or(0, |r| r.cur)
    })
    .unwrap_or(0)
}
