//! [[conflict]] 段：组名唯一、成员非空、why 必填与在册核对。

use super::*;

/// `conflict` 段：组名唯一、成员至少一侧、why 必填；controllers 成员须本表在册。
pub(super) fn rr3_plat_conf(t: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let mut groups_seen = BTreeSet::<String>::new();
    let Some(v) = t.get("conflict") else {
        errs.push(
            "平台表缺 conflict 键（无冲突也必须显式写 conflict = [] 并带'评审确认无冲突矩阵'注记，裁定②）"
                .into(),
        );
        return;
    };
    let Value::Array(items) = v else {
        errs.push(format!(
            "平台表 conflict 应为数组（[[conflict]] / []），收到 {v:?}"
        ));
        return;
    };
    rr3_conf_items(items, table, &mut groups_seen, errs);
}

/// `[[conflict]]` 数组逐条：组名唯一状态随遍历累积。
fn rr3_conf_items(
    items: &[Value],
    table: &mut Rr3PlatformTable,
    groups_seen: &mut BTreeSet<String>,
    errs: &mut Vec<String>,
) {
    for (i, item) in items.iter().enumerate() {
        rr3_conf_item(item, i, table, groups_seen, errs);
    }
}

/// 单条 `[[conflict]]`：未知键 → group → 成员集 → why → 组名唯一 → 入册。
/// 每条错误文案逐字保留（编号 i 是排障入口）。
fn rr3_conf_item(
    item: &Value,
    i: usize,
    table: &mut Rr3PlatformTable,
    groups_seen: &mut BTreeSet<String>,
    errs: &mut Vec<String>,
) {
    let Some(c) = item.as_table() else {
        errs.push(format!(
            "conflict[{i}] 应为内联表 {{ group, pins/controllers, why }}"
        ));
        return;
    };
    rr3_conf_keys(c, i, errs);
    let group = rr3_conf_group(c, i, errs);
    let (pins, controllers) = rr3_conf_members(c, i, &group, table, errs);
    let why = rr3_conf_why(c, i, errs);
    if let Some(g) = group {
        rr3_conf_register(
            Rr3ConflictDraft {
                i,
                group: g,
                pins,
                controllers,
                why: why.to_string(),
            },
            groups_seen,
            table,
            errs,
        );
    }
}

/// 键词表核：只认 group / pins / controllers / why（其余逐键点名）。
fn rr3_conf_keys(c: &toml::Table, i: usize, errs: &mut Vec<String>) {
    for k in c.keys() {
        rr3_conf_key_err(k, i, errs);
    }
}

/// 单个冲突键裁决（四键外点名）。
fn rr3_conf_key_err(k: &str, i: usize, errs: &mut Vec<String>) {
    if !["group", "pins", "controllers", "why"].contains(&k) {
        errs.push(format!(
            "conflict[{i}] 未知键 {k}（只认 group / pins / controllers / why）"
        ));
    }
}

/// 组名：非空字符串（空串/非串各自点名，返回 None 即不登记）。
fn rr3_conf_group(c: &toml::Table, i: usize, errs: &mut Vec<String>) -> Option<String> {
    let Some(g) = c.get("group").and_then(Value::as_str) else {
        errs.push(format!("conflict[{i}].group 应为字符串"));
        return None;
    };
    if g.trim().is_empty() {
        errs.push(format!("conflict[{i}].group 不得为空串"));
        return None;
    }
    Some(g.to_string())
}

/// 理由：进册必须带非空可读理由（缺失/空串点名，登记值取空串保持旧形状）。
fn rr3_conf_why<'a>(c: &'a toml::Table, i: usize, errs: &mut Vec<String>) -> &'a str {
    let Some(w) = c.get("why").and_then(Value::as_str) else {
        errs.push(format!("conflict[{i}] 缺 why（进册的冲突必须带可读理由）"));
        return "";
    };
    if w.trim().is_empty() {
        errs.push(format!("conflict[{i}].why 不得为空串"));
        return "";
    }
    w
}

