//! [bindings] 与 [enabled_slots] 段解读：格位→模块与模块→格位集的收册。

use std::collections::BTreeMap;

/// [bindings]：非空表、每格非空字符串值。[os] 段运行期不读，解析时无视（见文件头）。
pub(super) fn rr3_bindings_parse(
    root: &toml::Value,
    errs: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut bindings = BTreeMap::new();
    match root.get("bindings").and_then(toml::Value::as_table) {
        None => errs
            .push("缺少 [bindings]（body/cognition 格位 → 模块；可为空表但段必须在）".to_string()),
        Some(t) => rr3_bind_rows(t, &mut bindings, errs),
    }
    bindings
}

/// 逐格位收册：非空串入表，其余逐条点名。
fn rr3_bind_rows(t: &toml::Table, bindings: &mut BTreeMap<String, String>, errs: &mut Vec<String>) {
    for (slot, v) in t {
        match v.as_str() {
            Some(m) if !m.is_empty() => {
                bindings.insert(slot.clone(), m.to_string());
            }
            _ => errs.push(format!("bindings.{slot} 应为非空字符串（模块名）")),
        }
    }
}

/// [enabled_slots]：非空表、每值非空字符串数组。enabled_slots 决定起谁（Q3 裁定），
/// 空表 = 无可起模块。
pub(super) fn rr3_enabled_parse(
    root: &toml::Value,
    errs: &mut Vec<String>,
) -> BTreeMap<String, Vec<String>> {
    let mut enabled_slots = BTreeMap::new();
    match root.get("enabled_slots").and_then(toml::Value::as_table) {
        None => errs.push("缺少 [enabled_slots]（模块 → 本形态启用的格位集合）".to_string()),
        Some(t) => rr3_enabled_rows(t, &mut enabled_slots, errs),
    }
    enabled_slots
}

/// 逐模块格位集：空表点名，值须为字符串数组。
fn rr3_enabled_rows(
    t: &toml::Table,
    out: &mut BTreeMap<String, Vec<String>>,
    errs: &mut Vec<String>,
) {
    if t.is_empty() {
        errs.push("[enabled_slots] 为空：enabled_slots 决定起谁，空装配单没有可起模块".to_string());
    }
    for (module, v) in t {
        match v.as_array() {
            Some(items) => {
                let slots = rr3_slot_items(module, items, errs);
                out.insert(module.clone(), slots);
            }
            None => errs.push(format!("enabled_slots.{module} 应为字符串数组")),
        }
    }
}

/// 单模块格位集：每项必须是非空字符串，逐条点名。
fn rr3_slot_items(module: &str, items: &[toml::Value], errs: &mut Vec<String>) -> Vec<String> {
    let mut slots = Vec::new();
    for item in items {
        rr3_slot_item(module, item, &mut slots, errs);
    }
    slots
}

/// 单格位项：非空字符串收册，其余逐条点名（文案只报一次模块名）。
fn rr3_slot_item(
    module: &str,
    item: &toml::Value,
    slots: &mut Vec<String>,
    errs: &mut Vec<String>,
) {
    match item.as_str() {
        Some(s) if !s.is_empty() => slots.push(s.to_string()),
        _ => errs.push(format!(
            "enabled_slots.{module} 的每一项都必须是非空字符串（格位名）"
        )),
    }
}
