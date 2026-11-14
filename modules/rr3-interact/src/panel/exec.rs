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

use serde_json::Value;

use proto::{rr3_reply_miss, rr3_reply_proto};
use read::rr3_reply_read;
use write::rr3_reply_write;

use super::server::{rr3_panel_route, Rr3HttpRequest, Rr3PanelDispatch, Rr3PanelShared};
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

mod act;
mod proto;
mod read;
mod selftest;
mod write;

#[cfg(test)]
pub(super) use selftest::{rr3_leg_relay, rr3_relay_expect, rr3_selftest_verdict};
#[cfg(test)]
pub(super) use write::rr3_bus_reply_status;
