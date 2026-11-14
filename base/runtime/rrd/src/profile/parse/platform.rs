//! [platform] 段解读：tier 三档与 chip 平台事实表外键的必填判据。

use super::super::model::*;

/// [platform]：必填段、单键 tier（ADR-0018 修订 1 ④ 七段之一）。tier 取值域
/// host/rtos/bare 三档；chip = 平台事实表外键（§4.6，校验③④⑤⑧的判据前件选册
/// ——contracts/platform/<chip>.toml，批N1 基-22），非空必填：芯片是装机事实，
/// 留空 = ⑤⑧ 判不了 = fail-closed（同"留空即红"纪律），未知键逐条点名。
pub(super) fn rr3_platform_parse(root: &toml::Value, errs: &mut Vec<String>) -> (String, String) {
    let Some(t) = root.get("platform").and_then(toml::Value::as_table) else {
        errs.push("缺少 [platform]（tier = host/rtos/bare；schema_version=2 起必填）".to_string());
        return (String::new(), String::new());
    };
    let tier = rr3_plat_tier(t, errs);
    let chip = rr3_plat_chip(t, errs);
    rr3_plat_unknown(t, errs);
    (tier, chip)
}

/// [platform] 未知键点名（只认 tier / chip）。
fn rr3_plat_unknown(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        if k != "tier" && k != "chip" {
            errs.push(format!("[platform] 未知键 {k}（只认 tier / chip）"));
        }
    }
}

/// [platform].tier：封闭三档（host/rtos/bare）；缺键或出域逐条点名。
fn rr3_plat_tier(t: &toml::Table, errs: &mut Vec<String>) -> String {
    let Some(v) = t.get("tier") else {
        errs.push("[platform] 缺 tier（host/rtos/bare）".to_string());
        return String::new();
    };
    let other = v.as_str();
    if let Some(s) = other.filter(|s| RR3_PLATFORM_TIERS.contains(s)) {
        return s.to_string();
    }
    errs.push(format!(
        "platform.tier 必须是 host/rtos/bare 之一，收到 {other:?}"
    ));
    String::new()
}

/// [platform].chip：平台事实表外键，非空必填（留空 = ③④⑤⑧ 判不了，fail-closed）。
fn rr3_plat_chip(t: &toml::Table, errs: &mut Vec<String>) -> String {
    let Some(v) = t.get("chip") else {
        errs.push(
            "[platform] 缺 chip（平台事实表外键——校验③④⑤⑧按它选 contracts/platform/<chip>.toml，留空即红）"
                .to_string(),
        );
        return String::new();
    };
    let Some(s) = v.as_str() else {
        errs.push(format!("[platform].chip 应为字符串，收到 {v:?}"));
        return String::new();
    };
    if s.trim().is_empty() {
        errs.push(
            "[platform].chip 不得为空串（平台事实表外键，contracts/platform/<chip>.toml 选册）"
                .to_string(),
        );
        return String::new();
    }
    s.to_string()
}
