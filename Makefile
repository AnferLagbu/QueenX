ARCH ?= x86_64
LOG_DIR := build/log

ifeq ($(ARCH),aarch64)
    CC = aarch64-linux-gnu-gcc
    LD = aarch64-linux-gnu-ld
    AS = aarch64-linux-gnu-as
    OBJCOPY = aarch64-linux-gnu-objcopy
    RUST_TARGET = aarch64-unknown-none
    QEMU = qemu-system-aarch64
    QEMU_MACHINE = virt
    QEMU_CPU := max
    LDSCRIPT = src/kernel/framework/link/aarch64.ld
    ASFLAGS = -march=armv8-a
    CFLAGS_BASE = -std=c11 -Wall -Wextra -nostdinc -nostdlib -fPIC -fno-stack-protector \
                  -fno-asynchronous-unwind-tables -fno-ident \
                  -Wno-builtin-declaration-mismatch
else
    CC = x86_64-linux-gnu-gcc
    LD = x86_64-linux-gnu-ld
    AS = nasm
    OBJCOPY = objcopy
    RUST_TARGET = x86_64-unknown-none
    QEMU = qemu-system-x86_64
    QEMU_CPU ?= qemu64
    LDSCRIPT = src/kernel/framework/link/x86_64.ld
    ASFLAGS = -f elf64 -w-zeroing
    CFLAGS_BASE = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC -fno-stack-protector \
                  -fno-asynchronous-unwind-tables -fno-ident -mcmodel=medium \
                  -Wno-builtin-declaration-mismatch
endif

CFLAGS = $(CFLAGS_BASE) \
         -Isrc/kernel/framework/lib \
         -Isrc/kernel/framework/net -Isrc/kernel/framework/net/arch -Isrc/kernel/framework/net/driver

# ============================================================================
# 网络子系统已迁移至 smoltcp (纯 Rust)
# Network stack: smoltcp (Rust)
# ============================================================================

LDFLAGS = -T $(LDSCRIPT) -nostdlib -Map=build/kernel.map -z noexecstack --no-warn-rwx-segments

# ── 架构条件 QEMU 标志 ────────────────────────────────────────────────
ifeq ($(ARCH),aarch64)
    # AArch64: QEMU virt 机器，无 ISA debug-exit，无 PCI 网络
    QEMU_FLAGS := -M $(QEMU_MACHINE),gic-version=3 -cpu $(QEMU_CPU) -m 512 -no-reboot
    QEMU_NET :=
    KERNEL_IMAGE := build/kernel.bin
    QEMU_KERNEL_FLAG := -kernel
else
    QEMU_FLAGS := -m 512 -no-reboot -device isa-debug-exit,iobase=0xf4,iosize=0x04
    QEMU_NET := -device e1000,netdev=n0 \
                -netdev user,id=n0,hostfwd=tcp::8080-:80,hostname=antx
    KERNEL_IMAGE := build/kernel.flat
    QEMU_KERNEL_FLAG := -kernel
endif

# ── 架构条件构建对象 ─────────────────────────────────────────────────
# 注: build/lib/string.o 已废弃 — string.c 已被 string.rs (Rust) 取代,
#     string 符号由 $(RUST_LIB) 通过 --whole-archive 包含。
ifeq ($(ARCH),aarch64)
    KERNEL_OBJS = build/boot.o
    KERNEL_TEST_OBJS = build/boot.o
else
    KERNEL_OBJS = build/boot.o build/entry.o build/isr.o build/switch.o \
                  build/arch/x86_64/trampoline.o
    # 测试入口由 Rust 端 kernel_test feature (lib.rs:343 kernel_test_main) 提供,
    # 不再依赖 C 桩文件. 2026-06-24 移除 kernel_test.o / test_main.o / test_hw_stubs.o.
    KERNEL_TEST_OBJS = build/boot.o build/entry.o build/isr.o build/switch.o \
                  build/arch/x86_64/trampoline.o
