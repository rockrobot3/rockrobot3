// SPDX-License-Identifier: Apache-2.0
//! 装配与契约两个只读读面的渲染（续轮十五）：`rr3.ctl.wiring` /
//! `rr3.ctl.contracts` 的应答在这里成人读表。判据真值在 rrd 的装配/契约面，
//! 本文件只透传；`available:false`（`--manifest` 调试路径）如实报不可得。

use serde_json::Value;

use super::{cell, render_table, rr3_ctl_result};

/// 快照未在册（`available:false`，`--manifest` 调试路径）的统一渲染：如实报
/// 不可得与原因，不造表（与 rrd 侧"不拿 0 冒充"同一口径）。
fn format_unavailable(result: &Value) -> String {
    format!("不可得：{}\n", cell(result, "reason"))
}

/// `available` 为 true 才算快照在册；缺字段按不可得处理，不假装有数据。
fn rr3_available(result: &Value) -> bool {
    result.get("available").and_then(Value::as_bool) == Some(true)
}

/// 取 `result.<key>` 的对象数组（缺键/非数组即形状错，fail-closed）。
fn rr3_entries<'a>(result: &'a Value, key: &str) -> Result<&'a [Value], String> {
    result
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("应答缺 result.{key} 数组"))
}

/// 一段表：标题带条数；空段只打 `（无）`，不输出只有表头的噪声。
fn rr3_section(out: &mut String, title: &str, header: &[&str], rows: &[Vec<String>]) {
    out.push_str(&format!("{title}（{}）\n", rows.len()));
    if rows.is_empty() {
        out.push_str("（无）\n");
    } else {
        out.push_str(&render_table(header, rows));
    }
}

/// 字符串数组按逗号拼接（角色的 requires/derives）；空数组显示 `-`，坏形状 `?`。
fn cell_join(v: &Value, key: &str) -> String {
    let Some(items) = v.get(key).and_then(Value::as_array) else {
        return "?".to_string();
    };
    if items.is_empty() {
        return "-".to_string();
    }
    items
        .iter()
        .map(|i| i.as_str().unwrap_or("?"))
        .collect::<Vec<_>>()
        .join(",")
}

/// 布尔渲染：`true`/`false`，缺字段 `?`（不与"假"混用）。
fn cell_bool(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_bool)
        .map_or_else(|| "?".to_string(), |b| b.to_string())
}

/// 判果行（wiring/contracts 共用）：RESULT 列 `ok` / `FAIL` / `?`。
fn rr3_check_rows(result: &Value) -> Result<Vec<Vec<String>>, String> {
    Ok(rr3_entries(result, "checks")?
        .iter()
        .map(|c| {
            let pass = match c.get("pass").and_then(Value::as_bool) {
                Some(true) => "ok".to_string(),
                Some(false) => "FAIL".to_string(),
                None => "?".to_string(),
            };
            vec![cell(c, "item"), pass, cell(c, "reason")]
        })
        .collect())
}

/// 通过数（判果行第二列）。
fn rr3_pass_count(rows: &[Vec<String>]) -> usize {
    rows.iter()
        .filter(|r| r.get(1).map(String::as_str) == Some("ok"))
        .count()
}

/// `wiring` 各段表头与行映射（段序 = 引脚 → 供电域 → 接线 → 轴 → 角色 →
/// 交叉判据）；表头是渲染契约，行映射只取字段、不改判据真值。
const RR3_PIN_HEAD: [&str; 12] = [
    "PIN_ID",
    "ROLE",
    "INSTANCE",
    "CONTROLLER",
    "CHANNEL",
    "PHYSICAL",
    "DIRECTION",
    "RAIL",
    "ADDR",
    "DEVICE",
    "FAILSAFE",
    "MUX_GROUP",
];
const RR3_DOM_HEAD: [&str; 12] = [
    "NAME",
    "NOMINAL_MV",
    "MILD_MV",
    "SEVERE_MV",
    "CAP_MA",
    "SENSE",
    "CURRENT_SENSE",
    "SWITCH",
    "SWITCHABLE",
    "ARMED",
    "MAX_AGE_MS",
    "EVIDENCE",
];
const RR3_WIRE_HEAD: [&str; 9] = [
    "CONNECTOR",
    "HDR_PIN",
    "SIGNAL",
    "NET",
    "FEEDS",
    "MATE",
    "KEYED",
    "RATED_A",
    "GND",
];
const RR3_AXIS_HEAD: [&str; 13] = [
    "PIN",
    "CENTER",
    "LO",
    "HI",
    "DIR",
    "SLEW",
    "SETTLE_MS",
    "HOLD",
    "CEIL_MA",
    "WARMUP",
    "HARD_STOP",
    "CALIBRATED",
    "SOURCE",
];
const RR3_ROLE_HEAD: [&str; 5] = ["NAME", "SLOT", "DIRECTION", "REQUIRES", "DERIVES"];

fn rr3_pin_rows(result: &Value) -> Result<Vec<Vec<String>>, String> {
    Ok(rr3_entries(result, "pins")?
        .iter()
        .map(|p| {
            vec![
                cell(p, "pin_id"),
                cell(p, "role"),
                cell(p, "instance"),
                cell(p, "controller"),
                cell(p, "channel"),
                cell(p, "physical"),
                cell(p, "direction"),
                cell(p, "rail"),
                cell(p, "addr"),
                cell(p, "device"),
                cell_bool(p, "failsafe"),
                cell(p, "mux_group"),
            ]
        })
        .collect())
}

