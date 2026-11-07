//! 帧面收发机制（泛型于 UART/时钟端口，P0-A A2 自板档适配层迁入）。
//!
//! 只做"收一帧 → 协议门 → 视图"与"视图 → 应答帧 → 写回"两件机械动作；
//! method/主题分发、状态机、作业队列全在适配层（控制面），本模块不碰状态。
//! 换平台只需实现 [`Rr3UartPort`]/[`Rr3ClockPort`]，帧面字节零改动。

use rr_ipc::{Rr3Frame, Rr3FrameBuf, Rr3TransportConn};
use rr_periph_proto::ports::{Rr3ClockPort, Rr3UartPort};
use rr_periph_proto::{rr3_periph_request_parse, rr3_periph_response_frame, Rr3PeriphRequest};
use serde_json::Value;

/// 帧面请求视图别名：id/method/params 摘出，帧头位形随行携带（应答回带
/// trace_id / dst=src_task 用）。形状真值单点在 [`rr_periph_proto`]。
pub type FrameReq = Rr3PeriphRequest;

/// 收帧 + 协议门（全过才返回 Ok）：整帧收 → 帧还原（magic/长度校验在 rr-ipc）
/// → 协议门（dst/payload 前缀/JSON/id 在 [`rr_periph_proto::rr3_periph_request_parse`]，
/// 单点真值）。任一门失败回 Err = **丢弃不回帧**（fail-closed：帧级不可信的请求
/// 不作确定性应答；对端侧重开连接即恢复）。半帧失败后流上可能残留尾巴字节——
/// 下一轮 magic 窥视大概率落错误面，自愈路径由适配层负责。
pub fn frame_recv(io: &mut impl Rr3UartPort) -> Result<FrameReq, String> {
    let buf = io.rr3_recv_buf().map_err(|e| format!("收帧失败: {e:?}"))?;
    let frame = Rr3Frame::try_from(buf).map_err(|e| format!("帧还原失败: {e:?}"))?;
    rr3_periph_request_parse(&frame)
}

/// Response 回帧：result 在位（业务失败折 `{"error": 文案}`——宿主解析契约），
/// trace_id 原样回带、src=本板位形、dst=调用方、timestamp 由时钟端口注入
/// （换平台不换帧面）。组帧单点在 [`rr_periph_proto`]；发送失败只回 Err 文案，
/// 由适配层点名不重试（对端 io 失败自会重开连接重发）。
pub fn frame_send_response(
    io: &mut impl Rr3UartPort,
    req: &FrameReq,
    result: Result<Value, String>,
    clock: &impl Rr3ClockPort,
) -> Result<(), String> {
    let resp = rr3_periph_response_frame(req, result, clock.rr3_now_ns());
    io.rr3_send_buf(Rr3FrameBuf::from(&resp))
        .map_err(|e| format!("Response 发送失败: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_ipc::{rr3_body_split, Rr3ByteStream};
    use rr_periph_proto::consts::{RR3_SERVO_TOPIC, RR3_TASK_HOST};
    use rr_periph_proto::host_call::rr3_bridge_call_frame;
    use rr_periph_proto::rr3_bridge_parse_response;
    use serde_json::json;

    /// 测试字节流：读侧按序吐 `rd`，写侧累积到 `wr`（端口契约的两个 usize 方法）。
    struct FakeIo {
        rd: Vec<u8>,
        pos: usize,
        wr: Vec<u8>,
    }

    impl FakeIo {
        fn new(rd: Vec<u8>) -> Self {
            Self {
                rd,
                pos: 0,
                wr: Vec::new(),
            }
        }
    }

    impl Rr3ByteStream for FakeIo {
        fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
            let n = (self.rd.len() - self.pos).min(buf.len());
            buf[..n].copy_from_slice(&self.rd[self.pos..self.pos + n]);
            self.pos += n;
            n
        }

        fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
            self.wr.extend_from_slice(buf);
            buf.len()
        }
    }

    struct FakeClock(u64);

    impl Rr3ClockPort for FakeClock {
        fn rr3_now_ns(&self) -> u64 {
            self.0
        }
    }

    /// 合法宿主请求帧 → 收帧 → 协议门 → 视图（为应答测试提供 req）。
    fn recv_host_call(id: u64) -> FakeIo {
        let frame = rr3_bridge_call_frame(id, RR3_SERVO_TOPIC, &json!({ "angle_deg": 95 }));
        let buf = Rr3FrameBuf::from(&frame);
        FakeIo::new(buf.rr3_as_sealed_bytes().expect("已 seal").to_vec())
    }

    #[test]
    fn recv_parses_host_call_and_rejects_torn_frame() {
        let mut io = recv_host_call(7);
        let req = frame_recv(&mut io).expect("合法帧必须过全部门");
        assert_eq!(req.id, 7);
        assert_eq!(req.method, "rr3.call");
        assert_eq!(req.topic, RR3_SERVO_TOPIC);
        assert_eq!(req.args["angle_deg"], 95);
        assert_eq!(req.src_task, RR3_TASK_HOST);

        // 截断帧：rr-ipc 帧层先拒（TornHeader），本层折 Err 不回帧
        let mut torn = recv_host_call(8);
        torn.rd.truncate(64);
        assert!(frame_recv(&mut torn).is_err());
    }

    #[test]
    fn response_roundtrip_bytes_and_clock_stamp() {
        let req = {
            let mut io = recv_host_call(11);
            frame_recv(&mut io).expect("请求可解析")
        };
        let mut io = FakeIo::new(Vec::new());
        frame_send_response(
            &mut io,
            &req,
            Ok(json!({ "angle_deg": 95 })),
            &FakeClock(1234),
        )
        .expect("写侧整段成功");

        let mut back = FakeIo::new(io.wr);
        let frame = Rr3Frame::try_from(back.rr3_recv_buf().expect("回读整帧")).expect("帧还原");
        assert_eq!(frame.header.trace_id, req.trace_id);
        assert_eq!(frame.header.timestamp_ns, 1234);
        let (ty, body) = rr3_body_split(&frame.payload).expect("payload 前缀合法");
        let v = rr3_bridge_parse_response(ty, &body, 11).expect("宿主应答门全过");
        assert_eq!(v["angle_deg"], 95);
        // 读侧已耗尽（供知识面自证：back 只作帧还原，不再有字节）
        assert_eq!(back.pos, back.rd.len());
    }
}