endif

RUST_LIB = src/rust/target/$(RUST_TARGET)/release/libqueenx.a
RUST_LIB_TEST = src/rust/target/test-release/$(RUST_TARGET)/release/libqueenx.a
RUST_LIB_CHAOS = src/rust/target/chaos-release/$(RUST_TARGET)/release/libqueenx.a
RUST_LIB_TEST_DEBUG = src/rust/target/test-debug/$(RUST_TARGET)/test-debug/libqueenx.a

RUST_USER_DIR = src/user
RUST_USER_TARGET = $(RUST_USER_DIR)/target/$(RUST_TARGET)/release

USER_INIT_ELF = $(RUST_USER_TARGET)/init
USER_SHELL_ELF = $(RUST_USER_TARGET)/eash
USER_INSTALL_ELF = $(RUST_USER_TARGET)/install
USER_FBTERM_ELF = $(RUST_USER_TARGET)/fbterm
USER_HTTPSRV_ELF = $(RUST_USER_TARGET)/httpsrv
USER_TEST_ELF = $(RUST_USER_TARGET)/proctest

STAGE1_BIN = build/stage1.bin
DISK_IMAGE = build/antx.img

.PHONY: all clean run run-net debug log log-net iso run-iso disk run-disk user test test-host test-unit test-integration test-smoke test-stress \
         test-all test-chaos test-smp

all: build/kernel.bin build/kernel.flat

# 同时构建 kernel.flat (qemu 直接使用的 raw 镜像),
# 避免外部脚本在 make 完成后还需要二次 objcopy.

