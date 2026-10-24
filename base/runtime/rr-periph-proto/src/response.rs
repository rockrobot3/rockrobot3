//! 应答组帧（板侧）+ 应答解析（宿主侧）。
//!
//! 板侧：`result` 在位（业务性失败折 `{"error": 文案}`——宿主解析契约），
//! `trace_id` 原样回带、`src` = 本板位形、`dst` = 调用方。
//!
//! 宿主侧：Response 类型门 → `jsonrpc` 2.0 → `id` 回显 → `result` 在位 →
//! `result.error` 折 Err（板侧业务失败的协议面锚）。

use rr_ipc::{opcode, rr3_body_encode, Rr3Frame, Rr3Header, Rr3PayloadType};
use serde_json::{json, Value};

use crate::consts::RR3_PERIPH_TASK;
use crate::request::Rr3PeriphRequest;

/// 应答帧构造（纯函数）：`now_ns` 由调用方平台供给（板侧 esp_timer、宿主
/// SystemTime）——时间戳只作诊断参考，不参与校验。
pub fn rr3_periph_response_frame(
    req: &Rr3PeriphRequest,
    result: Result<Value, String>,
    now_ns: u64,
) -> Rr3Frame {
    let result = match result {
        Ok(v) => v,
        Err(e) => json!({ "error": e }),
    };
    let body = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
    let payload = rr3_body_encode(Rr3PayloadType::Response, body.to_string().as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = req.trace_id;
    hdr.timestamp_ns = now_ns;
    hdr.src_task = RR3_PERIPH_TASK;
    hdr.dst_task = req.src_task;
    Rr3Frame {
        header: hdr,
        payload,
    }
}

/// 应答解析（纯函数）：payload 类型必须 Response、`jsonrpc` 必须 2.0、id 必须
/// 回显一致、result 在位；`result.error`（板侧业务性失败折形状）折 Err 点名。
pub fn rr3_bridge_parse_response(
    got_ty: Rr3PayloadType,
    body: &[u8],
    want_id: u64,
) -> Result<Value, String> {
    if got_ty != Rr3PayloadType::Response {
        return Err(format!("payload 类型非 Response: {got_ty:?}"));
    }
    let v: Value = serde_json::from_slice(body).map_err(|e| format!("应答体非 JSON: {e}"))?;
    if v.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("jsonrpc 必须为 \"2.0\"".into());
    }
    if v.get("id").and_then(Value::as_u64) != Some(want_id) {
        return Err(format!("id 回显不一致（要 {want_id}）"));
    }
    let result = v.get("result").cloned().ok_or("缺 result")?;
    if let Some(e) = result.get("error").and_then(Value::as_str) {
        return Err(format!("板侧业务失败: {e}"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_call::rr3_bridge_call_frame;
    use crate::request::rr3_periph_request_parse;
    use rr_ipc::rr3_body_split;
    use serde_json::json;

    /// 板侧应答 → 宿主解析：字节级往返一致（组帧与解析是同 crate 直接对拍）。
    #[test]
    fn response_roundtrip_ok_and_error_shapes() {
        let req = rr3_periph_request_parse(&rr3_bridge_call_frame(
            21,
            crate::consts::RR3_SERVO_TOPIC,
            &json!({ "angle_deg": 90 }),
        ))
        .expect("请求可解析");

        let resp = rr3_periph_response_frame(&req, Ok(json!({ "angle_deg": 90 })), 1234);
        assert_eq!(resp.header.src_task, RR3_PERIPH_TASK);
        assert_eq!(resp.header.dst_task, crate::consts::RR3_TASK_HOST);
        assert_eq!(resp.header.trace_id, req.trace_id);
        assert_eq!(resp.header.timestamp_ns, 1234);
        let (ty, body) = rr3_body_split(&resp.payload).expect("payload 前缀合法");
        let v = rr3_bridge_parse_response(ty, &body, 21).expect("宿主应答门全过");
        assert_eq!(v["angle_deg"], 90);

        // 业务失败折 {"error": ...} → 宿主门折 Err 并保留原文
        let bad = rr3_periph_response_frame(&req, Err("角度超范围 0-180: 200".into()), 0);
        let (ty, body) = rr3_body_split(&bad.payload).expect("payload 前缀合法");
        let e = rr3_bridge_parse_response(ty, &body, 21).unwrap_err();
        assert!(e.contains("板侧业务失败"), "{e}");
        assert!(e.contains("角度超范围"), "{e}");
    }

    #[test]
    fn response_parse_gates() {
        let ok = json!({"jsonrpc": "2.0", "id": 3, "result": {"angle_deg": 90}});
        let (ty, raw) = (
            Rr3PayloadType::Response,
            rr3_body_encode(Rr3PayloadType::Response, ok.to_string().as_bytes()),
        );
        assert_eq!(
            rr3_bridge_parse_response(ty, &raw[2..], 3).unwrap()["angle_deg"],
            90
        );
        // 类型错
        let ev = rr3_body_encode(Rr3PayloadType::Event, ok.to_string().as_bytes());
        assert!(rr3_bridge_parse_response(Rr3PayloadType::Event, &ev[2..], 3).is_err());
        // id 错
        assert!(rr3_bridge_parse_response(ty, &raw[2..], 4).is_err());
        // 缺 result
        let no_res = json!({"jsonrpc": "2.0", "id": 3});
        let nr = rr3_body_encode(ty, no_res.to_string().as_bytes());
        assert!(rr3_bridge_parse_response(ty, &nr[2..], 3).is_err());
        // jsonrpc 版本错
        let bad_ver = json!({"jsonrpc": "1.0", "id": 3, "result": {}});
        let bv = rr3_body_encode(ty, bad_ver.to_string().as_bytes());
        assert!(rr3_bridge_parse_response(ty, &bv[2..], 3).is_err());
    }
}
