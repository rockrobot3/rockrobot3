//! 装配快照渲染面（自 assembly.rs 抽出）：pin/domain/wire/axis/role/checks 行投影
//! 与 `rr3.ctl.wiring` 应答壳；纯函数，不碰全局态。

use serde_json::{json, Value};

use super::*;

/// `rr3.ctl.wiring` 应答渲染（纯函数）。字段面 = 门 d 分母四路的字段级全集
/// （pin 十一键 / domain 十一键 / axis 十四键 / role requires 逐项）——covers
/// 申报的"真实渲染"承诺在这里兑现，渲染完整性由本文件测试区用门 d 同源词表
/// （`RR3_PIN_KEYS` / `RR3_DOMAIN_KEYS` / `RR3_ACT_PROFILE_FIELDS`）钉死。
/// 不含 `available` 位：那是全局单例语义（快照在不在册），归
/// [`rr3_assembly_json`] 的壳——纯函数才能用局部快照无竞态地单测。
pub fn rr3_assembly_render(s: &Rr3AssemblySnapshot) -> Value {
    json!({
        "profile": s.profile_name,
        "pins": rr3_pin_rows(&s.profile),
        "domains": rr3_domain_rows(&s.profile),
        "wires": rr3_wire_rows(&s.wiring),
        "axis": rr3_axis_rows(&s.axis),
        "roles": rr3_role_rows(&s.roles),
        "checks": rr3_check_rows(s),
    })
}

/// pins 行（pin 十一键 + pin_id：与门 d 词表同源，字段不多不少）。
fn rr3_pin_rows(p: &Rr3Profile) -> Vec<Value> {
    p.pins
        .iter()
        .map(|pin| {
            json!({
                "pin_id": pin.rr3_pin_id(),
                "role": pin.role,
                "instance": pin.instance,
                "controller": pin.controller,
                "channel": pin.channel,
                "physical": pin.physical,
                "direction": pin.direction,
                "rail": pin.rail,
                "addr": pin.addr,
                "device": pin.device,
                "failsafe": pin.failsafe,
                "mux_group": pin.mux_group,
            })
        })
        .collect()
}

/// domains 行（domain 十一键）。
fn rr3_domain_rows(p: &Rr3Profile) -> Vec<Value> {
    p.supplies
        .domains
        .iter()
        .map(|d| {
            json!({
                "name": d.name,
                "nominal_mv": d.nominal_mv,
                "mild_mv": d.mild_mv,
                "severe_mv": d.severe_mv,
                "current_cap_ma": d.current_cap_ma,
                "sense": d.sense,
                "current_sense": d.current_sense,
                "switch": d.switch,
                "switchable": d.switchable,
                "armed": d.armed,
                "max_age_ms": d.max_age_ms,
                "evidence": d.evidence,
            })
        })
        .collect()
}

/// wires 行（接线档九字段）。
fn rr3_wire_rows(w: &Rr3Wiring) -> Vec<Value> {
    w.wires
        .iter()
        .map(|w| {
            json!({
                "connector": w.connector,
                "hdr_pin": w.hdr_pin,
                "signal": w.signal,
                "net": w.net,
                "feeds": w.feeds,
                "mate": w.mate,
                "keyed": w.keyed,
                "rated_a": w.rated_a,
                "gnd_common": w.gnd_common,
            })
        })
        .collect()
}

/// axis 行（轴档案十四键）：`schema` 装载面只认 1（对账后不留存），快照里必为 1。
fn rr3_axis_rows(axis: &[Rr3AxisFile]) -> Vec<Value> {
    axis.iter()
        .map(|f| {
            let a = &f.profile;
            json!({
                "pin": f.pin,
                "schema": 1,
                "center_deg": a.center_deg,
                "lo_deg": a.lo_deg,
                "hi_deg": a.hi_deg,
                "dir_sign": a.dir_sign,
                "slew_deg_s": a.slew_deg_s,
                "settle_ms": a.settle_ms,
                "hold": a.hold.rr3_act_hold_name(),
                "current_ceil_ma": a.current_ceil_ma,
                "warmup_ms": a.warmup_ms,
                "hard_stop": a.hard_stop.rr3_hard_stop_name(),
                "calibrated": a.calibrated,
                "source": a.source.rr3_act_source_name(),
            })
        })
        .collect()
}

/// roles 行（角色登记面五字段）。
fn rr3_role_rows(roles: &Rr3RoleTable) -> Vec<Value> {
    roles
        .rr3_role_names()
        .iter()
        .map(|name| {
            json!({
                "name": name,
                "slot": roles.rr3_role_slot(name),
                "requires": roles.rr3_role_requires(name),
                "derives": roles.rr3_role_derives(name),
                "direction": roles.rr3_role_direction(name),
            })
        })
        .collect()
}

/// checks 行（门 d 逐条点名的机器可读投影）。
fn rr3_check_rows(s: &Rr3AssemblySnapshot) -> Vec<Value> {
    rr3_cross_checks(&s.profile, &s.wiring, &s.peak_by_mate)
        .iter()
        .map(|c| json!({ "item": c.item, "pass": c.pass, "reason": c.reason }))
        .collect()
}

/// 全局壳（conn.rs 单点调用）：快照在册 → 渲染 + `available:true`；未以
/// `--profile` 启动（`--manifest` 调试路径无装配快照）→ 如实报不可得，不造假
/// 数据。None 分支不单测——OnceLock 全局单例语义下，cargo test 同二进制并行
/// 线程里"未 set"无法与"已 set"的用例共存（首份胜出的一锤子买卖），该分支由
/// `--manifest` 路径的端到端用例覆盖。
pub fn rr3_assembly_json() -> Value {
    match rr3_assembly_snap() {
        Some(s) => {
            let mut v = rr3_assembly_render(s);
            v["available"] = json!(true);
            v
        }
        None => json!({
            "available": false,
            "reason": "未以 --profile 启动（--manifest 调试路径无装配快照），接线图不可得",
        }),
    }
}
