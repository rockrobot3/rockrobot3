//! 命令行与总线路径解析（纯逻辑，平台无关）。

use std::path::PathBuf;

use rr_ipc::{RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};

/// rrd 运行配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RrdConfig {
    pub socket: PathBuf,
    pub overlay: Option<PathBuf>,
    /// 待启动模块的 manifest（绝对路径，可重复 `--manifest`；实际 spawn 序按 start_priority 重排）
    pub manifests: Vec<PathBuf>,
    /// 重启退避基数（表值 × base）。生产 1000ms；集成测试压到 20ms 才跑得完上限序列
    pub restart_base_ms: u64,
    /// 连接接受后多久没完成注册判为启动失败（legacy 教训 76c0ad8：注册前不判活）
    pub pre_register_timeout_ms: u64,
}

const USAGE: &str = "用法: rrd [--socket <path>] [--topics-overlay <path>] [--manifest <path>]... [--restart-base-ms <n>] [--pre-register-timeout-ms <n>]";

fn need_value(flag: &str, args: &[String], i: usize) -> Result<String, String> {
    args.get(i + 1).cloned().ok_or_else(|| format!("{flag} 缺参数\n{USAGE}"))
}

/// 默认总线路径：env 覆盖优先（与 rr-ipc 常量一致）
fn rr3_cli_default_socket() -> PathBuf {
    PathBuf::from(std::env::var(RR3_BUS_PATH_ENV).unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string()))
}

/// 解析参数。未知选项 / 缺参数 / 非数字一律报错，不静默回落默认值（fail-closed）。
pub fn rr3_cli_parse(args: &[String]) -> Result<RrdConfig, String> {
    let mut cfg = RrdConfig {
        socket: rr3_cli_default_socket(),
        overlay: None,
        manifests: Vec::new(),
        restart_base_ms: 1000,
        pre_register_timeout_ms: 3000,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" => {
                cfg.socket = PathBuf::from(need_value("--socket", args, i)?);
                i += 2;
            }
            "--topics-overlay" => {
                cfg.overlay = Some(PathBuf::from(need_value("--topics-overlay", args, i)?));
                i += 2;
            }
            "--manifest" => {
                cfg.manifests.push(PathBuf::from(need_value("--manifest", args, i)?));
                i += 2;
            }
            "--restart-base-ms" => {
                cfg.restart_base_ms = parse_ms("--restart-base-ms", &need_value("--restart-base-ms", args, i)?)?;
                i += 2;
            }
            "--pre-register-timeout-ms" => {
                cfg.pre_register_timeout_ms =
                    parse_ms("--pre-register-timeout-ms", &need_value("--pre-register-timeout-ms", args, i)?)?;
                i += 2;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                i += 1;
            }
            other => return Err(format!("未知选项 {other}\n{USAGE}")),
        }
    }
    if cfg.socket.as_os_str().is_empty() {
        return Err("--socket 不得为空".to_string());
    }
    Ok(cfg)
}

fn parse_ms(flag: &str, raw: &str) -> Result<u64, String> {
    raw.parse::<u64>().map_err(|_| format!("{flag} 需为非负整数毫秒，收到 {raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// 进程 env 是全局可变态，同二进制内的测试默认多线程并行。
    /// 不变式：本 lib 内改动/读取 `RR3_BUS_PATH_ENV` 的测试必须先取得此锁
    /// （沿用 rr-ipc/rrd 旧 main.rs 已确立的纪律，否则并行时"默认值"断言会读到别人设的值）。
    /// （edition 2021：`set_var`/`remove_var` 安全；升 edition 2024 时需改 unsafe 块并保留该锁。）
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn defaults_come_from_contract_constants() {
        let _guard = env_guard();
        let saved = std::env::var(RR3_BUS_PATH_ENV).ok();
        std::env::remove_var(RR3_BUS_PATH_ENV);
        let cfg = rr3_cli_parse(&a(&[])).unwrap();
        assert_eq!(cfg.socket, rr3_cli_default_socket());
        assert_eq!(rr3_cli_default_socket(), PathBuf::from(RR3_UDS_PATH_DEFAULT));
        assert_eq!(cfg.overlay, None);
        assert!(cfg.manifests.is_empty());
        assert_eq!((cfg.restart_base_ms, cfg.pre_register_timeout_ms), (1000, 3000));
        match saved {
            Some(v) => std::env::set_var(RR3_BUS_PATH_ENV, v),
            None => std::env::remove_var(RR3_BUS_PATH_ENV),
        }
    }

    /// env 覆盖必须生效（rrd 与模块用同一个 env 键找总线，spec §4）
    #[test]
    fn default_socket_follows_bus_env() {
        let _guard = env_guard();
        let saved = std::env::var(RR3_BUS_PATH_ENV).ok();
        std::env::set_var(RR3_BUS_PATH_ENV, "/tmp/rr3-m2-env.sock");
        assert_eq!(rr3_cli_default_socket(), PathBuf::from("/tmp/rr3-m2-env.sock"));
        match saved {
            Some(v) => std::env::set_var(RR3_BUS_PATH_ENV, v),
            None => std::env::remove_var(RR3_BUS_PATH_ENV),
        }
    }

    #[test]
    fn overlay_and_repeated_manifests_keep_order() {
        let cfg = rr3_cli_parse(&a(&[
            "--topics-overlay",
            "/tmp/sim-topics.toml",
            "--manifest",
            "/m/b.toml",
            "--manifest",
            "/m/a.toml",
        ]))
        .unwrap();
        assert_eq!(cfg.overlay, Some(PathBuf::from("/tmp/sim-topics.toml")));
        assert_eq!(cfg.manifests, vec![PathBuf::from("/m/b.toml"), PathBuf::from("/m/a.toml")]);
    }

    /// 数字参数必须真是数字：笔误不许悄悄变成默认值
    #[test]
    fn numeric_flags_reject_garbage() {
        for args in [a(&["--restart-base-ms", "fast"]), a(&["--pre-register-timeout-ms", "-1"])] {
            let e = rr3_cli_parse(&args).expect_err("非法数字必须报错");
            assert!(e.contains("需为非负整数毫秒"), "{e}");
        }
    }

    #[test]
    fn missing_value_unknown_option_and_empty_socket_are_errors() {
        let e = rr3_cli_parse(&a(&["--socket"])).expect_err("缺参数必须报错");
        assert!(e.contains("--socket 缺参数") && e.contains("用法"), "{e}");
        assert!(rr3_cli_parse(&a(&["--shard", "/tmp/x"]))
            .expect_err("未知选项必须报错")
            .contains("未知选项 --shard"));
        assert!(rr3_cli_parse(&a(&["--socket", ""])).expect_err("空路径必须报错").contains("不得为空"));
    }
}
