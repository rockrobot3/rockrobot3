//! 宿主侧请求组帧（自 `rr-hal::esp32_uart` 平迁，函数名与信封形状不变）。
//!
//! 信封：`{"jsonrpc":"2.0","id":N,"method":M,"params":P}`——`rr3.call` 与
//! `rr3.set_bindings` 同走本信封（method 与 params 原样内嵌）。帧头：SEND /
//! direct trace / `src` = 宿主 1 / `dst` = 板 2。
//!
//! 平台无关：时间戳取 `SystemTime`（std 面），宿主与 espidf 固件目标都可编译；
//! 固件侧不用本模块（板侧应答组帧走 [`crate::response`]）。

use std::time::{SystemTime, UNIX_EPOCH};

use rr_ipc::{opcode, rr3_body_encode, Rr3Frame, Rr3Header, Rr3PayloadType};
use serde_json::{json, Value};

use crate::consts::{RR3_PERIPH_TASK, RR3_TASK_HOST, RR3_TRACE_DIRECT};

/// 墙钟纳秒（帧头 timestamp；取不到 epoch 时落 0——时间戳不参与校验，只作
/// 诊断参考，不猜不造）。
fn rr3_bridge_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// JSON-RPC 请求体（通用 method 面）：method 与 params 原样内嵌——`rr3.call` 与
/// `rr3.set_bindings`（基-24）同走本信封。独立纯函数：形状可单测。
pub fn rr3_bridge_request_body_params(id: u64, method: &str, params: &Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

/// 请求帧构造：body → Request payload → 帧头（SEND / direct trace / src=宿主 1 /
/// dst=板 2）。独立纯函数：形状可单测。
pub fn rr3_bridge_request_frame(id: u64, method: &str, params: &Value) -> Rr3Frame {
    let body = rr3_bridge_request_body_params(id, method, params);
    let payload = rr3_body_encode(Rr3PayloadType::Request, body.as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = RR3_TRACE_DIRECT;
    hdr.timestamp_ns = rr3_bridge_now_ns();
    hdr.src_task = RR3_TASK_HOST;
    hdr.dst_task = RR3_PERIPH_TASK;
    Rr3Frame {
        header: hdr,
        payload,
    }
}

/// 主题点名请求帧：[`rr3_bridge_request_frame`] 的 `rr3.call` 信封形
/// （topic 在 `params.topic`、args 在 `params.args`）。
pub fn rr3_bridge_call_frame(id: u64, topic: &str, args: &Value) -> Rr3Frame {
    rr3_bridge_request_frame(id, "rr3.call", &json!({ "topic": topic, "args": args }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::RR3_SERVO_TOPIC;

    /// 主题点名信封形状：method 恒 rr3.call、主题在 params.topic、args 原样内嵌
    /// （rrd 转发形状的分发依据；漂移此处即红）。
    #[test]
    fn call_frame_is_rr3_call_with_topic_in_params() {
        let frame = rr3_bridge_call_frame(7, RR3_SERVO_TOPIC, &json!({ "angle_deg": 90 }));
        assert_eq!(frame.header.src_task, RR3_TASK_HOST);
        assert_eq!(frame.header.dst_task, RR3_PERIPH_TASK);
        assert_eq!(frame.header.trace_id, RR3_TRACE_DIRECT);
        let (ty, body) = rr_ipc::rr3_body_split(&frame.payload).expect("payload 前缀合法");
        assert_eq!(ty, Rr3PayloadType::Request);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["method"], "rr3.call");
        assert_eq!(v["params"]["topic"], RR3_SERVO_TOPIC);
        assert_eq!(v["params"]["args"]["angle_deg"], 90);
    }

    /// 通用 method 面信封：method/params 原样内嵌（rr3.set_bindings 的协议形状锚）。
    #[test]
    fn request_body_params_is_generic_method_envelope() {
        let s = rr3_bridge_request_body_params(9, "rr3.set_bindings", &json!({ "args": {} }));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 9);
        assert_eq!(v["method"], "rr3.set_bindings");
        assert_eq!(v["params"]["args"], json!({}));
        assert!(v.get("result").is_none(), "请求信封不携带 result");
    }
}
