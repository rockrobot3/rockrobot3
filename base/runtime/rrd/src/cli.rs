//! 命令行与总线路径解析（纯逻辑，平台无关）。

use std::path::{Path, PathBuf};

use rr_ipc::{RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};

/// rrd 运行配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RrdConfig {
    pub socket: PathBuf,
    pub overlay: Option<PathBuf>,
    /// 待启动模块的 manifest（绝对路径，可重复 `--manifest`）。
    /// **已落地**（Task 11b）：`supervisor` 按 `start_priority` **升序、稳定**排后逐条 boot spawn，
    /// 同优先级保持本 CLI 顺序（`sort_by_key` 是稳定排序）。CLI 序只是"同优先级之间的 tie-break"，
    /// 不是 spawn 序本身。
    pub manifests: Vec<PathBuf>,
    /// 重启退避基数（表值 × base）。生产 1000ms；集成测试压到 20ms 才跑得完上限序列。
    /// 下界 1 与上界 `RR3_RESTART_BASE_MS_MAX` 都在 parse 处强制（见 `parse_restart_base`）。
    pub restart_base_ms: u64,
    /// 连接接受后多久没完成注册判为启动失败（legacy 教训 76c0ad8：注册前不判活）
    pub pre_register_timeout_ms: u64,
    /// 唯一 watchdog 线程的扫描间隔。生产默认 200ms：远小于任何真实
    /// `watchdog_timeout_ms`，所以 tick 误差不参与"判活/判死"的时机争议。
    /// **0 在 parse 处就拒，不 `.max(1)` 静默夹**：0 的语义是"每毫秒全表扫一遍"，
    /// 那是一条烧核的忙循环，不是"差一点点"。静默夹成 1 等于横幅与实况各说一套
    /// ——对照 `--restart-base-ms` 的上界处理：策略结论必须在启动时让运维听到。
    pub watchdog_tick_ms: u64,
    /// 注入给 boot 子进程的 `RR3_SIM_PEER_ROLE` 值（spec §5.4"由 rrd 注入"的那件环境）。
    /// **原样透传、rrd 不校验取值**（E-6）：role 的合法集是**夹具**的约定，不是守护进程的契约；
    /// rrd 一旦校验就要跟着夹具新增的脚本改，那是第二处真值。默认 `resident`。
    pub boot_role: String,
}

const USAGE: &str = "用法: rrd [--socket <path>] [--topics-overlay <path>] [--manifest <path>]... [--restart-base-ms <n>] [--pre-register-timeout-ms <n>] [--watchdog-tick-ms <n>] [--boot-role <role>]\n说明: --boot-role 的值原样注入 boot 子进程的 RR3_SIM_PEER_ROLE，取值由夹具解释，rrd 不校验（E-6）。";

/// `--restart-base-ms` 的上界：退避表末档（60）乘上去仍要不溢出 u64。
/// 为什么在 CLI 就拒、而不是让 `rr3_reg_death` 的饱和乘法默默兜住：饱和成 `u64::MAX`
/// 意思是"永不重启"，那是策略结论而不是算术意外，运维必须在启动时就听到，
/// 而不是模块崩了之后才发现它永远不会回来（`rr3_reg_death` 的饱和仍在，作为纵深防御）。
/// 分母 60 = `RR3_RESTART_BACKOFF` 的末档，由 `cli::tests::restart_base_ms_ceiling_is_tied_to_the_backoff_table`
/// 按表重算并钉住——改表不改这里会让那条用例红，不会出现两处各说一套。
pub const RR3_RESTART_BASE_MS_MAX: u64 = u64::MAX / 60;

fn need_value(flag: &str, args: &[String], i: usize) -> Result<String, String> {
    args.get(i + 1)
        .cloned()
        .ok_or_else(|| format!("{flag} 缺参数\n{USAGE}"))
}

/// 路径型开关取值：以 `-` 开头的一律当"下一个开关名"拒掉，不作值。
/// 否则 `rrd --socket --topics-overlay` 会把 `--topics-overlay` 当成套接字路径吞下，
/// 叠加层静默丢失（fail-open）。数字型开关由 `parse_ms` 拒 `-…`，口径一致。
fn need_path_value(flag: &str, args: &[String], i: usize) -> Result<String, String> {
    let raw = need_value(flag, args, i)?;
    if raw.starts_with('-') {
        return Err(format!(
            "{flag} 缺参数：收到疑似开关的 {raw}（路径值不得以 - 开头）\n{USAGE}"
        ));
    }
    Ok(raw)
}

