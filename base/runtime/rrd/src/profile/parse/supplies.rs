//! [supplies] 段解读：看门狗/时序预算标量、子段分派与整数读取。

use super::super::model::*;
use super::budget::rr3_budget_into;
use super::domain::rr3_domains_parse;

/// [supplies]：必填段（看门狗三键 + 时序预算位三键，六键全必填、类型逐键检查、
/// 未知键点名）——这是 G6 档位一致性校验与 0.0.6 点验矩阵的输入事实。
pub(super) fn rr3_supplies_parse(
    root: &toml::Value,
    tier: &str,
    errs: &mut Vec<String>,
) -> Rr3Supplies {
    let mut supplies = Rr3Supplies::default();
    match root.get("supplies").and_then(toml::Value::as_table) {
        None => errs.push(
            "缺少 [supplies]（平台看门狗与时序预算声明；schema_version=2 起必填）".to_string(),
        ),
        Some(t) => {
            rr3_supplies_core(t, &mut supplies, errs);
            rr3_supplies_sub(t, &mut supplies, tier, errs);
        }
    }
    supplies
}

/// `[supplies]` 标量位：看门狗声明/时序预算/喂狗者 + 未知键点名。
fn rr3_supplies_core(t: &toml::Table, supplies: &mut Rr3Supplies, errs: &mut Vec<String>) {
    rr3_supplies_flags(t, supplies, errs);
    rr3_int_into(
        t,
        "watchdog_timeout_ms",
        &mut supplies.watchdog_timeout_ms,
        errs,
    );
    rr3_int_into(t, "tick_period_ms", &mut supplies.tick_period_ms, errs);
    rr3_int_into(t, "tick_jitter_ms", &mut supplies.tick_jitter_ms, errs);
    rr3_int_into(
        t,
        "safety_core_interval_ms",
        &mut supplies.safety_core_interval_ms,
        errs,
    );
    // domain / budget 是子段键，各自成块解析（第七供给位 / 校验⑩ 分母）
    rr3_supplies_keys(t, errs);
}

fn rr3_supplies_keys(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        if !rr3_sup_key_ok(k) {
            errs.push(format!(
                "[supplies] 未知键 {k}（只认 {}）",
                RR3_SUPPLIES_KEYS.join("/")
            ));
        }
    }
}

/// supplies 键白名单：两个子段键（domain/budget）不在 [`RR3_SUPPLIES_KEYS`] 里也照收。
fn rr3_sup_key_ok(k: &str) -> bool {
    k == "domain" || k == "budget" || RR3_SUPPLIES_KEYS.contains(&k)
}

/// 看门狗有无与非空喂者两键（B9 自 supplies_core 抽出）：缺/型错各自点名。
fn rr3_supplies_flags(t: &toml::Table, supplies: &mut Rr3Supplies, errs: &mut Vec<String>) {
    if let Some(b) = t.get("watchdog_present").and_then(toml::Value::as_bool) {
        supplies.watchdog_present = b;
    } else {
        errs.push("supplies.watchdog_present 缺失或不是布尔".to_string());
    }
    if let Some(s) = t
        .get("watchdog_feeder")
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
    {
        supplies.watchdog_feeder = s.to_string();
    } else {
        errs.push("supplies.watchdog_feeder 缺失或不是非空字符串".to_string());
    }
}

/// 整数键落位（B9 自 supplies_core 抽出）：一条键一条点名，字段引用直给。
fn rr3_int_into(t: &toml::Table, key: &str, slot: &mut i64, errs: &mut Vec<String>) {
    match supplies_int(t.get(key), key) {
        Ok(v) => *slot = v,
        Err(e) => errs.push(e),
    }
}

fn rr3_supplies_sub(
    t: &toml::Table,
    supplies: &mut Rr3Supplies,
    tier: &str,
    errs: &mut Vec<String>,
) {
    // 缺数组 = 红——schema 3 装载期校验⑥-a 需要名单源。
    match t.get("domain").and_then(toml::Value::as_array) {
        Some(items) => supplies.domains = rr3_domains_parse(items, errs),
        None => errs.push(
            "缺少 [[supplies.domain]]（电力网络声明，第七供给位；schema_version=3 起必填，至少声明一域）"
                .to_string(),
        ),
    }
    rr3_budget_into(t, supplies, tier, errs);
}

/// [supplies] 的整数键读取：缺失或类型不合同一条错误点名（运维一次改对）
pub(super) fn supplies_int(v: Option<&toml::Value>, key: &str) -> Result<i64, String> {
    v.and_then(toml::Value::as_integer)
        .ok_or(format!("supplies.{key} 缺失或不是整数"))
}
