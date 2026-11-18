//! cli 的单测（自同名单测模块原样抽出，判据不变）。

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
    // 装配面必选门（基-23）后"纯缺省"不再可解析：本用例只关心与装配面无关的
    // 默认值，走 --profile 正门满足必选门（manifests 缺省空断言由它保住）
    let cfg =
        rr3_cli_parse_env(&a(&["--profile", "/root/profiles/default.toml"]), &env_none).unwrap();
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
    let ok = rr3_cli_parse_env(
        &a(&["--manifest", "/m/d.toml", "--socket", "/tmp/x.sock"]),
        &env_none,
    )
    .unwrap();
    assert_eq!(
        ok.watchdog_tick_ms, 200,
        "默认 tick 必须与 USAGE/字段文档同口径"
    );
    let ok = rr3_cli_parse_env(
        &a(&["--manifest", "/m/d.toml", "--watchdog-tick-ms", "20"]),
        &env_none,
    )
    .unwrap();
    assert_eq!(ok.watchdog_tick_ms, 20);
    let err = rr3_cli_parse_env(&a(&["--watchdog-tick-ms", "0"]), &env_none).unwrap_err();
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
    // 各分支只关心 boot_role 的缺省与透传，统一带 --manifest 满足装配面必选门
    let dflt = rr3_cli_parse_env(&a(&["--manifest", "/m/d.toml"]), &env_none).unwrap();
    assert_eq!(
        dflt.boot_role, "resident",
        "默认必须是 resident（与夹具不传 RR3_SIM_PEER_ROLE 时的回落一致）"
    );
    // 原样透传：不做大小写归一、不裁剪、不映射
    for role in ["abort", "go_silent", "publish_once", "SOME-UNKNOWN-role"] {
        let got = rr3_cli_parse_env(
            &a(&["--manifest", "/m/d.toml", "--boot-role", role]),
            &env_none,
        )
        .unwrap();
        assert_eq!(
            got.boot_role, role,
            "rrd 不校验 role 取值，必须逐字进 cfg（{role} 被改写就是它在替夹具做主）"
        );
    }
    // 空值拒，且文案要点名开关与理由
    let e = rr3_cli_parse_env(
        &a(&["--manifest", "/m/d.toml", "--boot-role", ""]),
        &env_none,
    )
    .unwrap_err();
    assert!(
        e.contains("--boot-role") && e.contains("不得为空") && e.contains("用法"),
        "报错要点名开关、理由与用法: {e}"
    );
    // 缺值走 need_value 的同一条口径（`--boot-role` 是最后一个参数）
    let e =
        rr3_cli_parse_env(&a(&["--manifest", "/m/d.toml", "--boot-role"]), &env_none).unwrap_err();
    assert!(
        e.contains("--boot-role 缺参数") && e.contains("用法"),
        "{e}"
    );
    // 疑似开关的值不当 role 吞下（否则后面那个真开关就被吃了，与 need_path_value 同形）
    let e = rr3_cli_parse_env(
        &a(&[
            "--manifest",
            "/m/d.toml",
            "--boot-role",
            "--socket",
            "/tmp/x.sock",
        ]),
        &env_none,
    )
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
    let injected = |k: &str| (k == RR3_BUS_PATH_ENV).then(|| "/tmp/rr3-m2-env.sock".to_string());
    let cfg = rr3_cli_parse_env(&a(&["--manifest", "/m/d.toml"]), &injected).unwrap();
    assert_eq!(cfg.socket, PathBuf::from("/tmp/rr3-m2-env.sock"));
    // 命令行显式给值时压过 env
    let overridden = rr3_cli_parse_env(
        &a(&[
            "--manifest",
            "/m/d.toml",
            "--socket",
            "/tmp/rr3-m2-order.sock",
        ]),
        &injected,
    )
    .unwrap();
    assert_eq!(overridden.socket, PathBuf::from("/tmp/rr3-m2-order.sock"));
}

/// 默认值的来源必须是**注入的参数**，不是进程环境：本用例全程不 `set_var`，
/// 所以它与其它用例并行时既不会污染别人也不会读到别人的值——ENV_LOCK 因此不再需要。
#[test]
fn default_socket_comes_from_injected_env() {
    let injected = |k: &str| (k == RR3_BUS_PATH_ENV).then(|| "/tmp/rr3-m2-inject.sock".to_string());
    let cfg = rr3_cli_parse_env(&a(&["--manifest", "/m/d.toml"]), &injected).unwrap();
    assert_eq!(cfg.socket, PathBuf::from("/tmp/rr3-m2-inject.sock"));
    // 表里没有这个键 → 回落契约常量（两条分支各自可达，不是一律吞注入值）
    let none = rr3_cli_parse_env(&a(&["--manifest", "/m/d.toml"]), &env_none).unwrap();
    assert_eq!(none.socket, PathBuf::from(RR3_UDS_PATH_DEFAULT));
}

