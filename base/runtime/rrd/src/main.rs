//! rrd —— base 层守护进程骨架（architecture.md §6）。
//!
//! v0.0.1 只做"握手信息打印"：证明 workspace 六个 crate 能互相引用。
//! 模块注册、manifest 校验放行、心跳广播随 M2 的 UDS 传输一起实现。

use rr_ipc::{RR3_BUS_PATH_ENV, RR3_CONTRACT_SCHEMA_VERSION, RR3_UDS_PATH_DEFAULT};
use rr_registry::RR3_MANIFEST_SCHEMA_VERSION;

fn rr3_banner(contract: u32, manifest: u32, bus: &str) -> String {
    format!("rrd 0.0.1 (contracts schema v{contract}, manifest schema v{manifest}, bus {bus})")
}

fn rr3_bus_path() -> String {
    std::env::var(RR3_BUS_PATH_ENV).unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string())
}

fn main() {
    println!(
        "{}",
        rr3_banner(
            RR3_CONTRACT_SCHEMA_VERSION,
            RR3_MANIFEST_SCHEMA_VERSION,
            &rr3_bus_path()
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrd_banner_names_both_schemas() {
        let s = rr3_banner(1, 1, "/var/run/rock3-ipc.sock");
        assert!(s.contains("contracts schema v1"), "{s}");
        assert!(s.contains("manifest schema v1"), "{s}");
        assert!(s.contains("rock3-ipc.sock"), "{s}");
    }

    #[test]
    fn rrd_bus_path_defaults_and_overrides() {
        // 无环境变量时取默认路径；设了则覆盖（测试串行执行，用完即清）
        std::env::remove_var(RR3_BUS_PATH_ENV);
        assert_eq!(rr3_bus_path(), RR3_UDS_PATH_DEFAULT);
        std::env::set_var(RR3_BUS_PATH_ENV, "/tmp/rr3-test.sock");
        assert_eq!(rr3_bus_path(), "/tmp/rr3-test.sock");
        std::env::remove_var(RR3_BUS_PATH_ENV);
    }
}
