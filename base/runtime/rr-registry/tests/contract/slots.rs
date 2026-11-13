//! 能力格查册门（ADR-0012）：基础层表内化与全量 manifest 能力查册。

use super::*;

/// 基础层 = base_capabilities 的表内化（ADR-0012 裁定 1）：
/// 两份名单逐条钉死等价——第二份白名单不存在，仲裁与查册共用同一 base 真值。
#[test]
fn slots_base_layer_is_pinned_to_base_capabilities() {
    let mut base = rr_registry::rr3_slot_parse(&slots_toml()).rr3_slot_base();
    base.sort();
    let mut caps = rr_registry::rr3_rule_list("base_capabilities")
        .expect("module.schema.toml 必须登记 base_capabilities 规则");
    caps.sort();
    assert_eq!(base, caps, "slots 基础层与 base_capabilities 漂移");
}

/// 格位表的全部 23 条 `(name, layer)`，顺序即 `contracts/slots.toml` 登记顺序。
/// 与 `registry_shape_is_pinned` 同一钉法：append 合法但必须是有意识的编辑——
/// 同一次改动里同步更新 `contracts/slots.toml`、本常量与层分布计数、
/// `profiles/default.toml`（如涉及绑定面），以及把这些数字当事实抄进正文的文档。
const RR3_PINNED_SLOTS: [(&str, &str); 23] = [
    ("bus.publish", "base"),
    ("bus.subscribe", "base"),
    ("bus.rpc", "base"),
    ("net.online", "base"),
    ("power.battery", "base"),
    ("power.rail", "base"),
    ("power.budget", "base"),
    ("act.servo", "body"),
    ("act.led", "body"),
    ("act.relay", "body"),
    ("net.wifi", "body"),
    ("net.bt", "body"),
    ("ui.face", "body"),
    ("audio.playback", "body"),
    ("audio.capture", "body"),
    ("audio.vad", "body"),
    ("sensor.env", "body"),
    ("sensor.imu", "body"),
    ("power.charger", "body"),
    ("cognition.dialog", "cognition"),
    ("cognition.asr", "cognition"),
    ("cognition.llm", "cognition"),
    ("cognition.tts", "cognition"),
];

#[test]
fn slots_registry_shape_is_pinned() {
    use std::collections::BTreeMap;

    let table = rr_registry::rr3_slot_parse(&slots_toml());
    assert!(
        table.rr3_slot_check().is_empty(),
        "{:?}",
        table.rr3_slot_check()
    );

    // 1) 23 格、逐条 (name, layer) 在册且不多不少（配合唯一性与等长断言）
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (name, layer) in RR3_PINNED_SLOTS {
        assert_eq!(
            table.rr3_slot_layer(name).as_deref(),
            Some(layer),
            "格位缺失或层漂移: {name}"
        );
        *counts.entry(layer).or_insert(0) += 1;
    }
    assert_eq!(
        table.rr3_slot_names().len(),
        RR3_PINNED_SLOTS.len(),
        "格位总数漂移"
    );

    // 2) 层分布 7/12/4
    assert_eq!(counts.get("base"), Some(&7));
    assert_eq!(counts.get("body"), Some(&12));
    assert_eq!(counts.get("cognition"), Some(&4));
}

/// 门禁②（manifest 侧）：provides 与 depends.* 引用的每个能力必须命中格位表。
/// 单件校验只保语法（ADR-0009 裁定 1），"是否在册"只在全集层判——本用例就是那道全集门。
#[test]
fn every_manifest_capability_is_a_registered_slot() {
    let table = rr_registry::rr3_slot_parse(&slots_toml());
    let manifests = module_manifest_values();
    assert_eq!(manifests.len(), 6, "modules/ 模块集合漂移");
    for (name, v) in &manifests {
        let mut caps = str_list(v, "provides.capabilities");
        caps.extend(str_list(v, "depends.required"));
        caps.extend(str_list(v, "depends.optional"));
        assert!(
            !caps.is_empty(),
            "{name} 的能力面为空（provides 或 depends 至少登记一项）"
        );
        for cap in caps {
            assert!(
                table.rr3_slot_contains(&cap),
                "{name} 引用了表外能力 {cap}（新能力先立格再用，ADR-0012 裁定 1）"
            );
        }
    }
}
