//! 面板的 ctl 短连面（ADR-0010）：一问一答即断，与外部运维探针同权。

use super::*;

/// ctl 短连一问一答（ADR-0010 的面板侧执行面）：裸 UDS 连 rrd，发一条
/// `rr3.ctl.*` 请求（src=0，与外部运维探针同权），收一条应答即断。
/// 错误信封折 `Err`（HTTP 层转 502 语义的 JSON），成功回 result 对象。
/// 读耐心 = [`Rr3PanelLimits::read_timeout`]（I-18②：调用方自 shared 取）。
pub fn rr3_ctl_query(
    bus_path: &str,
    method: &str,
    id: u64,
    read_timeout: Duration,
) -> Result<Value, String> {
    rr3_ctl_send(bus_path, method, &json!({}), id, read_timeout)
}

/// ctl 短连底层（批N22 件一自 [`rr3_ctl_query`] 拆分）：带 params 透传面——
/// 装配侧写口 body 原样转发（参数裁决在 rrd 写口门序单点，第二真值点禁令）；
/// [`rr3_ctl_query`] 是它的空参数特例（八处既有调用点零行为漂移）。
pub fn rr3_ctl_send(
    bus_path: &str,
    method: &str,
    params: &Value,
    id: u64,
    read_timeout: Duration,
) -> Result<Value, String> {
    let mut stream = rr3_ctl_conn(bus_path, read_timeout)?;
    rr3_ctl_roundtrip(&mut stream, method, params, id)
}

/// 连总线短连并设读耐心。
fn rr3_ctl_conn(
    bus_path: &str,
    read_timeout: Duration,
) -> Result<std::os::unix::net::UnixStream, String> {
    let stream = std::os::unix::net::UnixStream::connect(bus_path)
        .map_err(|e| format!("连总线失败: {e}"))?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|e| format!("设读超时失败: {e}"))?;
    Ok(stream)
}

/// 一问一答：发 Request 帧、收一条应答帧。
fn rr3_ctl_roundtrip(
    stream: &mut std::os::unix::net::UnixStream,
    method: &str,
    params: &Value,
    id: u64,
) -> Result<Value, String> {
    let frame = rr3_ctl_frame(method, params, id);
    rr3_frame_write(&mut *stream, &frame).map_err(|e| format!("写 ctl 请求失败: {e:?}"))?;
    let frame = rr3_frame_read_cap(&mut *stream, RR3_FRAMING_PLEN_CAP_DEFAULT)
        .map_err(|e| format!("读 ctl 应答失败: {e:?}"))?;
    rr3_ctl_reply(frame, method)
}

/// ctl 请求帧装配（jsonrpc 信封 + 帧头）。
fn rr3_ctl_frame(method: &str, params: &Value, id: u64) -> Rr3Frame {
    let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    let payload = rr3_body_encode(Rr3PayloadType::Request, body.to_string().as_bytes());
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: 0, // ctl 连接没有 task，与外部运维探针同一写法
            dst_task: rr_core::RR3_TASK_RRD,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 应答帧解码：类型必须是 Response。
fn rr3_ctl_reply(frame: Rr3Frame, method: &str) -> Result<Value, String> {
    let (ty, payload) = Rr3BusClient::rr3_frame_body(&frame)?;
    if ty != Rr3PayloadType::Response {
        return Err(format!("ctl 应答类型错误: {ty:?}"));
    }
    rr3_ctl_result(payload, method)
}

/// 应答体折 result：错误信封折 Err，缺 result 也回错。
fn rr3_ctl_result(payload: Value, method: &str) -> Result<Value, String> {
    if let Some(err) = payload.get("error") {
        return Err(format!(
            "ctl {method} 被拒: {}",
            err.get("message").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    payload
        .get("result")
        .cloned()
        .ok_or_else(|| format!("ctl {method} 应答缺 result"))
}
