//! rrd —— base 层守护进程骨架（architecture.md §6）。
//!
//! v0.0.1 只做"握手信息打印"：证明 workspace 六个 crate 能互相引用。
//! 模块注册、manifest 校验放行、心跳广播随 M2 的 UDS 传输一起实现。

use rr_ipc::{RR3_BUS_PATH_ENV, RR3_CONTRACT_SCHEMA_VERSION, RR3_UDS_PATH_DEFAULT};
use rr_registry::RR3_MANIFEST_SCHEMA_VERSION;

fn banner(contract: u32, manifest: u32, bus: &str) -> String {
    format!("rrd 0.0.1 (contracts schema v{contract}, manifest schema v{manifest}, bus {bus})")
}

fn bus_path() -> String {
    std::env::var(RR3_BUS_PATH_ENV).unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string())
}

fn main() {
    println!(
        "{}",
        banner(
            RR3_CONTRACT_SCHEMA_VERSION,
            RR3_MANIFEST_SCHEMA_VERSION,
            &bus_path()
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    /// 进程 env 是全局可变态，且同二进制内的测试默认多线程并行执行。
    /// 不变式：本二进制内改动/读取 `RR3_BUS_PATH_ENV` 的只有下面这一个测试；
    /// 若未来新增触碰 env 的测试，必须先取得此锁再读写。
    /// （edition 2021：`set_var`/`remove_var` 安全；升 edition 2024 时需改 unsafe 块并保留该锁。）
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn rrd_banner_names_both_schemas() {
        let s = banner(1, 1, "/var/run/rock3-ipc.sock");
        assert!(s.contains("contracts schema v1"), "{s}");
        assert!(s.contains("manifest schema v1"), "{s}");
        assert!(s.contains("rock3-ipc.sock"), "{s}");
    }

    #[test]
    fn rrd_bus_path_defaults_and_overrides() {
        let _guard = env_guard();
        // 无环境变量时取默认路径；设了则覆盖；结束恢复原值（不依赖测试执行顺序）
        let saved = std::env::var(RR3_BUS_PATH_ENV).ok();
        std::env::remove_var(RR3_BUS_PATH_ENV);
        assert_eq!(bus_path(), RR3_UDS_PATH_DEFAULT);
        std::env::set_var(RR3_BUS_PATH_ENV, "/tmp/rr3-test.sock");
        assert_eq!(bus_path(), "/tmp/rr3-test.sock");
        match saved {
            Some(v) => std::env::set_var(RR3_BUS_PATH_ENV, v),
            None => std::env::remove_var(RR3_BUS_PATH_ENV),
        }
    }
}
