//! 面板执行面（0.0.8 审计收口，`server.rs` 1261 行 → 服务面/执行面两分）。
//!
//! 本模块只回答一件事：**路由产物 → 应答字节的判据**。监听、accept、限额、
//! 连接线程归 `server.rs`；路由表（[`super::RR3_PANEL_ROUTES`]）与分派
//! （[`super::Rr3PanelDispatch`]）也在 `server.rs`。执行面按应答形状分三域
//! （协议层自述 / 读口 / 写口），域内按分派臂折小函数（`rr3_exec_*`），
//! 自检腿（`rr3_leg_*`）与 verdict 判定同址。
//!
//! 判据边界（与拆分前逐字一致，结构重构只动形状不动语义）：
//! - BusCall 载荷含 `error` → 422 + ok:false（上游受理但拒，与 502 传输失败分码）；
//! - ctl 读口失败 → 502；reboot 拒 → 403（rrd 双重门原文案转达）；
//! - 面板不持业务语义：装配侧写口 body 原样透传，判决在 rrd 单点。

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use serde_json::{json, Value};

use super::secrets::{rr3_secrets_list_at, rr3_secrets_path, rr3_secrets_upsert};
use super::server::{
    rr3_ctl_query, rr3_ctl_send, rr3_panel_route, Rr3HttpRequest, Rr3PanelAction, Rr3PanelDispatch,
    Rr3PanelShared, Rr3SelftestExpect, RR3_PANEL_ROUTES,
};
use super::RR3_PANEL_INDEX_HTML;

/// 面板应答载体：状态码 + 原因 + 内容类型 + body。execute 一次定形，
/// handle 只管写出——没有"执行产物"与"编码"两层错位。
pub(super) struct Rr3PanelReply {
    pub(super) status: u16,
    pub(super) reason: &'static str,
    pub(super) content_type: &'static str,
    pub(super) body: Vec<u8>,
}

impl Rr3PanelReply {
    pub(super) fn json(status: u16, reason: &'static str, v: Value) -> Self {
        Self {
            status,
            reason,
            content_type: "application/json",
            body: v.to_string().into_bytes(),
        }
    }

    /// `GET /`：单文件内嵌静态页（text/html）
    fn html() -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type: "text/html; charset=utf-8",
            body: RR3_PANEL_INDEX_HTML.as_bytes().to_vec(),
        }
    }
}

/// BusCall 应答的状态码裁定（批N4 件六 I-21③）：载荷含 `error` = 被调方业务
/// 失败 → 422 + ok:false（"上游受理但拒"——与 502 传输失败分码，脚本与产线按
/// 状态码判成败不得恒真）；其余 200 + ok:true。判定纯函数化：应答折装归调用方。
pub(super) fn rr3_bus_reply_status(result: &Value) -> (u16, &'static str) {
    if result.get("error").is_some() {
        (422, "Unprocessable Entity")
    } else {
        (200, "OK")
    }
}

