use std::sync::{Arc, Mutex};

use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3FrameBuf, Rr3Header, Rr3PayloadType,
    Rr3TransportConn,
};
use serde_json::{json, Value};

use crate::board::{
    axis_check, Axis, RR3_FACE_EXPRS, RR3_FACE_TOPIC, RR3_PERIPH_TASK, RR3_SERVO_TOPIC,
};
use crate::link::{rr3_frame_now_ns, UartIo};
use crate::oled::expr_eye_base;
use crate::state::{lock_bus, push_job, Bus, CtlState, Job};

/// 帧面请求（协议门全过后的视图）：id/method/params 摘出，帧头位形随行携带
/// （应答回带 trace_id / dst=src_task 用）。
pub(crate) struct FrameReq {
    pub(crate) id: u64,
    pub(crate) method: String,
    pub(crate) topic: String,
    pub(crate) args: Value,
    pub(crate) src_task: u64,
    pub(crate) trace_id: u64,
}

/// 收帧 + 协议门（全过才返回 Ok）：`rr3_framebuf_read` 整帧收 → 帧还原（magic/
/// 长度校验在 rr-ipc）→ dst 门（非本板位形丢弃）→ payload 前缀门（非 Request
/// 丢弃）→ JSON 门 → id 门。任一门失败回 Err = **丢弃不回帧**（fail-closed：
/// 帧级不可信的请求不作确定性应答；宿主侧重开连接即恢复）。半帧失败后流上可能
/// 残留尾巴字节——下一轮 magic 窥视大概率落行面 ERR，自愈路径在案。
pub(crate) fn frame_recv(io: &mut UartIo) -> Result<FrameReq, String> {
    let buf = io.rr3_recv_buf().map_err(|e| format!("收帧失败: {e:?}"))?;
    let frame = Rr3Frame::try_from(buf).map_err(|e| format!("帧还原失败: {e:?}"))?;
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
    Ok(FrameReq {
        id,
        method,
        topic,
        args,
        src_task: frame.header.src_task,
        trace_id: frame.header.trace_id,
    })
}

/// 主题分发（**控制面**：只解析/校验/改状态/推作业，不碰硬件）：method 恒
/// rr3.call（rrd 转发形状，宿主桥同款）；主题只认两条在册能力面。method/主题/
/// args 层面的失败是**业务性失败**——折 result.error 点名回帧（模块面同款形状，
/// 宿主 rr3_bridge_parse_response 的解析契约）。
pub(crate) fn frame_dispatch(
    bus: &Arc<Mutex<Bus>>,
    ctl: &mut CtlState,
    req: &FrameReq,
) -> Result<Value, String> {
    if req.method != "rr3.call" {
        return Err(format!("method 非法: \"{}\"（恒 rr3.call）", req.method));
    }
    match req.topic.as_str() {
        RR3_SERVO_TOPIC => frame_handle_servo(bus, &req.args),
        RR3_FACE_TOPIC => frame_handle_face(bus, ctl, &req.args),
        other => Err(format!("未知主题: \"{other}\"")),
    }
}

/// 舵机执行（rr3-act/servo/command，**控制面**）：args 带 angle_deg = 写（校验 +
/// 状态记账 + 推 `Job::Move` 作业给运行面执行 + 生效角回显），不带 = 读（回状态
/// 记账角——LEDC 只由运行面本面作业改动，状态即真值、±10ms 内一致）。axis 可选
/// 缺省 pan（宿主单轴 trait ↔ 板上双轴的映射面，RR3_HAL_PERIPH_AXIS env + 本参数
/// 两级）。写路径推 `Job::RedrawOled` 令角度行随动（运行面按模式守卫决定刷否）。
fn frame_handle_servo(bus: &Arc<Mutex<Bus>>, args: &Value) -> Result<Value, String> {
    let axis = match args.get("axis") {
        None | Some(Value::Null) => Axis::Pan,
        Some(v) => match v.as_str() {
            Some("pan") => Axis::Pan,
            Some("tilt") => Axis::Tilt,
            _ => return Err(format!("axis 非法: {v}（pan|tilt）")),
        },
    };
    match args.get("angle_deg") {
        None | Some(Value::Null) => {
            let deg = {
                let b = lock_bus(bus);
                match axis {
                    Axis::Pan => b.st.pan_deg,
                    Axis::Tilt => b.st.tilt_deg,
                }
            };
            Ok(json!({ "angle_deg": deg, "move_ms": 0 }))
        }
        Some(v) => {
            let deg = v.as_u64().ok_or_else(|| format!("angle_deg 非法: {v}"))? as u32;
            let d = axis_check(axis, deg)?;
            {
                let mut b = lock_bus(bus);
                b.st.wander_on = false; // 外部点名优先，自主扫摆让位
                match axis {
                    Axis::Pan => b.st.pan_deg = d,
                    Axis::Tilt => b.st.tilt_deg = d,
                }
            }
            push_job(
                bus,
                match axis {
                    Axis::Pan => Job::Move {
                        pan: Some(d),
                        tilt: None,
                    },
                    Axis::Tilt => Job::Move {
                        pan: None,
                        tilt: Some(d),
                    },
                },
            );
            push_job(bus, Job::RedrawOled);
            Ok(json!({ "angle_deg": d, "move_ms": 0 }))
        }
    }
}

