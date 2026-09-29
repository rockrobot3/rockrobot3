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
    /// 重启退避基数（表值 × base）。生产 1000ms；集成测试压到 20ms 才跑得完上限序列。
    /// 下界 1 与上界 `RR3_RESTART_BASE_MS_MAX` 都在 parse 处强制（见 `parse_restart_base`）。
    pub restart_base_ms: u64,
    /// 连接接受后多久没完成注册判为启动失败（legacy 教训 76c0ad8：注册前不判活）
    pub pre_register_timeout_ms: u64,
}

const USAGE: &str = "用法: rrd [--socket <path>] [--topics-overlay <path>] [--manifest <path>]... [--restart-base-ms <n>] [--pre-register-timeout-ms <n>]";

/// `--restart-base-ms` 的上界：退避表末档（60）乘上去仍要不溢出 u64。
/// 为什么在 CLI 就拒、而不是让 `rr3_reg_death` 的饱和乘法默默兜住：饱和成 `u64::MAX`
/// 意思是"永不重启"，那是策略结论而不是算术意外，运维必须在启动时就听到，
/// 而不是模块崩了之后才发现它永远不会回来（`rr3_reg_death` 的饱和仍在，作为纵深防御）。
/// 分母 60 = `RR3_RESTART_BACKOFF` 的末档，由 `cli::tests::restart_base_ms_ceiling_is_tied_to_the_backoff_table`
/// 按表重算并钉住——改表不改这里会让那条用例红，不会出现两处各说一套。
pub const RR3_RESTART_BASE_MS_MAX: u64 = u64::MAX / 60;

fn need_value(flag: &str, args: &[String], i: usize) -> Result<String, String> {
    args.get(i + 1).cloned().ok_or_else(|| format!("{flag} 缺参数\n{USAGE}"))
}

/// 路径型开关取值：以 `-` 开头的一律当"下一个开关名"拒掉，不作值。
/// 否则 `rrd --socket --topics-overlay` 会把 `--topics-overlay` 当成套接字路径吞下，
/// 叠加层静默丢失（fail-open）。数字型开关由 `parse_ms` 拒 `-…`，口径一致。
fn need_path_value(flag: &str, args: &[String], i: usize) -> Result<String, String> {
    let raw = need_value(flag, args, i)?;
    if raw.starts_with('-') {
        return Err(format!("{flag} 缺参数：收到疑似开关的 {raw}（路径值不得以 - 开头）\n{USAGE}"));
    }
    Ok(raw)
}

/// 默认总线路径：env 覆盖优先（与 rr-ipc 常量一致）
fn rr3_cli_default_socket() -> PathBuf {
    PathBuf::from(std::env::var(RR3_BUS_PATH_ENV).unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string()))
}

