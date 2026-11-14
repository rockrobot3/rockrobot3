//! 握手面：首帧形状/dst 门禁与静默连接的收尾。

use super::support::*;
use rr_bus::RR3_TASK_RRD;
use rr_ipc::{rr3_body_split, Rr3PayloadType, Rr3StatusCode};
use serde_json::json;
use std::time::Duration;

/// 首帧必须是 rr3.register；心跳不能抢跑（legacy 教训 76c0ad8 的线上级写法）
#[test]
fn first_frame_must_be_register() {
    let mut rrd = RrdTest::rr3_start("order");
    let mut c = rrd.rr3_connect();
    rr3_send(&mut c, &rr3_rpc_frame("rr3.heartbeat", 1, json!({}), 0));
    let v = rr3_read_rpc(&mut c);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("rr3.register"),
        "{v}"
    );
    assert!(rrd.rr3_wait_log("注册被拒").contains("rr3.register"));

    let mut d = rrd.rr3_connect();
    rr3_send(
        &mut d,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(
        rr3_read_rpc(&mut d)["result"]["src_task"],
        2,
        "被拒连接之后守护必须照常接受注册"
    );
}

/// 注册后伪造 src_task：拒这一帧、回原 id、不断连、不牵连后续心跳
#[test]
fn spoofed_src_task_is_rejected_without_killing_connection() {
    let mut rrd = RrdTest::rr3_start("spoof");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(rr3_read_rpc(&mut a)["result"]["src_task"], 2);

    let f = {
        rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 7, json!({}), 99));
        rr3_read_rpc_frame(&mut a)
    };
    let v = rr3_json_of(&f);
    assert_eq!(v["id"], 7, "拒绝要回到请求者给的 id");
    assert_eq!(v["error"]["code"], Rr3StatusCode::Invalid as i32);
    assert!(
        v["error"]["message"].as_str().unwrap().contains("src_task"),
        "{v}"
    );
    assert_eq!(f.header.dst_task, 2, "拒绝也只回给这一条连接");

    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 8, json!({}), 2));
    assert_eq!(
        rr3_read_rpc(&mut a)["result"]["beat"],
        "accepted",
        "一次伪造不该杀掉整条连接"
    );
    assert!(rrd
        .rr3_logs(Duration::from_millis(100))
        .contains("src_task"));
}

/// 上行只许 Request：把心跳塞进 Notification 帧里上来 → 拒（分派依据是前缀，不是内容）
#[test]
fn upstream_must_use_request_type() {
    let rrd = RrdTest::rr3_start("type");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    let body = rr3_request("rr3.heartbeat", 3, json!({}));
    rr3_send(
        &mut a,
        &rr3_json_frame(Rr3PayloadType::Notification, &body, 2, RR3_TASK_RRD),
    );
    let v = rr3_read_rpc(&mut a);
    assert_eq!(v["error"]["code"], Rr3StatusCode::Invalid as i32);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("上行不支持 payload 类型"),
        "{v}"
    );

    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 4, json!({}), 2));
    assert_eq!(rr3_read_rpc(&mut a)["result"]["beat"], "accepted");
}

/// 连上却不注册：读超时收掉，且只是那一条连接的事
#[test]
fn silent_connection_times_out_and_rrd_survives() {
    let mut rrd = RrdTest::rr3_start("idle");
    let _idle = rrd.rr3_connect(); // 一帧不发；--pre-register-timeout-ms 300
    assert!(rrd.rr3_wait_log("连接收尾").contains("首帧读取失败"));

    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(rr3_read_rpc(&mut a)["result"]["src_task"], 2);
}

/// 首帧的 `dst_task` 必须是 rrd（评审 A-1/I-2 的 m08）：形状完全正确、只把 dst 填成 0 →
/// 拒 EINVAL 且文案点名 dst_task；这条被拒的连接不牵连后来的正常注册。
#[test]
fn first_frame_dst_must_be_rrd() {
    let mut rrd = RrdTest::rr3_start("dstfirst");
    let mut c = rrd.rr3_connect();
    // 与 rr3_register_frame 同形（src 按契约填 0），唯一差别是 dst_task=0 广播而不是 1=rrd
    let manifest = rr3_manifest("rr3-sima", 100, 500, "always");
    let body = rr3_request(
        "rr3.register",
        1,
        json!({"name": "rr3-sima", "manifest": manifest}),
    );
    rr3_send(
        &mut c,
        &rr3_json_frame(Rr3PayloadType::Request, &body, 0, 0),
    );
    // 握手期的拒答按对端自报的 src_task 回（conn.rs 的 rr3_refuse），所以照常用 rpc 读法就得到
    let v = rr3_read_rpc(&mut c);
    assert_eq!(
        v["error"]["code"],
        Rr3StatusCode::Invalid as i32,
        "dst 不是 rrd 的首帧该回 EINVAL（放行就等于替广播地址代收注册），实际 {v}"
    );
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("dst_task"),
        "文案必须点名 dst_task，实际 {v}"
    );
    assert!(rrd.rr3_wait_log("注册被拒").contains("dst_task"));

    let mut d = rrd.rr3_connect();
    rr3_send(&mut d, &rr3_register_frame(&manifest, "rr3-sima"));
    assert_eq!(
        rr3_read_rpc(&mut d)["result"]["src_task"],
        2,
        "被拒的 dst 错首帧不得占走 task 号，守护要照常接受注册"
    );
}

/// 注册后的上行 `dst_task` 也必须是 rrd（评审 A-1/I-2 的 m07）。这条路径是
/// `rr3_serve_frame` 返回 `Err` → 连接按崩溃收尾、**不回错误帧**，所以判据只能是否定式：
/// 连接被关掉之前不会收到 `beat=accepted` 的应答。删掉门禁即走到 `rr3_serve_request` 回 accepted。
#[test]
fn upstream_dst_must_be_rrd() {
    let mut rrd = RrdTest::rr3_start("updst");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(rr3_read_rpc(&mut a)["result"]["src_task"], 2);

    rr3_send(
        &mut a,
        &rr3_json_frame(
            Rr3PayloadType::Request,
            &rr3_request("rr3.heartbeat", 6, json!({})),
            2,
            0,
        ),
    );
    let got = rr3_drain_until_closed(&mut a);
    let accepted = got
        .iter()
        .filter(|f| {
            rr3_body_split(&f.payload)
                .map(|(ty, _)| ty == Rr3PayloadType::Response)
                .unwrap_or(false)
                && rr3_json_of(f)["result"]["beat"] == "accepted"
        })
        .count();
    assert_eq!(
        accepted,
        0,
        "dst=0 的上行心跳不得被代收（那是跨模块串话的门）；连接被关掉前收到 {} 帧",
        got.len()
    );
    // 崩溃收尾的原因文案点名门禁（`rr3_conn_finish` 的"退出（原因）"）
    assert!(rrd.rr3_wait_log("上行 dst_task").contains("上行 dst_task"));

    let mut b = rrd.rr3_connect();
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "always"), "rr3-simb"),
    );
    assert_eq!(
        rr3_read_rpc(&mut b)["result"]["src_task"],
        3,
        "崩掉一条连接之后守护照常发号"
    );
}
