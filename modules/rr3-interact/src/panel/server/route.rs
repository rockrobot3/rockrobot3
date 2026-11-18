//! 面板路由面：方法 + 路径 + body → 分派（纯函数，无 I/O）。
//!
//! 端点真值单点 = [`super::RR3_PANEL_ROUTES`]；本文件只切判读面（sys/mod/cfg/act
//! 四组，每条路径只在一组出现），不复制申报列——漂移由
//! `route_table_matches_dispatch` 逐表项钉死。面板不当发布者：分派类型里没有
//! Publish 变体（ADR-0006 裁定 3 的静态形态）。
//!
//! 拆分动因：`server.rs` 550 行，路由纯函数 ~240 行与连接/监听面分属两簇。

use serde_json::{json, Value};

use super::{Rr3PanelAction, Rr3PanelDispatch};
use crate::panel::http::rr3_panel_body_json;

/// 五模块 ops 读面（批N4 件九二）：同一 BusCall 透传范式——参数裁决在 owner
/// （各模块白名单组体快照，槽名 = assemble/composite.toml 槽表申报同源），
/// 面板只透传。表驱动：路径 → 模块名，新增模块只加一行。
const RR3_OPS_ROUTES: [(&str, &str); 5] = [
    ("/api/act/ops", "rr3-act"),
    ("/api/connect/ops", "rr3-connect"),
    ("/api/interact/ops", "rr3-interact"),
    ("/api/power/ops", "rr3-power"),
    ("/api/sense/ops", "rr3-sense"),
];

/// ops 读面路径 → 名单点名 topic（非 ops 路径返回 None）
fn rr3_ops_topic(path: &str) -> Option<String> {
    RR3_OPS_ROUTES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, m)| format!("{m}/ops/list"))
}

/// 无参读口 BusCall（空对象参数）
fn rr3_get_call(topic: &str) -> Rr3PanelDispatch {
    Rr3PanelDispatch::BusCall {
        topic: topic.to_string(),
        args: json!({}),
    }
}

/// 带 body 写口 BusCall：body 非 JSON 即 400，参数裁决归 owner
fn rr3_post_call(topic: &str, body: &[u8]) -> Rr3PanelDispatch {
    match rr3_panel_body_json(body) {
        Ok(args) => Rr3PanelDispatch::BusCall {
            topic: topic.to_string(),
            args,
        },
        Err(e) => Rr3PanelDispatch::BadRequest(e),
    }
}

/// 路由纯函数：方法 + 路径 + body → 分派。**端点白名单 = [`RR3_PANEL_ROUTES`]**，
/// 未知路径 404、方法不对 405、body 坏 400——面板不是攻击面兜底者，一切都显式。
///
/// 结构约束（B8 分面）：按资源归属分 sys/mod/cfg/act 四组匹配，每条路径只在
/// 一组出现（组内仍一端点一行、无第二层 match）；方法收窄走
/// [`rr3_get_only`]/[`rr3_post_only`]，body 解析走 `rr3_*_set` 小函数。
/// 分面只切判读面，不复制申报列——covers/applies 真值仍单点在端点表。
pub(crate) fn rr3_panel_route(method: &str, path: &str, body: &[u8]) -> Rr3PanelDispatch {
    if let Some(d) = rr3_route_sys(method, path) {
        return d;
    }
    if let Some(d) = rr3_route_mod(method, path, body) {
        return d;
    }
    rr3_route_writes(method, path, body)
}

/// 写口两组（cfg/act）：都未命中即 404。
fn rr3_route_writes(method: &str, path: &str, body: &[u8]) -> Rr3PanelDispatch {
    if let Some(d) = rr3_route_cfg(method, path, body) {
        return d;
    }
    if let Some(d) = rr3_route_act(method, path, body) {
        return d;
    }
    Rr3PanelDispatch::NotFound
}

