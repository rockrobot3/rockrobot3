#![cfg(unix)]
//! `rr3.call` 的线级用例（M3）：真 rrd 进程 + 两条裸连接（调用方 / 被调方）。
//!
//! 钉的是路由协议的**线上形状**：
//! - 转发的 Request 帧：src = 调用方 task、dst = 被调方 task、**trace_id = 调用方 task**、
//!   body 原样（被调方按 body 里的 JSON-RPC id 应答）；
//! - 上行 Response 的回路：trace_id 原样带回，rrd 只改写 dst；
//! - 三种拒法（无人在册 / 自呼 / trace_id=0）都回错误帧且**不断连**（下一帧还能正常服务）。
//!
//! 复用 register_publish.rs 的裸帧口径（support 夹具），不引 rr-bus client——客户端侧
//! 的行为已由 rr-bus 自己的假 rrd 用例覆盖，这里只证 rrd 这一半。

mod support;

use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3Header, Rr3PayloadType, Rr3StatusCode,
};
use serde_json::{json, Value};

use support::{
    rr3_json_of, rr3_read_where, rr3_register_frame, rr3_rpc_frame, rr3_send, RrdClient, RrdTest,
};

/// 带 `[rpc].exposes` 的 manifest 文本（exposes 为空 = 无 [rpc] 段）
fn manifest_exposes(name: &str, exposes: &[&str]) -> String {
    let rpc = if exposes.is_empty() {
        String::new()
    } else {
        let list = exposes
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[rpc]\nexposes = [{list}]\n\n")
    };
    format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/bin/{name}"

[events]
publishes = []
subscribes = ["rr3-runtime/module/state"]

{rpc}[resources]
restart_policy = "never"

[health]
heartbeat_ms = 200
watchdog_timeout_ms = 1000
"#
    )
}

/// 注册并取回 rrd 分配的 task 号（应答里 result.src_task 是唯一来源）
fn register(c: &mut RrdClient, manifest: &str, name: &str) -> u64 {
    rr3_send(c, &rr3_register_frame(manifest, name));
    let v = {
        let f = rr3_read_where(c, &format!("注册应答 {name}"), |f| {
            rr3_body_split(&f.payload)
                .map(|(ty, _)| ty == Rr3PayloadType::Response)
                .unwrap_or(false)
        });
        rr3_json_of(&f)
    };
    assert!(v.get("error").is_none(), "注册 {name} 被拒: {v}");
    v["result"]["src_task"]
        .as_u64()
        .unwrap_or_else(|| panic!("注册应答缺 src_task: {v}"))
}

/// 被调方视角：等转发来的 rr3.call（Request + method=rr3.call），返回 (帧, 体)
fn read_forwarded_call(c: &mut RrdClient) -> (Rr3Frame, Value) {
    let f = rr3_read_where(c, " 转发的 rr3.call", |f| {
        match rr3_body_split(&f.payload) {
            Ok((Rr3PayloadType::Request, body)) => {
                matches!(
                    serde_json::from_slice::<Value>(&body).ok().and_then(|v| v["method"].as_str().map(str::to_string)),
                    Some(m) if m == rr_bus::RR3_METHOD_CALL
                )
            }
            _ => false,
        }
    });
    let v = rr3_json_of(&f);
    (f, v)
}

/// 上行 Response 帧（trace_id 是应答回路的凭据）
fn send_response(c: &mut RrdClient, trace_id: u64, src: u64, id: u64, result: &Value) {
    let payload = rr3_body_encode(
        Rr3PayloadType::Response,
        json!({"jsonrpc": "2.0", "id": id, "result": result})
            .to_string()
            .as_bytes(),
    );
    rr3_send(
        c,
        &Rr3Frame {
            header: Rr3Header {
                opcode: opcode::SEND,
                flags: 0,
                trace_id,
                timestamp_ns: 0,
                src_task: src,
                dst_task: rr_bus::RR3_TASK_RRD,
                payload_len: payload.len() as u32,
                payload_crc32: rr_ipc::rr3_crc32(&payload),
            },
            payload,
        },
    );
}

/// 全链路 round trip：调用方 → rrd（转发）→ 被调方 → rrd（改道）→ 调用方
#[test]
fn call_routes_to_callee_and_response_comes_back() {
    let rrd = RrdTest::rr3_start("mcall-rt");
    let mut callee = rrd.rr3_connect();
    let task_c = register(
        &mut callee,
        &manifest_exposes("rr3-interact", &["rr3-interact/tts/text"]),
        "rr3-interact",
    );
    let mut caller = rrd.rr3_connect();
    let task_r = register(
        &mut caller,
        &manifest_exposes("rr3-sense", &[]),
        "rr3-sense",
    );

    rr3_send(
        &mut caller,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            42,
            json!({"topic": "rr3-interact/tts/text", "args": {"text": "hi"}}),
            task_r,
        ),
    );
    // 被调方收到的转发帧：头三处改写 + body 原样（id/params 原封不动）
    let (fwd, v) = read_forwarded_call(&mut callee);
    assert_eq!(
        fwd.header.src_task, task_r,
        "转发帧的 src 必须是调用方 task"
    );
    assert_eq!(
        fwd.header.dst_task, task_c,
        "转发帧的 dst 必须是被调方 task"
    );
    assert_eq!(
        fwd.header.trace_id, task_r,
        "trace_id = 调用方 task（回路的唯一凭据）"
    );
    assert_eq!(v["id"], 42, "body 原样：JSON-RPC id 不许被 rrd 重写");
    assert_eq!(v["params"]["topic"], "rr3-interact/tts/text");
    assert_eq!(v["params"]["args"]["text"], "hi");

    // 被调方沿回路应答：trace_id 原样带回
    send_response(
        &mut callee,
        fwd.header.trace_id,
        task_c,
        42,
        &json!({"echoed": "hi"}),
    );
    let back = rr3_read_where(&mut caller, " 回程应答 id=42", |f| match rr3_body_split(
        &f.payload,
    ) {
        Ok((Rr3PayloadType::Response, body)) => serde_json::from_slice::<Value>(&body)
            .map(|v| v["id"].as_u64() == Some(42))
            .unwrap_or(false),
        _ => false,
    });
    let v = rr3_json_of(&back);
    assert_eq!(v["result"]["echoed"], "hi");
    assert_eq!(back.header.src_task, task_c, "回程帧的 src 是应答方 task");
    assert_eq!(back.header.dst_task, task_r, "rrd 把 dst 改写回调用方 task");
}

