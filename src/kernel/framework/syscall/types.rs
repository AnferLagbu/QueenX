#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯类型定义和常量。
//! Syscall 类型定义和常量 — framework 机制实现
//!
//! ## DECISION-J 归属反转记录 (2026-09-13)
//!
//! 原定义于 T5-4 (2026-06-16) 迁至 `services::syscall::types`, 本文件仅
//! re-export。按"机制持有的数据结构/常量归 framework"统一判据反转：
//! **syscall 编号表是用户态 ABI 机制** (编号空间分配 DECISION-037 承载物,
//! 用户态程序直接依赖), 被 framework dispatch/dispatch_trait 消费 +
//! services dispatch 消费, 依赖闭包为空 (纯 const + type alias, 仅
//! re-export framework::errno::Errno) — 属机制项, 迁回 (同 fd_alloc 模式)。
//!
//! services 侧改 `pub use crate::framework::syscall::types::*`
//! 保持 API 兼容 (services→framework 合法方向)。

// POSIX errno 命名约定 (EAGAIN/EACCES/...) — 全大写缩写是有意的
#![allow(clippy::upper_case_acronyms)]

/// Syscall 类型定义和常量
///
/// 编号空间分配 (DECISION-037 + queenx-naming-standpoint.md):
///   0-299   : Linux 兼容编号 (SYS_*), 直接使用 Linux 标准编号
///   300-399 : 保留
///   400-499 : Credo 私有 syscall (避开 424-452 的 Linux 现代扩展区)
///   500-599 : 进程 / 内存 / 文件基础
///   600-699 : 网络 / IPC
///   700-799 : 设备 / 系统
///   800-899 : 扩展

pub const SYSCALL_INT: u8 = 0x80;

/// syscall 编号空间上界 (非 dispatch 数组边界 — dispatch 全为 match, 无 SYSCALL_TABLE).
///
/// 覆盖全部 QX_* 扩展区 (0-899), 与 `QX_FTRACE_ENABLE = 800` 等 800 段常量错开,
/// 避免编号常量语义误导.
pub const MAX_SYSCALLS: u64 = 900;

/// services 层未处理 syscall 的返回码 (=-ENOSYS), 供 framework 回退处理.
///
/// 作为 services→framework 分发回退哨兵, 与 `FallbackSyscallDispatch` 返回值一致.
pub const ENOSYS_RET: i64 = -38;

// ==================== POSIX 标准 syscall 编号 ====================

// 文件 I/O
pub const SYS_read: u64 = 0;
pub const SYS_write: u64 = 1;
pub const SYS_open: u64 = 2;
pub const SYS_close: u64 = 3;
pub const SYS_stat: u64 = 4;
pub const SYS_fstat: u64 = 5;
pub const SYS_lstat: u64 = 6;
pub const SYS_poll: u64 = 7;
pub const SYS_lseek: u64 = 8;

// 内存管理
pub const SYS_mmap: u64 = 9;
pub const SYS_mprotect: u64 = 10;
pub const SYS_munmap: u64 = 11;
pub const SYS_brk: u64 = 12;

// 信号 (基础存根)
pub const SYS_rt_sigaction: u64 = 13;
pub const SYS_rt_sigprocmask: u64 = 14;
pub const SYS_rt_sigreturn: u64 = 15;

// 设备 I/O
pub const SYS_ioctl: u64 = 16;

// 文件访问
pub const SYS_access: u64 = 21;
pub const SYS_pipe: u64 = 22;
pub const SYS_select: u64 = 23;
pub const SYS_sched_yield: u64 = 24;

// 内存重映射
// TODO(TRACK-90BFB0): Phase N — implement mremap
pub const SYS_mremap: u64 = 25;

// 文件描述符
pub const SYS_dup: u64 = 32;
pub const SYS_dup2: u64 = 33;

// 进程优先级
pub const SYS_nice: u64 = 34;

// 暂停
pub const SYS_nanosleep: u64 = 35;

// ITIMER
// TODO(TRACK-8B3C91): Phase N — implement getitimer
pub const SYS_getitimer: u64 = 36;
pub const SYS_alarm: u64 = 37;
// TODO(TRACK-6564B9): Phase N — implement setitimer
pub const SYS_setitimer: u64 = 38;

// 进程基础
pub const SYS_getpid: u64 = 39;

// 网络 socket
pub const SYS_socket: u64 = 41;
pub const SYS_connect: u64 = 42;
pub const SYS_accept: u64 = 43;
pub const SYS_sendto: u64 = 44;
pub const SYS_recvfrom: u64 = 45;
pub const SYS_sendmsg: u64 = 46;
pub const SYS_recvmsg: u64 = 47;
pub const SYS_shutdown: u64 = 48;
pub const SYS_bind: u64 = 49;
pub const SYS_listen: u64 = 50;
pub const SYS_getsockname: u64 = 51;
pub const SYS_getpeername: u64 = 52;
pub const SYS_setsockopt: u64 = 54;
pub const SYS_getsockopt: u64 = 55;

// 进程
// TODO(TRACK-0FF0F0): Phase N — implement clone for thread creation
pub const SYS_clone: u64 = 56;
pub const SYS_fork: u64 = 57;
pub const SYS_execve: u64 = 59;
pub const SYS_exit: u64 = 60;
pub const SYS_wait4: u64 = 61;
pub const SYS_kill: u64 = 62;

// 系统信息
pub const SYS_uname: u64 = 63;

// 文件描述符操作
pub const SYS_fcntl: u64 = 72;

// 文件截断
pub const SYS_truncate: u64 = 76;
pub const SYS_ftruncate: u64 = 77;

// 目录
pub const SYS_getdents: u64 = 78;

// 路径
pub const SYS_getcwd: u64 = 79;
pub const SYS_chdir: u64 = 80;

