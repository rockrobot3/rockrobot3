//! 协议层应答族：路由判决（404/405/400）与自述（index/healthz/version）。

use serde_json::json;

use super::super::server::{Rr3PanelDispatch, RR3_PANEL_ROUTES};
use super::Rr3PanelReply;

/// 未接线兜底：新增分派变体没有并入三域时的唯一出口（fail-closed，点名不断言）
pub(super) fn rr3_reply_miss() -> Rr3PanelReply {
    Rr3PanelReply::json(
        500,
        "Internal Server Error",
        json!({ "error": "面板分派臂未接线（新变体须并入三域之一）" }),
    )
}

/// 协议层应答：路由判决（404/405/400）与自述（index/healthz/version）——
/// 零总线依赖，纯常量应答。
pub(super) fn rr3_reply_proto(d: &Rr3PanelDispatch) -> Option<Rr3PanelReply> {
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