/// 没人 exposes 的 topic：Invalid 错误帧，连接不断（下一帧照常服务）
#[test]
fn call_unknown_topic_is_rejected_and_connection_survives() {
    let rrd = RrdTest::rr3_start("mcall-unknown");
    let mut caller = rrd.rr3_connect();
    let task_r = register(
        &mut caller,
        &manifest_exposes("rr3-sense", &[]),
        "rr3-sense",
    );

    rr3_send(
        &mut caller,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            43,
            json!({"topic": "rr3-interact/tts/nobody"}),
            task_r,
        ),
    );
    let f = rr3_read_where(
        &mut caller,
        " 无人在册的错误应答",
        |f| match rr3_body_split(&f.payload) {
            Ok((Rr3PayloadType::Response, body)) => serde_json::from_slice::<Value>(&body)
                .map(|v| v["id"].as_u64() == Some(43) && v["error"].is_object())
                .unwrap_or(false),
            _ => false,
        },
    );
    let v = rr3_json_of(&f);
    assert_eq!(
        v["error"]["code"],
        Rr3StatusCode::Invalid as i32,
        "无人在册是路由事实，不是忙: {v}"
    );
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("没有在册模块 exposes"),
        "{v}"
    );

    // 连接不断：同一连接再发一颗心跳，照常 accepted
    rr3_send(
        &mut caller,
        &rr3_rpc_frame("rr3.heartbeat", 44, json!({}), task_r),
    );
    let v = {
        let f = rr3_read_where(&mut caller, " 心跳应答", |f| {
            match rr3_body_split(&f.payload) {
                Ok((Rr3PayloadType::Response, body)) => serde_json::from_slice::<Value>(&body)
                    .map(|v| v["id"].as_u64() == Some(44))
                    .unwrap_or(false),
                _ => false,
            }
        });
        rr3_json_of(&f)
    };
    assert_eq!(
        v["result"]["beat"], "accepted",
        "拒帧不断连：被拒后连接必须还能服务"
    );
}

/// 自呼拒：唯一 exposes 该 topic 的模块就是调用方自己
#[test]
fn call_to_self_is_rejected() {
    let rrd = RrdTest::rr3_start("mcall-self");
    let mut caller = rrd.rr3_connect();
    let task_r = register(
        &mut caller,
        &manifest_exposes("rr3-sense", &["rr3-sense/audio/pcm"]),
        "rr3-sense",
    );

    rr3_send(
        &mut caller,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            45,
            json!({"topic": "rr3-sense/audio/pcm"}),
            task_r,
        ),
    );
    let f = rr3_read_where(&mut caller, " 自呼的错误应答", |f| match rr3_body_split(
        &f.payload,
    ) {
        Ok((Rr3PayloadType::Response, body)) => serde_json::from_slice::<Value>(&body)
            .map(|v| v["id"].as_u64() == Some(45) && v["error"].is_object())
            .unwrap_or(false),
        _ => false,
    });
    let v = rr3_json_of(&f);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("自呼"),
        "{v}"
    );
}

/// trace_id = 0 的上行 Response：没有回路，拒帧不断连
#[test]
fn response_without_trace_id_is_rejected() {
    let rrd = RrdTest::rr3_start("mcall-trace0");
    let mut callee = rrd.rr3_connect();
    let task_c = register(
        &mut callee,
        &manifest_exposes("rr3-interact", &["rr3-interact/tts/text"]),
        "rr3-interact",
    );

    send_response(&mut callee, 0, task_c, 99, &json!({"orphan": true}));
    let f = rr3_read_where(&mut callee, " 缺回路的错误应答", |f| match rr3_body_split(
        &f.payload,
    ) {
        Ok((Rr3PayloadType::Response, body)) => serde_json::from_slice::<Value>(&body)
            .map(|v| v["id"].as_u64() == Some(99) && v["error"].is_object())
            .unwrap_or(false),
        _ => false,
    });
    let v = rr3_json_of(&f);
    assert_eq!(v["error"]["code"], Rr3StatusCode::Invalid as i32);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("trace_id"),
        "{v}"
    );
}
