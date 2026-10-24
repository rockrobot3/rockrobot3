//! 板外设语义命令 + 板帧分类（自 `rr-hal::esp32audio` 平迁）。
//!
//! - 命令面：[`Rr3PeriphCmd`] 语义命令 → 整帧，走 [`crate::host_call::
//!   rr3_bridge_call_frame`] 桥同形单点（`rr3.call` 信封、`src`=宿主 / `dst`=板、
//!   direct trace）——与舵机/显示桥逐字同源。
//! - 板帧分类：宿主收流把 Response（命令旁路回执）与 Event（音频上行）分流；
//!   音频载荷的门序与解码留在宿主消费面，本层只做协议级判别。

use rr_ipc::{rr3_body_split, Rr3Frame, Rr3Header, Rr3PayloadType};
use serde_json::{json, Value};

use crate::consts::{RR3_SERVO_TOPIC, RR3_TASK_HOST};
use crate::host_call::rr3_bridge_call_frame;

/// 板外设语义命令（旁路载荷；帧构造在生产/消费任一侧的单点，id/帧头/信封
/// 全走桥形状）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3PeriphCmd {
    /// 舵机 pan 轴绝对角 → `rr3-act/servo/command`，args
    /// `{"angle_deg":N,"axis":"pan"}`（payload 形状事实源
    /// contracts/events/act-servo_command.schema.json）。板侧行程收窄
    /// fail-closed：越界拒收点名，宿主侧不猜行程不预钳。
    ServoPan(u16),
    /// 舵机 tilt 轴绝对角 → 同主题 args `{"angle_deg":N,"axis":"tilt"}`
    /// （点头示意动作）。行程同样板侧裁决。
    ServoTilt(u16),
}

/// 语义命令 → 整帧（纯函数）：桥同形单点，见模块文档。
pub fn rr3_esp32_cmd_frame(id: u64, cmd: &Rr3PeriphCmd) -> Rr3Frame {
    let (topic, args) = match cmd {
        Rr3PeriphCmd::ServoPan(deg) => (
            RR3_SERVO_TOPIC,
            json!({ "angle_deg": *deg, "axis": "pan" }),
        ),
        Rr3PeriphCmd::ServoTilt(deg) => (
            RR3_SERVO_TOPIC,
            json!({ "angle_deg": *deg, "axis": "tilt" }),
        ),
    };
    rr3_bridge_call_frame(id, topic, &args)
}

/// 板帧协议级分类：Response = 命令旁路回执（板侧执行**前**回成功），其余
/// 交给消费面（音频四道门）继续判别。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3BoardFrame {
    /// 板侧请求应答：id 回显 + 业务结果（`err` = 板侧业务性失败文案）。
    Reply { id: u64, err: Option<String> },
    /// 非应答帧（上行事件），载荷门序归消费面。
    Event,
}

/// 板帧分类（门序：`dst` 门 → payload 前缀门 → Response 判别）。
pub fn rr3_board_frame_classify(
    hdr: &Rr3Header,
    payload: &[u8],
) -> Result<Rr3BoardFrame, String> {
    if hdr.dst_task != RR3_TASK_HOST {
        return Err(format!(
            "dst_task={} 非宿主位形 {RR3_TASK_HOST}",
            hdr.dst_task
        ));
    }
    let (ty, body) = rr3_body_split(payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if ty == Rr3PayloadType::Response {
        let v: Value = serde_json::from_slice(&body).map_err(|e| format!("应答体非 JSON: {e}"))?;
        let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
        let err = v
            .get("result")
            .and_then(|r| r.get("error"))
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        return Ok(Rr3BoardFrame::Reply { id, err });
    }
    Ok(Rr3BoardFrame::Event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::RR3_TRACE_DIRECT;
    use rr_ipc::{opcode, rr3_body_encode};

    /// 构造板形应答帧（分类面测试孪生）：`src`=板 2 / `dst`=宿主 1。
    fn board_reply_frame(id: u64, error: Option<&str>) -> Rr3Frame {
        let result = match error {
            Some(e) => json!({ "error": e }),
            None => json!({ "angle_deg": 90 }),
        };
        let body = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        let payload = rr3_body_encode(Rr3PayloadType::Response, body.to_string().as_bytes());
        let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
        hdr.trace_id = 0;
        hdr.timestamp_ns = 100;
        hdr.src_task = crate::consts::RR3_PERIPH_TASK;
        hdr.dst_task = RR3_TASK_HOST;
        Rr3Frame { header: hdr, payload }
    }

    /// 命令帧与桥帧字节级同形（payload 逐字节；帧头位形同值，时间戳不参与对拍）。
    #[test]
    fn cmd_frame_matches_bridge_shape() {
        let frame = rr3_esp32_cmd_frame(7, &Rr3PeriphCmd::ServoPan(105));
        assert_eq!(frame.header.src_task, RR3_TASK_HOST);
        assert_eq!(frame.header.dst_task, crate::consts::RR3_PERIPH_TASK);
        assert_eq!(frame.header.trace_id, RR3_TRACE_DIRECT);

        let tilt_frame = rr3_esp32_cmd_frame(8, &Rr3PeriphCmd::ServoTilt(98));
        let tilt_bridge = rr3_bridge_call_frame(
            8,
            RR3_SERVO_TOPIC,
            &json!({ "angle_deg": 98, "axis": "tilt" }),
        );
        assert_eq!(tilt_frame.payload, tilt_bridge.payload, "tilt payload 字节级同形");

        let bridge = rr3_bridge_call_frame(
            7,
            RR3_SERVO_TOPIC,
            &json!({ "angle_deg": 105, "axis": "pan" }),
        );
        assert_eq!(frame.payload, bridge.payload, "payload 字节级同形");
        assert_eq!(frame.header.src_task, bridge.header.src_task);
        assert_eq!(frame.header.dst_task, bridge.header.dst_task);
        assert_eq!(frame.header.trace_id, bridge.header.trace_id);
        assert_eq!(frame.header.payload_len, bridge.header.payload_len);
    }

    #[test]
    fn classify_routes_reply_and_event() {
        let ok = board_reply_frame(7, None);
        match rr3_board_frame_classify(&ok.header, &ok.payload) {
            Ok(Rr3BoardFrame::Reply { id, err }) => {
                assert_eq!(id, 7);
                assert_eq!(err, None);
            }
            other => panic!("应答帧须落 Reply 臂: {other:?}"),
        }
        let bad = board_reply_frame(8, Some("越程拒收"));
        match rr3_board_frame_classify(&bad.header, &bad.payload) {
            Ok(Rr3BoardFrame::Reply { id, err }) => {
                assert_eq!(id, 8);
                assert_eq!(err.as_deref(), Some("越程拒收"));
            }
            other => panic!("失败应答须落 Reply 臂带 err: {other:?}"),
        }
        // 非 Response → Event（载荷门序留给消费面）
        let ev = {
            let payload = rr3_body_encode(Rr3PayloadType::Event, br#"{"topic":"x"}"#);
            let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
            hdr.dst_task = RR3_TASK_HOST;
            Rr3Frame { header: hdr, payload }
        };
        assert_eq!(
            rr3_board_frame_classify(&ev.header, &ev.payload),
            Ok(Rr3BoardFrame::Event)
        );
        // dst 门：非宿主位形拒收
        let mut other_dst = board_reply_frame(9, None);
        other_dst.header.dst_task = 9;
        assert!(rr3_board_frame_classify(&other_dst.header, &other_dst.payload).is_err());
    }
}
