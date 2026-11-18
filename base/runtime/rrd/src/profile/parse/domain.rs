//! [[supplies.domain]] 段解读：键表驱动的逐域取数与未知键点名。

use super::super::model::*;
use super::supplies::supplies_int;

/// [[supplies.domain]] 逐域解析（段名沿用 supplies.domains 文案口径）
pub(super) fn rr3_domains_parse(items: &[toml::Value], errs: &mut Vec<String>) -> Vec<Rr3Domain> {
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let label = format!("supplies.domains[{i}]");
        let Some(dt) = item.as_table() else {
            errs.push(format!("{label} 必须是 [[supplies.domain]] 表"));
            continue;
        };
        out.push(rr3_domain_one(dt, &label, errs));
    }
    out
}

/// 单域解析：键表驱动（[`RR3_DOMAIN_KEYS`]）逐键取数，未知键点名。
fn rr3_domain_one(dt: &toml::Table, label: &str, errs: &mut Vec<String>) -> Rr3Domain {
    let mut d = Rr3Domain::default();
    for key in RR3_DOMAIN_KEYS {
        let path = format!("{label}.{key}");
        match key {
            "name" | "sense" | "current_sense" | "switch" | "evidence" => {
                rr3_domain_str(&mut d, key, dt.get(key), &path, errs)
            }
            "switchable" | "armed" => rr3_domain_bool(&mut d, key, dt.get(key), &path, errs),
            _ => rr3_domain_int(&mut d, key, dt.get(key), &path, errs),
        }
    }
    rr3_domain_unknown(dt, label, errs);
    d
}

/// 域表未知键点名：只认 [`RR3_DOMAIN_KEYS`]。
fn rr3_domain_unknown(dt: &toml::Table, label: &str, errs: &mut Vec<String>) {
    for k in dt.keys() {
        if !RR3_DOMAIN_KEYS.contains(&k.as_str()) {
            errs.push(format!(
                "{label} 未知键 {k}（只认 {}）",
                RR3_DOMAIN_KEYS.join("/")
            ));
        }
    }
}

/// 字符串类域键：非空即收；缺失/空串同文案。
fn rr3_domain_str(
    d: &mut Rr3Domain,
    key: &str,
    v: Option<&toml::Value>,
    path: &str,
    errs: &mut Vec<String>,
) {
    match v.and_then(toml::Value::as_str) {
        Some(s) if !s.is_empty() => rr3_domain_set(d, key, s),
        _ => errs.push(format!("{path} 缺失或不是非空字符串")),
    }
}

/// 字符串值按键落位。
fn rr3_domain_set(d: &mut Rr3Domain, key: &str, s: &str) {
    let v = s.to_string();
    let mut fields = [
        ("name", &mut d.name),
        ("sense", &mut d.sense),
        ("current_sense", &mut d.current_sense),
        ("evidence", &mut d.evidence),
    ];
    match fields.iter_mut().find(|(k, _)| *k == key) {
        Some((_, slot)) => **slot = v,
        None => d.switch = v,
    }
}

/// 布尔类域键：缺失/非布尔同文案。
fn rr3_domain_bool(
    d: &mut Rr3Domain,
    key: &str,
    v: Option<&toml::Value>,
    path: &str,
    errs: &mut Vec<String>,
) {
    match (key, v.and_then(toml::Value::as_bool)) {
        ("switchable", Some(b)) => d.switchable = b,
        ("armed", Some(b)) => d.armed = b,
        _ => errs.push(format!("{path} 缺失或不是布尔")),
    }
}

/// 数值类域键：经 [`supplies_int`] 校验，按键落位。
fn rr3_domain_int(
    d: &mut Rr3Domain,
    key: &str,
    v: Option<&toml::Value>,
    path: &str,
    errs: &mut Vec<String>,
) {
    let Ok(n) = supplies_int(v, path).map_err(|e| errs.push(e)) else {
        return;
    };
    let mut fields = [
        ("nominal_mv", &mut d.nominal_mv),
        ("mild_mv", &mut d.mild_mv),
        ("severe_mv", &mut d.severe_mv),
        ("current_cap_ma", &mut d.current_cap_ma),
    ];
    match fields.iter_mut().find(|(k, _)| *k == key) {
        Some((_, slot)) => **slot = n,
        None => d.max_age_ms = n,
    }
}
