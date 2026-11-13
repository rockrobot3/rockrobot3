//! wiring 的单测（自同名单测模块原样抽出，判据不变）。

use std::path::Path;

use super::*;

/// 与生产档同构的最小合法线组（profile.rs 测试区 GOOD_WIRING 同一形状）
const GOOD: &str = r#"
[[wire]]
connector = "J1"
hdr_pin = [1, 2]
signal = ["vcc", "gnd"]
net = "rail5v"
feeds = []
mate = "none"
keyed = true
rated_a = 1.0
gnd_common = "star-1"
"#;

#[test]
fn good_text_parses_field_by_field() {
    let w = rr3_wiring_from_text(GOOD).expect("应能解析");
    assert_eq!(w.wires.len(), 1);
    let wire = &w.wires[0];
    assert_eq!(wire.connector, "J1");
    assert_eq!(wire.hdr_pin, vec![1, 2]);
    assert_eq!(wire.signal, vec!["vcc", "gnd"]);
    assert_eq!(wire.net, "rail5v");
    assert!(wire.feeds.is_empty());
    assert_eq!(wire.mate, "none");
    assert!(wire.keyed);
    assert_eq!(wire.rated_a, 1.0);
    assert_eq!(wire.gnd_common, "star-1");
    assert!(!w.rr3_wiring_is_empty());
    // vcc/gnd-only 线组：signal/mate 空集即闭合；net 是 domains 外键——
    // 给 rail5v 命中域后全绿（"none" 显式值的同一判据在下方反例组钉）
    let mut domains = BTreeSet::new();
    domains.insert("rail5v".to_string());
    let errs = w.rr3_wiring_check(&BTreeSet::new(), &domains, &BTreeSet::new());
    assert!(errs.is_empty(), "{errs:?}");
}

/// 缺 [[wire]] / 空表 / TOML 坏文逐条点名（fail-closed：没有"无线的机器"）
#[test]
fn empty_and_broken_text_are_fatal() {
    for (text, want) in [
        ("", "缺少 [[wire]]"),
        ("wire = []\n", "接线档为空"),
        ("[[wire\n", "TOML 解析失败"),
    ] {
        let errs = rr3_wiring_from_text(text).expect_err(text);
        assert!(errs[0].contains(want), "{text:?} => {errs:?}");
    }
}

/// 形状反例：缺键/坏类型/未知键/位序不等/位号重复一次收集完
#[test]
fn shape_errors_are_all_collected_and_named() {
    let text = format!(
        "{GOOD}\n[[wire]]\nhdr_pin = [1, 2]\nsignal = [\"vcc\"]\nfeeds = []\n\
             mate = \"none\"\nkeyed = \"yes\"\nrated_a = 0\ngnd_common = \"\"\nghost = 1\n\
             \n[[wire]]\nconnector = \"J1\"\n"
    );
    let errs = rr3_wiring_from_text(&text).expect_err("形状不合必须报错");
    for want in [
        "wire #1.connector 缺失或为空",
        "wire #1 的 hdr_pin（2 个）与 signal（1 个）数目不等",
        "wire #2.signal 必须是非空字符串数组",
        "wire #1.net 缺失或为空",
        "wire #1.keyed 缺失或不是布尔",
        "wire #1.rated_a 缺失或不是正数",
        "wire #1.gnd_common 缺失或为空",
        "wire #1 未知键 ghost",
        "wire #2 位号 J1 与 wire #0 重复",
    ] {
        assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
    }
}

/// feeds 缺键即红（§5 缺失语义列），显式 [] 合法
#[test]
fn feeds_missing_is_fatal_but_empty_array_is_legal() {
    let text = GOOD.replace("feeds = []\n", "");
    let errs = rr3_wiring_from_text(&text).expect_err("feeds 缺失必须报错");
    assert!(errs.iter().any(|e| e.contains("feeds 缺失")), "{errs:?}");
    assert!(rr3_wiring_from_text(GOOD).is_ok(), "显式 [] 是合法空集");
}