/// 解析参数。未知选项 / 缺参数 / 值以 `-` 开头（路径型）/ 非数字一律报错，
/// 不静默回落默认值（fail-closed）。
///
/// 数值门禁：`--restart-base-ms` 必须 ∈ `1..=RR3_RESTART_BASE_MS_MAX`（见 `parse_restart_base`），
/// `--pre-register-timeout-ms` 仍只要求非负整数。
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
                cfg.socket = PathBuf::from(need_path_value("--socket", args, i)?);
                i += 2;
            }
            "--topics-overlay" => {
                cfg.overlay = Some(PathBuf::from(need_path_value("--topics-overlay", args, i)?));
                i += 2;
            }
            "--manifest" => {
                cfg.manifests.push(PathBuf::from(need_path_value("--manifest", args, i)?));
                i += 2;
            }
            "--restart-base-ms" => {
                cfg.restart_base_ms =
                    parse_restart_base(&need_value("--restart-base-ms", args, i)?)?;
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

/// `--restart-base-ms` 专用门禁：`0` 与"末档乘溢出"都在 parse 处拒掉，不落默认值。
///
/// 不复用 `parse_ms`：那条口径是"非负整数"（`--pre-register-timeout-ms 0` 合法，conn 层
/// 按 `max(1)` 用），而退避基数为 0 是**有语义的坏值**——它不会让 rrd 报错，只会让
/// `always` 策略的模块以零延迟无限重启。上界同理：过了界 `delay_ms` 饱和成 `u64::MAX`，
/// 表现是"模块再也不回来"，两者都必须在启动那一刻就听见。
fn parse_restart_base(raw: &str) -> Result<u64, String> {
    const FLAG: &str = "--restart-base-ms";
    let v = parse_ms(FLAG, raw)?;
    if v == 0 {
        return Err(format!(
            "{FLAG} 需为正整数毫秒：0 会让重启退避恒为 0（热重启循环，spec §5.3）\n{USAGE}"
        ));
    }
    if v > RR3_RESTART_BASE_MS_MAX {
        return Err(format!(
            "{FLAG} {v} 过大：退避末档乘上它会溢出 u64（饱和 = 永不重启），最大 {RR3_RESTART_BASE_MS_MAX}\n{USAGE}"
        ));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RR3_RESTART_BACKOFF;
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
        let _guard = env_guard(); // rr3_cli_parse 读 env 取默认值，见上面 ENV_LOCK 注释
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
        let _guard = env_guard();
        for args in [a(&["--restart-base-ms", "fast"]), a(&["--pre-register-timeout-ms", "-1"])] {
            let e = rr3_cli_parse(&args).expect_err("非法数字必须报错");
            assert!(e.contains("需为非负整数毫秒"), "{e}");
        }
    }

    #[test]
    fn missing_value_unknown_option_and_empty_socket_are_errors() {
        let _guard = env_guard();
        let e = rr3_cli_parse(&a(&["--socket"])).expect_err("缺参数必须报错");
        assert!(e.contains("--socket 缺参数") && e.contains("用法"), "{e}");
        assert!(rr3_cli_parse(&a(&["--shard", "/tmp/x"]))
            .expect_err("未知选项必须报错")
            .contains("未知选项 --shard"));
        assert!(rr3_cli_parse(&a(&["--socket", ""])).expect_err("空路径必须报错").contains("不得为空"));
    }

    /// 值以 `-` 开头 = 那是下一个开关，不是值。`--socket --topics-overlay` 曾经把
    /// `--topics-overlay` 当套接字路径吞下、静默丢掉叠加层（唯一的 fail-open 缝）。
    #[test]
    fn path_values_must_not_start_with_dash() {
        let _guard = env_guard();
        for (flag, next) in [
            ("--socket", "--topics-overlay"),
            ("--topics-overlay", "--manifest"),
            ("--manifest", "--socket"),
        ] {
            let e = rr3_cli_parse(&a(&[flag, next])).expect_err("路径型开关不能把开关名当值吞下");
            assert!(
                e.contains(&format!("{flag} 缺参数")) && e.contains(next) && e.contains("用法"),
                "报错必须点名开关与值: {e}"
            );
        }
        // 单横杠同样拦（`-h` 不是路径）
        assert!(rr3_cli_parse(&a(&["--socket", "-h"])).expect_err("-h 不是路径值").contains("不得以 - 开头"));
        // 数字型开关的负值仍按原口径报，不是这条规则替它背锅
        let n = rr3_cli_parse(&a(&["--restart-base-ms", "-5"])).expect_err("负数毫秒必须拒");
        assert!(n.contains("需为非负整数毫秒"), "{n}");
    }

    /// `--restart-base-ms 0` 不是"没配"，是"退避恒为 0"：`RrdRegistry::rr3_reg_death`
    /// 给出 `delay_ms = 表值 × 0 = 0`，一个反复崩溃的模块会被零延迟重启到拖死整机
    /// （spec §5.3 的退避表正是为了避免这个）。下界因此是 1，不是"允许 0 当缺省"。
    #[test]
    fn restart_base_ms_floor_is_one() {
        let _guard = env_guard();
        let e = rr3_cli_parse(&a(&["--restart-base-ms", "0"])).expect_err("0 必须拒：退避恒为 0 = 热重启循环");
        assert!(e.contains("--restart-base-ms") && e.contains("正整数") && e.contains("用法"), "报错要点名下界: {e}");
        // 1 是下界本身，必须收：否则"拒 0"会悄悄变成"拒一切小值"
        assert_eq!(rr3_cli_parse(&a(&["--restart-base-ms", "1"])).unwrap().restart_base_ms, 1);
    }

    /// 上界由退避表末档导出：末档 × base 不许溢出 u64。
    /// 溢出后 `rr3_reg_death` 的饱和乘法会给出 `u64::MAX`（= 永不重启），那是**策略结论**
    /// 而不是算术意外，必须在启动时就听到，不能等模块再也回不来才发现。
    #[test]
    fn restart_base_ms_ceiling_is_tied_to_the_backoff_table() {
        let _guard = env_guard();
        let last = *RR3_RESTART_BACKOFF.last().expect("退避表非空");
        let max_ok = u64::MAX / last;
        // 常量和表必须同源：改表（加一档、把末档改成 120）不改常量的话，溢出缝就悄悄回来了
        assert_eq!(RR3_RESTART_BASE_MS_MAX, max_ok, "上界必须由退避表末档导出");
        let ok = rr3_cli_parse(&a(&["--restart-base-ms", &max_ok.to_string()]))
            .expect("末档乘上去仍不溢出的值必须收");
        assert_eq!(ok.restart_base_ms, max_ok);
        let e = rr3_cli_parse(&a(&["--restart-base-ms", &(max_ok + 1).to_string()]))
            .expect_err("溢出必须拒，不能饱和成「永不重启」");
        assert!(e.contains("--restart-base-ms") && e.contains("用法"), "报错要点名开关: {e}");
    }
}
