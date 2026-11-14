//! [runtime] 段解读：日志环容量、HAL 后端与三个可选后端覆盖键。

use super::super::model::*;

/// [runtime] 段解析产物（五键）
pub(super) struct Rr3RuntimeParsed {
    pub(super) log_ring_cap: usize,
    pub(super) hal_backend: String,
    pub(super) audio_backend: Option<String>,
    pub(super) servo_backend: Option<String>,
    pub(super) display_backend: Option<String>,
}

/// 后端名可选覆盖键：取值域同 hal_backend，不认即点名（组合档归属键）
fn rr3_backend_opt(t: &toml::Table, key: &str, errs: &mut Vec<String>) -> Option<String> {
    let v = t.get(key).and_then(toml::Value::as_str)?;
    if RR3_HAL_BACKEND_LEGAL.contains(&v) {
        Some(v.to_string())
    } else {
        errs.push(format!(
            "runtime.{key} 取值不认: {v}（允许 {}）",
            RR3_HAL_BACKEND_LEGAL.join("/")
        ));
        None
    }
}

/// `[runtime].log_ring_cap`：正整数且有上界（环按行常驻内存，超界随日志量吃光
/// 板载内存）；缺/非整数/非正/超界逐条点名，坏值一律留 0（装载红在前，值不外泄）。
fn rr3_rt_cap(t: &toml::Table, errs: &mut Vec<String>) -> usize {
    let Some(v) = t.get("log_ring_cap").and_then(toml::Value::as_integer) else {
        errs.push("runtime.log_ring_cap 缺失或不是整数（日志环容量，行数；批N5 件二）".to_string());
        return 0;
    };
    if (1..=RR3_LOG_CAP_MAX).contains(&v) {
        return v as usize;
    }
    errs.push(rr3_cap_bad(v));
    0
}

/// 环容量坏值点名（超上界 / 非正各一句，调用方已排除 0 与负数之外的分支）。
fn rr3_cap_bad(v: i64) -> String {
    if v > 0 {
        format!(
            "runtime.log_ring_cap = {v} 超上界 {RR3_LOG_CAP_MAX}（日志环按行常驻内存，超界会随日志量吃光板载内存）"
        )
    } else {
        format!("runtime.log_ring_cap = {v} 须为正整数（日志环容量，0 = 运维留痕无环）")
    }
}

/// `[runtime].hal_backend`：P0-3 根因门——必填且须在白名单内。后端选择是部署
/// 事实，必须进装配单显式数据，不靠隐式缺省（漏 env 静默落 sim = 生产假验证）。
fn rr3_rt_hal(t: &toml::Table, errs: &mut Vec<String>) -> String {
    let Some(v) = t.get("hal_backend").and_then(toml::Value::as_str) else {
        errs.push(
            "runtime.hal_backend 缺失（P0-3：后端选择必须进装配单显式数据，不靠隐式缺省）"
                .to_string(),
        );
        return String::new();
    };
    if RR3_HAL_BACKEND_LEGAL.contains(&v) {
        return v.to_string();
    }
    errs.push(format!(
        "runtime.hal_backend 取值不认: {v}（允许 {}）",
        RR3_HAL_BACKEND_LEGAL.join("/")
    ));
    String::new()
}

/// `[runtime]` 未知键点名（只认 [`RR3_RUNTIME_KEYS`]，拼错键不静默当缺省）。
fn rr3_rt_unknown(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        if !RR3_RUNTIME_KEYS.contains(&k.as_str()) {
            errs.push(format!(
                "[runtime] 未知键 {k}（只认 {}）",
                RR3_RUNTIME_KEYS.join("/")
            ));
        }
    }
}

/// [runtime]：必填段（schema 5 起必填，批N5 件二 F-30——"环容量 ∈ 装配单键集，
/// 装载期缺即红"与 F-6 缺失语义同一列）。键序：log_ring_cap → hal_backend →
/// 三个可选覆盖键 → 未知键；错误追加序即此序（运维逐条改）。
pub(super) fn rr3_runtime_parse(root: &toml::Value, errs: &mut Vec<String>) -> Rr3RuntimeParsed {
    let mut out = Rr3RuntimeParsed {
        log_ring_cap: 0,
        hal_backend: String::new(),
        audio_backend: None,
        servo_backend: None,
        display_backend: None,
    };
    let Some(t) = root.get("runtime").and_then(toml::Value::as_table) else {
        errs.push("缺少 [runtime]（rrd 运行参数；schema_version=5 起必填，批N5 件二）".to_string());
        return out;
    };
    out.log_ring_cap = rr3_rt_cap(t, errs);
    out.hal_backend = rr3_rt_hal(t, errs);
    // 三个可选覆盖键：不设 → 对应能力面回落全局 hal_backend
    out.audio_backend = rr3_backend_opt(t, "audio_backend", errs);
    out.servo_backend = rr3_backend_opt(t, "servo_backend", errs);
    out.display_backend = rr3_backend_opt(t, "display_backend", errs);
    rr3_rt_unknown(t, errs);
    out
}