// 文件重命名
pub const SYS_rename: u64 = 82;

// 目录操作
pub const SYS_mkdir: u64 = 83;
pub const SYS_rmdir: u64 = 84;

// 文件创建
pub const SYS_creat: u64 = 85;

// 文件链接
// TODO(TRACK-B62489): Phase N — implement hard links
pub const SYS_link: u64 = 86;
pub const SYS_unlink: u64 = 87;
// TODO(TRACK-CFB870): Phase N — implement symlinks
pub const SYS_symlink: u64 = 88;
pub const SYS_readlink: u64 = 89;

// 文件权限
pub const SYS_chmod: u64 = 90;
pub const SYS_fchmod: u64 = 91;
pub const SYS_chown: u64 = 92;
// TODO(TRACK-C3720B): Phase N — implement fchown as chown(fd→path) alias
pub const SYS_fchown: u64 = 93;

// 文件属性
pub const SYS_umask: u64 = 95;

// 时间
pub const SYS_gettimeofday: u64 = 96;
pub const SYS_getrlimit: u64 = 97;
pub const SYS_getrusage: u64 = 98;
pub const SYS_sysinfo: u64 = 99;

// 系统
// TODO(TRACK-1475D8): Phase N — implement times(2)
pub const SYS_times: u64 = 100;

// 用户/组
pub const SYS_getuid: u64 = 102;
pub const SYS_getgid: u64 = 104;
pub const SYS_setuid: u64 = 105;
pub const SYS_setgid: u64 = 106;
pub const SYS_geteuid: u64 = 107;
pub const SYS_getegid: u64 = 108;

pub const SYS_seteuid: u64 = 597; // QX 私有 (x86_64 无独立 seteuid syscall, 与 QX_SETEUID 同值)
pub const SYS_setegid: u64 = 598; // QX 私有 (x86_64 无独立 setegid syscall, 与 QX_SETEGID 同值)
pub const SYS_setreuid: u64 = 113;
pub const SYS_setregid: u64 = 114;

// 进程组
pub const SYS_getppid: u64 = 110;
pub const SYS_getpgid: u64 = 121;
pub const SYS_setsid: u64 = 112;
pub const SYS_getsid: u64 = 124;
pub const SYS_setpgid: u64 = 109;

// 进程调度
pub const SYS_getpriority: u64 = 140;
pub const SYS_setpriority: u64 = 141;

// 文件同步
pub const SYS_sync: u64 = 162;
pub const SYS_fsync: u64 = 74;
pub const SYS_fdatasync: u64 = 75;

// 挂载
pub const SYS_mount: u64 = 165;
pub const SYS_umount2: u64 = 166;

// 其他 POSIX
pub const SYS_gettid: u64 = 186;
pub const SYS_time: u64 = 201;
pub const SYS_clock_gettime: u64 = 228;
pub const SYS_exit_group: u64 = 231;
pub const SYS_tgkill: u64 = 234;

// 同步
pub const SYS_futex: u64 = 202;
// CPU 亲和性 (Linux 兼容号)
pub const SYS_sched_setaffinity: u64 = 203;
pub const SYS_sched_getaffinity: u64 = 204;

// 事件轮询
pub const SYS_epoll_create: u64 = 213;
pub const SYS_epoll_ctl: u64 = 233;
pub const SYS_epoll_wait: u64 = 232;

// eventfd / signalfd / timerfd (Linux x86_64 标准编号)
pub const SYS_eventfd: u64 = 284;
pub const SYS_eventfd2: u64 = 290;
pub const SYS_signalfd: u64 = 282;
pub const SYS_signalfd4: u64 = 289;
pub const SYS_timerfd_create: u64 = 283;
pub const SYS_timerfd_settime: u64 = 286;
pub const SYS_timerfd_gettime: u64 = 287;

// 内存建议 / 锁定 / mincore (Linux x86_64 标准编号)
pub const SYS_madvise: u64 = 28;
pub const SYS_mincore: u64 = 27;
pub const SYS_mlock: u64 = 149;
pub const SYS_munlock: u64 = 150;
pub const SYS_mlockall: u64 = 151;
pub const SYS_munlockall: u64 = 152;

// inotify (Linux x86_64 标准编号)
pub const SYS_inotify_init: u64 = 253;
pub const SYS_inotify_add_watch: u64 = 254;
pub const SYS_inotify_rm_watch: u64 = 255;
pub const SYS_inotify_init1: u64 = 294;

// POSIX Timer (Linux x86_64 标准编号)
pub const SYS_timer_create: u64 = 222;
pub const SYS_timer_settime: u64 = 223;
pub const SYS_timer_gettime: u64 = 224;
pub const SYS_timer_getoverrun: u64 = 225;
pub const SYS_timer_delete: u64 = 226;
pub const SYS_clock_getres: u64 = 229;

// 熵源 (Linux x86_64 标准编号)
pub const SYS_getrandom: u64 = 318;

// NUMA (Linux x86_64 标准编号)
pub const SYS_mbind: u64 = 237;
pub const SYS_set_mempolicy: u64 = 238;
pub const SYS_get_mempolicy: u64 = 239;
pub const SYS_migrate_pages: u64 = 256;
pub const SYS_getcpu: u64 = 309;

