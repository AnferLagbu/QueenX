pub mod api;
pub mod clone;
pub mod dispatch;
/// T-03: 系统调用分发决策 trait
pub mod dispatch_trait;
pub mod epoll;
pub mod eventfd;
pub mod firmware;
pub mod ftrace_kgdb;
pub mod futex;
pub mod info;
pub mod io;
pub mod madvise_mlock;
pub mod mmap;
pub mod mprotect;
pub mod sendfile;
pub mod signalfd;
pub mod timerfd;
pub mod wait4;

/// Syscall 模块 — `QueenX` 原生系统调用分发
///
/// 编号空间 (DECISION-037, 2026-08-03):
///   0-299   : 直接使用 Linux 标准 syscall 编号 (POSIX/syscall 透明兼容)
///   300-399 : 保留
///   400-499 : Credo 私有 syscall
///   500-599 : QueenX 自由 syscall (QX_*) — 进程 / 内存 / 文件基础
///   600-699 : QueenX 自由 syscall (QX_*) — 网络 / IPC
///   700-799 : QueenX 自由 syscall (QX_*) — 设备 / 系统
///   800-899 : QueenX 自由 syscall (QX_*) — 扩展
///
/// 0-299 直接走 Linux ABI, 无翻译层. 500+ 与 Linux 错开, 避免与未来 Linux 新增 syscall 冲突.
// 公共接口 re-export — 避免跨子系统直接访问内部子模块
pub use epoll::{EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLOUT, EPOLLRDHUP, epoll_pwake};
pub use sendfile::{
    SPLICE_F_GIFT, SPLICE_F_MORE, SPLICE_F_MOVE, SPLICE_F_NONBLOCK, sys_sendfile, sys_splice,
};
pub use types::*;
pub use types::Errno;

// dispatch_trait 公共接口 re-export - T-03 策略-机制分离
pub use dispatch_trait::{
    FallbackSyscallDispatch, SyscallDispatch, current_syscall_dispatch, register_syscall_dispatch,
};
pub mod types;

pub use dispatch::*;

pub fn validate_user_ptr(ptr: u64) -> bool {
    crate::kernel::framework::userptr::validate_user_ptr(ptr)
}

pub fn validate_user_buf(ptr: u64, len: u64) -> bool {
    crate::kernel::framework::userptr::validate_user_buf(ptr, len)
}

// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// 调用者处于内核上下文. `ptr` 是已校验的用户态指针.
pub unsafe extern "C" fn syscall_init() {
    // SAFETY: klog_write 是 C-ABI 日志函数；byte string literal 是 'static
    // 字节切片，传递给 C 时按指针 + 长度使用。
    unsafe {
        crate::kernel::framework::klog::klog_write(
            1,
            7,
            core::ptr::null(),
            core::ptr::null(),
            0,
            b"POSIX syscall subsystem ready".as_ptr(),
        );
    }

    // 注册 epoll 的 fd 关闭通知回调, 解耦 fs→syscall 依赖
    // SAFETY: epoll_pwake 是 'static 函数指针, 在内核运行期间始终有效.
    unsafe {
        crate::kernel::framework::fd_notify::register_pwake(
            crate::kernel::framework::syscall::epoll::epoll_pwake,
        );
    }
}

// ============================================================================
// raw 子模块 — 集中所有 unsafe 操作与 FFI 声明
// ============================================================================
//
// 设计目的：
// 1. 隔离 unsafe 到单一文件作用域，降低 sys_* 业务函数的认知负载
// 2. 复用 services/credo、services/barrier 的"raw 子模块"模式
// 3. 为 Phase 2.5.1 的 60+ unsafe 函数提供统一的 SAFETY 注释入口
//
// 调用契约：
// - 所有 read_* / write_* 函数均要求调用方先调用 check_user_ptr 或
//   check_user_buf 完成边界校验；否则会触发 UAF/越界写。
// - 所有 FFI 包装函数 (sm_*_call、read_keyboard_byte 等) 假定在中断
//   上下文中调用，不可在持锁睡眠上下文中调用。
// ============================================================================

pub(crate) mod raw {
    // ============= 集中 FFI 声明 =============
    // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
    unsafe extern "C" {
        // 时间
        fn timer_get_ticks() -> u64;
        // smoltcp 网络栈 — 已迁移到 net_socket.rs 路径
        // 链接器符号
        static _kernel_start: u8;
        static _kernel_end: u8;
    }

    // x86_64 专属: 键盘 — kernel_test 模式下不接触真实硬件
    #[cfg(all(target_arch = "x86_64", not(feature = "kernel_test")))]
    // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
    unsafe extern "C" {
        fn keyboard_has_data() -> bool;
        fn keyboard_get_char() -> i32;
    }

    // ============= 用户指针校验（safe 包装） =============

