//! manifest 文本入口与主题契约交叉校验的单测。

use super::*;
use crate::topics::rr3_topic_parse;

const CONTRACT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../contracts/topics.toml"
));
const OVERLAY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/sim-topics.toml"
));

fn merged() -> crate::topics::Rr3TopicSet {
    rr3_topic_parse(CONTRACT)
        .rr3_topic_merge(&rr3_topic_parse(OVERLAY))
        .expect("夹具叠加层不该撞名")
}

fn sima(publishes: &str, hb: u64, wd: u64) -> String {
    format!(
        r#"[manifest]
schema_version = 1
name = "rr3-sima"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/sim_peer"

[events]
publishes = ["{publishes}"]
subscribes = ["rr3-runtime/module/state"]

[resources]
restart_policy = "always"

[health]
heartbeat_ms = {hb}
watchdog_timeout_ms = {wd}
"#
    )
}

/// 只在叠加层里的 topic：合并后放行，未合并按未注册主题拒
#[test]
fn overlay_topic_passes_only_when_merged() {
    let base = rr3_topic_parse(CONTRACT);
    let r1 = rr3_manifest_chk(&sima("rr3-sima/ping/state", 200, 1000), &base);
    assert!(
        r1.errors.iter().any(|e| e.contains("未注册主题")),
        "{:?}",
        r1.errors
    );

    let r2 = rr3_manifest_chk(&sima("rr3-sima/ping/state", 200, 1000), &merged());
    assert!(r2.ok(), "合并后必须放行: {:?}", r2.errors);
}

/// 叠加层不放行越权与健康规则（只扩主题名，不放宽判定）
#[test]
fn overlay_relaxes_nothing_but_membership() {
    let r = rr3_manifest_chk(&sima("rr3-simb/pong/state", 200, 1000), &merged());
    assert!(
        r.errors.iter().any(|e| e.contains("越权主题")),
        "{:?}",
        r.errors
    );

    let r2 = rr3_manifest_chk(&sima("rr3-sima/ping/state", 200, 200), &merged());
    assert!(
        r2.errors
            .iter()
            .any(|e| e.contains("heartbeat_ms(200) 必须小于 watchdog_timeout_ms(200)")),
        "{:?}",
        r2.errors
    );
}

/// C14 申报面退役门（0.0.5 批J）：retired 主题不得被 publishes/rpc.exposes/subscribes
/// 申报引用；正常主题不受影响（contract 真值上跑，asr/text 与 dialog/turn 已显式 retired）
#[test]
fn retired_topics_cannot_be_declared() {
    let set = rr3_topic_parse(CONTRACT);
    let manifest = |publishes: &str, subscribes: &str| {
        format!(
            r#"[manifest]
schema_version = 1
name = "rr3-interact"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/rr3-interact"

[events]
publishes = [{publishes}]
subscribes = [{subscribes}]

[resources]
restart_policy = "always"

[health]
heartbeat_ms = 200
watchdog_timeout_ms = 1000
"#
        )
    };

    // 发布申报 → 拒并点名
    let r = rr3_manifest_chk(&manifest("\"rr3-interact/asr/text\"", ""), &set);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("retired 主题 rr3-interact/asr/text 不得被申报")),
        "{:?}",
        r.errors
    );

    // RPC 暴露申报（retired 主题不能过 rpc.exposes——用同一点名门）与订阅申报 → 拒
    let r = rr3_manifest_chk(&manifest("", "\"rr3-interact/dialog/turn\""), &set);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("events.subscribes 引用 retired 主题 rr3-interact/dialog/turn")),
        "{:?}",
        r.errors
    );

    // 正常主题申报不受退役门影响（号位保留语义：非 retired 的在册主题照常放行）
    let r = rr3_manifest_chk(
        &manifest(
            "\"rr3-interact/face/expression\"",
            "\"rr3-runtime/module/state\"",
        ),
        &set,
    );
    assert!(r.ok(), "正常主题不得被误伤: {:?}", r.errors);
}

/// 空主题集 = 一条错误，不做后续校验
#[test]
fn empty_topic_set_fail_closes() {
    let r = rr3_manifest_chk(
        &sima("rr3-sima/ping/state", 200, 1000),
        &crate::topics::Rr3TopicSet::default(),
    );
    assert_eq!(
        r.errors,
        vec!["主题集为空：拒绝校验任何 manifest（空注册表不是放行许可证）"]
    );
}

/// 新旧入口对同一份 fixture 必须给出逐字相同的报错（防搬运丢规则）
#[test]
fn new_entry_matches_old_entry_on_repo_fixture() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rr3-sense.toml");
    let old = rr3_manifest_gate(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    let new = rr3_manifest_chk(&text, &rr3_topic_parse(CONTRACT));
    assert!(old.ok(), "fixture 本该合规: {:?}", old.errors);
    assert_eq!(new.errors, old.errors);
}

/// 坏 manifest 的三类错误都必须由新入口点名具体字段（spec §9 的机器可验项）
#[test]
fn bad_manifests_name_the_field() {
    let m = merged();
    let missing =
        sima("rr3-sima/ping/state", 200, 1000).replace("restart_policy = \"always\"\n", "");
    assert!(rr3_manifest_chk(&missing, &m)
        .errors
        .iter()
        .any(|e| e.contains("缺少必填字段 resources.restart_policy")));

    let bad_policy = sima("rr3-sima/ping/state", 200, 1000).replace(
        "restart_policy = \"always\"",
        "restart_policy = \"sometimes\"",
    );
    assert!(rr3_manifest_chk(&bad_policy, &m)
        .errors
        .iter()
        .any(|e| e.contains("restart_policy 越界")));

    let hb_ge_wd = sima("rr3-sima/ping/state", 1000, 200);
    assert!(rr3_manifest_chk(&hb_ge_wd, &m)
        .errors
        .iter()
        .any(|e| e.contains("heartbeat_ms(1000) 必须小于 watchdog_timeout_ms(200)")));
}