/// 生产入口的默认值仍然来自**进程环境**（spec §4：rrd 与模块共用同一个 env 键）。
/// 只读不写，所以这条也不依赖任何串行化锁；它钉住"委托没写反"，防止
/// `rr3_cli_parse` 被悄悄改成"常量默认值"而没人发现。
#[test]
fn production_entry_delegates_to_the_process_env() {
    let from_parse = rr3_cli_parse(&a(&["--manifest", "/m/d.toml"])).unwrap();
    let from_reader =
        rr3_cli_parse_env(&a(&["--manifest", "/m/d.toml"]), &|k| std::env::var(k).ok()).unwrap();
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
    let cfg = rr3_cli_parse_env(
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

/// `--profile`（0.0.2 Q3）：显式值原样携带、与叠加层可并存、路径口径与其它路径开关一致。
/// "缺省 None 即后门路径"的旧读法随装配面必选门（基-23）作废：None 只有在
/// manifests 非空（--manifest 后门）时才合法，纯 None 由双缺拒用例钉住。
#[test]
fn profile_is_the_front_door_and_coexists_with_overlay() {
    let cfg = rr3_cli_parse_env(
        &a(&[
            "--topics-overlay",
            "/tmp/sim-topics.toml",
            "--profile",
            "/root/profiles/default.toml",
        ]),
        &env_none,
    )
    .unwrap();
    assert_eq!(
        cfg.profile,
        Some(PathBuf::from("/root/profiles/default.toml"))
    );
    // 缺值 / 疑似开关的值：路径开关共用同一条口径
    let e = rr3_cli_parse(&a(&["--profile"])).expect_err("缺参数必须报错");
    assert!(e.contains("--profile 缺参数") && e.contains("用法"), "{e}");
    let e =
        rr3_cli_parse(&a(&["--profile", "--socket", "/tmp/x.sock"])).expect_err("开关名不当值吞下");
    assert!(
        e.contains("--profile 缺参数") && e.contains("--socket"),
        "{e}"
    );
}

/// 装配面必选门（基-23，L-31 的出口）：`--profile` 与 `--manifest` 都不给即拒——
/// 否则装载门整体被跳过（daemon None 臂空 hal_bindings 空降级集，十条校验一个
/// 判定位都不跑而 rrd 退出码 0）。拒在 parse 处非 0 退出；文案要点名两个开关、
/// 为什么（装载门整体不跑）与用法。
#[test]
fn assembly_surface_is_mandatory_neither_flag_is_rejected() {
    for args in [
        a(&[]),
        a(&["--socket", "/tmp/x.sock"]),
        a(&["--boot-role", "x"]),
    ] {
        let e = rr3_cli_parse_env(&args, &env_none).expect_err("双缺必须拒");
        assert!(
            e.contains("启动缺装配面")
                && e.contains("--profile")
                && e.contains("--manifest")
                && e.contains("装载门整体不跑")
                && e.contains("基-23")
                && e.contains("用法"),
            "报错要点名开关、理由与用法: {e}"
        );
    }
    // 两面各给其一必须收（门不拦合法路径）：正门/后门各验一次
    assert!(rr3_cli_parse(&a(&["--profile", "/p.toml"])).is_ok());
    assert!(rr3_cli_parse(&a(&["--manifest", "/m.toml"])).is_ok());
}

/// `--contracts`（0.0.2 T5）：默认 None（编译期仓内路径）、显式值原样携带、
/// 路径口径与其它路径开关同族（缺参数 / `-` 开头值都拒）。
#[test]
fn contracts_is_optional_with_shared_path_flag_rules() {
    let dflt = rr3_cli_parse_env(&a(&["--manifest", "/m/d.toml"]), &env_none).unwrap();
    assert_eq!(
        dflt.contracts, None,
        "不带 --contracts 就是仓内编译期路径，默认值必须为 None"
    );
    let cfg = rr3_cli_parse_env(
        &a(&[
            "--manifest",
            "/m/d.toml",
            "--contracts",
            "/usr/lib/rockrobot3/contracts",
        ]),
        &env_none,
    )
    .unwrap();
    assert_eq!(
        cfg.contracts,
        Some(PathBuf::from("/usr/lib/rockrobot3/contracts")),
        "装机口径：包内契约目录"
    );
    let e = rr3_cli_parse(&a(&["--contracts"])).expect_err("缺参数必须报错");
    assert!(
        e.contains("--contracts 缺参数") && e.contains("用法"),
        "{e}"
    );
    let e = rr3_cli_parse(&a(&["--contracts", "--socket", "/tmp/x.sock"]))
        .expect_err("开关名不当值吞下");
    assert!(
        e.contains("--contracts 缺参数") && e.contains("--socket"),
        "{e}"
    );
}

/// 同给当场拒（Q3 裁定细则）：两参数不管谁先谁后，parse 处即非 0 退出，
/// 不留"谁赢"的歧义。文案要点名两个开关与裁定出处。
#[test]
fn profile_and_manifest_together_are_rejected_at_parse_in_both_orders() {
    for args in [
        a(&[
            "--profile",
            "/root/profiles/default.toml",
            "--manifest",
            "/m/x.toml",
        ]),
        a(&[
            "--manifest",
            "/m/x.toml",
            "--profile",
            "/root/profiles/default.toml",
        ]),
    ] {
        let e = rr3_cli_parse(&args).expect_err("同给必须拒");
        assert!(
            e.contains("--profile 与 --manifest 不得同给")
                && e.contains("Q3")
                && e.contains("用法"),
            "报错要点名两个开关与裁定出处: {e}"
        );
    }
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
        rr3_cli_parse(&a(&["--manifest", "/m/d.toml", "--restart-base-ms", "1"]))
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
    let ok = rr3_cli_parse(&a(&[
        "--manifest",
        "/m/d.toml",
        "--restart-base-ms",
        &max_ok.to_string(),
    ]))
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
    let abs = rr3_socket_abs(raw).expect("待创建路径不该被拒");
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
    assert_eq!(rr3_socket_abs(raw).unwrap(), raw);
}