// 文件 I/O 扩展 (Linux x86_64 标准编号)
pub const SYS_readv: u64 = 19;
pub const SYS_writev: u64 = 20;
pub const SYS_pread64: u64 = 17;
pub const SYS_pwrite64: u64 = 18;
pub const SYS_sendfile: u64 = 40;
pub const SYS_preadv: u64 = 295;
pub const SYS_pwritev: u64 = 296;
pub const SYS_preadv2: u64 = 327;
pub const SYS_pwritev2: u64 = 328;
pub const SYS_flock: u64 = 73;
pub const SYS_fchmodat: u64 = 268;
pub const SYS_fchownat: u64 = 260;
pub const SYS_newfstatat: u64 = 262;
pub const SYS_unlinkat: u64 = 263;
pub const SYS_renameat: u64 = 264;
pub const SYS_renameat2: u64 = 316;
pub const SYS_linkat: u64 = 265;
pub const SYS_symlinkat: u64 = 266;
pub const SYS_readlinkat: u64 = 267;
pub const SYS_faccessat: u64 = 269;
pub const SYS_faccessat2: u64 = 439;
pub const SYS_fchmodat2: u64 = 452;
pub const SYS_statx: u64 = 332;
pub const SYS_copy_file_range: u64 = 326;
pub const SYS_name_to_handle_at: u64 = 303;
pub const SYS_open_by_handle_at: u64 = 304;
pub const SYS_fallocate: u64 = 285;
pub const SYS_utimensat: u64 = 280;
pub const SYS_openat: u64 = 257;
pub const SYS_openat2: u64 = 437;
pub const SYS_close_range: u64 = 436;

// FD 扩展 (Linux x86_64 标准编号)
pub const SYS_dup3: u64 = 292;
pub const SYS_pipe2: u64 = 293;
pub const SYS_epoll_create1: u64 = 291;
pub const SYS_epoll_pwait: u64 = 281;
pub const SYS_epoll_pwait2: u64 = 441;

// select / pselect / ppoll (Linux x86_64 标准编号)
pub const SYS_pselect6: u64 = 270;
pub const SYS_ppoll: u64 = 271;

// 进程扩展 (Linux x86_64 标准编号)
pub const SYS_set_robust_list: u64 = 273;
pub const SYS_get_robust_list: u64 = 274;
pub const SYS_pidfd_open: u64 = 434;
pub const SYS_pidfd_getfd: u64 = 438;
pub const SYS_pidfd_send_signal: u64 = 424;
pub const SYS_clone3: u64 = 435;
pub const SYS_execveat: u64 = 322;
pub const SYS_waitid: u64 = 247;
pub const SYS_process_vm_readv: u64 = 310;
pub const SYS_process_vm_writev: u64 = 311;

// 内存扩展 (Linux x86_64 标准编号)
pub const SYS_memfd_create: u64 = 319;
pub const SYS_userfaultfd: u64 = 323;

// 网络扩展 (Linux x86_64 标准编号)
pub const SYS_recvmmsg: u64 = 299;
pub const SYS_sendmmsg: u64 = 307;
pub const SYS_socketpair: u64 = 53;
pub const SYS_accept4: u64 = 288;

// 事件扩展 (Linux x86_64 标准编号)

// Seccomp / prctl (Linux x86_64 标准编号)
pub const SYS_seccomp: u64 = 317;
pub const SYS_prctl: u64 = 157;
pub const SYS_arch_prctl: u64 = 158;

// 安全 / 权限 (Linux x86_64 标准编号)
pub const SYS_capget: u64 = 125;
pub const SYS_capset: u64 = 126;
pub const SYS_pivot_root: u64 = 155;
pub const SYS_chroot: u64 = 161;

// 时间扩展 (Linux x86_64 标准编号)
pub const SYS_clock_nanosleep: u64 = 230;
pub const SYS_settimeofday: u64 = 164;
pub const SYS_adjtimex: u64 = 159;

// 杂项 (Linux x86_64 标准编号)
pub const SYS_reboot: u64 = 169;
pub const SYS_sethostname: u64 = 170;
pub const SYS_setdomainname: u64 = 171;

// ==================== Credo 私有 syscall (400-499, 避开 Linux 424-452) ====================
//
// 编号空间: 400-423 + 453-499. 424-452 保留给 Linux 现代扩展 syscall
// (pidfd_send_signal/io_uring/clone3/close_range/openat2/faccessat2/fchmodat2 等),
// 避免 QueenX 私有编号与未来 Linux ABI 冲突 (DECISION-037: 500+ 与 Linux 错开).

// ---------- 400-413: 认证 / 身份 ----------
pub const SYS_CREDO_LOGIN: u64 = 400;
pub const SYS_CREDO_LOGOUT: u64 = 401;
pub const SYS_CREDO_CREATE_IDENTITY: u64 = 402;
pub const SYS_CREDO_DELETE_IDENTITY: u64 = 403;
pub const SYS_CREDO_IDENTITY_INFO: u64 = 404;
pub const SYS_CREDO_CHANGE_PASSWORD: u64 = 405;
pub const SYS_CREDO_VERIFY_PASSWORD: u64 = 406;
pub const SYS_CREDO_CREATE_FIRST: u64 = 407;
pub const SYS_CREDO_GRANT: u64 = 408;
pub const SYS_CREDO_REVOKE: u64 = 409;
pub const SYS_CREDO_CHECK_CAP: u64 = 410;
pub const SYS_CREDO_GET_CAPS: u64 = 411;
pub const SYS_CREDO_GET_PWM: u64 = 412;
pub const SYS_CREDO_SET_PWM: u64 = 413;
// 414-419: 保留

// ---------- 420-423: 存储设备 ----------
pub const SYS_CREDO_DISK_LIST: u64 = 420;
pub const SYS_CREDO_DISK_INFO: u64 = 421;
pub const SYS_CREDO_DISK_FORMAT: u64 = 422;
pub const SYS_CREDO_DISK_PARTITION: u64 = 423;