/// 默认总线路径：env 覆盖优先（与 rr-ipc 常量一致）。
/// 环境值的读取由调用方注入（`env`），本函数**不碰进程环境**——理由见 `rr3_cli_parse_with_env`。
fn rr3_default_socket(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    PathBuf::from(env(RR3_BUS_PATH_ENV).unwrap_or_else(|| RR3_UDS_PATH_DEFAULT.to_string()))
}

/// 解析参数（生产入口：默认值从进程环境取）。
pub fn rr3_cli_parse(args: &[String]) -> Result<RrdConfig, String> {
    rr3_cli_parse_with_env(args, &|k| std::env::var(k).ok())
}

/// 解析参数，默认值来源是注入的 `env` 而不是进程环境。
///
/// 为什么要有这一层：`RrdConfig.socket` 的默认值来自 `RR3_BUS_PATH_ENV`，而进程 env 是
/// **全局可变态**。上一版的测试为了断言这条默认值，只能在用例里 `set_var`/`remove_var`，
/// 再挂一把 `OnceLock<Mutex<()>>`（ENV_LOCK）把同批用例串行化——那是把生产代码的环境依赖
/// 搬进测试来还债：锁一漏，并行时"默认值"读到的就是别人刚设的值（假绿）；edition 升 2024 后
/// `set_var` 还得写 unsafe。现在默认值走参数：测试塞一张表即可，不碰全局态、也不需要锁；
/// 生产路径（`rr3_cli_parse`）的形状对 `main.rs` 与既有调用方保持不变。
///
/// 其余门禁不变：未知选项 / 缺参数 / 值以 `-` 开头（路径型）/ 非数字一律报错，
/// 不静默回落默认值（fail-closed）。
pub fn rr3_cli_parse_with_env(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<RrdConfig, String> {
    let mut cfg = RrdConfig {
        socket: rr3_default_socket(env),
        overlay: None,
        manifests: Vec::new(),
        restart_base_ms: 1000,
        pre_register_timeout_ms: 3000,
        watchdog_tick_ms: 200,
        boot_role: "resident".to_string(),
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
                cfg.manifests
                    .push(PathBuf::from(need_path_value("--manifest", args, i)?));
                i += 2;
            }
            "--restart-base-ms" => {
                cfg.restart_base_ms =
                    parse_restart_base(&need_value("--restart-base-ms", args, i)?)?;
                i += 2;
            }
            "--pre-register-timeout-ms" => {
                cfg.pre_register_timeout_ms = parse_ms(
                    "--pre-register-timeout-ms",
                    &need_value("--pre-register-timeout-ms", args, i)?,
                )?;
                i += 2;
            }
            "--watchdog-tick-ms" => {
                let raw = need_value("--watchdog-tick-ms", args, i)?;
                let n = parse_ms("--watchdog-tick-ms", &raw)?;
                if n == 0 {
                    return Err(
                        "--watchdog-tick-ms 需为正整数毫秒（0 是忙循环，不是\"更快一点\"）"
                            .to_string(),
                    );
                }
                cfg.watchdog_tick_ms = n;
                i += 2;
            }
            "--boot-role" => {
                let raw = need_value("--boot-role", args, i)?;
                // 两道拒法都只为一个目的：**role 进了子进程才炸，就查不出是谁给的**。
                // 空值：`sim_peer` 会打"未知 RR3_SIM_PEER_ROLE: "（一个空名字的错误行，
                // 既指不出参数也指不出用例）；疑似开关的值：`--boot-role --socket` 会把
                // `--socket` 当成 role 吞下、于是**下一个真开关**丢了（与 `need_path_value`
                // 当年修的"叠加层被静默丢掉"同一形状，只是这里丢的是 socket 路径）。
                if raw.is_empty() {
                    return Err(format!(
                        "--boot-role 不得为空（role 由夹具解释，空值只会得到一条查不出来源的\"未知 RR3_SIM_PEER_ROLE\"）\n{USAGE}"
                    ));
                }
                if raw.starts_with('-') {
                    return Err(format!(
                        "--boot-role 缺参数：收到疑似开关的 {raw}（role 不得以 - 开头）\n{USAGE}"
                    ));
                }
                // **不校验取值**（E-6）：合法集是夹具的约定，rrd 只透传。
                cfg.boot_role = raw;
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
    raw.parse::<u64>()
        .map_err(|_| format!("{flag} 需为非负整数毫秒，收到 {raw}"))
}

/// 把 `--socket` 解析成绝对路径：横幅回显与 `UnixListener::bind` 必须共用同一个值。
///
/// 用 `std::path::absolute` 而不是 `topics_load` 那套 `std::fs::canonicalize`：bind 之前
/// 套接字文件**必然还不存在**，canonicalize 会以 ENOENT 拒掉一个完全合法的待创建路径。
/// 代价是这条路径不解析符号链接、也不保证唯一——对"横幅说的就是绑的地方"这个目的足够。
/// 失败（极少数畸形路径，如空字符串配空 CWD）如实报错，不回落原词：静默回落等于把
/// "运维照横幅连错地方"这条缝留在原地。
pub fn rr3_socket_absolute_path(path: &Path) -> Result<PathBuf, String> {
    std::path::absolute(path)
        .map_err(|e| format!("套接字路径 {} 无法解析为绝对路径: {e}", path.display()))
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

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// 没有 env 覆盖时的那张"空表"：默认值断言全部按它跑，不碰进程环境
    fn env_none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn defaults_come_from_contract_constants() {
        let cfg = rr3_cli_parse_with_env(&a(&[]), &env_none).unwrap();
        // M-6：原来这里还有一行 `assert_eq!(cfg.socket, rr3_default_socket(&env_none))`，
        // 两边同走一个函数、恒真（自证），删掉。真正起锚作用的是下面与契约常量的对账。
        assert_eq!(
            rr3_default_socket(&env_none),
            PathBuf::from(RR3_UDS_PATH_DEFAULT)
        );
        assert_eq!(cfg.overlay, None);
        assert!(cfg.manifests.is_empty());
        assert_eq!(
            (
                cfg.restart_base_ms,
                cfg.pre_register_timeout_ms,
                cfg.watchdog_tick_ms
            ),
            (1000, 3000, 200)
        );
    }

    /// tick 默认 200、可配、且 0 必被拒（忙循环不是"再快一点"）。
    /// 三值同钉：`restart_base_ms` / `pre_register_timeout_ms` / `watchdog_tick_ms`
    /// 都从同一个 parse 出来，漂任何一个都会让集成测试的时间口径与生产不一致。
    #[test]
    fn watchdog_tick_is_configurable_and_never_zero() {
        let ok = rr3_cli_parse_with_env(&a(&["--socket", "/tmp/x.sock"]), &env_none).unwrap();
        assert_eq!(
            ok.watchdog_tick_ms, 200,
            "默认 tick 必须与 USAGE/字段文档同口径"
        );
        let ok = rr3_cli_parse_with_env(&a(&["--watchdog-tick-ms", "20"]), &env_none).unwrap();
        assert_eq!(ok.watchdog_tick_ms, 20);
        let err = rr3_cli_parse_with_env(&a(&["--watchdog-tick-ms", "0"]), &env_none).unwrap_err();
        assert!(
            err.contains("忙循环"),
            "拒 0 的文案必须说清为什么不是夹成 1: {err}"
        );
    }

    /// `--boot-role`（E-6）：默认 `resident`、**原样透传不解释**、空值必拒。
    /// 为什么拒空值而不是让子进程去报："未知 RR3_SIM_PEER_ROLE: " 后面跟一个空名字，
    /// 那条失败查不出来源（是参数丢了？还是解析成了空串？），而 CLI 拒一次就点名到位。
    /// 为什么**不**校验取值：role 的合法集是夹具 `sim_peer` 的约定，不是守护进程的契约——
    /// rrd 校验它就要跟着夹具新增的脚本一起改（第二处真值），而且真设备模块根本不读这个变量。
    #[test]
    fn boot_role_defaults_to_resident_and_is_pass_through_verbatim() {
        let dflt = rr3_cli_parse_with_env(&a(&[]), &env_none).unwrap();
        assert_eq!(
            dflt.boot_role, "resident",
            "默认必须是 resident（与夹具不传 RR3_SIM_PEER_ROLE 时的回落一致）"
        );
        // 原样透传：不做大小写归一、不裁剪、不映射
        for role in ["abort", "go_silent", "publish_once", "SOME-UNKNOWN-role"] {
            let got = rr3_cli_parse_with_env(&a(&["--boot-role", role]), &env_none).unwrap();
            assert_eq!(
                got.boot_role, role,
                "rrd 不校验 role 取值，必须逐字进 cfg（{role} 被改写就是它在替夹具做主）"
            );
        }
        // 空值拒，且文案要点名开关与理由
        let e = rr3_cli_parse_with_env(&a(&["--boot-role", ""]), &env_none).unwrap_err();
        assert!(
            e.contains("--boot-role") && e.contains("不得为空") && e.contains("用法"),
            "报错要点名开关、理由与用法: {e}"
        );
        // 缺值走 need_value 的同一条口径（`--boot-role` 是最后一个参数）
        let e = rr3_cli_parse_with_env(&a(&["--boot-role"]), &env_none).unwrap_err();
        assert!(
            e.contains("--boot-role 缺参数") && e.contains("用法"),
            "{e}"
        );
        // 疑似开关的值不当 role 吞下（否则后面那个真开关就被吃了，与 need_path_value 同形）
        let e = rr3_cli_parse_with_env(&a(&["--boot-role", "--socket", "/tmp/x.sock"]), &env_none)
            .unwrap_err();
        assert!(
            e.contains("--boot-role 缺参数") && e.contains("--socket"),
            "必须点名被误当值的开关: {e}"
        );
    }

    /// env 覆盖必须生效（rrd 与模块用同一个 env 键找总线，spec §4）——但覆盖的是**注入的表**：
    /// 本文件里没有任何 `set_var`，进程环境从头到尾没被碰过，因此也不需要串行化锁。
    #[test]
    fn default_socket_follows_bus_env() {
        let injected =
            |k: &str| (k == RR3_BUS_PATH_ENV).then(|| "/tmp/rr3-m2-env.sock".to_string());
        let cfg = rr3_cli_parse_with_env(&a(&[]), &injected).unwrap();
        assert_eq!(cfg.socket, PathBuf::from("/tmp/rr3-m2-env.sock"));
        // 命令行显式给值时压过 env
        let overridden =
            rr3_cli_parse_with_env(&a(&["--socket", "/tmp/rr3-m2-order.sock"]), &injected).unwrap();
        assert_eq!(overridden.socket, PathBuf::from("/tmp/rr3-m2-order.sock"));
    }

    /// 默认值的来源必须是**注入的参数**，不是进程环境：本用例全程不 `set_var`，
    /// 所以它与其它用例并行时既不会污染别人也不会读到别人的值——ENV_LOCK 因此不再需要。
    #[test]
    fn default_socket_comes_from_injected_env() {
        let injected =
            |k: &str| (k == RR3_BUS_PATH_ENV).then(|| "/tmp/rr3-m2-inject.sock".to_string());
        let cfg = rr3_cli_parse_with_env(&a(&[]), &injected).unwrap();
        assert_eq!(cfg.socket, PathBuf::from("/tmp/rr3-m2-inject.sock"));
        // 表里没有这个键 → 回落契约常量（两条分支各自可达，不是一律吞注入值）
        let none = rr3_cli_parse_with_env(&a(&[]), &env_none).unwrap();
        assert_eq!(none.socket, PathBuf::from(RR3_UDS_PATH_DEFAULT));
    }

    /// 生产入口的默认值仍然来自**进程环境**（spec §4：rrd 与模块共用同一个 env 键）。
    /// 只读不写，所以这条也不依赖任何串行化锁；它钉住"委托没写反"，防止
    /// `rr3_cli_parse` 被悄悄改成"常量默认值"而没人发现。
    #[test]
    fn production_entry_delegates_to_the_process_env() {
        let from_parse = rr3_cli_parse(&a(&[])).unwrap();
        let from_reader = rr3_cli_parse_with_env(&a(&[]), &|k| std::env::var(k).ok()).unwrap();
        assert_eq!(from_parse, from_reader);
        match std::env::var(RR3_BUS_PATH_ENV) {
            Ok(v) => assert_eq!(
                from_parse.socket,
                PathBuf::from(v),
                "env 设了值就必须照它来"
            ),
            Err(_) => assert_eq!(from_parse.socket, PathBuf::from(RR3_UDS_PATH_DEFAULT)),
        }
    }

    #[test]
    fn overlay_and_repeated_manifests_keep_order() {
        let cfg = rr3_cli_parse_with_env(
            &a(&[
                "--topics-overlay",
                "/tmp/sim-topics.toml",
                "--manifest",
                "/m/b.toml",
                "--manifest",
                "/m/a.toml",
            ]),
            &env_none,
        )
        .unwrap();
        assert_eq!(cfg.overlay, Some(PathBuf::from("/tmp/sim-topics.toml")));
        assert_eq!(
            cfg.manifests,
            vec![PathBuf::from("/m/b.toml"), PathBuf::from("/m/a.toml")]
        );
    }

    /// 数字参数必须真是数字：笔误不许悄悄变成默认值
    #[test]
    fn numeric_flags_reject_garbage() {
        for args in [
            a(&["--restart-base-ms", "fast"]),
            a(&["--pre-register-timeout-ms", "-1"]),
        ] {
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
        assert!(rr3_cli_parse(&a(&["--socket", ""]))
            .expect_err("空路径必须报错")
            .contains("不得为空"));
    }

    /// 值以 `-` 开头 = 那是下一个开关，不是值。`--socket --topics-overlay` 曾经把
    /// `--topics-overlay` 当套接字路径吞下、静默丢掉叠加层（唯一的 fail-open 缝）。
    #[test]
    fn path_values_must_not_start_with_dash() {
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
        assert!(rr3_cli_parse(&a(&["--socket", "-h"]))
            .expect_err("-h 不是路径值")
            .contains("不得以 - 开头"));
        // 数字型开关的负值仍按原口径报，不是这条规则替它背锅
        let n = rr3_cli_parse(&a(&["--restart-base-ms", "-5"])).expect_err("负数毫秒必须拒");
        assert!(n.contains("需为非负整数毫秒"), "{n}");
    }

    /// `--restart-base-ms 0` 不是"没配"，是"退避恒为 0"：`RrdRegistry::rr3_reg_death`
    /// 给出 `delay_ms = 表值 × 0 = 0`，一个反复崩溃的模块会被零延迟重启到拖死整机
    /// （spec §5.3 的退避表正是为了避免这个）。下界因此是 1，不是"允许 0 当缺省"。
    #[test]
    fn restart_base_ms_floor_is_one() {
        let e = rr3_cli_parse(&a(&["--restart-base-ms", "0"]))
            .expect_err("0 必须拒：退避恒为 0 = 热重启循环");
        assert!(
            e.contains("--restart-base-ms") && e.contains("正整数") && e.contains("用法"),
            "报错要点名下界: {e}"
        );
        // 1 是下界本身，必须收：否则"拒 0"会悄悄变成"拒一切小值"
        assert_eq!(
            rr3_cli_parse(&a(&["--restart-base-ms", "1"]))
                .unwrap()
                .restart_base_ms,
            1
        );
    }

    /// 上界由退避表末档导出：末档 × base 不许溢出 u64。
    /// 溢出后 `rr3_reg_death` 的饱和乘法会给出 `u64::MAX`（= 永不重启），那是**策略结论**
    /// 而不是算术意外，必须在启动时就听到，不能等模块再也回不来才发现。
    #[test]
    fn restart_base_ms_ceiling_is_tied_to_the_backoff_table() {
        let last = *RR3_RESTART_BACKOFF.last().expect("退避表非空");
        let max_ok = u64::MAX / last;
        // 常量和表必须同源：改表（加一档、把末档改成 120）不改常量的话，溢出缝就悄悄回来了
        assert_eq!(RR3_RESTART_BASE_MS_MAX, max_ok, "上界必须由退避表末档导出");
        let ok = rr3_cli_parse(&a(&["--restart-base-ms", &max_ok.to_string()]))
            .expect("末档乘上去仍不溢出的值必须收");
        assert_eq!(ok.restart_base_ms, max_ok);
        let e = rr3_cli_parse(&a(&["--restart-base-ms", &(max_ok + 1).to_string()]))
            .expect_err("溢出必须拒，不能饱和成「永不重启」");
        assert!(
            e.contains("--restart-base-ms") && e.contains("用法"),
            "报错要点名开关: {e}"
        );
    }

    /// 待创建的套接字路径**不存在**是常态（bind 之前哪来的文件），解析必须照样成功。
    /// 这条钉住"为什么是 `std::path::absolute` 而不是 `canonicalize`"：后者会 ENOENT。
    #[test]
    fn socket_absolute_path_does_not_require_existence() {
        let raw = Path::new("rr3-m2-never-created-check.sock");
        assert!(
            !raw.exists(),
            "夹具必须真的不存在：这条测的就是「不存在也能解析」"
        );
        let abs = rr3_socket_absolute_path(raw).expect("待创建路径不该被拒");
        assert!(abs.is_absolute(), "{abs:?} 不是绝对路径");
        assert_eq!(
            abs.file_name(),
            Some(std::ffi::OsStr::new("rr3-m2-never-created-check.sock"))
        );
    }

    /// 已经是绝对路径时原样返回（幂等）：`main.rs` 只解析一次，重复解析不会改值，
    /// 所以横幅、bind、以及模块侧看到的 `RR3_BUS_PATH` 才是同一个字符串。
    #[cfg(unix)]
    #[test]
    fn absolute_socket_path_is_returned_verbatim_on_unix() {
        let raw = Path::new("/tmp/rr3-m2-absolute-check.sock");
        assert_eq!(rr3_socket_absolute_path(raw).unwrap(), raw);
    }
}
