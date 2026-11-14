//! 平台表顶面三事：七键封闭、schema_version、platform 名。

use super::*;

/// 顶面三事（B10 自 plat_parse 抽出）：七键封闭、schema_version=1、
/// platform 非空——点名文案逐字保留。
pub(super) fn rr3_plat_head(t: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    rr3_plat_keys(t, errs);
    rr3_plat_ver(t, errs);
    rr3_plat_name(t, table, errs);
}

/// 顶层七键封闭（未知键点名，文案逐字保留）。
fn rr3_plat_keys(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        rr3_plat_key_err(k, errs);
    }
}

/// 单个顶层键裁决：七键外点名（文案逐字保留）。
fn rr3_plat_key_err(k: &str, errs: &mut Vec<String>) {
    if ![
        "schema_version",
        "platform",
        "controller",
        "capabilities",
        "conflict",
        "construction",
        "controller_wordform",
    ]
    .contains(&k)
    {
        errs.push(format!(
            "平台表顶层未知键 {k}（只认 schema_version / platform / controller / capabilities / conflict / construction / controller_wordform）"
        ));
    }
}

/// schema_version：平台表自己的版本域（当前 1；破坏性变更才 bump）。
fn rr3_plat_ver(t: &toml::Table, errs: &mut Vec<String>) {
    let Some(v) = t.get("schema_version") else {
        errs.push("平台表缺 schema_version（必须为 1）".into());
        return;
    };
    let Value::Integer(v) = v else {
        errs.push(format!("平台表 schema_version 应为整数，收到 {v:?}"));
        return;
    };
    if *v != 1 {
        errs.push(format!("平台表 schema_version 必须为 1，收到 {v}"));
    }
}

/// platform：非空字符串（与 profiles [platform] 的平台标识同源，§4.6）。
fn rr3_plat_name(t: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let Some(v) = t.get("platform") else {
        errs.push("平台表缺 platform 键".into());
        return;
    };
    let Value::String(s) = v else {
        errs.push(format!("平台表 platform 应为字符串，收到 {v:?}"));
        return;
    };
    if s.trim().is_empty() {
        errs.push("平台表 platform 不得为空串".into());
        return;
    }
    table.platform = s.clone();
}