/// 系统读面：静态页、状态/存活/版本/计数/日志、装配快照。
/// 四条路由组（sys/mod/cfg/act）按资源归属划面，每条路径**只在一组**出现；
/// 端点真值仍是 [`RR3_PANEL_ROUTES`]（本组表不复制申报列），漂移由
/// `route_table_matches_dispatch` 逐表项钉死。
fn rr3_route_sys(method: &str, path: &str) -> Option<Rr3PanelDispatch> {
    let d = match path {
        "/" => Rr3PanelDispatch::Html,
        "/api/status" => Rr3PanelDispatch::StatusQuery,
        // 存活/版本探针（2026-10-31）：不触总线、零依赖，可高频轮询
        "/api/healthz" => Rr3PanelDispatch::HealthQuery,
        "/api/version" => Rr3PanelDispatch::VersionQuery,
        _ => return rr3_route_sys_tail(method, path),
    };
    Some(rr3_get_only(method, d))
}

/// 系统读面续组：计数/日志/接线图（未知路径仍 404）。
fn rr3_route_sys_tail(method: &str, path: &str) -> Option<Rr3PanelDispatch> {
    let d = match path {
        "/api/data" => Rr3PanelDispatch::CountersQuery,
        "/api/logs" => Rr3PanelDispatch::LogsQuery,
        // 接线图读口：ctl.wiring 透传，判据真值单点在 rrd assembly.rs
        "/api/wiring" => Rr3PanelDispatch::WiringQuery,
        _ => return None,
    };
    Some(rr3_get_only(method, d))
}

/// 模块面：五模块 ops 读口（表驱动 [`RR3_OPS_ROUTES`]）＋ brain ops/模型行
/// ＋ 模型密钥面（只报 ref 名，POST 应答不回值）。
fn rr3_route_mod(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    if let Some(topic) = rr3_ops_topic(path) {
        return Some(rr3_get_only(method, rr3_get_call(&topic)));
    }
    rr3_route_brain(method, path, body)
}

/// brain 读/写面（ops 同源、模型行、密钥面在 [`rr3_route_keys`]）。
fn rr3_route_brain(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    match path {
        // brain ops 读/写同源（读渲染与写同一批 ops 槽）
        "/api/brain/ops" => Some(rr3_brain_ops(method, body)),
        "/api/brain/models" => Some(rr3_get_only(method, rr3_get_call("rr3-brain/model/list"))),
        "/api/brain/model" => Some(rr3_post_only(
            method,
            rr3_post_call("rr3-brain/model/set", body),
        )),
        "/api/brain/model/test" => Some(rr3_post_only(
            method,
            rr3_post_call("rr3-brain/model/test", body),
        )),
        _ => rr3_route_keys(method, path, body),
    }
}

/// 模型密钥面（只报 ref 名，POST 应答不回值）。
fn rr3_route_keys(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    Some(match path {
        "/api/model/keys" => rr3_get_only(method, Rr3PanelDispatch::ModelKeysQuery),
        "/api/model/key" => rr3_post_only(method, rr3_key_set(body)),
        _ => return None,
    })
}

/// 配置面：装配侧写口（rrd 门序单点裁决）＋ 配网三 RPC（参数裁决在 owner 模块）。
fn rr3_route_cfg(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    Some(match path {
        // 装配侧写口：body 原样透传 rrd 写口门序（confirm/证据/key 白名单）
        "/api/profile/domain" => rr3_post_only(method, rr3_domain_set(body)),
        "/api/wifi/scan" => rr3_get_only(method, rr3_get_call("rr3-connect/wifi/scan")),
        "/api/wifi/connect" => {
            rr3_post_only(method, rr3_post_call("rr3-connect/wifi/connect", body))
        }
        // forget 无参数：空对象 args（legacy 语义，body 不读）
        "/api/wifi/forget" => rr3_post_only(method, rr3_get_call("rr3-connect/wifi/forget")),
        _ => return None,
    })
}

/// 动作面：继电器（归属门在 rrd）、进程内 beep/face、自检、重启（双重门缺省拒）。
fn rr3_route_act(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    Some(match path {
        // relay：面板只裁决布尔 on，归属门在 rrd（topic 属 act exposes）
        "/api/relay" => rr3_post_only(method, rr3_relay_set(body)),
        "/api/beep" => rr3_post_only(method, Rr3PanelDispatch::PanelAct(Rr3PanelAction::Beep)),
        "/api/face" => rr3_post_only(method, rr3_face_set(body)),
        "/api/selftest" => rr3_post_only(method, Rr3PanelDispatch::SelfTest),
        "/api/reboot" => rr3_post_only(method, Rr3PanelDispatch::Reboot),
        _ => return None,
    })
}