/// 闭合面：feeds 指向同文件位号；悬空引用与自环都红
#[test]
fn feeds_must_close_within_the_file() {
    let text = format!(
        "{GOOD}\n[[wire]]\nconnector = \"J2\"\nhdr_pin = [1, 2]\nsignal = [\"vcc\", \"gnd\"]\n\
             net = \"rail5v\"\nfeeds = [\"J9\"]\nmate = \"none\"\nkeyed = false\nrated_a = 1.0\n\
             gnd_common = \"star-1\"\n"
    );
    let errs = rr3_wiring_from_text(&text).expect_err("悬空 feeds 必须报错");
    assert!(
        errs.iter()
            .any(|e| e.contains("wiring[J2].feeds = J9") && e.contains("位号集")),
        "{errs:?}"
    );

    let text = GOOD.replace("feeds = []", "feeds = [\"J1\"]");
    let errs = rr3_wiring_from_text(&text).expect_err("自环必须报错");
    assert!(errs.iter().any(|e| e.contains("自环")), "{errs:?}");
}

/// 校验⑦-a/b/d 反例：signal 越出 pin_id ∪ {vcc,gnd}、net 越出 domains、
/// mate 越出 devices mate 集，逐条点名；合法显式值 none 不误报。
/// 底稿 GOOD 的 net 先置换成 none（纯信号/地线组合法显式值）——反例集只携
/// 三条越界噪声，条数断言才钉得住"一次收集、不短路"。
#[test]
fn wiring_check_names_each_cross_file_violation() {
    let base = GOOD.replace("net = \"rail5v\"", "net = \"none\"");
    let text = format!(
        "{base}\n[[wire]]\nconnector = \"J2\"\nhdr_pin = [1, 2, 3]\n\
             signal = [\"vcc\", \"act.relay.gpio\", \"gnd\"]\nnet = \"rail24\"\nfeeds = []\n\
             mate = \"JST-NOPE\"\nkeyed = false\nrated_a = 1.0\ngnd_common = \"star-1\"\n"
    );
    let w = rr3_wiring_from_text(&text).expect("形状层须先过（闭合面成立）");
    let errs = w.rr3_wiring_check(&BTreeSet::new(), &BTreeSet::new(), &BTreeSet::new());
    assert_eq!(errs.len(), 3, "{errs:?}");
    assert!(
        errs.iter()
            .any(|e| e.contains("校验⑦-a") && e.contains("act.relay.gpio")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("校验⑦-b") && e.contains("rail24")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("校验⑦-d") && e.contains("JST-NOPE")),
        "{errs:?}"
    );

    // 命中集：signal ∈ pin_id、net ∈ domains、mate ∈ mate 集全绿；
    // J2 的 vcc/gnd 与 none 显式值不误报
    let mut pins = BTreeSet::new();
    pins.insert("act.relay.gpio".to_string());
    let mut domains = BTreeSet::new();
    domains.insert("rail24".to_string());
    let mut mates = BTreeSet::new();
    mates.insert("JST-NOPE".to_string());
    let errs = w.rr3_wiring_check(&pins, &domains, &mates);
    assert!(errs.is_empty(), "{errs:?}");

    // "none" 是合法显式值（net/mate/gnd_common 三处），空集上不报 ⑦-b/⑦-d
    let text = GOOD
        .replace("net = \"rail5v\"", "net = \"none\"")
        .replace("gnd_common = \"star-1\"", "gnd_common = \"none\"");
    let w = rr3_wiring_from_text(&text).expect("应能解析");
    let errs = w.rr3_wiring_check(&BTreeSet::new(), &BTreeSet::new(), &BTreeSet::new());
    assert!(errs.is_empty(), "{errs:?}");
}

/// 接线路径推导：profiles/<机>.toml → hardware/<机>/wiring.toml；
/// 装配单不存在 = 路径不可推导（canonicalize 的 ENOENT 口径）
#[test]
fn wiring_path_follows_install_layout() {
    let dir = std::env::temp_dir().join(format!("rr3-wiring-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("profiles")).unwrap();
    let p = dir.join("profiles").join("mini.toml");
    std::fs::write(&p, "schema_version = 5\n").unwrap();
    // 比对基准必须是规范化后的根目录：推导走 canonicalize（macOS 的 /tmp 是
    // /private/tmp 的符号链接），拿夹具原始路径比对会红在系统布局上
    let root_canon = std::fs::canonicalize(&dir).unwrap();
    let want = root_canon.join("hardware").join("mini").join("wiring.toml");
    assert_eq!(rr3_wiring_path(&p).unwrap(), want);
    let e = rr3_wiring_path(Path::new("/nonexistent/nope.toml")).expect_err("读不到装配单必须致命");
    assert!(e.contains("无法解析为绝对路径"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
