// SPDX-License-Identifier: Apache-2.0
//! `rr3_chk_list` 的形状判据：无规则 list 也必须逐项验字符串（与 atom 侧同构）。

use super::*;

/// 契约未给规则时仍拒非字符串项——否则契约演进出无规则 list 即 fail-open
/// （atom 侧 `rr3_atom_list` 一直逐项验，module 侧此前只在有规则时验）。
#[test]
fn list_no_rule_rej_item() {
    let spec = Rr3FieldSpec {
        path: "events.publishes".to_string(),
        kind: "list".to_string(),
        required: false,
        rule: None,
    };
    let root: Value = toml::from_str("events = { publishes = [1, \"rr3-x/a/b\"] }\n")
        .expect("夹具 TOML 必须可解析");
    let topics = crate::topics::rr3_topic_parse("");
    let mut errs = Vec::new();
    let got = rr3_chk_list(
        &spec,
        rr3_get_by_path(&root, "events.publishes").expect("路径必须存在"),
        "",
        &topics,
        &mut errs,
    );
    assert!(got, "数组形状本身合法");
    assert_eq!(errs.len(), 1, "非字符串项必须点名: {errs:?}");
    assert!(errs[0].contains("第 1 项"), "{}", errs[0]);
}

/// 有规则时逐项仍只报一条类型错（不因规则缺失或字符串项漏检而重复报）。
#[test]
fn list_rule_keeps_item_err() {
    let spec = Rr3FieldSpec {
        path: "events.subscribes".to_string(),
        kind: "list".to_string(),
        required: false,
        rule: Some("topic".to_string()),
    };
    let root: Value = toml::from_str("events = { subscribes = [true] }\n").expect("夹具可解析");
    let topics = crate::topics::rr3_topic_parse("");
    let mut errs = Vec::new();
    rr3_chk_list(
        &spec,
        rr3_get_by_path(&root, "events.subscribes").expect("路径存在"),
        "",
        &topics,
        &mut errs,
    );
    assert_eq!(errs.len(), 1, "布尔项只报项形状错: {errs:?}");
}
