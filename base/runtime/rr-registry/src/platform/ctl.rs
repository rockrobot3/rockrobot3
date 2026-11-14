//! [[controller]] 段：逐条收集、重复名点名与词表建映射。

use super::*;

/// `[[controller]]` 段：先逐条收集再建映射，重复名在此点名（词表一名一条）。
pub(super) fn rr3_plat_ctl(t: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let mut entries: Vec<(String, String, i64)> = Vec::new();
    rr3_ctl_collect(t, &mut entries, errs);
    for (n, k, ch) in entries {
        rr3_ctl_insert(table, n, k, ch, errs);
    }
}

/// 单条控制器入册：同名两册点名（词表一名一条）。
fn rr3_ctl_insert(
    table: &mut Rr3PlatformTable,
    n: String,
    k: String,
    ch: i64,
    errs: &mut Vec<String>,
) {
    if table.controllers.contains_key(&n) {
        errs.push(format!("controller {n} 重复（词表一名一条）"));
        return;
    }
    table.controllers.insert(
        n,
        Rr3ControllerEntry {
            kind: k,
            channels: ch,
        },
    );
}

/// `[[controller]]` 数组形状与逐条解析（空表 = 词表事故，缺段/非数组分述点名）。
fn rr3_ctl_collect(
    t: &toml::Table,
    entries: &mut Vec<(String, String, i64)>,
    errs: &mut Vec<String>,
) {
    let Some(v) = t.get("controller") else {
        errs.push("平台表缺 [[controller]] 段（在册控制器词表必填）".into());
        return;
    };
    let Value::Array(items) = v else {
        errs.push(format!(
            "平台表 controller 应为数组（[[controller]]），收到 {v:?}"
        ));
        return;
    };
    rr3_ctl_items(items, entries, errs);
}

/// `[[controller]]` 数组逐条：空表 = 词表事故（在册控制器词表为空 = ⑧-b 无键空间可判）。
fn rr3_ctl_items(
    items: &[Value],
    entries: &mut Vec<(String, String, i64)>,
    errs: &mut Vec<String>,
) {
    if items.is_empty() {
        errs.push(
            "平台表 [[controller]] 为空（在册控制器词表为空 = ⑧-b 无键空间可判，词表事故）".into(),
        );
        return;
    }
    for (i, item) in items.iter().enumerate() {
        rr3_ctl_item(item, i, entries, errs);
    }
}

/// 单条 `[[controller]]`：未知键 → name → kind（封闭七值）→ channels → 入暂存。
/// 文案与编号逐字保留（排障入口）。
fn rr3_ctl_item(
    item: &Value,
    i: usize,
    entries: &mut Vec<(String, String, i64)>,
    errs: &mut Vec<String>,
) {
    let Some(c) = item.as_table() else {
        errs.push(format!(
            "controller[{i}] 应为内联表 {{ name, kind, channels }}"
        ));
        return;
    };
    rr3_ctl_keys(c, i, errs);
    let name = rr3_ctl_name(c, i, errs);
    let kind = rr3_ctl_kind(c, i, errs);
    let channels = rr3_ctl_chans(c, i, errs);
    if let (Some(n), Some(k), Some(ch)) = (name, kind, channels) {
        entries.push((n, k, ch));
    }
}

/// `[[controller]]` 键词表核：只认 name / kind / channels（其余逐键点名）。
fn rr3_ctl_keys(c: &toml::Table, i: usize, errs: &mut Vec<String>) {
    for k in c.keys() {
        rr3_ctl_key_err(k, i, errs);
    }
}

/// 单个控制器键裁决（词表外点名）。
fn rr3_ctl_key_err(k: &str, i: usize, errs: &mut Vec<String>) {
    if !rr3_ctl_key_ok(k) {
        errs.push(format!(
            "controller[{i}] 未知键 {k}（只认 name / kind / channels）"
        ));
    }
}

/// 控制器键在册（只认 name / kind / channels）。
fn rr3_ctl_key_ok(k: &str) -> bool {
    k == "name" || k == "kind" || k == "channels"
}

/// `[[controller]]` 单条 name：非空字符串。
fn rr3_ctl_name(c: &toml::Table, i: usize, errs: &mut Vec<String>) -> Option<String> {
    let Some(n) = c.get("name").and_then(Value::as_str) else {
        errs.push(format!("controller[{i}].name 应为字符串"));
        return None;
    };
    if n.trim().is_empty() {
        errs.push(format!("controller[{i}].name 不得为空串"));
        return None;
    }
    Some(n.to_string())
}

/// `[[controller]]` 单条 kind：封闭七值。
fn rr3_ctl_kind(c: &toml::Table, i: usize, errs: &mut Vec<String>) -> Option<String> {
    let Some(k) = c.get("kind").and_then(Value::as_str) else {
        errs.push(format!("controller[{i}].kind 应为字符串"));
        return None;
    };
    if !RR3_PLATFORM_KINDS.contains(&k) {
        errs.push(format!(
            "controller[{i}].kind \"{k}\" 非法（封闭七值 {RR3_PLATFORM_KINDS:?}，扩域走契约级评审）"
        ));
        return None;
    }
    Some(k.to_string())
}

/// `[[controller]]` 单条 channels：非负整数（0 = 无通道维，须显式写）。
fn rr3_ctl_chans(c: &toml::Table, i: usize, errs: &mut Vec<String>) -> Option<i64> {
    let Some(v) = c.get("channels") else {
        errs.push(format!(
            "controller[{i}] 缺 channels（0 = 无通道维，须显式写）"
        ));
        return None;
    };
    let Value::Integer(n) = v else {
        errs.push(format!("controller[{i}].channels 应为整数，收到 {v:?}"));
        return None;
    };
    if *n < 0 {
        errs.push(format!("controller[{i}].channels 须非负，收到 {n}"));
        return None;
    }
    Some(*n)
}