// ---------- 453-463: 存储扩展 + 进程管理 + 系统信息 (避开 424-452) ----------
pub const SYS_CREDO_DISK_INSTALL: u64 = 453;
pub const SYS_CREDO_FAT_FORMAT: u64 = 454;
pub const SYS_CREDO_PROC_LIST: u64 = 455;
pub const SYS_CREDO_PROC_SETPRI: u64 = 456;
pub const SYS_CREDO_PROC_SLEEP: u64 = 457;
pub const SYS_CREDO_PROC_CPUTIME: u64 = 458;
pub const SYS_CREDO_GETHOSTNAME: u64 = 459;
pub const SYS_CREDO_SETHOSTNAME: u64 = 460;
pub const SYS_CREDO_BOOT_CHECK: u64 = 461;
pub const SYS_CREDO_REBOOT: u64 = 462;
pub const SYS_CREDO_HOTPLUG_STATUS: u64 = 463;

// ==================== 帧缓冲设备 (QueenX 私有, 与 QX_FB_* 同值) ====================
pub const SYS_FB_OPEN: u64 = 720;
pub const SYS_FB_MMAP: u64 = 721;
pub const SYS_FB_RELEASE: u64 = 722;

// ============================================================================
// QueenX 原生 syscall 编号 (500+)
//
// 遵循 queenx-naming-standpoint.md:
//   500-599 : 进程 / 内存 / 文件基础
//   600-699 : 网络 / IPC
//   700-799 : 设备 / 系统
//   800-899 : 扩展
//
// 编号原则:
//   - 不抄任何 OS 编号
//   - 按功能分区, 每区留扩展空间
//   - 0-299 直接使用 Linux 标准编号 (SYS_*)
// ============================================================================

// ---------- 500-509: Core I/O ----------
pub const QX_EXIT: u64 = 501;
pub const QX_WRITE: u64 = 502;
pub const QX_READ: u64 = 503;
pub const QX_OPEN: u64 = 504;
pub const QX_CLOSE: u64 = 505;
pub const QX_STAT: u64 = 506;
pub const QX_FSTAT: u64 = 507;
pub const QX_LSTAT: u64 = 508;
pub const QX_LSEEK: u64 = 509;

// ---------- 510-519: 内存管理 ----------
pub const QX_MMAP: u64 = 510;
pub const QX_BRK: u64 = 511;
pub const QX_MPROTECT: u64 = 512;
pub const QX_MUNMAP: u64 = 513;
pub const QX_MREMAP: u64 = 514;
// 515-519: reserved (madvise, mlock, munlock, mlockall, munlockall)

// ---------- 520-539: 进程管理 ----------
pub const QX_GETPID: u64 = 520;
pub const QX_FORK: u64 = 521;
pub const QX_EXECVE: u64 = 522;
pub const QX_CLONE: u64 = 523;
pub const QX_WAIT4: u64 = 524;
pub const QX_EXIT_GROUP: u64 = 525;
pub const QX_GETPPID: u64 = 526;
pub const QX_GETTID: u64 = 527;
pub const QX_GETPGID: u64 = 528;
pub const QX_SETPGID: u64 = 529;
pub const QX_GETSID: u64 = 530;
pub const QX_SETSID: u64 = 531;
pub const QX_NICE: u64 = 532;
pub const QX_SCHED_YIELD: u64 = 533;
pub const QX_SCHED_SETAFFINITY: u64 = 534;
pub const QX_SCHED_GETAFFINITY: u64 = 535;
pub const QX_GETPRIORITY: u64 = 536;
pub const QX_SETPRIORITY: u64 = 537;
pub const QX_TCGETPGRP: u64 = 538;
pub const QX_TCSETPGRP: u64 = 539;

// ---------- 540-559: 信号 ----------
pub const QX_RT_SIGACTION: u64 = 540;
pub const QX_RT_SIGPROCMASK: u64 = 541;
pub const QX_RT_SIGRETURN: u64 = 542;
pub const QX_KILL: u64 = 543;
pub const QX_TGKILL: u64 = 544;
// 545-559: 保留 (tkill, sigaltstack, rt_sigsuspend, ...)  // syscall 编号预留
pub const QX_TKILL: u64 = 545;
// P1-I-45: 接线 sigaltstack 替代栈系统调用
pub const QX_SIGALTSTACK: u64 = 546;

// ---------- 560-579: 文件系统操作 ----------
pub const QX_MKDIR: u64 = 560;
pub const QX_RMDIR: u64 = 561;
pub const QX_RENAME: u64 = 562;
pub const QX_LINK: u64 = 563;
pub const QX_UNLINK: u64 = 564;
pub const QX_SYMLINK: u64 = 565;
pub const QX_READLINK: u64 = 566;
pub const QX_CHMOD: u64 = 567;
pub const QX_FCHMOD: u64 = 568;
pub const QX_CHOWN: u64 = 569;
pub const QX_FCHOWN: u64 = 570;
pub const QX_FCHMODAT: u64 = 570; // fchmodat 映射到 FCHOWN 空间, 实际由 dispatch 区分
pub const QX_UMASK: u64 = 571;
pub const QX_ACCESS: u64 = 572;
pub const QX_TRUNCATE: u64 = 573;
pub const QX_FTRUNCATE: u64 = 574;
pub const QX_GETDENTS: u64 = 575;
pub const QX_GETCWD: u64 = 576;
pub const QX_CHDIR: u64 = 577;
pub const QX_CREAT: u64 = 578;
pub const QX_PIPE: u64 = 579;
pub const QX_PIPE2: u64 = 579; // pipe2 映射到 PIPE, flags 差异由 libc 处理

// ---------- 580-589: FD / 同步 / 挂载 ----------
pub const QX_DUP: u64 = 580;
pub const QX_DUP2: u64 = 581;
pub const QX_DUP3: u64 = 581; // dup3 映射到 DUP2, flags 差异由 libc 处理
pub const QX_FCNTL: u64 = 582;
pub const QX_IOCTL: u64 = 583;
pub const QX_SYNC: u64 = 584;
pub const QX_FSYNC: u64 = 585;
pub const QX_MOUNT: u64 = 586;
pub const QX_UMOUNT2: u64 = 587;
pub const QX_POLL: u64 = 588;
pub const QX_SELECT: u64 = 589;

