//! 自检应答族：并列五腿取数与 verdict 判定（判据纯函数化）。

use std::time::Duration;

use serde_json::{json, Value};

use super::super::server::{rr3_ctl_query, Rr3PanelAction, Rr3PanelShared, Rr3SelftestExpect};
use super::act::rr3_panel_act;
use super::Rr3PanelReply;

/// ctl 应答里的判据对象数组（wiring/contracts 腿共用）：缺键或类型不对都当空表，
/// 由空表腿与逐条转述兜底
fn checks_of(v: &Value) -> Vec<Value> {
    v.get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// 出厂自检：并列五腿（speaker/relay/face/wiring/contracts）各答各展示，
/// 无依赖链、单项失败不连锁（不是编排——brain 才是唯一编排者，ADR-0007）。
/// 逐腿 `{leg, pass, reason?}`，pass = pass|fail|n-a 三态；失败腿如实报 fail
/// 不折算通过。verdict 判定归 [`rr3_selftest_verdict`]（预期表等值，纯函数化）。
pub(super) fn rr3_exec_selftest(shared: &Rr3PanelShared) -> Rr3PanelReply {
    let mut legs = Vec::with_capacity(8);
    legs.push(rr3_leg_beep(shared));
    let (relay_leg, relay_want) = rr3_leg_relay(shared);
    legs.push(relay_leg);
    legs.push(rr3_leg_face(shared));
    rr3_leg_group(
        shared,
        &mut legs,
        "wiring",
        "rr3.ctl.wiring",
        "n-a",
        "装配快照在册但零判据对象（零线组零 pin）",
    );
    rr3_leg_group(
        shared,
        &mut legs,
        "contracts",
        "rr3.ctl.contracts",
        "fail",
        "契约记账在册但零判据对象（四册点名清单漂移）",
    );
    let expect = Rr3SelftestExpect {
        relay: relay_want,
        ..shared.selftest_expect
    };
    let verdict = rr3_selftest_verdict(&legs, &expect);
    Rr3PanelReply::json(200, "OK", json!({ "legs": legs, "verdict": verdict }))
}

/// 腿对象定形：pass 随拒因有无，拒因存在时随腿带上（腿名由调用方点名）。
fn rr3_leg_json(name: &str, reason: Option<String>) -> Value {
    let mut leg = json!({
        "leg": name,
        "pass": if reason.is_none() { "pass" } else { "fail" },
    });
    if let Some(e) = reason {
        leg["reason"] = json!(e);
    }
    leg
}

/// 腿1 speaker：内置测试音播放成功即 pass（主循环缺席/写播放面失败如实 fail）
fn rr3_leg_beep(shared: &Rr3PanelShared) -> Value {
    let reason = rr3_panel_act(shared, Rr3PanelAction::Beep).err();
    rr3_leg_json("speaker", reason)
}

/// 腿2 relay：能力面先判——`rr3-act` 不在册即 n-a，不向总线发无人接的
/// on/off；ctl 取数失败仍 fail-closed 记 fail（不把系统面故障伪装成 n-a）。
/// 返回 (腿, 期望态)——ctl 失败时期望态记 fail 与腿等值（既有判据，行为不变）。
pub(crate) fn rr3_leg_relay(shared: &Rr3PanelShared) -> (Value, &'static str) {
    match rr3_act_enabled(&shared.bus_path, shared.limits.read_timeout) {
        Ok(enabled) => rr3_relay_leg_ok(shared, enabled),
        // 取数失败 = 能力面不可核验：腿如实 fail，期望态按最严的 pass 记——
        // 旧实现把期望也记 fail，等值判定会把这条故障腿抵消成"符合预期"，
        // 自检 verdict 可能在 rrd 状态面故障时假绿（0.0.8c 审计收口）。
        Err(e) => rr3_relay_leg_err(&e),
    }
}

/// 取数失败腿的应答与期望态（腿 fail、期望 pass，防等值判定假绿）。
fn rr3_relay_leg_err(e: &str) -> (Value, &'static str) {
    (
        json!({
            "leg": "relay", "pass": "fail",
            "reason": format!("ctl.status 取数失败: {e}"),
        }),
        "pass",
    )
}

/// 能力在册时的 relay 腿：n-a 档早退，其余走 on/off 往返并带上拒因。
fn rr3_relay_leg_ok(shared: &Rr3PanelShared, enabled: bool) -> (Value, &'static str) {
    let want = rr3_relay_expect(enabled, shared.selftest_expect.relay);
    if want == "n-a" {
        return rr3_relay_na(enabled);
    }
    let (pass, reason) = rr3_relay_cycle(shared);
    let mut leg = json!({ "leg": "relay", "pass": pass });
    if let Some(e) = reason {
        leg["reason"] = json!(e);
    }
    (leg, want)
}

/// relay 腿 n-a 档：拒因区分"配置声明不适用"与"rr3-act 不在册"。
fn rr3_relay_na(enabled: bool) -> (Value, &'static str) {
    let why = if enabled {
        "expect.relay=n-a（配置声明不适用）"
    } else {
        "rr3-act 未启用（host-only 能力面 n-a）"
    };
    (
        json!({ "leg": "relay", "pass": "n-a", "reason": why }),
        "n-a",
    )
}

/// relay on→off 往返：on 失败不执行 off；off 失败如实点名
fn rr3_relay_cycle(shared: &Rr3PanelShared) -> (&'static str, Option<String>) {
    match shared.bus.call(
        "rr3-act/relay/set",
        &json!({ "on": true }),
        shared.limits.call_timeout,
    ) {
        Err(e) => ("fail", Some(format!("on 未成功（off 未执行）: {e}"))),
        Ok(_) => rr3_relay_off(shared),
    }
}

/// 断开臂：吸合成功后复位（失败点名未复位，成功即 pass）。
fn rr3_relay_off(shared: &Rr3PanelShared) -> (&'static str, Option<String>) {
    match shared.bus.call(
        "rr3-act/relay/set",
        &json!({ "on": false }),
        shared.limits.call_timeout,
    ) {
        Ok(_) => ("pass", None),
        Err(e) => ("fail", Some(format!("off 未复位: {e}"))),
    }
}

/// 腿3 face：切到期望表情（期望值 config 数据化，产线按形态改 face_expect）
fn rr3_leg_face(shared: &Rr3PanelShared) -> Value {
    let reason = rr3_panel_act(
        shared,
        Rr3PanelAction::Face {
            expression: shared.selftest_face_expect.to_string(),
        },
    )
    .err();
    rr3_leg_json("face", reason)
}

/// 腿4/5 组腿：ctl 取数失败/快照不可得 = 单腿 fail 如实点名；取到即把 rrd
/// 三态判果逐条转述（"not-applicable" → n-a，其余未知值 fail-closed 折 fail）。
/// `empty_pass/empty_reason` = 空判据对象的态与原因（wiring 空表 n-a、contracts
/// 空表 fail——四册点名清单漂移不折通过）。
fn rr3_leg_group(
    shared: &Rr3PanelShared,
    legs: &mut Vec<Value>,
    prefix: &str,
    topic: &str,
    empty_pass: &str,
    empty_reason: &str,
) {
    match rr3_ctl_query(&shared.bus_path, topic, 5, shared.limits.read_timeout) {
        Err(e) => legs.push(json!({
            "leg": format!("{prefix}/取数"), "pass": "fail", "reason": e,
        })),
        Ok(v) => rr3_leg_avail(legs, v, prefix, empty_pass, empty_reason),
    }
}

/// available 门 + 空表态 + 逐条转述（rrd 三态折叠见 [`rr3_leg_item`]）。
fn rr3_leg_avail(
    legs: &mut Vec<Value>,
    v: Value,
    prefix: &str,
    empty_pass: &str,
    empty_reason: &str,
) {
    if !rr3_leg_shape(legs, &v, prefix, empty_pass, empty_reason) {
        return;
    }
    for c in checks_of(&v) {
        legs.push(rr3_leg_item(prefix, &c));
    }
}

/// available 门 + 空表态：不可用即记 fail 腿并返回 false（不再逐条转述）。
fn rr3_leg_shape(
    legs: &mut Vec<Value>,
    v: &Value,
    prefix: &str,
    empty_pass: &str,
    empty_reason: &str,
) -> bool {
    if v.get("available").and_then(Value::as_bool) != Some(true) {
        rr3_leg_fail(legs, v, prefix);
        return false;
    }
    if checks_of(v).is_empty() {
        legs.push(json!({
            "leg": format!("{prefix}/空"), "pass": empty_pass,
            "reason": empty_reason,
        }));
    }
    true
}

/// 取数腿失败（reason 缺省点名数据漂移）。
fn rr3_leg_fail(legs: &mut Vec<Value>, v: &Value, prefix: &str) {
    legs.push(json!({
        "leg": format!("{prefix}/取数"), "pass": "fail",
        "reason": v.get("reason").cloned().unwrap_or(json!(
            format!("ctl.{prefix} 应答缺 available:true（数据漂移）")
        )),
    }));
}

/// 单条判据转述：pass → pass、not-applicable → n-a、其余未知 fail-closed 折 fail。
fn rr3_leg_item(prefix: &str, c: &Value) -> Value {
    let item = c.get("item").and_then(Value::as_str).unwrap_or("?");
    let pass = rr3_leg_pass(c);
    let mut leg = json!({ "leg": format!("{prefix}/{item}"), "pass": pass });
    if let Some(r) = c.get("reason") {
        leg["reason"] = r.clone();
    }
    leg
}

/// 判据三态转述：pass → pass、not-applicable → n-a、其余未知 fail-closed 折 fail。
fn rr3_leg_pass(c: &Value) -> &'static str {
    match c.get("pass").and_then(Value::as_str) {
        Some("pass") => "pass",
        Some("not-applicable") => "n-a",
        _ => "fail",
    }
}

