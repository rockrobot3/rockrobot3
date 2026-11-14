//! 宿主对手件自测：帧形状、parity 向量与直方图分桶。

use super::*;

use rr_core::control::parity::{rr3_parity_run, RR3_PARITY_EXPECTED, RR3_PARITY_VERSION};
use rr_core::{rr3_frame_compose, RR3_TASK_RRD, RR3_TRACE_ID_DIRECT};
use rr_ipc::{opcode, rr3_body_encode, rr3_body_split, Rr3PayloadType};
use serde_json::{json, Value};

use crate::probe::{
    heartbeat_reply, parity_expect_json, parity_first_diff, parity_request, parse_upstream,
    register_reply, round_trip_request,
};
use crate::report::histogram_text;

/// 注册应答帧形状：src=rrd、dst=分配 task、Response 前缀 + JSON-RPC id 回显 +
/// result.src_task（rrd rr3_handshake 线上形状的对拍锚）。
#[test]
fn register_reply_frame_shape() {
    let f = register_reply(1, 2, 777);
    assert_eq!(f.header.opcode, opcode::SEND);
    assert_eq!(f.header.src_task, RR3_TASK_RRD);
    assert_eq!(f.header.dst_task, 2);
    assert_eq!(f.header.payload_len as usize, f.payload.len());
    let (ty, body) = rr3_body_split(&f.payload).unwrap();
    assert_eq!(ty, Rr3PayloadType::Response);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["src_task"], 2);
}

/// 心跳应答形状：{"beat": "accepted"}（rrd D-6 OK 臂同款）。
#[test]
fn heartbeat_reply_frame_shape() {
    let f = heartbeat_reply(5, 2, 1);
    let (ty, body) = rr3_body_split(&f.payload).unwrap();
    assert_eq!(ty, Rr3PayloadType::Response);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["id"], 5);
    assert_eq!(v["result"]["beat"], "accepted");
    assert!(v.get("error").is_none());
}

/// parse_upstream：对哨兵同款组帧（Request + rr3.register + params{name,manifest}）
/// 逐字段还原；dst_task 非法被点名（rrd 握手 dst 门同口径）。
#[test]
fn parse_upstream_round_trip_register() {
    let manifest = "[manifest]\nname = \"rr3-sentinel\"\n[health]\nheartbeat_ms = 200\n";
    let f = rr3_frame_compose(
        opcode::SEND,
        0,
        RR3_TRACE_ID_DIRECT,
        42,
        0, // 注册前 src=0（rr-bus client 同款）
        RR3_TASK_RRD,
        rr3_body_encode(
            Rr3PayloadType::Request,
            json!({"jsonrpc": "2.0", "id": 1, "method": "rr3.register",
                   "params": {"name": "rr3-sentinel", "manifest": manifest}})
            .to_string()
            .as_bytes(),
        ),
    );
    let (ty, id, method, params, _) = parse_upstream(f).unwrap();
    assert_eq!(ty, Rr3PayloadType::Request);
    assert_eq!(id, 1);
    assert_eq!(method.as_deref(), Some("rr3.register"));
    assert_eq!(params["name"], "rr3-sentinel");
    assert!(params["manifest"]
        .as_str()
        .unwrap()
        .contains("heartbeat_ms = 200"));

    // dst 门：dst≠1 的上行帧必须被点名拒（fail-closed）
    let bad = rr3_frame_compose(
        opcode::SEND,
        0,
        0,
        0,
        3,
        9,
        rr3_body_encode(Rr3PayloadType::Request, b"{}"),
    );
    assert!(parse_upstream(bad).unwrap_err().contains("dst_task"));
}

/// round-trip 下行请求形状：Request + rr3.call + params{topic,args}（rrd 转发同款）。
#[test]
fn round_trip_request_shape() {
    let f = round_trip_request(2, 9);
    assert_eq!(f.header.dst_task, 2);
    // M3 忠实形状：转发帧 src=调用方 task（conn.rs rr3_route_call 实锤：
    // trace_id=src=调用方、dst=被调方）——调用方即本 harness，非 rrd
    assert_eq!(f.header.src_task, HARNESS_TASK);
    let (ty, body) = rr3_body_split(&f.payload).unwrap();
    assert_eq!(ty, Rr3PayloadType::Request);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["id"], ROUND_TRIP_ID);
    assert_eq!(v["method"], "rr3.call");
    assert_eq!(v["params"]["args"]["ping"], "pong");
}

/// parity 下行请求形状（批G ④）：同款 rr3.call 形状，topic=diag/parity，
/// id=9002 与 probe 区分。method 不改写是 rrd 转发实锤（conn.rs L779/L816）。
#[test]
fn parity_request_shape() {
    let f = parity_request(2, 9);
    assert_eq!(f.header.dst_task, 2);
    assert_eq!(f.header.src_task, HARNESS_TASK);
    let (ty, body) = rr3_body_split(&f.payload).unwrap();
    assert_eq!(ty, Rr3PayloadType::Request);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["id"], PARITY_CALL_ID);
    assert_eq!(v["method"], "rr3.call");
    assert_eq!(v["params"]["topic"], "rr3-sentinel/diag/parity");
}

/// 期望形状函数与差分函数的自证：宿主 runner == 契约期望（契约腿在测试面
/// 也钉一道）；差分函数对逐字段篡改逐个点名。
#[test]
fn parity_expect_json_and_diff() {
    let host = parity_expect_json(&rr3_parity_run());
    let expected = parity_expect_json(&RR3_PARITY_EXPECTED);
    assert_eq!(host, expected, "runner 与期望向量漂移（契约腿红）");
    assert_eq!(host["v"], RR3_PARITY_VERSION);
    assert_eq!(host["rate"].as_array().map(Vec::len), Some(8));
    assert_eq!(host["plan"].as_array().map(Vec::len), Some(3));

    assert_eq!(parity_first_diff(&host, &expected), None);
    let mut bad = host.clone();
    bad["clamp"] = json!([0, 0, 0, 0, 0]);
    let d = parity_first_diff(&bad, &expected).unwrap();
    assert!(d.contains("clamp") && d.contains("不一致"), "{d}");
    let mut missing = host.clone();
    missing.as_object_mut().unwrap().remove("sat");
    let d = parity_first_diff(&missing, &expected).unwrap();
    assert!(d.contains("sat") && d.contains("缺"), "{d}");
}

/// ⑦ 直方图分桶标签：10ms 粒度、桶区间首尾正确（[190-199ms) 形状）。
#[test]
fn histogram_buckets_are_labeled_by_ms_range() {
    let text = histogram_text(&[195, 200, 205, 205, 310]);
    assert!(text.contains("[190-199ms)=1"), "{text}");
    assert!(text.contains("[200-209ms)=3"), "{text}");
    assert!(text.contains("[310-319ms)=1"), "{text}");
}
