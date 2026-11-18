//! 读口应答族：status/counters/logs/wiring 走 ctl 查询，自检与重启臂转 selftest。

use serde_json::json;

use super::super::server::{rr3_ctl_query, Rr3PanelDispatch, Rr3PanelShared};
use super::selftest::rr3_exec_selftest;
use super::Rr3PanelReply;

/// 读口应答：全部经 ctl 查询或总线取数，只读不动运行态。接线图与 reboot 的
/// 状态码差异在各自 `rr3_exec_*` 里定形（数据态/拒因语义归 rrd）。
pub(super) fn rr3_reply_read(
    shared: &Rr3PanelShared,
    d: &Rr3PanelDispatch,
) -> Option<Rr3PanelReply> {
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