fn rr3_dom_rows(result: &Value) -> Result<Vec<Vec<String>>, String> {
    Ok(rr3_entries(result, "domains")?
        .iter()
        .map(|d| {
            vec![
                cell(d, "name"),
                cell(d, "nominal_mv"),
                cell(d, "mild_mv"),
                cell(d, "severe_mv"),
                cell(d, "current_cap_ma"),
                cell(d, "sense"),
                cell(d, "current_sense"),
                cell(d, "switch"),
                cell_bool(d, "switchable"),
                cell_bool(d, "armed"),
                cell(d, "max_age_ms"),
                cell(d, "evidence"),
            ]
        })
        .collect())
}

fn rr3_wire_rows(result: &Value) -> Result<Vec<Vec<String>>, String> {
    Ok(rr3_entries(result, "wires")?
        .iter()
        .map(|w| {
            vec![
                cell(w, "connector"),
                cell(w, "hdr_pin"),
                cell(w, "signal"),
                cell(w, "net"),
                cell(w, "feeds"),
                cell(w, "mate"),
                cell_bool(w, "keyed"),
                cell(w, "rated_a"),
                cell_bool(w, "gnd_common"),
            ]
        })
        .collect())
}

fn rr3_axis_rows(result: &Value) -> Result<Vec<Vec<String>>, String> {
    Ok(rr3_entries(result, "axis")?
        .iter()
        .map(|a| {
            vec![
                cell(a, "pin"),
                cell(a, "center_deg"),
                cell(a, "lo_deg"),
                cell(a, "hi_deg"),
                cell(a, "dir_sign"),
                cell(a, "slew_deg_s"),
                cell(a, "settle_ms"),
                cell(a, "hold"),
                cell(a, "current_ceil_ma"),
                cell(a, "warmup_ms"),
                cell(a, "hard_stop"),
                cell_bool(a, "calibrated"),
                cell(a, "source"),
            ]
        })
        .collect())
}

fn rr3_role_rows(result: &Value) -> Result<Vec<Vec<String>>, String> {
    Ok(rr3_entries(result, "roles")?
        .iter()
        .map(|r| {
            vec![
                cell(r, "name"),
                cell(r, "slot"),
                cell(r, "direction"),
                cell_join(r, "requires"),
                cell_join(r, "derives"),
            ]
        })
        .collect())
}

/// `wiring` 固定五段（引脚 → 供电域 → 接线 → 轴 → 角色）按表渲染。
fn rr3_wiring_std(out: &mut String, result: &Value) -> Result<(), String> {
    rr3_section(out, "PINS", &RR3_PIN_HEAD, &rr3_pin_rows(result)?);
    rr3_section(out, "DOMAINS", &RR3_DOM_HEAD, &rr3_dom_rows(result)?);
    rr3_section(out, "WIRES", &RR3_WIRE_HEAD, &rr3_wire_rows(result)?);
    rr3_section(out, "AXIS", &RR3_AXIS_HEAD, &rr3_axis_rows(result)?);
    rr3_section(out, "ROLES", &RR3_ROLE_HEAD, &rr3_role_rows(result)?);
    Ok(())
}

/// `wiring` 装配快照读口（批N4 件九笔4 实装，续轮十五补 CLI）：段序 = 引脚 →
/// 供电域 → 接线 → 轴 → 角色 → 交叉判据；判据真值在 rrd 的装配面，CLI 只透传。
pub(super) fn format_wiring(envelope: &Value) -> Result<String, String> {
    let result = rr3_ctl_result(envelope)?;
    if !rr3_available(result) {
        return Ok(format_unavailable(result));
    }
    let mut out = String::new();
    rr3_wiring_std(&mut out, result)?;
    let checks = rr3_check_rows(result)?;
    rr3_section(
        &mut out,
        &format!(
            "CHECKS（{}/{} 通过）",
            rr3_pass_count(&checks),
            checks.len()
        ),
        &["ITEM", "RESULT", "REASON"],
        &checks,
    );
    Ok(out)
}

/// `contracts` 契约审计读口（批N5 件五笔4 实装，续轮十五补 CLI）：四册账 →
/// 真值集账 → 自检判果；sha256 全量给出，供与装机清单逐字节对账。
pub(super) fn format_contracts(envelope: &Value) -> Result<String, String> {
    let result = rr3_ctl_result(envelope)?;
    if !rr3_available(result) {
        return Ok(format_unavailable(result));
    }
    let mut out = String::new();
    rr3_section(
        &mut out,
        "CONTRACTS",
        &["FILE", "BYTES", "SHA256"],
        &rr3_entries(result, "contracts")?
            .iter()
            .map(|c| vec![cell(c, "file"), cell(c, "bytes"), cell(c, "sha256")])
            .collect::<Vec<_>>(),
    );
    rr3_section(
        &mut out,
        "TRUTH_SET",
        &["MEMBER", "BYTES", "SHA256"],
        &rr3_entries(result, "truth_set")?
            .iter()
            .map(|t| vec![cell(t, "member"), cell(t, "bytes"), cell(t, "sha256")])
            .collect::<Vec<_>>(),
    );
    let checks = rr3_check_rows(result)?;
    rr3_section(
        &mut out,
        &format!(
            "CHECKS（{}/{} 通过）",
            rr3_pass_count(&checks),
            checks.len()
        ),
        &["ITEM", "RESULT", "REASON"],
        &checks,
    );
    Ok(out)
}