/// ctl 应答里的判据对象数组（wiring/contracts 腿共用）：缺键或类型不对都当空表，
/// 由空表腿与逐条转述兜底
fn checks_of(v: &Value) -> Vec<Value> {
    v.get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// 路由产物的执行面：状态码三元组在返回处一次定形。
pub(super) fn rr3_panel_execute(req: &Rr3HttpRequest, shared: &Rr3PanelShared) -> Rr3PanelReply {
    rr3_panel_reply(rr3_panel_route(&req.method, &req.path, &req.body), shared)
}

/// 分派 → 应答（执行面主表）：按**应答形状**三域折叠——协议层（不触总线）、
/// 读口（ctl 查询）、写口（总线/主循环/secrets）。与路由面 `rr3_route_*` 同一
/// 分面纪律：组函数只认自己那一域，认不出返回 `None`，主表按序兜底。文案与
/// 状态码逐字保留，本拆分只动形状不动判据。
///
/// 三域并集 = 分派枚举全集；`None` 兜底只在**新增变体未并入任何一域**时显形
/// （500 + 点名），不会把未接线臂静默当 200。改枚举须同步改本表——这是分派的
/// 单真值点。
fn rr3_panel_reply(d: Rr3PanelDispatch, shared: &Rr3PanelShared) -> Rr3PanelReply {
    if let Some(r) = rr3_reply_proto(&d) {
        return r;
    }
    if let Some(r) = rr3_reply_read(shared, &d) {
        return r;
    }
    rr3_reply_write(shared, d).unwrap_or_else(rr3_reply_miss)
}

/// 未接线兜底：新增分派变体没有并入三域时的唯一出口（fail-closed，点名不断言）
fn rr3_reply_miss() -> Rr3PanelReply {
    Rr3PanelReply::json(
        500,
        "Internal Server Error",
        json!({ "error": "面板分派臂未接线（新变体须并入三域之一）" }),
    )
}

/// 协议层应答：路由判决（404/405/400）与自述（index/healthz/version）——
/// 零总线依赖，纯常量应答。
fn rr3_reply_proto(d: &Rr3PanelDispatch) -> Option<Rr3PanelReply> {
    Some(match d {
        Rr3PanelDispatch::Html => Rr3PanelReply::html(),
        Rr3PanelDispatch::NotFound => {
            Rr3PanelReply::json(404, "Not Found", json!({ "error": "not found" }))
        }
        Rr3PanelDispatch::MethodNotAllowed => Rr3PanelReply::json(
            405,
            "Method Not Allowed",
            json!({ "error": "method not allowed" }),
        ),
        Rr3PanelDispatch::BadRequest(e) => {
            Rr3PanelReply::json(400, "Bad Request", json!({ "error": e }))
        }
        _ => return rr3_reply_self(d),
    })
}

/// 自述应答：healthz/version 两类纯常量（零总线依赖）。
fn rr3_reply_self(d: &Rr3PanelDispatch) -> Option<Rr3PanelReply> {
    Some(match d {
        Rr3PanelDispatch::HealthQuery => Rr3PanelReply::json(
            200,
            "OK",
            json!({
                "ok": true,
                "module": "rr3-interact",
                "version": env!("CARGO_PKG_VERSION"),
            }),
        ),
        Rr3PanelDispatch::VersionQuery => Rr3PanelReply::json(
            200,
            "OK",
            json!({
                "module": "rr3-interact",
                "version": env!("CARGO_PKG_VERSION"),
                "routes": RR3_PANEL_ROUTES.len(),
            }),
        ),
        _ => return None,
    })
}

/// 读口应答：全部经 ctl 查询或总线取数，只读不动运行态。接线图与 reboot 的
/// 状态码差异在各自 `rr3_exec_*` 里定形（数据态/拒因语义归 rrd）。
fn rr3_reply_read(shared: &Rr3PanelShared, d: &Rr3PanelDispatch) -> Option<Rr3PanelReply> {
    Some(match d {
        Rr3PanelDispatch::StatusQuery => rr3_exec_status(shared),
        Rr3PanelDispatch::CountersQuery => rr3_exec_ctl(shared, "rr3.ctl.counters", 2),
        Rr3PanelDispatch::LogsQuery => rr3_exec_ctl(shared, "rr3.ctl.logs", 3),
        // 接线图：rrd 应答原样透传（available:false 是数据态不是网关错）
        Rr3PanelDispatch::WiringQuery => rr3_exec_ctl(shared, "rr3.ctl.wiring", 5),
        _ => return rr3_reply_probe(shared, d),
    })
}

/// 读口自检臂：自检与重启（拒因语义归 rrd）。
fn rr3_reply_probe(shared: &Rr3PanelShared, d: &Rr3PanelDispatch) -> Option<Rr3PanelReply> {
    Some(match d {
        Rr3PanelDispatch::SelfTest => rr3_exec_selftest(shared),
        Rr3PanelDispatch::Reboot => rr3_exec_reboot(shared),
        _ => return None,
    })
}

/// 写口应答：总线透传（BusCall）、主循环代执行（PanelAct）、装配侧写
/// （ProfileDomainSet）与 secrets 面（ModelKeySet/ModelKeysQuery）。
fn rr3_reply_write(shared: &Rr3PanelShared, d: Rr3PanelDispatch) -> Option<Rr3PanelReply> {
    Some(match d {
        Rr3PanelDispatch::BusCall { topic, args } => rr3_exec_bus(shared, &topic, &args),
        Rr3PanelDispatch::PanelAct(action) => rr3_exec_act(shared, action),
        // 装配侧写口：body 原样透传 rrd 门序——502 而非 403：码→HTTP 映射是
        // rrd 应答层业务语义，面板只保真转达拒因文案（第二真值点禁令）
        Rr3PanelDispatch::ProfileDomainSet { params } => rr3_exec_domain(shared, &params),
        Rr3PanelDispatch::ModelKeysQuery => rr3_exec_keys(),
        Rr3PanelDispatch::ModelKeySet { ref_name, value } => rr3_exec_key(&ref_name, &value),
        _ => return None,
    })
}

/// 状态页：health 走 ctl 查询；link 是主循环订阅缓存（只读快照）
fn rr3_exec_status(shared: &Rr3PanelShared) -> Rr3PanelReply {
    let health = rr3_ctl_query(
        &shared.bus_path,
        "rr3.ctl.health",
        1,
        shared.limits.read_timeout,
    );
    let link = shared.link.lock().map(|g| g.clone()).unwrap_or(None);
    match health {
        Ok(h) => Rr3PanelReply::json(200, "OK", json!({ "health": h, "link": link })),
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
    }
}

/// ctl 读口统一折装：成功原样透传，失败 502（数据态仍 200 由 rrd 决定）
fn rr3_exec_ctl(shared: &Rr3PanelShared, method: &str, id: u64) -> Rr3PanelReply {
    match rr3_ctl_query(&shared.bus_path, method, id, shared.limits.read_timeout) {
        Ok(v) => Rr3PanelReply::json(200, "OK", v),
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
    }
}

/// 重启口：rrd 双重门缺省恒拒，拒因如实转达（403 + rrd 原文案）
fn rr3_exec_reboot(shared: &Rr3PanelShared) -> Rr3PanelReply {
    match rr3_ctl_query(
        &shared.bus_path,
        "rr3.ctl.reboot",
        4,
        shared.limits.read_timeout,
    ) {
        Ok(v) => Rr3PanelReply::json(200, "OK", v),
        Err(e) => Rr3PanelReply::json(403, "Forbidden", json!({ "error": e })),
    }
}

/// 装配侧写口：body 原样透传；拒因全文转达（含缺二次确认/证据不过等点名文案）
fn rr3_exec_domain(shared: &Rr3PanelShared, params: &Value) -> Rr3PanelReply {
    match rr3_ctl_send(
        &shared.bus_path,
        "rr3.ctl.domain.set",
        params,
        6,
        shared.limits.read_timeout,
    ) {
        Ok(v) => Rr3PanelReply::json(200, "OK", v),
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
    }
}

/// BusCall 动作：业务错误按载荷裁（422 + ok:false），传输失败 502
fn rr3_exec_bus(shared: &Rr3PanelShared, topic: &str, args: &Value) -> Rr3PanelReply {
    match shared.bus.call(topic, args, shared.limits.call_timeout) {
        Ok(result) => {
            let (status, reason) = rr3_bus_reply_status(&result);
            if status == 200 {
                Rr3PanelReply::json(status, reason, json!({ "ok": true, "result": result }))
            } else {
                Rr3PanelReply::json(
                    status,
                    reason,
                    json!({ "ok": false, "error": result["error"] }),
                )
            }
        }
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e })),
    }
}