/// 自检 relay 腿的能力面：`rr3-act` 是否在册（按 ctl.status 模块表）。
/// 只判“在册”，不判 phase——在册但未 running 的 relay 仍应走 on/off 如实
/// fail，不能把“模块起了但坏了”误判成 host-only 的 n-a。
fn rr3_act_enabled(bus_path: &str, timeout: Duration) -> Result<bool, String> {
    let v = rr3_ctl_query(bus_path, "rr3.ctl.status", 6, timeout)?;
    let modules = rr3_ctl_modules(&v)?;
    Ok(modules
        .iter()
        .any(|m| m.get("name").and_then(Value::as_str) == Some("rr3-act")))
}

/// ctl.status 应答的 modules 数组（缺键即点名）。
fn rr3_ctl_modules(v: &Value) -> Result<&[Value], String> {
    v.get("modules")
        .and_then(Value::as_array)
        .map(|m| m.as_slice())
        .ok_or_else(|| "ctl.status 应答缺 modules".to_string())
}

/// relay 腿的期望态：能力缺席或配置显式 n-a 即 n-a；其余沿用预期表。
/// 能力缺席优先于配置，避免 host-only 被共享 config 的 relay=pass 拖红。
pub(crate) fn rr3_relay_expect(enabled: bool, configured: &'static str) -> &'static str {
    if !enabled || configured == "n-a" {
        "n-a"
    } else {
        configured
    }
}

/// 自检 verdict 判定（批N5 件五 F-31，纯函数化）：verdict = 结果集与预期集
/// 等值——固定腿 speaker/relay/face 逐名三态等值，结果集缺腿即红、多出腿册外
/// 未知腿名 fail-closed 红；wiring/* 与 contracts/*（批N5 件五笔4）组内零 fail
/// 即组过（n-a 不拖累不救场）。整体红绿是派生量：预期表可按形态改数据
/// （[rr3-interact.selftest.expect]），恒败腿如实申报期望 fail 即等值过
/// （改数据不改码）。
pub(crate) fn rr3_selftest_verdict(legs: &[Value], expect: &Rr3SelftestExpect) -> &'static str {
    let mut seen = [false; 3]; // speaker/relay/face 到场记账（缺腿即红）
    if !rr3_legs_match(legs, expect, &mut seen) {
        return "fail";
    }
    if seen.iter().any(|s| !s) {
        return "fail"; // 结果集缺固定腿即红
    }
    "pass"
}

