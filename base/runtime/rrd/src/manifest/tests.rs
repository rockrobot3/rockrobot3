//! manifest 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;

const GOOD: &str = r#"[manifest]
schema_version = 1
name = "rr3-sima"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/sim_peer"

[events]
publishes = ["rr3-sima/ping/state"]
subscribes = []

[resources]
restart_policy = "always"
start_priority = 40

[health]
heartbeat_ms = 200
watchdog_timeout_ms = 1000
"#;

/// 7 个事实全部取到
#[test]
fn facts_are_extracted() {
    let f = rr3_manifest_parse(GOOD).expect("应能读取");
    assert_eq!(
        (
            f.name.as_str(),
            f.heartbeat_ms,
            f.watchdog_ms,
            f.restart_policy.as_str()
        ),
        ("rr3-sima", 200, 1000, "always")
    );
    assert_eq!(
        (f.entry.as_str(), f.start_priority),
        ("/usr/bin/sim_peer", 40)
    );
    assert!(f.depends_required.is_empty());
    assert!(f.provides.is_empty());
}

/// 装机改写（B45 起打包与演练布局共用本实现）：只动 entry 引号内的值，其余
/// 字节（行尾注释、段序、尾换行）原样；四道门各自的坏形拒且点名。
#[test]
fn rewrite_touches_only_the_entry_value() {
    let text = GOOD.replace(
        "entry = \"/usr/bin/sim_peer\"",
        "entry = \"/usr/bin/sim_peer\" # 行尾注释随原值保留",
    );
    let out = rr3_manifest_rewrite(
        &text,
        "/opt/rr3/modules/rr3-sima/rr3-sima",
        "/usr/bin/sim_peer",
        "rr3-sima",
    )
    .expect("合法改写应过");
    assert!(
        out.contains("entry = \"/opt/rr3/modules/rr3-sima/rr3-sima\" # 行尾注释随原值保留\n"),
        "{out}"
    );
    let strip = |s: &str| {
        s.lines()
            .filter(|l| !l.trim_start().starts_with("entry"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(strip(&text), strip(&out), "除 entry 外须逐字节零差异");
    assert!(
        rr3_manifest_rewrite(&text, "rel/bin", "/usr/bin/sim_peer", "m")
            .unwrap_err()
            .contains("不可装机")
    );
    assert!(
        rr3_manifest_rewrite(&text, "/abs/../esc", "/usr/bin/sim_peer", "m")
            .unwrap_err()
            .contains("不可装机")
    );
    assert!(rr3_manifest_rewrite(&text, "/x", "other", "m")
        .unwrap_err()
        .contains("不一致"));
    let dup = format!("{text}entry = \"/usr/bin/other\"\n");
    assert!(rr3_manifest_rewrite(&dup, "/x", "/usr/bin/sim_peer", "m")
        .unwrap_err()
        .contains("恰一条"));
}

/// 缺字段逐条点名且继续检查其余字段（不是遇一个就返回）
#[test]
fn missing_fields_are_all_reported_with_paths() {
    let text = GOOD
        .replace("entry = \"/usr/bin/sim_peer\"\n", "")
        .replace("restart_policy = \"always\"\n", "");
    let errs = rr3_manifest_parse(&text).expect_err("缺字段必须报错");
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert!(
        errs.iter().any(|e| e.contains("缺少 manifest.entry")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("缺少 resources.restart_policy")),
        "{errs:?}"
    );
}

/// 类型错与空串都是错误
#[test]
fn wrong_types_and_empty_values_are_errors() {
    let t = GOOD.replace("heartbeat_ms = 200", "heartbeat_ms = \"200\"");
    let errs = rr3_manifest_parse(&t).expect_err("类型错必须报错");
    assert_eq!(errs, vec!["health.heartbeat_ms 应为整数"], "{errs:?}");

    assert!(rr3_manifest_parse(
        &GOOD.replace("restart_policy = \"always\"", "restart_policy = \"\"")
    )
    .expect_err("空 policy 必须报错")
    .iter()
    .any(|e| e.contains("manifest") || e.contains("resources.restart_policy 不得为空")));

    assert!(rr3_manifest_parse(
        &GOOD.replace("watchdog_timeout_ms = 1000", "watchdog_timeout_ms = 0")
    )
    .expect_err("0 毫秒不可排看门狗")
    .iter()
    .any(|e| e.contains("health.watchdog_timeout_ms 应为正整数")));
}

/// 坏 TOML 只报一条解析错，不继续瞎猜
#[test]
fn unparsable_toml_is_single_error() {
    let errs = rr3_manifest_parse("[manifest\nname = ").expect_err("必须报错");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].starts_with("manifest TOML 解析失败"), "{errs:?}");
}

/// rrd 侧的 heartbeat>=watchdog 拦截（与 rr-registry 同规则，纵深防御）
#[test]
fn heartbeat_must_stay_below_watchdog_here_too() {
    let errs = rr3_manifest_parse(&GOOD.replace("heartbeat_ms = 200", "heartbeat_ms = 1000"))
        .expect_err("相等也不行");
    assert_eq!(
        errs,
        vec!["health.heartbeat_ms(1000) 必须小于 watchdog_timeout_ms(1000)"],
        "文案要带两个具体数字，运维才知道改哪个"
    );
}

/// depends.required 取出来，供 boot 仲裁判能力是否就位；start_priority 缺省 100
#[test]
fn depends_and_default_priority() {
    let text = GOOD.replace(
        "[resources]",
        "[depends]\nrequired = [\"bus.publish\", \"bus.rpc\"]\n\n[resources]",
    );
    let f = rr3_manifest_parse(&text).expect("应能读取");
    assert_eq!(
        f.depends_required,
        vec!["bus.publish".to_string(), "bus.rpc".to_string()]
    );

    let no_prio = GOOD.replace("start_priority = 40\n", "");
    assert_eq!(rr3_manifest_parse(&no_prio).unwrap().start_priority, 100);
}

/// [provides].capabilities：可选段（缺省空 vec），给了就逐项进 facts；
/// 类型错必须报错（与 depends.required 同一条纪律——旧代码对非字符串项
/// filter_map 静默吞掉是 fail-open，随 ADR-0009 落地改判错误）
#[test]
fn provides_is_optional_and_typed() {
    let with = GOOD.replace(
        "[health]",
        "[provides]\ncapabilities = [\"audio.capture\"]\n\n[health]",
    );
    let f = rr3_manifest_parse(&with).expect("带 provides 的 manifest 应能读取");
    assert_eq!(f.provides, vec!["audio.capture".to_string()]);

    let bad = GOOD.replace("[health]", "[provides]\ncapabilities = 3\n\n[health]");
    let errs = rr3_manifest_parse(&bad).expect_err("非数组必须报错");
    assert!(
        errs.iter()
            .any(|e| e.contains("provides.capabilities 应为字符串数组")),
        "{errs:?}"
    );

    // 非字符串项逐条点名，不再被静默吞掉（fail-closed）
    let item = GOOD.replace(
        "[health]",
        "[provides]\ncapabilities = [\"ok\", 1]\n\n[health]",
    );
    let errs = rr3_manifest_parse(&item).expect_err("非字符串项必须报错");
    assert!(
        errs.iter().any(|e| e.contains("每一项都必须是非空字符串")),
        "{errs:?}"
    );
}

/// [rpc].exposes：可选段（缺省空 vec = 不接 rr3.call），给了就逐项进 facts
#[test]
fn rpc_exposes_is_optional_and_typed() {
    // 缺省：GOOD 没有 [rpc] 段
    assert!(rr3_manifest_parse(GOOD).unwrap().rpc_exposes.is_empty());
    // 合法声明
    let with = GOOD.replace("[health]", "[rpc]\nexposes = [\"rr3-interact/tts/text\", \"rr3-interact/face/expression\"]\n\n[health]");
    let f = rr3_manifest_parse(&with).expect("带 exposes 的 manifest 应能读取");
    assert_eq!(
        f.rpc_exposes,
        vec![
            "rr3-interact/tts/text".to_string(),
            "rr3-interact/face/expression".to_string()
        ]
    );
    // 类型错必须报错，不许静默当空表
    let bad = GOOD.replace(
        "[health]",
        "[rpc]\nexposes = \"rr3-interact/tts/text\"\n\n[health]",
    );
    let errs = rr3_manifest_parse(&bad).expect_err("字符串不是数组必须报错");
    assert!(
        errs.iter()
            .any(|e| e.contains("rpc.exposes 应为字符串数组")),
        "{errs:?}"
    );
    let empty_item = GOOD.replace("[health]", "[rpc]\nexposes = [\"\"]\n\n[health]");
    let errs = rr3_manifest_parse(&empty_item).expect_err("空串项必须报错");
    assert!(
        errs.iter().any(|e| e.contains("每一项都必须是非空字符串")),
        "{errs:?}"
    );
}

/// [events].subscribes：可选段（缺省空 vec），类型错/空项 fail-closed；
/// 运行期消费面 = 广播影子统计（不改投递行为）
#[test]
fn subscribes_is_optional_and_typed() {
    assert!(rr3_manifest_parse(GOOD).unwrap().subscribes.is_empty());
    let with = GOOD.replace("subscribes = []", "subscribes = [\"rr3-sense/audio/pcm\"]");
    let f = rr3_manifest_parse(&with).expect("带 subscribes 的 manifest 应能读取");
    assert_eq!(f.subscribes, vec!["rr3-sense/audio/pcm".to_string()]);
    let bad = GOOD.replace("subscribes = []", "subscribes = \"rr3-sense/audio/pcm\"");
    let errs = rr3_manifest_parse(&bad).expect_err("字符串不是数组必须报错");
    assert!(
        errs.iter()
            .any(|e| e.contains("events.subscribes 应为字符串数组")),
        "{errs:?}"
    );
    let empty_item = GOOD.replace("subscribes = []", "subscribes = [\"\"]");
    let errs = rr3_manifest_parse(&empty_item).expect_err("空串项必须报错");
    assert!(
        errs.iter().any(|e| e.contains("每一项都必须是非空字符串")),
        "{errs:?}"
    );
}

/// entry 绝对性判定（M2 只接受绝对路径，见本计划"与 spec 的偏离 3"）。
/// 反斜杠开头在 POSIX 里是合法文件名而非绝对路径，必须判 false——判定不随 host 漂移。
#[test]
fn entry_absoluteness() {
    assert!(rr3_manifest_abs("/usr/bin/sim_peer"));
    for rel in [
        "sim_peer",
        "./sim_peer",
        "../x/sim_peer",
        "",
        "\\usr\\bin\\sim_peer",
    ] {
        assert!(!rr3_manifest_abs(rel), "{rel} 不是绝对路径");
    }
}

/// 模块名形状：路径分量（per-unit 日志落盘）不得带分隔符或 `..`；
/// 非 ASCII/空白/超长同拒——名字从线上注册与 boot 两路进同一解析器。
#[test]
fn module_name_shape_rejects_path_components() {
    let long = "x".repeat(65);
    for bad in ["../x", "a/b", ".", "..", "名字", "a b", long.as_str()] {
        let text = GOOD.replace("name = \"rr3-sima\"", &format!("name = \"{bad}\""));
        assert!(rr3_manifest_parse(&text).is_err(), "非法名应拒: {bad:?}");
    }
    for ok in ["rr3-a", "a_b.c", "x"] {
        let text = GOOD.replace("name = \"rr3-sima\"", &format!("name = \"{ok}\""));
        assert!(rr3_manifest_parse(&text).is_ok(), "合法名应过: {ok:?}");
    }
}