// ---------- 590-599: 身份 + 文件锁 ----------
pub const QX_FLOCK: u64 = 590;
pub const QX_GETUID: u64 = 591;
pub const QX_GETGID: u64 = 592;
pub const QX_SETUID: u64 = 593;
pub const QX_SETGID: u64 = 594;
pub const QX_GETEUID: u64 = 595;
pub const QX_GETEGID: u64 = 596;
pub const QX_SETEUID: u64 = 597;
pub const QX_SETEGID: u64 = 598;
pub const QX_SETREUID: u64 = 599;
// QX_SETREGID 映射到 QX_SETREUID, 由 dispatch 区分

// ---------- 600-619: 网络 ----------
pub const QX_SOCKET: u64 = 600;
pub const QX_SOCKETPAIR: u64 = 600; // socketpair 映射到 SOCKET, 由 dispatch 区分
pub const QX_BIND: u64 = 601;
pub const QX_LISTEN: u64 = 602;
pub const QX_ACCEPT: u64 = 603;
pub const QX_CONNECT: u64 = 604;
pub const QX_SENDTO: u64 = 605;
pub const QX_RECVFROM: u64 = 606;
pub const QX_SENDMSG: u64 = 607;
pub const QX_RECVMSG: u64 = 608;
pub const QX_SHUTDOWN: u64 = 609;
pub const QX_SETSOCKOPT: u64 = 610;
pub const QX_GETSOCKOPT: u64 = 611;
pub const QX_GETSOCKNAME: u64 = 612;
pub const QX_GETPEERNAME: u64 = 613;
// 614-619: 保留 (socketpair, ...)  // syscall 编号预留

// ---------- 620-639: 同步 / IPC ----------
pub const QX_FUTEX: u64 = 620;
pub const QX_EPOLL_CREATE: u64 = 621;
pub const QX_EPOLL_CTL: u64 = 622;
pub const QX_EPOLL_WAIT: u64 = 623;
pub const QX_EVENTFD: u64 = 624;
pub const QX_EVENTFD2: u64 = 625;
pub const QX_SIGNALFD: u64 = 626;
pub const QX_SIGNALFD4: u64 = 627;
pub const QX_TIMERFD_CREATE: u64 = 628;
pub const QX_TIMERFD_SETTIME: u64 = 629;
pub const QX_TIMERFD_GETTIME: u64 = 630;
// 631-639: 保留 (msgqueue, shm, sem)  // syscall 编号预留

// ---------- 640-649: inotify ----------
pub const QX_INOTIFY_INIT1: u64 = 640;
pub const QX_INOTIFY_ADD_WATCH: u64 = 641;
pub const QX_INOTIFY_RM_WATCH: u64 = 642;

// ---------- 650-659: sendfile / splice ----------  // 高效拷贝/拼接 syscall
pub const QX_SENDFILE: u64 = 650;
pub const QX_SPLICE: u64 = 651;

// ---------- 700-709: 系统信息 ----------
pub const QX_UNAME: u64 = 700;
pub const QX_SYSINFO: u64 = 701;
pub const QX_GETRLIMIT: u64 = 702;
pub const QX_SETRLIMIT: u64 = 703;
pub const QX_GETRUSAGE: u64 = 704;

// ---------- 710-719: 时间 ----------
pub const QX_CLOCK_GETTIME: u64 = 710;
pub const QX_GETTIMEOFDAY: u64 = 711;
// 712: 保留 (settimeofday)  // syscall 编号预留
pub const QX_CLOCK_SETTIME: u64 = 712; // reserved
pub const QX_NANOSLEEP: u64 = 713;
pub const QX_ALARM: u64 = 714;
pub const QX_GETITIMER: u64 = 715;
pub const QX_SETITIMER: u64 = 716;
pub const QX_TIME: u64 = 717;
pub const QX_TIMES: u64 = 718;

// ---------- 720-729: 设备 ----------
pub const QX_FB_OPEN: u64 = 720;
pub const QX_FB_MMAP: u64 = 721;
pub const QX_FB_RELEASE: u64 = 722;

// ---------- 730-739: 设备固件加载 ----------
pub const QX_FW_LOAD: u64 = 730;
pub const QX_FW_GET: u64 = 731;
pub const QX_FW_GET_INFO: u64 = 732;
pub const QX_FW_DETACH: u64 = 733;

// ---------- 740-745: POSIX Timer ----------
/// 创建 per-process 定时器 (`timer_create`)
pub const QX_TIMER_CREATE: u64 = 740;
/// 启动 / 调整 / 停止定时器 (`timer_settime`)
pub const QX_TIMER_SETTIME: u64 = 741;
/// 查询定时器剩余时间 (`timer_gettime`)
pub const QX_TIMER_GETTIME: u64 = 742;
/// 释放定时器 (`timer_delete`)
pub const QX_TIMER_DELETE: u64 = 743;
/// 返回补打次数 (`timer_getoverrun`)
pub const QX_TIMER_GETOVERRUN: u64 = 744;
/// 时钟分辨率 (`clock_getres`)
pub const QX_CLOCK_GETRES: u64 = 745;

// ---------- 746-747: 熵源 / Stack Canary (P1 #14) ----------
/// 从内核熵源填充用户 buffer (Linux getrandom 兼容)
pub const QX_GETRANDOM: u64 = 746;
/// 读取当前进程 8 字节 stack canary (低字节恒为 0)
pub const QX_GET_CANARY: u64 = 747;