    /// 校验单个用户指针是否在合法范围 [1, `USER_ADDR_MAX`)
    pub fn check_user_ptr(ptr: u64) -> bool {
        super::validate_user_ptr(ptr)
    }

    /// 校验用户缓冲区 [ptr, ptr+len) 是否完全在用户空间
    pub fn check_user_buf(ptr: u64, len: u64) -> bool {
        crate::kernel::framework::userptr::validate_user_buf(ptr, len)
    }

    // ============= 用户态读写助手（unsafe 集中点） =============

    /// 写一个 u8 到用户指针。
    /// # Safety
    /// 调用方必须先调用 `check_user_ptr(ptr as u64)` 验证指针合法。
    #[cfg(all(target_arch = "x86_64", not(feature = "kernel_test")))]
    pub unsafe fn write_u8(ptr: *mut u8, val: u8) {
        // SAFETY: 调用方已通过 `check_user_ptr` 验证 ptr 指向有效且对齐的
        // 用户空间地址 (1 字节自然对齐)；write_volatile 防止编译器优化掉
        // 设备/共享内存访问。
        unsafe { core::ptr::write_volatile(ptr, val) }
    }

    /// 写一个 u32 到用户指针。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 4)` 验证。
    pub unsafe fn write_u32(ptr: *mut u32, val: u32) {
        // SAFETY: 调用方已验证 ptr 对齐到 4 字节且指向 4 字节可写用户空间。
        unsafe { core::ptr::write_volatile(ptr, val) }
    }

    /// 写一个 u64 到用户指针。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 8)` 验证。
    pub unsafe fn write_u64(ptr: *mut u64, val: u64) {
        // SAFETY: 调用方已验证 ptr 对齐到 8 字节且指向 8 字节可写用户空间。
        unsafe { core::ptr::write_volatile(ptr, val) }
    }

    /// 写两个 u64 到用户指针 (用于 rlimit cur/max)。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 16)` 验证。
    pub unsafe fn write_u64_pair(ptr: *mut u64, cur: u64, max: u64) {
        // SAFETY: 调用方已验证 ptr 对齐到 8 字节且指向 16 字节可写用户空间。
        unsafe {
            core::ptr::write_volatile(ptr, cur);
            core::ptr::write_volatile(ptr.add(1), max);
        }
    }

