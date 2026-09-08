//! 路由表管理 — framework 层 smoltcp 同步 + re-export
//!
//! ## T3-3 迁移记录
//!
//! 策略代码 (路由表 CRUD + CIDR 匹配 + syscall)
//! 已于 2026-06-16 迁移到 services::net::route.
//! 本文件仅保留 smoltcp 同步逻辑 (依赖 raw::stack_mut) + re-export.

use core::sync::atomic::Ordering;

use crate::kernel::framework::errno::Errno;
use crate::kernel::framework::net::NET_CONFIGURED;

// ============================================================================
// re-export services 层类型与函数
// ============================================================================

pub use crate::kernel::services::net::route::{
    MAX_ROUTES, RouteEntry, RouteQueryResult, route_add, route_del, route_list, route_query,
    sys_route_add, sys_route_del, sys_route_query,
};

// ============================================================================
// smoltcp 同步 (framework 机制, 依赖 raw::stack_mut)
// ============================================================================

#[expect(
    clippy::unnecessary_wraps,
    reason = "保留 Option/Result<()> 包装便于 API 兼容性 (调用方可能 match 或 .unwrap); 移除包装需同步修改调用点, 风险大"
)]
// J-01 (2026-09-08): manual_let_else expect 仅裸机分支 (not kernel_test) 生效 —
// kernel_test 分支为 stub (无 if-let/manual_let_else 模式), expect 在 feature 下 unfulfilled
#[cfg_attr(
    not(feature = "kernel_test"),
    expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )
)]
/// 将单条路由同步到 smoltcp Routes (双栈: V4/V6 按 family 分发)
///
/// # Errors
/// 当前实现不会返回 `Err`: 网络未配置、smoltcp 栈不可用或路由 family 不匹配时
/// 直接返回 `Ok(())` 并忽略该条目.
pub fn sync_route_to_smoltcp(entry: &RouteEntry) -> Result<(), Errno> {
    if !NET_CONFIGURED.load(Ordering::Acquire) {
        return Ok(());
    }

    #[cfg(not(feature = "kernel_test"))]
    {
        use crate::kernel::framework::net::iface_trait::IpAddr;
        use crate::kernel::framework::net::raw;
        use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address, Ipv4Cidr, Ipv6Address, Ipv6Cidr};

        let stack = match raw::stack_mut() {
            Some(s) => s,
            None => return Ok(()),
        };

        let (via_router, cidr) = match (entry.gateway, entry.dest) {
            (IpAddr::V4(gw), IpAddr::V4(dest)) => {
                let gw = Ipv4Address::new(
                    gw.octets()[0],
                    gw.octets()[1],
                    gw.octets()[2],
                    gw.octets()[3],
                );
                let cidr = Ipv4Cidr::new(
                    Ipv4Address::new(
                        dest.octets()[0],
                        dest.octets()[1],
                        dest.octets()[2],
                        dest.octets()[3],
                    ),
                    entry.prefix_len,
                );
                (IpAddress::Ipv4(gw), IpCidr::Ipv4(cidr))
            }
            (IpAddr::V6(gw), IpAddr::V6(dest)) => {
                let gw = Ipv6Address::from_octets(gw.octets());
                let cidr = Ipv6Cidr::new(Ipv6Address::from_octets(dest.octets()), entry.prefix_len);
                (IpAddress::Ipv6(gw), IpCidr::Ipv6(cidr))
            }
            // family 不匹配 (如 V4 目标 + V6 网关) — 忽略该条目
            _ => return Ok(()),
        };

        stack.iface.routes_mut().update(|routes| {
            let route = smoltcp::iface::Route {
                cidr,
                via_router,
                preferred_until: None,
                expires_at: None,
            };
            let _ = routes.push(route);
        });

        Ok(())
    }

    #[cfg(feature = "kernel_test")]
    {
        let _ = entry;
        Ok(())
    }
}

// J-01 (2026-09-08): manual_let_else expect 仅裸机分支 (not kernel_test) 生效 —
// kernel_test 分支为 stub (无 if-let/manual_let_else 模式), expect 在 feature 下 unfulfilled
#[cfg_attr(
    not(feature = "kernel_test"),
    expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )
)]
/// 从内核路由表全量重建 smoltcp Routes (双栈)
pub fn rebuild_smoltcp_routes(table: &[RouteEntry]) {
    #[cfg(not(feature = "kernel_test"))]
    {
        use crate::kernel::framework::net::iface_trait::IpAddr;
        use crate::kernel::framework::net::raw;
        use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address, Ipv4Cidr, Ipv6Address, Ipv6Cidr};

        if !NET_CONFIGURED.load(Ordering::Acquire) {
            return;
        }

        let stack = match raw::stack_mut() {
            Some(s) => s,
            None => return,
        };

        stack.iface.routes_mut().update(|routes| {
            routes.clear();
            for entry in table {
                let (via_router, cidr) = match (entry.gateway, entry.dest) {
                    (IpAddr::V4(gw), IpAddr::V4(dest)) => {
                        let gw = Ipv4Address::new(
                            gw.octets()[0],
                            gw.octets()[1],
                            gw.octets()[2],
                            gw.octets()[3],
                        );
                        let cidr = Ipv4Cidr::new(
                            Ipv4Address::new(
                                dest.octets()[0],
                                dest.octets()[1],
                                dest.octets()[2],
                                dest.octets()[3],
                            ),
                            entry.prefix_len,
                        );
                        (IpAddress::Ipv4(gw), IpCidr::Ipv4(cidr))
                    }
                    (IpAddr::V6(gw), IpAddr::V6(dest)) => {
                        let gw = Ipv6Address::from_octets(gw.octets());
                        let cidr = Ipv6Cidr::new(
                            Ipv6Address::from_octets(dest.octets()),
                            entry.prefix_len,
                        );
                        (IpAddress::Ipv6(gw), IpCidr::Ipv6(cidr))
                    }
                    _ => continue, // family 不匹配, 跳过
                };
                let route = smoltcp::iface::Route {
                    cidr,
                    via_router,
                    preferred_until: None,
                    expires_at: None,
                };
                let _ = routes.push(route);
            }
        });
    }

    #[cfg(feature = "kernel_test")]
    {
        let _ = table;
    }
}