/// 进程内动作（beep/face）：主循环代执行，失败折 502
fn rr3_exec_act(shared: &Rr3PanelShared, action: Rr3PanelAction) -> Rr3PanelReply {
    match rr3_panel_act(shared, action) {
        Ok(result) => Rr3PanelReply::json(200, "OK", json!({ "ok": true, "result": result })),
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e })),
    }
}

/// 模型密钥清单：只回 ref 名，永不回值
fn rr3_exec_keys() -> Rr3PanelReply {
    match rr3_secrets_path() {
        Ok(path) => match rr3_secrets_list_at(&path) {
            Ok(keys) => Rr3PanelReply::json(200, "OK", json!({ "keys": keys })),
            Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
        },
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
    }
}

/// 模型密钥写入：应答只回 ref；applies=live（secrets 落盘后 brain 每轮现读）
fn rr3_exec_key(ref_name: &str, value: &str) -> Rr3PanelReply {
    match rr3_secrets_path() {
        Ok(path) => match rr3_secrets_upsert(&path, ref_name, value) {
            Ok(()) => Rr3PanelReply::json(
                200,
                "OK",
                json!({ "ok": true, "ref": ref_name, "applies": "live" }),
            ),
            Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e })),
        },
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e })),
    }
}

/// 出厂自检：并列五腿（speaker/relay/face/wiring/contracts）各答各展示，
/// 无依赖链、单项失败不连锁（不是编排——brain 才是唯一编排者，ADR-0007）。
/// 逐腿 `{leg, pass, reason?}`，pass = pass|fail|n-a 三态；失败腿如实报 fail
/// 不折算通过。verdict 判定归 [`rr3_selftest_verdict`]（预期表等值，纯函数化）。
fn rr3_exec_selftest(shared: &Rr3PanelShared) -> Rr3PanelReply {
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

/// 腿1 speaker：内置测试音播放成功即 pass（主循环缺席/写播放面失败如实 fail）
fn rr3_leg_beep(shared: &Rr3PanelShared) -> Value {
    let reason = rr3_panel_act(shared, Rr3PanelAction::Beep).err();
    let mut leg = json!({
        "leg": "speaker",
        "pass": if reason.is_none() { "pass" } else { "fail" },
    });
    if let Some(e) = reason {
        leg["reason"] = json!(e);
    }
    leg
}

/// 腿2 relay：能力面先判——`rr3-act` 不在册即 n-a，不向总线发无人接的
/// on/off；ctl 取数失败仍 fail-closed 记 fail（不把系统面故障伪装成 n-a）。
/// 返回 (腿, 期望态)——ctl 失败时期望态记 fail 与腿等值（既有判据，行为不变）。
pub(super) fn rr3_leg_relay(shared: &Rr3PanelShared) -> (Value, &'static str) {
    match rr3_act_enabled(&shared.bus_path, shared.limits.read_timeout) {
        Ok(enabled) => {
            let want = rr3_relay_expect(enabled, shared.selftest_expect.relay);
            if want == "n-a" {
                let why = if enabled {
                    "expect.relay=n-a（配置声明不适用）"
                } else {
                    "rr3-act 未启用（host-only 能力面 n-a）"
                };
                return (
                    json!({ "leg": "relay", "pass": "n-a", "reason": why }),
                    want,
                );
            }
            let (pass, reason) = rr3_relay_cycle(shared);
            let mut leg = json!({ "leg": "relay", "pass": pass });
            if let Some(e) = reason {
                leg["reason"] = json!(e);
            }
            (leg, want)
        }
        // 取数失败 = 能力面不可核验：腿如实 fail，期望态按最严的 pass 记——
        // 旧实现把期望也记 fail，等值判定会把这条故障腿抵消成"符合预期"，
        // 自检 verdict 可能在 rrd 状态面故障时假绿（0.0.8c 审计收口）。
        Err(e) => (
            json!({
                "leg": "relay", "pass": "fail",
                "reason": format!("ctl.status 取数失败: {e}"),
            }),
            "pass",
        ),
    }
}

/// relay on→off 往返：on 失败不执行 off；off 失败如实点名
fn rr3_relay_cycle(shared: &Rr3PanelShared) -> (&'static str, Option<String>) {
    match shared.bus.call(
        "rr3-act/relay/set",
        &json!({ "on": true }),
        shared.limits.call_timeout,
    ) {
        Err(e) => ("fail", Some(format!("on 未成功（off 未执行）: {e}"))),
        Ok(_) => match shared.bus.call(
            "rr3-act/relay/set",
            &json!({ "on": false }),
            shared.limits.call_timeout,
        ) {
            Ok(_) => ("pass", None),
            Err(e) => ("fail", Some(format!("off 未复位: {e}"))),
        },
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
    let mut leg = json!({
        "leg": "face",
        "pass": if reason.is_none() { "pass" } else { "fail" },
    });
    if let Some(e) = reason {
        leg["reason"] = json!(e);
    }
    leg
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
    if v.get("available").and_then(Value::as_bool) != Some(true) {
        legs.push(json!({
            "leg": format!("{prefix}/取数"), "pass": "fail",
            "reason": v.get("reason").cloned().unwrap_or(json!(
                format!("ctl.{prefix} 应答缺 available:true（数据漂移）")
            )),
        }));
        return;
    }
    let checks = checks_of(&v);
    if checks.is_empty() {
        legs.push(json!({
            "leg": format!("{prefix}/空"), "pass": empty_pass,
            "reason": empty_reason,
        }));
    }
    for c in &checks {
        legs.push(rr3_leg_item(prefix, c));
    }
}

/// 单条判据转述：pass → pass、not-applicable → n-a、其余未知 fail-closed 折 fail。
fn rr3_leg_item(prefix: &str, c: &Value) -> Value {
    let item = c.get("item").and_then(Value::as_str).unwrap_or("?");
    let pass = match c.get("pass").and_then(Value::as_str) {
        Some("pass") => "pass",
        Some("not-applicable") => "n-a",
        _ => "fail",
    };
    let mut leg = json!({ "leg": format!("{prefix}/{item}"), "pass": pass });
    if let Some(r) = c.get("reason") {
        leg["reason"] = r.clone();
    }
    leg
}

/// 自检 relay 腿的能力面：`rr3-act` 是否在册（按 ctl.status 模块表）。
/// 只判“在册”，不判 phase——在册但未 running 的 relay 仍应走 on/off 如实
/// fail，不能把“模块起了但坏了”误判成 host-only 的 n-a。
fn rr3_act_enabled(bus_path: &str, timeout: Duration) -> Result<bool, String> {
    let v = rr3_ctl_query(bus_path, "rr3.ctl.status", 6, timeout)?;
    let modules = v
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| "ctl.status 应答缺 modules".to_string())?;
    Ok(modules
        .iter()
        .any(|m| m.get("name").and_then(Value::as_str) == Some("rr3-act")))
}

/// relay 腿的期望态：能力缺席或配置显式 n-a 即 n-a；其余沿用预期表。
/// 能力缺席优先于配置，避免 host-only 被共享 config 的 relay=pass 拖红。
pub(super) fn rr3_relay_expect(enabled: bool, configured: &'static str) -> &'static str {
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
pub(super) fn rr3_selftest_verdict(legs: &[Value], expect: &Rr3SelftestExpect) -> &'static str {
    let mut seen = [false; 3]; // speaker/relay/face 到场记账（缺腿即红）
    for l in legs {
        let name = l.get("leg").and_then(Value::as_str).unwrap_or("");
        let state = l.get("pass").and_then(Value::as_str).unwrap_or("");
        if !rr3_leg_track(name, state, expect, &mut seen) {
            return "fail";
        }
    }
    if seen.iter().any(|s| !s) {
        return "fail"; // 结果集缺固定腿即红
    }
    "pass"
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

/// 进程内动作执行：递主循环、等应答（动作在主循环串行处理，与 TTS 点名互斥）。
/// 主循环回的是业务 Result——Err（不认的表情等）原样上抛给 execute 折 502 语义。
fn rr3_panel_act(shared: &Rr3PanelShared, action: Rr3PanelAction) -> Result<Value, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    shared
        .actions
        .send((action, tx))
        .map_err(|_| "主循环不在了（进程正在退出）".to_string())?;
    match rx.recv_timeout(shared.limits.call_timeout) {
        Ok(v) => v,
        Err(RecvTimeoutError::Timeout) => Err("主循环处理动作超时".to_string()),
        Err(RecvTimeoutError::Disconnected) => Err("应答通道已断（主循环退出）".to_string()),
    }
}
