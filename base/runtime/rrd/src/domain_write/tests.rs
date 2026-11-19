//! domain 写口单测（自同名单测模块原样抽出，判据不变）。

use super::*;
use serde_json::json;

/// parse 正例：三写口各自合法形状全收（armed 布尔、severe_mv/current_ceil_ma 正整数）
#[test]
fn parse_accepts_three_named_writes() {
    let p = rr3_write_parse(&json!({
        "target": "domain", "name": "rail5v", "key": "armed",
        "value": true, "confirm": true, "evidence": "实测/0002"
    }))
    .expect("armed 写应过");
    assert_eq!(p.target, Rr3WriteTarget::Domain);
    assert_eq!(p.key, "armed");
    assert_eq!(p.value, Rr3WriteValue::Bool(true));
    assert_eq!(p.evidence.as_deref(), Some("实测/0002"));

    let p = rr3_write_parse(&json!({
        "target": "domain", "name": "rail5v", "key": "severe_mv",
        "value": 4200, "confirm": true, "evidence": "实测/0001"
    }))
    .expect("severe_mv 写应过");
    assert_eq!(p.value, Rr3WriteValue::Num(4200));

    let p = rr3_write_parse(&json!({
        "target": "axis", "name": "pan", "key": "current_ceil_ma",
        "value": 700, "confirm": true, "evidence": "对拍/0001"
    }))
    .expect("current_ceil_ma 写应过");
    assert_eq!(p.target, Rr3WriteTarget::Axis);
    assert_eq!(p.value, Rr3WriteValue::Num(700));
}

/// parse 拒面：缺参 / 白名单外键 / 型坏 / 零值——全 Invalid 逐条点名
#[test]
fn parse_rejects_bad_shapes_fail_closed() {
    for (v, hint) in [
        (json!({"name":"x","key":"armed","value":true}), "target"),
        (
            json!({"target":"domain","key":"armed","value":true}),
            "name",
        ),
        (json!({"target":"domain","name":"x","value":true}), "key"),
        (
            json!({"target":"domain","name":"x","key":"nominal_mv","value":1}),
            "白名单",
        ),
        (
            json!({"target":"axis","name":"x","key":"armed","value":true}),
            "白名单",
        ),
        (
            json!({"target":"domain","name":"x","key":"armed","value":"yes"}),
            "布尔",
        ),
        (
            json!({"target":"domain","name":"x","key":"severe_mv","value":-1}),
            "正整数",
        ),
        (
            json!({"target":"axis","name":"x","key":"current_ceil_ma","value":0}),
            "正整数",
        ),
    ] {
        let (code, msg) = rr3_write_parse(&v).expect_err("坏形状必须拒");
        assert_eq!(code, Rr3StatusCode::Invalid, "{v}");
        assert!(msg.contains(hint), "文案缺 {hint}: {msg}");
    }
}

/// 路径推导正例：七成员记账行 → 六路路径（形状 = boot 记账真实成员名）
#[test]
fn paths_derive_from_truth_set_entries() {
    let ts = vec![
        ts_entry("profiles/esp32-mini.toml", "/x/profiles/esp32-mini.toml"),
        ts_entry("contracts/topics.toml", "/x/contracts/topics.toml"),
        ts_entry("contracts/slots.toml", "/x/contracts/slots.toml"),
        ts_entry("contracts/roles.toml", "/x/contracts/roles.toml"),
        ts_entry("contracts/devices.toml", "/x/contracts/devices.toml"),
        ts_entry(
            "contracts/platform/esp32.toml",
            "/x/contracts/platform/esp32.toml",
        ),
        ts_entry(
            "hardware/esp32-mini/wiring.toml",
            "/x/hardware/esp32-mini/wiring.toml",
        ),
    ];
    let p = rr3_write_paths(&ts).expect("七成员应推全六路");
    assert_eq!(p.profile, PathBuf::from("/x/profiles/esp32-mini.toml"));
    assert_eq!(p.slots, PathBuf::from("/x/contracts/slots.toml"));
    assert_eq!(p.platform_dir, PathBuf::from("/x/contracts/platform"));
    assert_eq!(
        p.wiring,
        PathBuf::from("/x/hardware/esp32-mini/wiring.toml")
    );
}

/// 路径推导缺员逐条点名（fail-closed——记账形状破不猜路径）
#[test]
fn paths_reject_missing_members_by_name() {
    let e = rr3_write_paths(&[]).expect_err("空记账必须拒");
    assert!(e.contains("profiles/<stem>.toml"), "{e}");
    let ts = vec![ts_entry(
        "profiles/esp32-mini.toml",
        "/x/profiles/esp32-mini.toml",
    )];
    let e = rr3_write_paths(&ts).expect_err("缺契约册必须拒");
    assert!(e.contains("contracts/slots.toml"), "{e}");
}

/// 同类成员重复 = 记账形状破：写口拒，不静默取首条（那会把写落到错误文件）
#[test]
fn paths_reject_duplicate_members() {
    let ts = vec![
        ts_entry("profiles/a.toml", "/x/profiles/a.toml"),
        ts_entry("profiles/b.toml", "/x/profiles/b.toml"),
    ];
    let e = rr3_write_paths(&ts).expect_err("同类重复必须拒");
    assert!(e.contains("重复"), "歧义要点名重复: {e}");
    assert!(e.contains("profiles/a.toml"), "要点名重复成员: {e}");
}

fn ts_entry(member: &str, path: &str) -> Rr3TruthSetEntry {
    Rr3TruthSetEntry {
        member: member.to_string(),
        path: PathBuf::from(path),
        sha256: "0".repeat(64),
        bytes: 1,
    }
}

/// 证据根推导（本轮审计）：两级父目录才成立；推不出即拒——旧序退回相对
/// 路径，root 侧写口的证据根会由启动目录决定（猜路径）。
#[test]
fn evidence_root_derives_or_rejects() {
    let root = rr3_evidence_root(Path::new("/x/profiles/a.toml")).unwrap();
    assert_eq!(root, PathBuf::from("/x/docs/evidence"));
    let e = rr3_evidence_root(Path::new("a.toml")).expect_err("无两级父目录必须拒");
    assert!(e.1.contains("证据容器根不可推导"), "{}", e.1);
}

/// 轴实例名形制（本轮审计）：只收单段标识——名字直接拼进轴档路径，`..`
/// 或分隔符会把写口放大成"改轴目录外的在册档"。
#[test]
fn axis_name_is_single_segment_only() {
    for ok in ["pan", "tilt", "arm_l", "a"] {
        assert!(rr3_axis_name_ok(ok), "{ok} 是合法单段名");
    }
    for bad in ["", ".", "..", "../pan", "a/b", "a\\b", "pan\0"] {
        assert!(!rr3_axis_name_ok(bad), "{bad:?} 必须拒");
    }
}

// execute 层集成测试（unix 门）：tmp 目录自建仓形状夹具，不打全局契约快照
// （全局量进程级单次，同 lib.rs log_ring_cap 单测的污染纪律）。
#[cfg(unix)]
mod execute_tests;