/// 逐腿裁决（任一腿不合即 false，同时记固定三腿到场）。
fn rr3_legs_match(legs: &[Value], expect: &Rr3SelftestExpect, seen: &mut [bool; 3]) -> bool {
    for l in legs {
        let name = l.get("leg").and_then(Value::as_str).unwrap_or("");
        let state = l.get("pass").and_then(Value::as_str).unwrap_or("");
        if !rr3_leg_track(name, state, expect, seen) {
            return false;
        }
    }
    true
}

/// 单腿裁决：固定三腿对预期值并记到场；wiring/* 与 contracts/* 组内零 fail 即
/// 组过（n-a 不拖累不救场，判据同质无豁免面）；腿册外未知腿名 fail-closed。
fn rr3_leg_track(
    name: &str,
    state: &str,
    expect: &Rr3SelftestExpect,
    seen: &mut [bool; 3],
) -> bool {
    if let Some(ix) = rr3_leg_ix(name) {
        seen[ix] = true;
        return state == [expect.speaker, expect.relay, expect.face][ix];
    }
    if name.starts_with("wiring/") || name.starts_with("contracts/") {
        return state != "fail";
    }
    false
}

/// 固定腿名 → 记账位（speaker/relay/face；册外 None）。
fn rr3_leg_ix(name: &str) -> Option<usize> {
    match name {
        "speaker" => Some(0),
        "relay" => Some(1),
        "face" => Some(2),
        _ => None,
    }
}