// ---------- 760-765: 内存建议与锁定 (madvise / mlock, P1 #15) ----------
/// 设置内存区域访问模式建议 (madvise)
pub const QX_MADVISE: u64 = 760;
/// 锁定 [addr, addr+len) 物理页禁止换出 (mlock)
pub const QX_MLOCK: u64 = 761;
/// 解除锁定 (munlock)
pub const QX_MUNLOCK: u64 = 762;
/// 进程级锁定所有/未来映射 (mlockall)
pub const QX_MLOCKALL: u64 = 763;
/// 解除进程级所有锁定 (munlockall)
pub const QX_MUNLOCKALL: u64 = 764;
/// 查询每页驻留性 (mincore)
pub const QX_MINCORE: u64 = 765;

// ---------- 800-809: 内核调试 / 跟踪 (ftrace / KGDB) ----------
/// 启用 ftrace 全局开关
pub const QX_FTRACE_ENABLE: u64 = 800;
/// 禁用 ftrace 全局开关
pub const QX_FTRACE_DISABLE: u64 = 801;
/// 从 ftrace ring buffer 读取一条事件到用户缓冲
pub const QX_FTRACE_READ: u64 = 802;
/// 查询 ftrace 状态 (`event_count` / `overflow_count`)
pub const QX_FTRACE_STAT: u64 = 803;
/// KGDB 主动断点 (用户态调试器触发)
pub const QX_KGDB_ENTER: u64 = 804;

// ==================== C7: Seccomp / prctl ====================

/// seccomp — 安装 Seccomp 过滤器
pub const QX_SECCOMP: u64 = 805;
/// prctl — 进程控制 (`Seccomp/no_new_privs` 子集)
pub const QX_PRCTL: u64 = 806;

// ==================== C5: 路由表 ====================

/// `route_add` — 添加路由条目
pub const QX_ROUTE_ADD: u64 = 807;
/// `route_del` — 删除路由条目
pub const QX_ROUTE_DEL: u64 = 808;
/// `route_query` — 查询路由 (最长前缀匹配)
pub const QX_ROUTE_QUERY: u64 = 809;

// ==================== C5: Netfilter ====================

/// `nf_add_rule` — 添加 Netfilter 规则
pub const QX_NF_ADD_RULE: u64 = 810;
/// `nf_del_rule` — 删除 Netfilter 规则
pub const QX_NF_DEL_RULE: u64 = 811;

// ==================== C4: io_uring ====================

/// `io_uring_setup` — 创建 `io_uring` 实例
pub const QX_IO_URING_SETUP: u64 = 812;
/// `io_uring_enter` — 提交/等待完成
pub const QX_IO_URING_ENTER: u64 = 813;
/// `io_uring_register` — 注册缓冲区/文件
pub const QX_IO_URING_REGISTER: u64 = 814;
/// `io_uring_submit_sqe` — 提交单个 SQE (简化版)
pub const QX_IO_URING_SUBMIT: u64 = 815;

// ==================== D1: Namespace ====================

/// unshare — 取消共享指定 namespace
pub const QX_UNSHARE: u64 = 820;
/// setns — 切换到指定 namespace
pub const QX_SETNS: u64 = 821;

// ==================== D2: cgroup ====================

/// `cgroup_create` — 创建子 cgroup
pub const QX_CGROUP_CREATE: u64 = 830;
/// `cgroup_destroy` — 删除 cgroup
pub const QX_CGROUP_DESTROY: u64 = 831;
/// `cgroup_attach` — 将进程迁移到 cgroup
pub const QX_CGROUP_ATTACH: u64 = 832;
/// `cgroup_set_limit` — 设置 cgroup 资源限制
pub const QX_CGROUP_SET_LIMIT: u64 = 833;
/// `cgroup_get_stat` — 获取 cgroup 统计信息
pub const QX_CGROUP_GET_STAT: u64 = 834;

// ==================== D3: NUMA ====================

/// `get_mempolicy` — 获取 NUMA 内存策略
pub const QX_GET_MEMPOLICY: u64 = 840;
/// `set_mempolicy` — 设置 NUMA 内存策略
pub const QX_SET_MEMPOLICY: u64 = 841;
/// `migrate_pages` — 迁移进程页面到目标节点
pub const QX_MIGRATE_PAGES: u64 = 842;
/// getcpu — 获取当前 CPU 和 NUMA 节点
pub const QX_GETCPU: u64 = 843;

// ==================== D4: eBPF ====================

/// bpf — BPF 系统调用多路复用
pub const QX_BPF: u64 = 850;

// ==================== D5: 电源管理 ====================

/// pm — 电源管理系统调用
pub const QX_PM: u64 = 860;

// ==================== D6: 安全启动 + TPM ====================

/// `secure_boot` — 安全启动系统调用
pub const QX_SECURE_BOOT: u64 = 870;

/// tpm — TPM 系统调用
pub const QX_TPM: u64 = 871;

// ==================== D7: Shadow Stack (CET) ====================

/// cet — CET/Shadow Stack 系统调用
pub const QX_CET: u64 = 880;

// ==================== D8: Tickless (NO_HZ) ====================  // 动态时钟节拍模式

/// tickless — Tickless 系统调用
pub const QX_TICKLESS: u64 = 881;

// ==================== D9: NTP/PTP 时钟同步 ====================

/// timesync — 时间同步系统调用
pub const QX_TIMESYNC: u64 = 882;

// ==================== D10: kexec ====================

/// kexec — 直接内核引导系统调用
pub const QX_KEXEC: u64 = 883;

// ==================== D11: UEFI ====================

/// uefi — UEFI 运行时服务系统调用
pub const QX_UEFI: u64 = 884;

// ==================== D12: 扩展属性 (xattr) ====================

/// setxattr — 设置扩展属性
pub const QX_SETXATTR: u64 = 890;
/// getxattr — 获取扩展属性
pub const QX_GETXATTR: u64 = 891;
/// listxattr — 列出扩展属性
pub const QX_LISTXATTR: u64 = 892;
/// removexattr — 删除扩展属性
pub const QX_REMOVEXATTR: u64 = 893;