/// conflict 单册解析产物（rr3_conf_item 五步的汇合点，入册前成型）。
struct Rr3ConflictDraft {
    i: usize,
    group: String,
    pins: BTreeSet<i64>,
    controllers: BTreeSet<String>,
    why: String,
}

/// 入册：成员集全空不登记（已是错误面）；组名唯一（同名两册点名）。
fn rr3_conf_register(
    d: Rr3ConflictDraft,
    groups_seen: &mut BTreeSet<String>,
    table: &mut Rr3PlatformTable,
    errs: &mut Vec<String>,
) {
    if rr3_sets_blank(&d.pins, &d.controllers) {
        return;
    }
    rr3_conf_dup_err(&d, groups_seen, errs);
    table.conflicts.push(Rr3ConflictEntry {
        group: d.group,
        pins: d.pins,
        controllers: d.controllers,
        why: d.why,
    });
}

/// 组名唯一（一组一条，互斥集不许同名两册）。
fn rr3_conf_dup_err(
    d: &Rr3ConflictDraft,
    groups_seen: &mut BTreeSet<String>,
    errs: &mut Vec<String>,
) {
    if !groups_seen.insert(d.group.clone()) {
        errs.push(format!(
            "conflict[{}] 组名 {} 重复（一组一条，互斥集不许同名两册）",
            d.i, d.group
        ));
    }
}

/// 冲突集成员：pins/controllers 两数组（缺省空集）；两集皆空即点名；controllers
/// 成员必须在本表 `[[controller]]` 在册（同文件真值，先立词表再引）。
fn rr3_conf_members(
    c: &toml::Table,
    i: usize,
    group: &Option<String>,
    table: &Rr3PlatformTable,
    errs: &mut Vec<String>,
) -> (BTreeSet<i64>, BTreeSet<String>) {
    let (pins, controllers) = rr3_conf_sets(c, i, group, errs);
    for ctl in &controllers {
        rr3_conf_ctl_err(ctl, i, table, errs);
    }
    (pins, controllers)
}

/// 单个 controllers 成员的在册性（同文件真值，先立词表再引）。
fn rr3_conf_ctl_err(ctl: &str, i: usize, table: &Rr3PlatformTable, errs: &mut Vec<String>) {
    if !table.controllers.contains_key(ctl) {
        errs.push(format!(
            "conflict[{i}] 的 controllers 成员 {ctl} 不在本表 [[controller]] 在册（同文件真值，先立词表再引）"
        ));
    }
}

/// 冲突集成员解析：pins/controllers 两数组（缺省空集，坏元素逐条点名），
/// 两集皆空即点名（点名不到任何成员的冲突集无判据对象）。
fn rr3_conf_sets(
    c: &toml::Table,
    i: usize,
    group: &Option<String>,
    errs: &mut Vec<String>,
) -> (BTreeSet<i64>, BTreeSet<String>) {
    let mut pins = BTreeSet::new();
    let mut controllers = BTreeSet::new();
    let pins_txt = format!("conflict[{i}].pins");
    let ctls_txt = format!("conflict[{i}].controllers");
    if let Some(v) = c.get("pins") {
        pins = rr3_pin_set_from(v, &pins_txt, errs);
    }
    if let Some(v) = c.get("controllers") {
        controllers = rr3_str_set_from(v, &ctls_txt, errs);
    }
    rr3_conf_blank_err(i, group, &pins, &controllers, errs);
    (pins, controllers)
}

/// 两集皆空即点名（点名不到任何成员的冲突集无判据对象）。
fn rr3_conf_blank_err(
    i: usize,
    group: &Option<String>,
    pins: &BTreeSet<i64>,
    controllers: &BTreeSet<String>,
    errs: &mut Vec<String>,
) {
    if rr3_sets_blank(pins, controllers) {
        errs.push(format!(
            "conflict[{i}]（组 {}）pins 与 controllers 均空：点名不到任何成员的冲突集无判据对象",
            group.as_deref().unwrap_or("")
        ));
    }
}
