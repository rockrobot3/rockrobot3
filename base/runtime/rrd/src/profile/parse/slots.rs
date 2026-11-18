//! [slots] 段解读：present 物理在位格位集与 device_absent 器件缺席登记。

use std::collections::BTreeMap;

/// [slots]：必填段、present = 本板物理在位格位声明（缺格三源合一第一源）。
/// 每项非空字符串；未知键点名。空数组合法——板可以什么都不在位（全 Degraded）。
/// device_absent = base 层格的器件缺席登记（D3 裁定 5）：表键为格位名、值为
/// 非空理由；留空即红（no_injection 范式——缺席不写理由 = 未申报的静默退化）。
pub(super) fn rr3_slots_parse(
    root: &toml::Value,
    errs: &mut Vec<String>,
) -> (Vec<String>, BTreeMap<String, String>) {
    let mut slots_present: Vec<String> = Vec::new();
    let mut slots_device_absent: BTreeMap<String, String> = BTreeMap::new();
    let Some(t) = root.get("slots").and_then(toml::Value::as_table) else {
        errs.push(
            "缺少 [slots]（present = 本板物理在位格位声明；schema_version=2 起必填）".to_string(),
        );
        return (slots_present, slots_device_absent);
    };
    rr3_slots_present(t, &mut slots_present, errs);
    rr3_slots_absent(t, &mut slots_device_absent, errs);
    rr3_slots_unknown(t, errs);
    (slots_present, slots_device_absent)
}

/// [slots] 未知键点名（只认 present / device_absent）。
fn rr3_slots_unknown(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        if !rr3_slot_key_ok(k) {
            errs.push(format!(
                "[slots] 未知键 {k}（只认 present / device_absent）"
            ));
        }
    }
}

/// slots 键白名单：present / device_absent 两键。
fn rr3_slot_key_ok(k: &str) -> bool {
    k == "present" || k == "device_absent"
}

/// [slots].present（B9 自 slots_parse 抽出）：本板物理在位格位数组，逐项非空。
fn rr3_slots_present(t: &toml::Table, out: &mut Vec<String>, errs: &mut Vec<String>) {
    match t.get("present").and_then(toml::Value::as_array) {
        Some(items) => rr3_present_items(items, out, errs),
        None => errs.push("[slots] 缺 present（本板物理在位格位数组）".to_string()),
    }
}

/// present 数组逐项：非空字符串收册，其余点名。
fn rr3_present_items(items: &[toml::Value], out: &mut Vec<String>, errs: &mut Vec<String>) {
    for item in items {
        match item.as_str() {
            Some(s) if !s.is_empty() => out.push(s.to_string()),
            _ => errs.push("[slots].present 的每一项都必须是非空字符串（格位名）".to_string()),
        }
    }
}

/// [slots].device_absent（B9 自 slots_parse 抽出）：缺席须显式申报非空理由。
fn rr3_slots_absent(t: &toml::Table, out: &mut BTreeMap<String, String>, errs: &mut Vec<String>) {
    let Some(absent) = t.get("device_absent").and_then(toml::Value::as_table) else {
        return;
    };
    for (slot, reason) in absent {
        rr3_absent_item(slot, reason, out, errs);
    }
}

/// 单条 device_absent：非空理由收册，否则按同一文案点名。
fn rr3_absent_item(
    slot: &str,
    reason: &toml::Value,
    out: &mut BTreeMap<String, String>,
    errs: &mut Vec<String>,
) {
    match reason.as_str() {
        Some(r) if !r.trim().is_empty() => {
            out.insert(slot.to_string(), r.to_string());
        }
        _ => errs.push(format!(
            "[slots].device_absent 格位 {slot} 的理由必须是非空字符串（缺席须显式申报理由，留空即红）"
        )),
    }
}