// ==================== D13: 快照 (snapshot) ====================

/// `snapshot_create` — 创建快照
pub const QX_SNAPSHOT_CREATE: u64 = 895;
/// `snapshot_destroy` — 销毁快照
pub const QX_SNAPSHOT_DESTROY: u64 = 896;
/// `snapshot_rollback` — 回滚快照
pub const QX_SNAPSHOT_ROLLBACK: u64 = 897;
/// `snapshot_clone` — 从快照创建克隆
pub const QX_SNAPSHOT_CLONE: u64 = 898;

// ==================== POSIX errno (使用 Linux 风格: 返回值 = -errno) ====================
//
// B09-12/DECISION-H13 P0-1: Errno 定义已迁回 framework (framework::errno),
// 本处 re-export 保持调用方兼容 (services→framework 单向依赖).
pub use crate::framework::errno::Errno;

// ==================== 辅助类型 ====================

pub type SyscallResult<T> = Result<T, Errno>;

// ============================================================================
// 编译期唯一性断言 (B05-02 防御)
//
// 防止未来新增 syscall 编号时引入重复值, 在编译期即失败.
// 若新增编号命中下方任一断言, 说明与现有 Linux 或私有编号冲突, 须重新分配.
// ============================================================================

/// 编译期断言: Linux 兼容编号 (0-299) 内部无重复
const _: () = {
    // 所有 Linux 标准编号 (0-299 区段)
    const LINUX_NUMS: &[u64] = &[
        SYS_read, SYS_write, SYS_open, SYS_close, SYS_stat, SYS_fstat, SYS_lstat, SYS_poll,
        SYS_lseek, SYS_mmap, SYS_mprotect, SYS_munmap, SYS_brk, SYS_rt_sigaction,
        SYS_rt_sigprocmask, SYS_rt_sigreturn, SYS_ioctl, SYS_access, SYS_pipe, SYS_select,
        SYS_sched_yield, SYS_mremap, SYS_dup, SYS_dup2, SYS_nanosleep, SYS_getitimer, SYS_alarm,
        SYS_setitimer, SYS_getpid, SYS_socket, SYS_connect, SYS_accept, SYS_sendto, SYS_recvfrom,
        SYS_sendmsg, SYS_recvmsg, SYS_shutdown, SYS_bind, SYS_listen, SYS_getsockname,
        SYS_getpeername, SYS_setsockopt, SYS_getsockopt, SYS_clone, SYS_fork, SYS_execve, SYS_exit,
        SYS_wait4, SYS_kill, SYS_uname, SYS_fcntl, SYS_flock, SYS_fsync, SYS_fdatasync,
        SYS_truncate, SYS_ftruncate, SYS_getdents, SYS_getcwd, SYS_chdir, SYS_rename, SYS_mkdir,
        SYS_rmdir, SYS_creat, SYS_link, SYS_unlink, SYS_symlink, SYS_readlink, SYS_chmod,
        SYS_fchmod, SYS_chown, SYS_fchown, SYS_umask, SYS_gettimeofday, SYS_getrlimit,
        SYS_getrusage, SYS_sysinfo, SYS_times, SYS_getuid, SYS_getgid, SYS_setuid, SYS_setgid,
        SYS_geteuid, SYS_getegid, SYS_setreuid, SYS_setregid, SYS_getppid, SYS_getpgid,
        SYS_setsid, SYS_getsid, SYS_setpgid, SYS_getpriority, SYS_setpriority, SYS_sync,
        SYS_mount, SYS_umount2, SYS_gettid, SYS_time, SYS_clock_gettime, SYS_exit_group,
        SYS_tgkill, SYS_futex, SYS_sched_setaffinity, SYS_sched_getaffinity, SYS_epoll_create,
        SYS_epoll_ctl, SYS_epoll_wait, SYS_eventfd, SYS_eventfd2, SYS_signalfd, SYS_signalfd4,
        SYS_timerfd_create, SYS_timerfd_settime, SYS_timerfd_gettime, SYS_madvise, SYS_mincore,
        SYS_mlock, SYS_munlock, SYS_mlockall, SYS_munlockall, SYS_inotify_init,
        SYS_inotify_add_watch, SYS_inotify_rm_watch, SYS_inotify_init1, SYS_timer_create,
        SYS_timer_settime, SYS_timer_gettime, SYS_timer_getoverrun, SYS_timer_delete,
        SYS_clock_getres, SYS_getrandom, SYS_mbind, SYS_set_mempolicy, SYS_get_mempolicy,
        SYS_migrate_pages, SYS_getcpu, SYS_readv, SYS_writev, SYS_pread64, SYS_pwrite64,
        SYS_sendfile, SYS_preadv, SYS_pwritev, SYS_preadv2, SYS_pwritev2, SYS_fchmodat,
        SYS_fchownat, SYS_newfstatat, SYS_unlinkat, SYS_renameat, SYS_renameat2, SYS_linkat,
        SYS_symlinkat, SYS_readlinkat, SYS_faccessat, SYS_faccessat2, SYS_fchmodat2, SYS_statx,
        SYS_copy_file_range, SYS_name_to_handle_at, SYS_open_by_handle_at, SYS_fallocate,
        SYS_utimensat, SYS_openat, SYS_openat2, SYS_close_range, SYS_dup3, SYS_pipe2,
        SYS_epoll_create1, SYS_epoll_pwait, SYS_epoll_pwait2, SYS_pselect6, SYS_ppoll,
        SYS_set_robust_list, SYS_get_robust_list, SYS_pidfd_open, SYS_pidfd_getfd,
        SYS_pidfd_send_signal, SYS_clone3, SYS_execveat, SYS_waitid, SYS_process_vm_readv,
        SYS_process_vm_writev, SYS_memfd_create, SYS_userfaultfd, SYS_recvmmsg, SYS_sendmmsg,
        SYS_socketpair, SYS_accept4, SYS_seccomp, SYS_prctl, SYS_arch_prctl, SYS_capget,
        SYS_capset, SYS_pivot_root, SYS_chroot, SYS_clock_nanosleep, SYS_settimeofday,
        SYS_adjtimex, SYS_reboot, SYS_sethostname, SYS_setdomainname,
    ];
    let mut i = 0;
    while i < LINUX_NUMS.len() {
        let mut j = i + 1;
        while j < LINUX_NUMS.len() {
            assert!(LINUX_NUMS[i] != LINUX_NUMS[j]);
            j += 1;
        }
        i += 1;
    }
};

