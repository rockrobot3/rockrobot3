//! 请求帧 → 请求视图（板侧协议门）。
//!
//! 门序（全过才 `Ok`，任一失败 `Err` = 板侧**丢弃不回帧**，fail-closed）：
//! 帧已由 rr-ipc 还原（magic/长度校验）→ `dst` 门（非本板位形丢弃）→ payload
//! 前缀门（非 Request 丢弃）→ JSON 门 → `id` 门。半帧失败后流上可能残留尾巴
//! 字节，下一轮 magic 窥视大概率落行面 ERR，自愈路径在案。

use rr_ipc::{rr3_body_split, Rr3Frame, Rr3PayloadType};
use serde_json::Value;

use crate::consts::RR3_PERIPH_TASK;

/// 帧面请求（协议门全过后的视图）：id/method/params 摘出，帧头位形随行携带
/// （应答回带 `trace_id` / `dst = src_task`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Rr3PeriphRequest {
    pub id: u64,
    pub method: String,
    pub topic: String,
    pub args: Value,
    pub src_task: u64,
    pub trace_id: u64,
}

/// 请求帧解析（纯函数，不碰 io）：协议门全过才返回视图。
pub fn rr3_periph_request_parse(frame: &Rr3Frame) -> Result<Rr3PeriphRequest, String> {
    if frame.header.dst_task != RR3_PERIPH_TASK {
        return Err(format!(
            "dst_task={} 非本板位形 {RR3_PERIPH_TASK}",
            frame.header.dst_task
        ));
    }
    let (ty, body) =
        rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if ty != Rr3PayloadType::Request {
        return Err(format!("payload 类型非 Request: {ty:?}"));
    }
    let v: Value = serde_json::from_slice(&body).map_err(|e| format!("请求体非 JSON: {e}"))?;
    let id = v.get("id").and_then(Value::as_u64).ok_or("请求缺 id")?;
    let method = v
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let (topic, args) = match v.get("params") {
        Some(p) => (
            p.get("topic")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            p.get("args").cloned().unwrap_or(Value::Null),
        ),
        None => (String::new(), Value::Null),
    };
    Ok(Rr3PeriphRequest {
        id,
        method,
        topic,
        args,
        src_task: frame.header.src_task,
        trace_id: frame.header.trace_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_call::rr3_bridge_call_frame;
    use crate::consts::RR3_SERVO_TOPIC;
    use rr_ipc::{opcode, rr3_body_encode, Rr3Header};
    use serde_json::json;

    #[test]
    fn parse_extracts_envelope_from_host_call_frame() {
        let frame = rr3_bridge_call_frame(11, RR3_SERVO_TOPIC, &json!({ "angle_deg": 95 }));
        let req = rr3_periph_request_parse(&frame).expect("宿主桥帧必须过全部门");
        assert_eq!(req.id, 11);
        assert_eq!(req.method, "rr3.call");
        assert_eq!(req.topic, RR3_SERVO_TOPIC);
        assert_eq!(req.args["angle_deg"], 95);
        assert_eq!(req.src_task, crate::consts::RR3_TASK_HOST);
        assert_eq!(req.trace_id, crate::consts::RR3_TRACE_DIRECT);
    }

    #[test]
    fn parse_gates_reject_dst_type_and_missing_id() {
        let frame = rr3_bridge_call_frame(1, RR3_SERVO_TOPIC, &json!({}));
        // dst 门
        let mut wrong_dst = frame.clone();
        wrong_dst.header.dst_task = 9;
        let e = rr3_periph_request_parse(&wrong_dst).unwrap_err();
        assert!(e.contains("非本板位形"), "{e}");
        // payload 前缀门
        let mut wrong_ty = frame.clone();
        wrong_ty.payload =
            rr3_body_encode(Rr3PayloadType::Event, br#"{"jsonrpc":"2.0","id":1}"#);
        let e = rr3_periph_request_parse(&wrong_ty).unwrap_err();
        assert!(e.contains("非 Request"), "{e}");
        // id 门
        let payload = rr3_body_encode(Rr3PayloadType::Request, br#"{"jsonrpc":"2.0","method":"rr3.call"}"#);
        let mut no_id = frame.clone();
        no_id.payload = payload;
        let e = rr3_periph_request_parse(&no_id).unwrap_err();
        assert!(e.contains("缺 id"), "{e}");
    }

    /// 缺 params 的请求：topic/method 落空串、args 落 Null（分发面据此拒收，
    /// 解析面不猜形状）。
    #[test]
    fn parse_tolerates_missing_params_with_empty_view() {
        let body = br#"{"jsonrpc":"2.0","id":5}"#;
        let payload = rr3_body_encode(Rr3PayloadType::Request, body);
        let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
        hdr.dst_task = RR3_PERIPH_TASK;
        hdr.src_task = crate::consts::RR3_TASK_HOST;
        let frame = Rr3Frame { header: hdr, payload };
        let req = rr3_periph_request_parse(&frame).expect("最小信封可解析");
        assert_eq!(req.id, 5);
        assert_eq!(req.method, "");
        assert_eq!(req.topic, "");
        assert_eq!(req.args, Value::Null);
    }
}