/// GET 限定：表内读口统一收窄（非 GET = 405）
fn rr3_get_only(method: &str, d: Rr3PanelDispatch) -> Rr3PanelDispatch {
    if method == "GET" {
        d
    } else {
        Rr3PanelDispatch::MethodNotAllowed
    }
}

/// POST 限定：动作/写口统一收窄（非 POST = 405）
fn rr3_post_only(method: &str, d: Rr3PanelDispatch) -> Rr3PanelDispatch {
    if method == "POST" {
        d
    } else {
        Rr3PanelDispatch::MethodNotAllowed
    }
}

/// brain ops 两面同端点：GET 读快照 / POST 写槽（参数裁决在 owner）
fn rr3_brain_ops(method: &str, body: &[u8]) -> Rr3PanelDispatch {
    match method {
        "GET" => rr3_get_call("rr3-brain/ops/list"),
        "POST" => rr3_post_call("rr3-brain/ops/set", body),
        _ => Rr3PanelDispatch::MethodNotAllowed,
    }
}

/// `POST /api/profile/domain`：body 原样透传（参数裁决在 rrd 写口门序）
fn rr3_domain_set(body: &[u8]) -> Rr3PanelDispatch {
    match rr3_panel_body_json(body) {
        Ok(params) => Rr3PanelDispatch::ProfileDomainSet { params },
        Err(e) => Rr3PanelDispatch::BadRequest(e),
    }
}

/// `POST /api/relay`：只认布尔 on
fn rr3_relay_set(body: &[u8]) -> Rr3PanelDispatch {
    let args = match rr3_panel_body_json(body) {
        Ok(v) => v,
        Err(e) => return Rr3PanelDispatch::BadRequest(e),
    };
    rr3_relay_call(&args)
}

/// relay 参数落派发（只认布尔 on）。
fn rr3_relay_call(args: &Value) -> Rr3PanelDispatch {
    match args.get("on").and_then(Value::as_bool) {
        Some(on) => Rr3PanelDispatch::BusCall {
            topic: "rr3-act/relay/set".to_string(),
            args: json!({ "on": on }),
        },
        None => Rr3PanelDispatch::BadRequest("缺布尔参数 on".to_string()),
    }
}

/// `POST /api/face`：只认字符串 expression
fn rr3_face_set(body: &[u8]) -> Rr3PanelDispatch {
    let args = match rr3_panel_body_json(body) {
        Ok(v) => v,
        Err(e) => return Rr3PanelDispatch::BadRequest(e),
    };
    rr3_face_call(&args)
}

/// face 参数落派发（只认字符串 expression）。
fn rr3_face_call(args: &Value) -> Rr3PanelDispatch {
    match args.get("expression").and_then(Value::as_str) {
        Some(expression) => Rr3PanelDispatch::PanelAct(Rr3PanelAction::Face {
            expression: expression.to_string(),
        }),
        None => Rr3PanelDispatch::BadRequest("缺字符串参数 expression".to_string()),
    }
}

/// `POST /api/model/key`：只认字符串 ref/value
fn rr3_key_set(body: &[u8]) -> Rr3PanelDispatch {
    let args = match rr3_panel_body_json(body) {
        Ok(v) => v,
        Err(e) => return Rr3PanelDispatch::BadRequest(e),
    };
    rr3_key_call(&args)
}

/// key 参数落派发（只认字符串 ref/value）。
fn rr3_key_call(args: &Value) -> Rr3PanelDispatch {
    match (
        args.get("ref").and_then(Value::as_str),
        args.get("value").and_then(Value::as_str),
    ) {
        (Some(ref_name), Some(value)) => Rr3PanelDispatch::ModelKeySet {
            ref_name: ref_name.to_string(),
            value: value.to_string(),
        },
        _ => Rr3PanelDispatch::BadRequest("缺字符串参数 ref/value".to_string()),
    }
}