/// 编译期断言: 私有编号区 (400+, 不含 Linux 兼容区) 内部无重复
///
/// 每个逻辑 syscall 只列一个代表值; 下列设计别名 (同编号, 由 dispatch 区分语义)
/// 已从断言中剔除, 避免误判:
///   - `SYS_FB_*` == `QX_FB_*` (720-722, 帧缓冲别名)
///   - `SYS_seteuid`/`SYS_setegid` == `QX_SETEUID`/`QX_SETEGID` (597/598)
///   - `QX_FCHMODAT` == `QX_FCHOWN` (570) / `QX_PIPE2` == `QX_PIPE` (579)
///   - `QX_DUP3` == `QX_DUP2` (581) / `QX_SOCKETPAIR` == `QX_SOCKET` (600)
const _: () = {
    const PRIVATE_NUMS: &[u64] = &[
        SYS_CREDO_LOGIN, SYS_CREDO_LOGOUT, SYS_CREDO_CREATE_IDENTITY, SYS_CREDO_DELETE_IDENTITY,
        SYS_CREDO_IDENTITY_INFO, SYS_CREDO_CHANGE_PASSWORD, SYS_CREDO_VERIFY_PASSWORD,
        SYS_CREDO_CREATE_FIRST, SYS_CREDO_GRANT, SYS_CREDO_REVOKE, SYS_CREDO_CHECK_CAP,
        SYS_CREDO_GET_CAPS, SYS_CREDO_GET_PWM, SYS_CREDO_SET_PWM, SYS_CREDO_DISK_LIST,
        SYS_CREDO_DISK_INFO, SYS_CREDO_DISK_FORMAT, SYS_CREDO_DISK_PARTITION,
        SYS_CREDO_DISK_INSTALL, SYS_CREDO_FAT_FORMAT, SYS_CREDO_PROC_LIST, SYS_CREDO_PROC_SETPRI,
        SYS_CREDO_PROC_SLEEP, SYS_CREDO_PROC_CPUTIME, SYS_CREDO_GETHOSTNAME,
        SYS_CREDO_SETHOSTNAME, SYS_CREDO_BOOT_CHECK, SYS_CREDO_REBOOT, SYS_CREDO_HOTPLUG_STATUS,
        QX_FB_OPEN, QX_FB_MMAP, QX_FB_RELEASE, QX_SETEUID, QX_SETEGID, QX_UNAME, QX_SYSINFO,
        QX_GETRLIMIT, QX_SETRLIMIT, QX_GETRUSAGE, QX_CLOCK_GETTIME, QX_GETTIMEOFDAY,
        QX_NANOSLEEP, QX_ALARM, QX_GETITIMER, QX_SETITIMER, QX_TIME, QX_TIMES, QX_FW_LOAD,
        QX_FW_GET, QX_FW_GET_INFO, QX_FW_DETACH, QX_TIMER_CREATE, QX_TIMER_SETTIME,
        QX_TIMER_GETTIME, QX_TIMER_DELETE, QX_TIMER_GETOVERRUN, QX_CLOCK_GETRES, QX_GETRANDOM,
        QX_GET_CANARY, QX_MADVISE, QX_MLOCK, QX_MUNLOCK, QX_MLOCKALL, QX_MUNLOCKALL, QX_MINCORE,
        QX_FTRACE_ENABLE, QX_FTRACE_DISABLE, QX_FTRACE_READ, QX_FTRACE_STAT, QX_KGDB_ENTER,
        QX_SECCOMP, QX_PRCTL, QX_ROUTE_ADD, QX_ROUTE_DEL, QX_ROUTE_QUERY, QX_NF_ADD_RULE,
        QX_NF_DEL_RULE, QX_IO_URING_SETUP, QX_IO_URING_ENTER, QX_IO_URING_REGISTER,
        QX_IO_URING_SUBMIT, QX_UNSHARE, QX_SETNS, QX_CGROUP_CREATE, QX_CGROUP_DESTROY,
        QX_CGROUP_ATTACH, QX_CGROUP_SET_LIMIT, QX_CGROUP_GET_STAT, QX_GET_MEMPOLICY,
        QX_SET_MEMPOLICY, QX_MIGRATE_PAGES, QX_GETCPU, QX_BPF, QX_PM, QX_SECURE_BOOT, QX_TPM,
        QX_CET, QX_TICKLESS, QX_TIMESYNC, QX_KEXEC, QX_UEFI, QX_SETXATTR, QX_GETXATTR, QX_LISTXATTR,
        QX_REMOVEXATTR, QX_SNAPSHOT_CREATE, QX_SNAPSHOT_DESTROY, QX_SNAPSHOT_ROLLBACK,
        QX_SNAPSHOT_CLONE,
    ];
    let mut i = 0;
    while i < PRIVATE_NUMS.len() {
        let mut j = i + 1;
        while j < PRIVATE_NUMS.len() {
            assert!(PRIVATE_NUMS[i] != PRIVATE_NUMS[j]);
            j += 1;
        }
        i += 1;
    }
};
