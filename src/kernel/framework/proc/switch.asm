BITS 64

section .text
global process_switch_asm
global user_entry_trampoline
extern user_entry_target
extern user_entry_cr3
; P2.B + F-13 (DECISION-051): GDT 选择子强绑定.
; O-04 (proc/switch.asm:113 `mov ax, 0x23` 硬编码) 同期处理.
extern SELECTOR_USER_CODE

; void process_switch_asm(ProcessContext* prev, const ProcessContext* next);
; RDI = *mut ProcessContext (save current register state here)
; RSI = *const ProcessContext (load next register state from here)
;
; ProcessContext layout (each field 8 bytes):
;   +0:   r15
;   +8:   r14
;  +16:   r13
;  +24:   r12
;  +32:   rbx
;  +40:   rbp
;  +48:   rax
;  +56:   rip
;  +64:   rsp
;  +72:   rflags
;  +80:   cr3
;  +88:   cs
;  +96:   ds
; +104:   es
; +112:   fs
; +120:   gs
; +128:   ss
; +136:   _fpu_pad (8 bytes padding for 16-byte alignment)
; +144:   fpu_state[64] (512 bytes, Phase 2: fxsave/fxrstor)
; +656:   fpcr  (aarch64)
; +664:   fpsr  (aarch64)
; +672:   extra_regs[8] (B05-55: rdi/rsi/rdx/rcx/r8/r9/r10/r11, 首次进入用户态
;                      的进程 (fork 子进程) 由 iretq 前恢复, 见 types.rs)

process_switch_asm:
    cli

    ; Save current context to [RDI] (prev)
    mov [rdi + 0], r15
    mov [rdi + 8], r14
    mov [rdi + 16], r13
    mov [rdi + 24], r12
    mov [rdi + 32], rbx
    mov [rdi + 40], rbp
    mov [rdi + 48], rax

    ; Save rip, rsp, rflags from stack
    mov rax, [rsp]
    mov [rdi + 56], rax        ; rip (return address)
    lea rax, [rsp + 8]
    mov [rdi + 64], rax        ; rsp
    pushfq
    pop rax
    mov [rdi + 72], rax        ; rflags

    ; Save cr3
    mov rax, cr3
    mov [rdi + 80], rax

    ; Save segment registers
    mov [rdi + 88], cs
    mov [rdi + 96], ds
    mov [rdi + 104], es
    mov [rdi + 112], fs
    mov [rdi + 120], gs
    mov [rdi + 128], ss

    ; Save FPU/SSE state (fxsave requires 16-byte aligned memory)
    ; fpu_state is at offset 144 (17 fields + 1 padding = 18 * 8 = 144 bytes)
    lea rax, [rdi + 144]
    fxsave [rax]

    ; Restore next context from [RSI] (next)
    mov r15, [rsi + 0]
    mov r14, [rsi + 8]
    mov r13, [rsi + 16]
    mov r12, [rsi + 24]
    mov rbx, [rsi + 32]
    mov rbp, [rsi + 40]

    ; Set cr3
    mov rax, [rsi + 80]
    mov cr3, rax

    ; Restore segment registers (ds, es, fs, gs)
    ; cs and ss are restored via iretq frame
    ; B05-55 修复: 调度器上下文 (syscall/中断入口 swapgs 后) GS base=per_cpu,
    ; KERNEL_GS_BASE=0. 切到用户进程前先 swapgs, 使 KERNEL_GS_BASE=per_cpu —
    ; 否则用户态异常/中断入口 isr_common/irq_common 的 swapgs 会把 KERNEL_GS_BASE
    ; (0) 换入 GS base → [gs:KERNEL_PML4_OFF] 访问地址 8 → #PF → 死循环.
    ; GS base 随后由 mov gs (用户数据段 base=0) 恢复为 0 (用户 GS).
    ; 仅 next 为用户态 (cs=0x23) 时 swapgs; 内核态/内核线程切换 (cs=0x08) 不 swapgs.
    ;
    ; GS 段寄存器装载**仅限用户态路径**: `mov gs, sel` 以描述符基址 (数据段基址恒 0)
    ; 写入 IA32_GS_BASE (见 arch/x86_64/mod.rs enter_user_asm 处同一结论). 用户态路径
    ; 正需 base=0, 且此时 KERNEL_GS_BASE 已在 swapgs 后为 per-CPU 地址; 而内核态恢复
    ; 路径 (任务阻塞在 syscall 内被换回) 若照搬装载, 会把 per-CPU 基址清零, 使随后
    ; 内核态 [gs:...] 访问 (KPTI 出口读 USER_PML4) 落到物理低地址 → 读垃圾 CR3 挂起.
    cmp word [rsi + 88], 0x23
    jne .no_swapgs_next
    swapgs
    mov ax, [rsi + 120]
    mov gs, ax