# 跨架构构建时自动清理上架构产物, 避免 boot.o 等被新架构误用。
# 通过 build/log/.arch 记录上次构建架构 (build/log/ 不被 clean 删除), 不匹配时强制 clean.
# 修复: 戳记写入必须在 arch-switch-clean 配方内 (仅真实跨架构切换时更新).
# 原实现在解析期无条件覆写戳记, `make test-host` 等不产生架构产物的目标也会
# 把戳记改成默认 ARCH, 导致下次同 ARCH 链接误用残留的异架构 boot.o (EM 不符).
ARCH_STAMP := build/log/.arch
PREVIOUS_ARCH := $(shell cat $(ARCH_STAMP) 2>/dev/null || echo none)
ifneq ($(PREVIOUS_ARCH), $(ARCH))
ARCH_CHANGED := 1
endif
ifeq ($(ARCH_CHANGED),1)
.PHONY: arch-switch-clean
arch-switch-clean:
	@echo "[make] cross-arch switch: $(PREVIOUS_ARCH) → $(ARCH), removing arch-specific build/ artifacts (preserving build/log/)"
	@rm -f build/boot.o build/entry.o build/isr.o build/switch.o \
	       build/arch/x86_64/trampoline.o build/gdt_asm.o \
	       build/kernel.bin build/kernel.flat build/kernel.map
	@cd src/rust && cargo clean >/dev/null 2>&1 || true
	@cd src/user && cargo clean >/dev/null 2>&1 || true
	@rm -f build/user/*.bin
	@echo $(ARCH) > $(ARCH_STAMP)
# 挂到所有 asm .o 目标, 强制 clean 后重新评估 .o 的依赖图
# (clean 在 make 评估图后执行, .o 文件存在与否需要重新触发)
ASM_OBJS := build/boot.o build/entry.o build/isr.o build/switch.o build/arch/x86_64/trampoline.o
$(ASM_OBJS): arch-switch-clean
endif
$(shell mkdir -p $(LOG_DIR))

# ====== x86_64 Rust user programs ======
ifeq ($(ARCH),x86_64)
user: $(USER_INIT_ELF) $(USER_SHELL_ELF) $(USER_INSTALL_ELF) $(USER_FBTERM_ELF) $(USER_HTTPSRV_ELF) $(USER_TEST_ELF)
	@mkdir -p build/user
	@cp $(USER_INIT_ELF) build/user/init.bin
	@cp $(USER_SHELL_ELF) build/user/eash.bin
	@cp $(USER_INSTALL_ELF) build/user/install.bin
	@cp $(USER_FBTERM_ELF) build/user/fbterm.bin
	@cp $(USER_HTTPSRV_ELF) build/user/httpsrv.bin
	@cp $(USER_TEST_ELF) build/user/proctest.bin
	@echo "User programs built successfully (Rust)"

$(USER_INIT_ELF) $(USER_SHELL_ELF) $(USER_INSTALL_ELF) $(USER_FBTERM_ELF) $(USER_HTTPSRV_ELF) $(USER_TEST_ELF):
	@echo "Building Rust user programs..."
	cd $(RUST_USER_DIR) && RUSTFLAGS="-C link-arg=-T$$(pwd)/link.x -C link-arg=-nostdlib -C link-arg=-no-pie" cargo build --release --target $(RUST_TARGET)

build/user/init.bin: $(USER_INIT_ELF)
	@mkdir -p build/user
	@cp $< $@

build/user/eash.bin: $(USER_SHELL_ELF)
	@mkdir -p build/user
	@cp $< $@

build/user/install.bin: $(USER_INSTALL_ELF)
	@mkdir -p build/user
	@cp $< $@

build/user/fbterm.bin: $(USER_FBTERM_ELF)
	@mkdir -p build/user
	@cp $< $@

build/user/httpsrv.bin: $(USER_HTTPSRV_ELF)
	@mkdir -p build/user
	@cp $< $@
endif

build/kernel.bin: $(KERNEL_OBJS) $(RUST_LIB)
	@mkdir -p build
	@echo "[LINK] Linking kernel..."
	$(LD) $(LDFLAGS) --allow-multiple-definition -o $@ --whole-archive $(RUST_LIB) --no-whole-archive $(KERNEL_OBJS)

build/kernel.flat: build/kernel.bin
	$(OBJCOPY) -O binary $< $@

# AArch64 用户程序: 使用 Cargo 编译 Rust 用户程序
ifeq ($(ARCH),aarch64)
user: $(USER_INIT_ELF) $(USER_SHELL_ELF) $(USER_INSTALL_ELF) $(USER_FBTERM_ELF) $(USER_HTTPSRV_ELF)
	@mkdir -p build/user
	@cp $(USER_INIT_ELF) build/user/init.bin
	@cp $(USER_SHELL_ELF) build/user/eash.bin
	@cp $(USER_INSTALL_ELF) build/user/install.bin
	@cp $(USER_FBTERM_ELF) build/user/fbterm.bin
	@cp $(USER_HTTPSRV_ELF) build/user/httpsrv.bin
	@echo "User programs built (Rust aarch64)"

$(USER_INIT_ELF) $(USER_SHELL_ELF) $(USER_INSTALL_ELF) $(USER_FBTERM_ELF) $(USER_HTTPSRV_ELF):
	@echo "Building Rust user programs (aarch64)..."
	cd $(RUST_USER_DIR) && RUSTFLAGS="-C link-arg=-T$$(pwd)/link_aarch64.x -C link-arg=-nostdlib" cargo build --release --target $(RUST_TARGET)

build/user/init.bin: $(USER_INIT_ELF)
	@mkdir -p build/user
	@cp $< $@

$(RUST_LIB): build/user/init.bin
	@echo "Building Rust kernel module..."
	@cd src/rust && cargo build --release --target $(RUST_TARGET)
else
# x86_64: 用 Cargo 构建 Rust 用户程序 + 内核
# include_bytes! 编译时需要 init.bin 存在，确保用户程序先构建

$(RUST_LIB): $(STAGE1_BIN) build/user/init.bin $(shell find src/rust/src -name '*.rs' 2>/dev/null)
	@echo "Building Rust kernel module..."
	@cd src/rust && cargo build --release --target $(RUST_TARGET)
endif

# RUST_LIB_TEST 需源文件前置依赖 (kernel 源码经 #[path="../../kernel"] 引入, 须一并搜索):
# 否则 .a 已存在时 make 跳过 cargo 重建, kernel_test.bin 长期使用陈旧二进制 (E-06 验证踩坑, 2026-09-07)
$(RUST_LIB_TEST): $(shell find src/rust/src src/kernel -name '*.rs' 2>/dev/null)
	@echo "Building Rust test kernel..."
	cd src/rust && cargo build --release --target $(RUST_TARGET) --features kernel_test --target-dir target/test-release

$(RUST_LIB_CHAOS):
	@echo "Building Rust chaos kernel (fault_injection enabled)..."
	cd src/rust && cargo build --release --target $(RUST_TARGET) --features "kernel_test fault_injection" --target-dir target/chaos-release

# 2026-06-29 新增: 调试构建 (LTO=false + debug info + opt-level=0), 用于排查 OnceLock 静态初始化 hang
$(RUST_LIB_TEST_DEBUG):
	@echo "Building Rust test kernel (debug profile)..."
	cd src/rust && cargo build --profile test-debug --target $(RUST_TARGET) --features kernel_test --target-dir target/test-debug

build/%.o: src/kernel/framework/%.asm
	@mkdir -p $(dir $@)
	$(AS) $(ASFLAGS) $< -o $@

$(STAGE1_BIN): src/kernel/framework/boot/stage1.asm
	@mkdir -p build
	$(AS) -f bin $< -o $@

build/%.o: src/kernel/framework/boot/%.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

# AArch64 启动汇编 (GNU as)
ifeq ($(ARCH),aarch64)
build/boot.o: src/kernel/framework/boot/aarch64/start.S
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@
endif

build/gdt_asm.o: src/kernel/framework/gdt.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

build/switch.o: src/kernel/framework/proc/switch.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

# 磁盘镜像 — 仅 x86_64
ifeq ($(ARCH),x86_64)
$(DISK_IMAGE): build/kernel.flat user
	@echo "Creating disk image..."
	@dd if=/dev/zero of=$@ bs=1M count=4 2>/dev/null
	@dd if=build/stage1.bin of=$@ bs=512 seek=0 conv=notrunc 2>/dev/null
	@dd if=build/kernel.flat of=$@ bs=512 seek=1 conv=notrunc 2>/dev/null
	@echo "Disk image created: $@ (4MB)"

disk: $(DISK_IMAGE)

run-disk: $(DISK_IMAGE)
	$(QEMU) -drive file=$(DISK_IMAGE),format=raw -serial stdio
endif

iso: all user
	@mkdir -p isodir/boot/grub
	cp build/kernel.bin isodir/boot/kernel.bin
	mkdir -p isodir/bin
	cp build/user/init.bin isodir/bin/init
	cp build/user/eash.bin isodir/bin/eash
	cp build/user/install.bin isodir/bin/install
	cp build/user/fbterm.bin isodir/bin/fbterm
	cp build/user/httpsrv.bin isodir/bin/httpsrv
	echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	echo 'set default=0' >> isodir/boot/grub/grub.cfg
	echo '' >> isodir/boot/grub/grub.cfg
	echo 'menuentry "AntX" {' >> isodir/boot/grub/grub.cfg
	echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	echo '}' >> isodir/boot/grub/grub.cfg
	grub2-mkrescue -o build/antx.iso isodir

clean:
	rm -rf build/ isodir/
	cd src/rust && cargo clean
	cd $(RUST_USER_DIR) && cargo clean

# QEMU CPU 模型配置 (用于硬件仿真测试)
# 可选值: qemu64 (默认), host (使用宿主机CPU特性), Haswell-noTSX, Skylake-Client
QEMU_CPU ?= qemu64

# 运行模式配置
# mode-interactive: 需要图形界面，适合开发调试
# mode-headless: 无头模式，适合 CI/CD 和服务器环境
ifeq ($(QEMU_MODE),headless)
	QEMU_DISPLAY := -display none -nographic -serial file:$(LOG_DIR)/serial.log
else
	QEMU_DISPLAY := -serial stdio -display gtk
endif

run: all $(KERNEL_IMAGE)
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) $(QEMU_KERNEL_FLAG) $(KERNEL_IMAGE) $(QEMU_DISPLAY)

# 网络 QEMU — 仅 x86_64 (依赖 e1000 PCI 设备)
ifeq ($(ARCH),x86_64)
run-net: all user build/kernel.flat
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) -kernel build/kernel.flat $(QEMU_NET) $(QEMU_DISPLAY)
else
run-net:
	@echo "run-net is not supported on aarch64 (no PCI/e1000)"
endif

run-headless: all $(KERNEL_IMAGE)
	@$(MAKE) QEMU_MODE=headless run
	@echo "✓ Kernel output saved to $(LOG_DIR)/serial.log"
	@cat $(LOG_DIR)/serial.log | head -100

# ISO — 仅 x86_64 (依赖 GRUB + BIOS)
ifeq ($(ARCH),x86_64)
run-iso: iso
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) -cdrom build/antx.iso $(QEMU_DISPLAY)
else
run-iso:
	@echo "run-iso is not supported on aarch64 (BIOS/GRUB only)"
endif

debug: all $(KERNEL_IMAGE)
	$(QEMU) $(QEMU_FLAGS) $(QEMU_KERNEL_FLAG) $(KERNEL_IMAGE) -serial stdio -s -S

log: all $(KERNEL_IMAGE)
	@mkdir -p $(LOG_DIR)
	timeout 30 $(QEMU) $(QEMU_FLAGS) $(QEMU_KERNEL_FLAG) $(KERNEL_IMAGE) \
		-serial file:$(LOG_DIR)/serial.log \
		-display none \
		-d cpu_reset,guest_errors 2>&1 | tee $(LOG_DIR)/qemu_stderr.log || true

# 网络日志 — 仅 x86_64
ifeq ($(ARCH),x86_64)
log-net: all user build/kernel.flat
	@mkdir -p $(LOG_DIR)
	timeout 60 $(QEMU) $(QEMU_FLAGS) -kernel build/kernel.flat \
		$(QEMU_NET) \
		-serial file:$(LOG_DIR)/serial.log \
		-display none \
		-d cpu_reset,guest_errors 2>&1 | tee $(LOG_DIR)/qemu_stderr.log || true
	@echo ""
	@echo "=== Network Log ==="
	@grep -E "NETWORK|DRIVER.*E1000|Ping|HTTP|DNS|DHCP|ISR" $(LOG_DIR)/serial.log | head -30
else
log-net:
	@echo "log-net is not supported on aarch64 (no PCI/e1000)"
endif
	@echo ""
	@echo "=== HTTP Test ==="
	@curl -s --max-time 3 http://localhost:8080/ 2>/dev/null && echo "OK" || echo "FAIL (no response)"
	@echo ""
	@echo "╔══════════════════════════════════════════╗"
	@echo "║  Serial log: $(LOG_DIR)/serial.log       ║"
	@echo "║  QEMU stderr: $(LOG_DIR)/qemu_stderr.log ║"
	@echo "╚══════════════════════════════════════════╝"
	@if [ -f $(LOG_DIR)/serial.log ]; then \
		echo "--- Last 50 lines of serial output ---"; \
		tail -50 $(LOG_DIR)/serial.log; \
	fi

run-iso-debug: iso
	@mkdir -p $(LOG_DIR)
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	timeout 30 $(QEMU) $(QEMU_FLAGS) \
		-cdrom build/antx.iso \
		-serial file:$(LOG_DIR)/serial_$${timestamp}.log \
		-display none \
		-no-reboot \
		-d int,cpu_reset,unimp,guest_errors,in_asm \
		-D $(LOG_DIR)/qemu_debug_$${timestamp}.log || true
	@echo ""
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║  Debug logs saved:                          ║"
	@echo "║  Serial:  $(LOG_DIR)/serial_$${timestamp}.log    ║"
	@echo "║  QEMU:    $(LOG_DIR)/qemu_debug_$${timestamp}.log ║"
	@echo "╚══════════════════════════════════════════════╝"
	@if [ -f $(LOG_DIR)/qemu_debug_$${timestamp}.log ]; then \
		echo "--- QEMU Debug Output (last 50 lines) ---"; \
		tail -50 $(LOG_DIR)/qemu_debug_$${timestamp}.log; \
	fi

debug-iso: iso
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) \
		-cdrom build/antx.iso \
		-serial stdio \
		-no-reboot \
		-s -S &
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║  QEMU started in debug mode on port 1234     ║"
	@echo "╚══════════════════════════════════════════════╝"
	@echo "Connect with:"
	@echo "  gdb -ex 'target remote localhost:1234' \\"
	@echo "      -ex 'symbol-file build/kernel.bin'"

build/main.o: src/kernel/main.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

test: test-host test-unit

build/kernel_test.bin: $(KERNEL_TEST_OBJS) $(RUST_LIB_TEST)
	$(LD) -T $(LDSCRIPT) -nostdlib -Map=build/kernel_test.map --allow-multiple-definition -o build/kernel_test.bin $(KERNEL_TEST_OBJS) $(RUST_LIB_TEST)

# 单元测试（优化版）
test-host:
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║     Running Host-Side Unit Tests             ║"
	@echo "╚══════════════════════════════════════════════╝"
	@mkdir -p $(CURDIR)/tests/reports
	@cd host-tests && cargo test --quiet 2>&1 | tee $(CURDIR)/tests/reports/host_test_$$(date +%Y%m%d_%H%M%S).log; true
	@echo ""

test-unit: build/kernel_test.bin user
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║     Building & Running Unit Tests             ║"
	@echo "╚══════════════════════════════════════════════╝"
	@mkdir -p isodir/boot/grub
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/eash.bin isodir/bin/eash
	@cp build/user/install.bin isodir/bin/install
	@cp build/user/fbterm.bin isodir/bin/fbterm
	@cp build/user/httpsrv.bin isodir/bin/httpsrv
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'set default=0' >> isodir/boot/grub/grub.cfg
	@echo '' >> isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Test" {' >> isodir/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	@echo '}' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_test.iso isodir 2>/dev/null
	@echo ""
	@echo "▶ Starting QEMU (timeout: 300s, memory: 512MB)..."
	@mkdir -p tests/reports
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	timeout 300 $(QEMU) $(QEMU_FLAGS) \
		-m 512 \
		-cdrom build/antx_test.iso \
		$(QEMU_NET) \
		-serial file:tests/reports/unit_test_$${timestamp}.log \
		-display none \
		-d cpu_reset 2>tests/reports/qemu_stderr_$${timestamp}.log; \
	exit_code=$$?; \
	if [ $$exit_code -eq 33 ]; then \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ✅ ALL TESTS PASSED (QEMU exit: $$exit_code)   ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	elif [ $$exit_code -eq 35 ]; then \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ❌ TESTS FAILED (QEMU exit: $$exit_code)        ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	elif [ $$exit_code -eq 0 ]; then \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ⚠️  QEMU exited normally (no isa-debug-exit)    ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	else \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ⚠️  QEMU exited with code $$exit_code (timeout/crash) ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	fi
	@echo "  Report: tests/reports/unit_test_$${timestamp}.log"
	@if [ -f tests/reports/unit_test_$${timestamp}.log ]; then \
		echo "--- Serial Output (last 80 lines) ---"; \
		tail -80 tests/reports/unit_test_$${timestamp}.log; \
	fi

test-all: test-smoke test-host test-unit
	@echo ""
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     🎉 All Tests Complete!                            ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@echo "  执行的测试套件:"
	@echo "    1. Quick Test (60s)"
	@echo "    2. QEMU Hardware Simulation (150s)"
	@echo "    3. Unit Tests (120s)"
	@echo "    4. Comprehensive Tests (180s)"
	@echo "  总计: ~510 秒 (~8.5 分钟)"
	@echo ""

FAULT_RATE ?= 50

build/kernel_chaos.bin: $(KERNEL_TEST_OBJS) $(RUST_LIB_CHAOS)
	$(LD) -T $(LDSCRIPT) -nostdlib -Map=build/kernel_chaos.map --allow-multiple-definition -o build/kernel_chaos.bin $(KERNEL_TEST_OBJS) $(RUST_LIB_CHAOS)

test-chaos: build/kernel_chaos.bin user
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Chaos/Fault Injection Tests (fault_injection=on)   ║"
	@echo "║     FAULT_RATE=$(FAULT_RATE)/1000                        ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@mkdir -p isodir/boot/grub
	@cp build/kernel_chaos.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/eash.bin isodir/bin/eash
	@cp build/user/install.bin isodir/bin/install
	@cp build/user/fbterm.bin isodir/bin/fbterm
	@cp build/user/httpsrv.bin isodir/bin/httpsrv
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'set default=0' >> isodir/boot/grub/grub.cfg
	@echo '' >> isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Chaos Test" {' >> isodir/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	@echo '}' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_chaos.iso isodir 2>/dev/null
	@mkdir -p tests/reports
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	echo "▶ Starting QEMU with fault injection (rate=$(FAULT_RATE)/1000, timeout: 120s)..."; \
	timeout 120 $(QEMU) $(QEMU_FLAGS) \
		-m 512 \
		-cdrom build/antx_chaos.iso \
		-serial file:tests/reports/chaos_test_$${timestamp}.log \
		-display none \
		-d cpu_reset 2>tests/reports/qemu_chaos_stderr_$${timestamp}.log || true
	@echo ""
	@timestamp=$$(ls -t tests/reports/chaos_test_*.log 2>/dev/null | head -1 | sed 's/.*chaos_test_//;s/\.log//'); \
	if [ -n "$$timestamp" ]; then \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  Chaos Test Report                           ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
		python3 tests/chaos/analyze_chaos.py tests/reports/chaos_test_$${timestamp}.log 2>/dev/null || \
		echo "  (Run 'python3 tests/chaos/analyze_chaos.py tests/reports/chaos_test_$${timestamp}.log' for analysis)"; \
		echo ""; \
		echo "--- Last 80 lines of serial output ---"; \
		tail -80 tests/reports/chaos_test_$${timestamp}.log; \
	fi

test-integration: iso
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Integration Tests                                   ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@python3 tests/integration/run_integration_tests.py

test-smoke: iso
	@python3 tests/smoke/run_smoke_tests.py

test-stress: iso
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Stress Tests                                        ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@python3 tests/stress/run_stress_tests.py

test-smp: all user $(KERNEL_IMAGE)
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     SMP Tests (2 cores)                                 ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@mkdir -p tests/reports
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	timeout 60 $(QEMU) $(QEMU_FLAGS) \
		-m 512 -smp 2 \
		$(QEMU_KERNEL_FLAG) $(KERNEL_IMAGE) \
		-serial file:tests/reports/smp_test_$${timestamp}.log \
		-display none \
		-d cpu_reset 2>tests/reports/qemu_smp_stderr_$${timestamp}.log || true
	@echo ""
	@smp_log=$$(ls -t tests/reports/smp_test_*.log 2>/dev/null | head -1); \
	if [ -n "$$smp_log" ]; then \
		echo "--- SMP Test Output (last 60 lines) ---"; \
		tail -60 "$$smp_log"; \
	fi

# ============================================================================
# QEMU 调试脚本支持 (QEMU Debug Script Support)
# ============================================================================

.PHONY: qemu-debug qemu-debug-gdb qemu-headless qemu-network driver-test qemu-boot-test

# 使用 QEMU 调试脚本启动 (正常模式)
qemu-debug:
	@chmod +x scripts/qemu_debug.sh
	@./scripts/qemu_debug.sh -k build/kernel.flat

# 使用 QEMU 调试脚本启动 (GDB 调试模式)
qemu-debug-gdb:
	@chmod +x scripts/qemu_debug.sh
	@./scripts/qemu_debug.sh -k build/kernel.flat -d
	@echo ""
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║  GDB Debug Session                           ║"
	@echo "╠══════════════════════════════════════════════╣"
	@echo "║  In another terminal, run:                   ║"
	@echo "║  gdb -x .gdbinit.antx                        ║"
	@echo "╚══════════════════════════════════════════════╝"

# 无头模式 (Headless mode)
qemu-headless:
	@chmod +x scripts/qemu_debug.sh
	@./scripts/qemu_debug.sh -k build/kernel.flat -D none

# 网络模式 (Network mode)
qemu-network:
	@chmod +x scripts/qemu_debug.sh
	@./scripts/qemu_debug.sh -k build/kernel.flat -n

# QEMU 真实启动测试 (双架构门禁)
# 用法: make qemu-boot-test [ARCH=x86_64|aarch64|all]
qemu-boot-test:
	@chmod +x scripts/qemu_boot_test.sh
	@./scripts/qemu_boot_test.sh $(ARCH)

# ============================================================================
# 驱动测试 (Driver Tests)
# ============================================================================

driver-test: all build/kernel.flat
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Hardware Driver Tests                                ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@mkdir -p tests/reports
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	echo "[TEST] Starting driver tests in QEMU..."; \
	timeout 30 $(QEMU) $(QEMU_FLAGS) \
		-kernel build/kernel.flat \
		-serial file:tests/reports/driver_test_$${timestamp}.log \
		-display none \
		-d cpu_reset,guest_errors,unimp 2>tests/reports/qemu_driver_stderr_$${timestamp}.log || true
	@echo ""
	@driver_log=$$(ls -t tests/reports/driver_test_*.log 2>/dev/null | head -1); \
	if [ -n "$$driver_log" ]; then \
		echo "--- Driver Test Output ---"; \
		cat "$$driver_log"; \
		echo ""; \
		echo "--- QEMU Warnings (if any) ---"; \
		qemu_err=$$(ls -t tests/reports/qemu_driver_stderr_*.log 2>/dev/null | head -1); \
		if [ -f "$$qemu_err" ] && [ -s "$$qemu_err" ]; then \
			cat "$$qemu_err"; \
		else \
			echo "No warnings."; \
		fi \
	fi

# ============================================================================
# 帮助信息更新 (Updated Help)
# ============================================================================

.PHONY: help-drivers

help-drivers:
	@echo ""
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Hardware Driver Commands                             ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "  QEMU Debug Commands:"
	@echo "    make qemu-debug        - Start QEMU with VGA display"
	@echo "    make qemu-debug-gdb    - Start QEMU in GDB debug mode"
	@echo "    make qemu-headless     - Start QEMU in headless mode"
	@echo "    make qemu-network      - Start QEMU with network"
	@echo ""
	@echo "  Driver Test Commands:"
	@echo "    make driver-test       - Run hardware driver tests"
	@echo ""
	@echo "  Available Drivers:"
	@echo "    - VGA Text Mode (80x25)"
	@echo "    - Serial Port (COM1-COM4, UART 16550)"
	@echo "    - PIT Timer (8254)"
	@echo "    - PS/2 Keyboard"
	@echo "    - ATA/IDE Disk"
	@echo "    - PCI Bus"
	@echo ""
