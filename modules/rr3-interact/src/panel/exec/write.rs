//! 写口应答族：总线透传、主循环代执行、装配侧写口与模型密钥面。

use serde_json::{json, Value};

use super::super::secrets::{rr3_secrets_list_at, rr3_secrets_path, rr3_secrets_upsert};
use super::super::server::{rr3_ctl_send, Rr3PanelAction, Rr3PanelDispatch, Rr3PanelShared};
use super::act::rr3_panel_act;
use super::Rr3PanelReply;

/// BusCall 应答的状态码裁定（批N4 件六 I-21③）：载荷含 `error` = 被调方业务
/// 失败 → 422 + ok:false（"上游受理但拒"——与 502 传输失败分码，脚本与产线按
/// 状态码判成败不得恒真）；其余 200 + ok:true。判定纯函数化：应答折装归调用方。
pub(crate) fn rr3_bus_reply_status(result: &Value) -> (u16, &'static str) {
    if result.get("error").is_some() {
        (422, "Unprocessable Entity")
    } else {
        (200, "OK")
    }
}

/// 写口应答：总线透传（BusCall）、主循环代执行（PanelAct）、装配侧写
/// （ProfileDomainSet）与 secrets 面（ModelKeySet/ModelKeysQuery）。
pub(super) fn rr3_reply_write(
    shared: &Rr3PanelShared,
    d: Rr3PanelDispatch,
) -> Option<Rr3PanelReply> {
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
            rr3_bus_reply(status, reason, &result)
        }
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e })),
    }
}

/// 业务应答体：200 回 result，其余回 error（状态码由 [`rr3_bus_reply_status`] 定）。
fn rr3_bus_reply(status: u16, reason: &'static str, result: &Value) -> Rr3PanelReply {
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
        Ok(path) => rr3_keys_at(&path),
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
    }
}

/// 清单读口（路径就绪后）：读失败同折 502，形状与路径失败一致。
fn rr3_keys_at(path: &std::path::Path) -> Rr3PanelReply {
    match rr3_secrets_list_at(path) {
        Ok(keys) => Rr3PanelReply::json(200, "OK", json!({ "keys": keys })),
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
    }
}

/// 模型密钥写入：应答只回 ref；applies=live（secrets 落盘后 brain 每轮现读）
fn rr3_exec_key(ref_name: &str, value: &str) -> Rr3PanelReply {
    match rr3_secrets_path() {
        Ok(path) => rr3_key_at(&path, ref_name, value),
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e })),
    }
}

/// 写入路（路径就绪后）：落盘成败折同一 502 形状。
fn rr3_key_at(path: &std::path::Path, ref_name: &str, value: &str) -> Rr3PanelReply {
    match rr3_secrets_upsert(path, ref_name, value) {
        Ok(()) => Rr3PanelReply::json(
            200,
            "OK",
            json!({ "ok": true, "ref": ref_name, "applies": "live" }),
        ),
        Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e })),
    }
}