    /// 读一个 u64。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 8)` 验证。
    pub unsafe fn read_u64(ptr: *const u64) -> u64 {
        // SAFETY: 调用方已验证 ptr 对齐到 8 字节且指向 8 字节可读用户空间。
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// 复制结构体到用户指针（repr(C) 类型）。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, size_of::<T>())` 验证。
    pub unsafe fn write_struct<T: Copy>(dst: *mut T, src: &T) {
        // SAFETY: 调用方已验证 dst 对齐到 align_of::<T>() 且 size_of::<T>()
        // 字节可写；src 是有效 T 引用。write_volatile 保留顺序语义。
        unsafe { core::ptr::write_volatile(dst, *src) }
    }

    /// Safe 包装: 在 services 层用, 写一个 repr(C) 结构体到 user 指针.
    ///
    /// 调用方无需 unsafe 块. 内部先 `check_user_buf` 验证后写.
    pub fn write_struct_to_user<T: Copy>(dst_ptr: u64, src: &T) -> bool {
        if dst_ptr == 0 {
            return false;
        }
        let size = core::mem::size_of::<T>() as u64;
        if !check_user_buf(dst_ptr, size) {
            return false;
        }
        // SAFETY: 上方 check_user_buf 已验证 dst_ptr 指向的 user 缓冲
        // 至少有 size_of::<T>() 字节可写, 且 src 持有有效 T 值.
        unsafe { write_struct(dst_ptr as *mut T, src) }
        true
    }

    /// Safe 包装: 写两个 u64 到 user 指针 (`rlim_cur`, `rlim_max`).
    pub fn write_rlimit_to_user(ptr: u64, cur: u64, max: u64) -> bool {
        if ptr == 0 {
            return false;
        }
        if !check_user_buf(ptr, 16) {
            return false;
        }
        // SAFETY: check_user_buf 已验证 16 字节可写
        unsafe { write_u64_pair(ptr as *mut u64, cur, max) }
        true
    }

    /// Safe 包装: 在 services 层用, 写一个 u64 到 user 指针.
    ///
    /// 调用方无需 unsafe 块. 内部先 `check_user_buf(dst, 8)` 验证.
    pub fn write_u64_to_user(dst_ptr: u64, val: u64) -> bool {
        if dst_ptr == 0 {
            return false;
        }
        if !check_user_buf(dst_ptr, 8) {
            return false;
        }
        // SAFETY: check_user_buf 已验证 8 字节可写
        unsafe { core::ptr::write_unaligned(dst_ptr as *mut u64, val) };
        true
    }

    /// Safe 包装: 从 user 指针读一个 repr(C) 结构体.
    /// 调用方无需 unsafe 块. 内部先 `check_user_buf` 验证后读.
    pub fn read_struct_from_user<T: Copy>(src_ptr: u64, dst: &mut T) -> bool {
        if src_ptr == 0 {
            return false;
        }
        let size = core::mem::size_of::<T>() as u64;
        if !check_user_buf(src_ptr, size) {
            return false;
        }
        // SAFETY: check_user_buf 已验证 src_ptr 指向的 user 缓冲
        // 至少有 size_of::<T>() 字节可读.
        *dst = unsafe { core::ptr::read_unaligned(src_ptr as *const T) };
        true
    }

    // ============= 设备输入抽象 =============

    /// `从键盘读取一个字节（x86_64` 专属）。None 表示无数据。
    /// # Safety
    /// FFI 调用，需在中断上下文。
    #[cfg(all(target_arch = "x86_64", not(feature = "kernel_test")))]
    pub fn read_keyboard_byte() -> Option<u8> {
        // SAFETY: keyboard_has_data 与 keyboard_get_char 是 C-ABI 函数，
        // 调用方保证在中断上下文 (disable_interrupts 已持有)。
        unsafe {
            if keyboard_has_data() {
                let c = keyboard_get_char();
                if c > 0 { Some(c as u8) } else { None }
            } else {
                None
            }
        }
    }

    // ============= 物理内存分配 =============

    /// 分配 count 个连续物理页。
    /// 委托到 `mm::api::pmm_alloc_pages`.
    pub fn alloc_pages(count: u64) -> *mut u8 {
        crate::kernel::framework::mm::pmm_alloc_pages(count as usize)
    }

    /// 释放 count 个连续物理页。
    /// 委托到 `mm::api::pmm_free_pages`.
    pub fn free_pages(addr: *mut u8, count: u64) {
        crate::kernel::framework::mm::pmm_free_pages(addr, count as usize);
    }

    // ============= 时间 =============

    /// 从用户指针读一个 u64. None 表示失败.
    /// 调用方无需 unsafe 块. 内部校验 + 读.
    pub fn read_u64_from_user(src_ptr: u64) -> Option<u64> {
        if src_ptr == 0 {
            return None;
        }
        if !check_user_buf(src_ptr, 8) {
            return None;
        }
        // SAFETY: check_user_buf 已验证 src_ptr 8 字节可读.
        Some(unsafe { core::ptr::read_unaligned(src_ptr as *const u64) })
    }

    /// 读取 tick 计数（1ms 粒度）。
    /// # Safety
    /// FFI 调用，硬件定时器寄存器读取。
    pub fn get_ticks() -> u64 {
        // SAFETY: timer_get_ticks 是 C-ABI 函数，读取 PIT/HPET 计数器，
        // 无副作用。
        unsafe { timer_get_ticks() }
    }

    // ============= smoltcp 网络栈 FFI 包装 =============

    // ============= 链接器符号访问 =============

    /// 内核映像起始虚拟地址。
    /// # Safety
    /// 链接器符号，仅在 boot 后有效。
    #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
    #[expect(
        clippy::borrow_as_ptr,
        reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
    )]
    pub fn kernel_start_ptr() -> *const u8 {
        // SAFETY: _kernel_start 是链接器符号 (extern "C")，是静态地址，
        // boot 后由 VMM 建立映射可读。
        unsafe { &_kernel_start as *const u8 }
    }

    /// 内核映像结束物理地址（已减 `HHDM_OFFSET`）。
    /// # SAFETY: `链接器符号，hhdm_offset` 必须与启动时一致。
    #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
    #[expect(
        clippy::borrow_as_ptr,
        reason = "borrow_as_ptr: &var as *const T 是已知安全 (Rust 2024 可用 &raw const; 替换需追改调用点, 当前优先 expect"
    )]
    pub fn kernel_end_phys(hhdm_offset: usize) -> usize {
        unsafe { (&_kernel_end as *const u8 as usize).wrapping_sub(hhdm_offset) }
    }

    // ============= CPU 控制指令集中点 =============

    /// 加载空 IDT 后触发异常，重启 `CPU（x86_64`）。
    /// # SAFETY: 不返回；调用方须确保已关闭其他 CPU。
    #[cfg(target_arch = "x86_64")]
    pub unsafe fn reboot_via_idt() -> ! {
        unsafe {
            core::arch::asm!(
                "lidt [rdi]",
                "int 0",
                in("rdi") &[0u16; 4],
                options(nostack, nomem)
            );
            loop {}
        }
    }

    /// 通过 SVC 触发 PSCI reset（aarch64）。
    /// # SAFETY: 不返回；调用方须确保已关闭其他 CPU。
    #[cfg(target_arch = "aarch64")]
    pub unsafe fn reboot_via_psci() -> ! {
        unsafe {
            core::arch::asm!("svc #0", in("x0") 0u64, options(nostack));
            loop {}
        }
    }
}