.no_swapgs_next:
    mov ax, [rsi + 96]
    mov ds, ax
    mov ax, [rsi + 104]
    mov es, ax
    mov ax, [rsi + 112]
    mov fs, ax

    ; Restore FPU/SSE state (fxrstor requires 16-byte aligned memory)
    ; fpu_state is at offset 144 (17 fields + 1 padding = 18 * 8 = 144 bytes)
    lea rax, [rsi + 144]
    fxrstor [rax]

    ; ── 路径分派: 内核线程 (cs=0x08) vs 用户态 (cs=0x23) ─────────────
    ; 这是本文件的第一个 0x23 判断, 作用是**选择栈切换方式**;
    ; 下方 caller-saved 恢复处还有第二个 0x23 判断, 作用是**决定是否恢复
    ; rdi/rsi/rdx/rcx/r8-r11** (见该处注释). 两者读同一字段, 目的不同.
    ;
    ; iretq 在同特权级下只弹 RIP/CS/RFLAGS, 不加载 RSP/SS (x86 SDM),
    ; 因此不能靠 iretq 完成内核线程的栈切换.
    ; 改用 mov rsp + jmp 实现: 保存侧把 rip 存为 context_switch 的返回地址、
    ; rsp 存为该返回地址之上的位置 (见本文件上方 mov rax,[rsp] / lea rax,[rsp+8]),
    ; 故 "mov rsp, [rsi+64]; jmp qword [rsi+56]" 与 ret 完全等价, 与保存侧对称.
    ; 此时 rsi 仍指向 next 的 ProcessContext (mov rsp 不改 rsi), 该结构位于
    ; 内核内存, 换栈后依旧可读.
    cmp word [rsi + 88], 0x23
    je .switch_user_path
    mov rax, [rsi + 48]         ; 内核线程仅需恢复 rax
    mov rsp, [rsi + 64]         ; 切到 next 的栈
    jmp qword [rsi + 56]        ; 跳到 next 的 rip (首次为 idle_entry)
.switch_user_path:

    ; 统一内核栈契约 (D5): 任务内核栈 (prev 的 syscall/中断栈) 已是高半区 VA
    ; (phys + KERNEL_BASE, 见 cpu::arch::set_kernel_stack), 经用户页表共享的
    ; 高半区直接映射 (pd_high) 天然可达, 无需别名转换; 仅"首个任务进入用户态
    ; 之前"的运行栈 (boot 低半区栈) 需要转 KERNEL_BASE 别名后才能在用户页表中
    ; push/iretq. 故此处按 RSP 实际所处半区条件转换, 避免对高半区栈重复偏移.
    mov rax, 0xFFFF800000000000    ; KERNEL_BASE
    cmp rsp, rax
    jae .rsp_already_high          ; 已是高半区 (任务内核栈) → 不再偏移
    add rsp, rax                   ; 低半区 (boot 栈) → 转高半区别名
.rsp_already_high:

    ; Build iretq frame
    push qword [rsi + 128]      ; ss
    push qword [rsi + 64]       ; rsp
    push qword [rsi + 72]       ; rflags
    push qword [rsi + 88]       ; cs
    push qword [rsi + 56]       ; rip

    ; B05-55 修复: 恢复 caller-saved 寄存器 (rdi/rsi/rdx/rcx/r8-r11) — 仅用户态.
    ; ProcessContext 布局: fpu_state[64] @ 144 (512B), fpcr @ 656, fpsr @ 664,
    ; extra_regs[8] @ 672 (B05-55 新增, 见 services/proc/types.rs).
    ; 已运行过的进程返回用户态时, 寄存器由 syscall/中断栈的 InterruptFrame
    ; (schedule 后 iretq) 覆盖恢复, 此处不影响. 首次被调度的进程 (fork 子进程)
    ; 用这些继承的寄存器值 (fork 时父进程的 rdi 等) 进入用户态.
    ; ⚠ 必须放在所有 [rsi] (ProcessContext) 访问之后、iretq 之前, 且恢复 rax
    ; 之后 (rax 是 fork 返回值, 不能被覆盖).
    ; 这是本文件的第二个 0x23 判断: 仅用户态恢复 caller-saved 寄存器
    ; (第一个 0x23 判断在 fxrstor 之后, 作用是选择内核线程/用户态的栈切换方式;
    ; 内核线程分支已在此前 jmp 走, 不会到达本处).
    cmp word [rsi + 88], 0x23
    jne .no_restore_callersaved
    mov rax, rsi                ; rax 暂存 ctx 指针 (rsi 即将被覆盖)
    mov rdi, [rax + 672]        ; rdi
    mov rsi, [rax + 680]        ; rsi (从 [rax+...] 读, rax 保持 ctx)
    mov rdx, [rax + 688]
    mov rcx, [rax + 696]
    mov r8,  [rax + 704]
    mov r9,  [rax + 712]
    mov r10, [rax + 720]
    mov r11, [rax + 728]
    mov rax, [rax + 48]         ; rax = fork 返回值 (最后恢复)
    jmp .iretq_now
.no_restore_callersaved:
    ; Restore rax before iretq (内核线程切换)
    mov rax, [rsi + 48]
.iretq_now:
    iretq

user_entry_trampoline:
    ; P2.B + F-13 (DECISION-051 简化方案): CS = 用户代码段 (DPL=3).
    ; 字节长度与原 mov ax, 0x23 一致 (4 字节), 避免 label 偏移重定义.
    ; 单一来源: src/kernel/framework/link/x86_64.ld SELECTOR_USER_CODE_RPL3 与
    ; gdt.rs pub const SELECTOR_USER_CODE 同步 (host-tests 校验).
    mov ax, 0x23
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    mov rax, [rel user_entry_cr3]
    mov cr3, rax

    jmp [rel user_entry_target]

section .note.GNU-stack noalloc noexec nowrite progbits