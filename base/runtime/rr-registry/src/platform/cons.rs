//! [construction] 与 [wordform] 段：具名构造三元键与词形。

use super::*;

/// 冲突集两成员皆空（点名不到任何成员 = 无判据对象）。
pub(super) fn rr3_sets_blank(pins: &BTreeSet<i64>, controllers: &BTreeSet<String>) -> bool {
    pins.is_empty() && controllers.is_empty()
}

/// `[construction]` 段：`controller:role:instance` 三元键，controller 须本表在册。
pub(super) fn rr3_plat_cons(t: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let Some(cons) = rr3_cons_tbl(t, errs) else {
        return;
    };
    for (k, v) in cons {
        rr3_cons_item(k, v, table, errs);
    }
}

/// `[construction]` 段形状：段缺/类型错各自点名回 None。
fn rr3_cons_tbl<'a>(t: &'a toml::Table, errs: &mut Vec<String>) -> Option<&'a toml::Table> {
    let Some(v) = t.get("construction") else {
        errs.push("平台表缺 [construction] 段（⑧ 对账前件；无绑定形态显式写空表，裁定①）".into());
        return None;
    };
    let Value::Table(cons) = v else {
        errs.push(format!(
            "平台表 construction 应为表（[construction]），收到 {v:?}"
        ));
        return None;
    };
    Some(cons)
}

/// 单条 construction：三元键（controller/role 非空、controller 在册）+ 非空路径。
fn rr3_cons_item(k: &str, v: &Value, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    if !rr3_cons_key(k, table, errs) {
        return;
    }
    rr3_cons_val(k, v, table, errs);
}

/// 构造路径值：非空字符串入表，空串/非串各自点名（文案逐字保留）。
fn rr3_cons_val(k: &str, v: &Value, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let Some(p) = v.as_str() else {
        errs.push(format!(
            "construction[\"{k}\"] 应为字符串（平台侧构造路径，§4.5），收到 {v:?}"
        ));
        return;
    };
    if p.trim().is_empty() {
        errs.push(format!("construction[\"{k}\"] 构造路径不得为空串"));
        return;
    }
    table.construction.insert(k.to_string(), p.to_string());
}

/// 三元键形状与在册性：`controller:role:instance`，前两段非空且 controller 在册。
/// 返回 false = 该键已拒，不登记。
fn rr3_cons_key(k: &str, table: &Rr3PlatformTable, errs: &mut Vec<String>) -> bool {
    let Some(segs) = rr3_cons_segs(k, errs) else {
        return false;
    };
    if !rr3_cons_segs_ok(&segs) {
        errs.push(format!(
            "construction 键 \"{k}\" 的 controller/role 段不得为空（instance 段可空）"
        ));
        return false;
    }
    rr3_cons_ctl_ok(k, segs[0], table, errs)
}

/// controller 段须在本表 `[[controller]]` 在册（同文件真值，先立词表再引）。
fn rr3_cons_ctl_ok(k: &str, ctl: &str, table: &Rr3PlatformTable, errs: &mut Vec<String>) -> bool {
    if table.controllers.contains_key(ctl) {
        return true;
    }
    errs.push(format!(
        "construction 键 \"{k}\" 的 controller 段 {ctl} 不在本表 [[controller]] 在册（同文件真值，先立词表再引）"
    ));
    false
}

/// 三元键前两段非空（instance 段允许留空）。
fn rr3_cons_segs_ok(segs: &[&str]) -> bool {
    !segs[0].is_empty() && !segs[1].is_empty()
}

/// 三元键切段：非三段即点名回 None（键已拒，不登记）。
fn rr3_cons_segs<'a>(k: &'a str, errs: &mut Vec<String>) -> Option<Vec<&'a str>> {
    let segs: Vec<&str> = k.split(':').collect();
    if segs.len() != 3 {
        errs.push(format!(
            "construction 键 \"{k}\" 不是三元串（⑧-a：controller:role:instance，非 per_instance 角色第三段留空）"
        ));
        return None;
    }
    Some(segs)
}

/// `[controller_wordform]` 段：封闭四键、值 = 非空前缀；空表 = 显式无词形派生面。
pub(super) fn rr3_plat_wf(t: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let Some(wf) = rr3_wf_tbl(t, errs) else {
        return;
    };
    for (k, v) in wf {
        rr3_wf_item(k, v, table, errs);
    }
}

/// `[controller_wordform]` 段形状：段缺/类型错各自点名回 None。
fn rr3_wf_tbl<'a>(t: &'a toml::Table, errs: &mut Vec<String>) -> Option<&'a toml::Table> {
    let Some(v) = t.get("controller_wordform") else {
        errs.push(
            "平台表缺 [controller_wordform] 段（host 词形派生表，实-6 (a)；无词形派生面须显式写空表并带注记，裁定①）"
                .into(),
        );
        return None;
    };
    let Value::Table(wf) = v else {
        errs.push(format!(
            "平台表 controller_wordform 应为表（[controller_wordform]），收到 {v:?}"
        ));
        return None;
    };
    Some(wf)
}

/// 单条词形：键在册四键内、值为非空前缀。
fn rr3_wf_item(k: &str, v: &Value, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    if !RR3_PLATFORM_WORDFORM_KEYS.contains(&k) {
        errs.push(format!(
            "[controller_wordform] 未知键 {k}（只认词形派生四键 {RR3_PLATFORM_WORDFORM_KEYS:?}；physical/channel/addr 类派生不经词形）"
        ));
        return;
    }
    rr3_wf_val(k, v, table, errs);
}

/// 词形值：非空前缀入表（空串/非串各自点名，文案逐字保留）。
fn rr3_wf_val(k: &str, v: &Value, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let Some(p) = v.as_str() else {
        errs.push(format!(
            "[controller_wordform].{k} 应为字符串前缀，收到 {v:?}"
        ));
        return;
    };
    if p.trim().is_empty() {
        errs.push(format!(
            "[controller_wordform].{k} 词形前缀不得为空串（无词形须删键走空表显式）"
        ));
        return;
    }
    table
        .controller_wordform
        .insert(k.to_string(), p.to_string());
}
