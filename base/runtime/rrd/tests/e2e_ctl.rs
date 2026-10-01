#![cfg(unix)]
//! F3（ADR-0010）：`rr3.ctl.*` 控制查询面的线级用例。
//!
//! 判据形状：
//! - ctl 连接**不注册**也能得到应答（握手旁支），且一问一答即断（应答后连接被收掉）；
//! - `status` / `health` 的数据来自在册表快照，列出的必须只有真注册的模块——
//!   ctl 连接自己**不得**出现在快照里（裁定 2：运维探针不进账本）；
//! - `logs` 读 rrd 日志环，启动横幅（`监听 …`）必须在环里；
//! - `ota` 恒拒（AIRY_EPERM + 文案点 M6）；
//! - 已注册模块连接上行 `rr3.ctl.*` 被拒（Permission 错误帧）但**连接不断**
//!   （与 src_task 伪造同一口径：一次笔误不掉线）。

mod support;

use support::{
    rr3_drain_until_closed, rr3_json_of, rr3_manifest, rr3_read_rpc, rr3_read_where,
    rr3_register_frame, rr3_rpc_frame, rr3_send, RrdClient, RrdTest,
};

use rr_ipc::{rr3_body_split, Rr3PayloadType, Rr3StatusCode};

/// 发一帧 ctl 请求（src 按契约填 0，与注册帧同口径），收回第一条 Response 体
fn rr3_ask_ctl(c: &mut RrdClient, method: &str, id: u64) -> serde_json::Value {
    rr3_send(c, &rr3_rpc_frame(method, id, serde_json::json!({}), 0));
    let f = rr3_read_where(c, &format!("{method} 的应答"), |f| {
        rr3_body_split(&f.payload).map(|(ty, _)| ty == Rr3PayloadType::Response).unwrap_or(false)
    });
    rr3_json_of(&f)
}

/// ctl 一问一答即断：应答之后这条连接读不到更多帧（EOF / 读超时收尾）
fn rr3_assert_ctl_closed(c: &mut RrdClient) {
    let rest = rr3_drain_until_closed(c);
    assert!(
        rest.iter().all(|f| rr3_body_split(&f.payload).map(|(ty, _)| ty).unwrap_or(Rr3PayloadType::Event)
            != Rr3PayloadType::Response),
        "应答后不该再有 Response 帧，实际: {rest:?}"
    );
}

#[test]
fn ctl_status_lists_registered_modules_and_does_not_register_itself() {
    let t = RrdTest::rr3_start_no_overlay("ctl-status");
    let mut a = t.rr3_connect();
    rr3_send(&mut a, &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"));
    let reg = rr3_read_rpc(&mut a);
    let task = reg["result"]["src_task"].as_u64().expect("注册应答带 src_task");

    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.status", 1);
    let modules = v["result"]["modules"].as_array().expect("status 应带 result.modules");
    assert_eq!(modules.len(), 1, "只有 rr3-sima 在册——ctl 连接自己不得进快照: {v}");
    assert_eq!(modules[0]["name"], "rr3-sima");
    assert_eq!(modules[0]["task"], task);
    assert_eq!(modules[0]["phase"], "registered");
    assert!(v["result"]["denied_publish"].is_u64(), "denied_publish 计数必须在场: {v}");
    rr3_assert_ctl_closed(&mut ctl);
}

#[test]
fn ctl_health_echoes_heartbeat_and_watchdog_numbers() {
    let t = RrdTest::rr3_start_no_overlay("ctl-health");
    let mut a = t.rr3_connect();
    rr3_send(&mut a, &rr3_register_frame(&rr3_manifest("rr3-sima", 200, 900, "always"), "rr3-sima"));
    rr3_read_rpc(&mut a);

    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.health", 1);
    let rows = v["result"]["modules"].as_array().expect("health 应带 result.modules");
    assert_eq!(rows.len(), 1, "{v}");
    let r = &rows[0];
    // 配置回显条目**自己**的三个数（Rr3Expiry 同一条理据：判定用的数必须回显）
    assert_eq!(r["heartbeat_ms"], 200, "{v}");
    assert_eq!(r["watchdog_ms"], 900, "{v}");
    assert!(r["since_beat_ms"].is_u64(), "距上次心跳必须以数回显: {v}");
    assert_eq!(r["phase"], "registered");
    rr3_assert_ctl_closed(&mut ctl);
}

#[test]
fn ctl_logs_returns_ring_tail_containing_banner() {
    let t = RrdTest::rr3_start_no_overlay("ctl-logs");
    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.logs", 1);
    let lines = v["result"]["lines"].as_array().expect("logs 应带 result.lines");
    let joined: Vec<&str> = lines.iter().filter_map(|l| l.as_str()).collect();
    assert!(
        joined.iter().any(|l| l.contains("监听")),
        "启动横幅必须已在日志环里（rr3_log 单点落环）: {joined:?}"
    );
    rr3_assert_ctl_closed(&mut ctl);
}

#[test]
fn ctl_ota_is_always_refused_with_permission() {
    let t = RrdTest::rr3_start_no_overlay("ctl-ota");
    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.ota", 7);
    let err = &v["error"];
    assert!(
        err.is_object(),
        "ota 必须以错误信封应答（协议面定型、语义随 M6）: {v}"
    );
    // AIRY_EPERM = 3（契约冻结集；不新增 NotSupported 码）
    assert_eq!(err["code"], Rr3StatusCode::Permission as i64, "{v}");
    assert!(
        err["message"].as_str().unwrap_or_default().contains("M6"),
        "恒拒文案必须点 M6: {v}"
    );
    rr3_assert_ctl_closed(&mut ctl);
}

#[test]
fn registered_module_uplink_ctl_is_refused_but_connection_survives() {
    let t = RrdTest::rr3_start_no_overlay("ctl-uplink-deny");
    let mut a = t.rr3_connect();
    rr3_send(&mut a, &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"));
    let task = rr3_read_rpc(&mut a)["result"]["src_task"].as_u64().expect("注册应答带 src_task");

    // 注册后上行帧的 src_task 必须等于分配值（AOS-STD-IPC-024），ctl 上行也不例外
    rr3_send(&mut a, &rr3_rpc_frame("rr3.ctl.status", 9, serde_json::json!({}), task));
    let f = rr3_read_where(&mut a, "ctl 上行的拒绝应答", |f| {
        rr3_body_split(&f.payload).map(|(ty, _)| ty == Rr3PayloadType::Response).unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    assert_eq!(v["error"]["code"], Rr3StatusCode::Permission as i64, "{v}");

    // 同一连接仍活着：喂一次心跳，应答照常（一次笔误不该让整机掉线）
    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 10, serde_json::json!({}), task));
    let f = rr3_read_where(&mut a, "拒绝后的心跳应答", |f| {
        rr3_body_split(&f.payload).map(|(ty, _)| ty == Rr3PayloadType::Response).unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    assert!(v["result"].is_object(), "心跳应答必须是成功信封: {v}");
}
