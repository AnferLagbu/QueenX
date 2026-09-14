//! DECISION-H storage 专项: NVMe/AHCI 架构契约验证 (3 号子步 storage_init 退位后)
//!
//! 验证退位后的状态契约 (services 权威, framework 机制保留):
//! 1. framework `nvme.rs` 仅剩 wire 类型 (NvmeCommand/NvmeCompletion), 控制器业务已删
//! 2. framework `ahci.rs` 仅剩 wire 命令结构 (H2dFis/命令头/命令表), HBA 寄存器布局已删
//! 3. framework `storage_init` 仅 ATA 回退路径; PCI AHCI/NVMe 探测/注册由 services 接管
//! 4. services `storage_init` 调用 `_block` 适配器注册 Chitin + MSI-X 接线
//! 5. crate root lib.rs 编排 services storage_init (合法双向编排者)
//! 6. 双侧均无文件级 dead_code 豁免 (I-49 契约延续)
//!
//! 主机端无法实际跑 PCI 探测, 这里做静态契约验证: 读源文件做关键字检查.

use std::fs;
use std::path::Path;

const FRAMEWORK_DIR: &str = "../src/kernel/framework/driver/storage";
const SERVICES_DIR: &str = "../src/kernel/services/driver/storage";

fn read_source(dir: &str, name: &str) -> String {
    let path = Path::new(dir).join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {} failed: {}", path.display(), e))
}

/// 剥离 `//` 与 `//!` 注释行 — 静态契约只匹配真实代码, 不匹配文档图示
fn strip_comment_lines(src: &str) -> String {
    src.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ============================================================================
// framework 侧: 机制保留面 (wire 类型 + ATA 回退)
// ============================================================================

#[test]
fn test_framework_nvme_wire_types_only() {
    let src = read_source(FRAMEWORK_DIR, "nvme.rs");
    // wire 类型必须保留 (framework safe wrapper 与 services 驱动共用)
    for sym in ["pub struct NvmeCommand", "pub struct NvmeCompletion"] {
        assert!(src.contains(sym), "framework nvme.rs 缺失 {}", sym);
    }
    // 控制器业务必须已退位 (DECISION-H 3 号子步)
    assert!(
        !src.contains("pub struct NvmeController"),
        "framework nvme.rs 仍含 NvmeController (应已迁 services)"
    );
    assert!(
        !src.contains("#![allow(dead_code)]"),
        "framework nvme.rs 不应有文件级 dead_code 豁免"
    );
}

#[test]
fn test_framework_ahci_wire_types_only() {
    let src = read_source(FRAMEWORK_DIR, "ahci.rs");
    // wire 命令结构必须保留 (framework mod.rs 填充原语使用)
    for sym in [
        "pub struct AhciCommandHeader",
        "pub struct AhciCommandTable",
        "pub struct H2dFis",
    ] {
        assert!(src.contains(sym), "framework ahci.rs 缺失 {}", sym);
    }
    // HBA 寄存器布局与控制器业务必须已退位
    for sym in ["pub struct AhciHbaGhc", "pub struct AhciPort", "pub struct AhciController"] {
        assert!(
            !src.contains(sym),
            "framework ahci.rs 仍含 {} (应已迁 services)",
            sym
        );
    }
    assert!(
        !src.contains("#![allow(dead_code)]"),
        "framework ahci.rs 不应有文件级 dead_code 豁免"
    );
}

#[test]
fn test_framework_storage_init_ata_fallback_only() {
    let code = strip_comment_lines(&read_source(FRAMEWORK_DIR, "mod.rs"));
    // ATA 回退路径保留
    assert!(
        code.contains("ata_init") && code.contains("register_block_device"),
        "framework storage_init 应保留 ATA 检测与注册"
    );
    // PCI AHCI/NVMe 探测业务必须已退位
    assert!(
        !code.contains("scan_all_buses"),
        "framework storage_init 仍做 PCI 扫描 (应已迁 services)"
    );
    assert!(
        !code.contains("AhciController::new") && !code.contains("NvmeController::new"),
        "framework storage_init 仍初始化控制器 (应已迁 services)"
    );
    // MSI-X ISR 编排机制保留 (注册契约槽 + ISR 注册入口)
    for sym in [
        "nvme_register_msix_isr",
        "nvme_register_services_msix_dispatch",
    ] {
        assert!(code.contains(sym), "framework mod.rs 缺失机制入口 {}", sym);
    }
}

// ============================================================================
// services 侧: 权威实现 (控制器业务 + 注册路径 + MSI-X 接线)
// ============================================================================

#[test]
fn test_services_controllers_present() {
    let ahci = read_source(SERVICES_DIR, "ahci.rs");
    assert!(ahci.contains("pub struct AhciController"), "services AhciController 缺失");
    assert!(ahci.contains("pub struct AhciPort"), "services AhciPort 缺失");
    assert!(
        ahci.contains("#![deny(unsafe_code)]"),
        "services ahci.rs 必须 0 unsafe"
    );
    let nvme = read_source(SERVICES_DIR, "nvme.rs");
    assert!(nvme.contains("pub struct NvmeController"), "services NvmeController 缺失");
    assert!(
        nvme.contains("#![deny(unsafe_code)]"),
        "services nvme.rs 必须 0 unsafe"
    );
}

#[test]
fn test_services_storage_init_uses_block_devices() {
    // 验证 services 启动路径实际调用了 block 设备注册 (非死代码)
    let src = read_source(SERVICES_DIR, "mod.rs");
    assert!(
        src.contains("AhciBlockDevice::new"),
        "services storage_init 未调用 AhciBlockDevice::new"
    );
    assert!(
        src.contains("NvmeBlockDevice::new"),
        "services storage_init 未调用 NvmeBlockDevice::new"
    );
    assert!(
        src.contains("register_block_device"),
        "services storage_init 未注册 block 设备到 Chitin"
    );
    // MSI-X 接线 (DECISION-H 2 号子步): 启用 + ISR 注册 + services 分发契约注册
    assert!(
        src.contains("enable_msix") && src.contains("nvme_register_msix_isr"),
        "services storage_init 未接线 NVMe MSI-X"
    );
    assert!(
        src.contains("nvme_register_services_msix_dispatch"),
        "services storage_init 未注册 services MSI-X 分发契约"
    );
}

#[test]
fn test_lib_rs_orchestrates_services_storage_init() {
    // crate root (合法双向编排者) 必须调用 services storage_init (x86_64 门控)
    let src =
        fs::read_to_string("../src/rust/src/lib.rs").expect("read lib.rs failed");
    assert!(
        src.contains("services::driver::storage::storage_init"),
        "crate root lib.rs 未编排 services storage_init"
    );
}

// ============================================================================
// 双侧公共契约: 无 dead_code 豁免 (I-49 延续)
// ============================================================================

#[test]
fn test_no_dead_code_allow_in_storage() {
    for (dir, name) in [
        (FRAMEWORK_DIR, "mod.rs"),
        (FRAMEWORK_DIR, "nvme.rs"),
        (FRAMEWORK_DIR, "ahci.rs"),
        (SERVICES_DIR, "mod.rs"),
        (SERVICES_DIR, "nvme.rs"),
        (SERVICES_DIR, "ahci.rs"),
    ] {
        let src = read_source(dir, name);
        assert!(
            !src.contains("#![allow(dead_code)]") && !src.contains("#![allow(unused)]"),
            "{}/{} 含文件级 dead_code 豁免",
            dir,
            name
        );
    }
}