/// 表情执行（rr3-interact/face/expression，**控制面**）：expression 变体出胶囊眼
/// （基态预设 + 微动作动画，rr-hal-api Rr3FaceFrame 契约之外的表情名面）——控制面
/// 改状态（模式/基态/时钟）+ 推 `Job::RenderCapsules`，previous 记账在 [`CtlState`]；
/// frame 变体 8x8 点阵 → OLED 中央 64x64 放大渲染（HAL 渲染变体：行主序 8 字节、
/// 每字节 bit7 为该行最左点）——推 `Job::RenderFrame`。
fn frame_handle_face(
    bus: &Arc<Mutex<Bus>>,
    ctl: &mut CtlState,
    args: &Value,
) -> Result<Value, String> {
    if let Some(v) = args.get("expression") {
        let expr = v
            .as_str()
            .ok_or_else(|| format!("expression 非法: {v}（须字符串）"))?;
        if !RR3_FACE_EXPRS.contains(&expr) {
            return Err(format!("expression 不在白名单: \"{expr}\""));
        }
        let (base_open, base_dx, base_dy) = expr_eye_base(expr);
        let previous = std::mem::replace(&mut ctl.prev_expr, expr.to_string());
        {
            let mut b = lock_bus(bus);
            b.st.expr_mode = true;
            b.st.frame_mode = false;
            b.st.base_open = base_open;
            b.st.base_dx = base_dx;
            b.st.base_dy = base_dy;
            b.st.last_text = format!("EXPR {expr}");
            b.st.anim_reset();
            b.st.drawn = (base_open, base_dx, base_dy);
        }
        push_job(bus, Job::RenderCapsules);
        Ok(json!({ "expression": expr, "previous": previous }))
    } else if let Some(v) = args.get("frame") {
        let arr = v
            .as_array()
            .ok_or_else(|| format!("frame 非法: {v}（须 8 字节数组）"))?;
        if arr.len() != 8 {
            return Err(format!("frame 长度非法: {}（须 8）", arr.len()));
        }
        let mut f = [0u8; 8];
        for (i, item) in arr.iter().enumerate() {
            let b = item
                .as_u64()
                .ok_or_else(|| format!("frame[{i}] 非法: {item}"))?;
            if b > 255 {
                return Err(format!("frame[{i}] 超 u8: {b}"));
            }
            f[i] = b as u8;
        }
        {
            let mut b = lock_bus(bus);
            b.st.frame_mode = true;
            b.st.expr_mode = false;
        }
        push_job(bus, Job::RenderFrame(f));
        Ok(json!({ "applied": true }))
    } else {
        Err("缺 expression/frame 之一".into())
    }
}

/// Response 回帧：result 在位（业务失败折 {"error": 文案}——宿主解析契约），
/// trace_id 原样回带、src=本板位形 2、dst=调用方、timestamp 单调时基。发送失败
/// eprintln 点名不重试（宿主 io 失败自会重开连接重发）。
pub(crate) fn frame_send_response(io: &mut UartIo, req: &FrameReq, result: Result<Value, String>) {
    let result = match result {
        Ok(v) => v,
        Err(e) => json!({ "error": e }),
    };
    let body = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
    let payload = rr3_body_encode(Rr3PayloadType::Response, body.to_string().as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = req.trace_id;
    hdr.timestamp_ns = rr3_frame_now_ns();
    hdr.src_task = RR3_PERIPH_TASK;
    hdr.dst_task = req.src_task;
    let resp = Rr3Frame {
        header: hdr,
        payload,
    };
    if let Err(e) = io.rr3_send_buf(Rr3FrameBuf::from(&resp)) {
        eprintln!("[periph] Response 发送失败: {e:?}");
    }
}
