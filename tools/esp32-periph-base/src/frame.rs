//! 帧级请求分派：`set_bindings` / `servo` / `face` 三臂的解码与应答组装。
//!
//! 上行帧按 topic 分派到本文件，参数裁决与状态迁移在 `state.rs`（机制/策略
//! 分离）；应答体形状是宿主侧 `rr3.call` 的契约面，错误文案带根因点名。

use std::sync::{Arc, Mutex};

use rr_ipc::{rr3_board_bind_entry, Rr3BoardBindings, RR3_METHOD_SET_BINDINGS};
use rr_periph_proto::ports::{Rr3ClockPort, Rr3UartPort};
use serde_json::{json, Value};

use crate::board::{rr3_bindings_check, RR3_FACE_EXPRS, RR3_FACE_TOPIC, RR3_SERVO_TOPIC};
use crate::face::expr_eye_base;
use crate::servo::{axis_check, Axis};
use crate::state::{lock_bus, push_job, rr3_board_binds_set, Bus, CtlState, Job};

// 收帧/应答组帧的机制体已迁 periph-mech（P0-A A3）：本档只留控制面分发与状态机。
pub(crate) use periph_mech::frame::{frame_recv, FrameReq};

/// 主题分发（**控制面**：只解析/校验/改状态/推作业，不碰硬件）：method 两面——
/// `rr3.set_bindings`（绑定注入，宿主桥连接建立即推）与 rr3.call（rrd 转发形状，
/// 宿主桥同款；主题只认两条在册能力面）。method/主题/args 层面的失败是**业务性
/// 失败**——折 result.error 点名回帧（模块面同款形状，宿主 rr3_bridge_decode
/// 的解析契约）。
pub(crate) fn frame_dispatch(
    bus: &Arc<Mutex<Bus>>,
    ctl: &mut CtlState,
    req: &FrameReq,
) -> Result<Value, String> {
    if req.method == RR3_METHOD_SET_BINDINGS {
        return rr3_handle_bindings(bus, &req.args);
    }
    if req.method != "rr3.call" {
        return Err(format!("method 非法: \"{}\"（恒 rr3.call）", req.method));
    }
    match req.topic.as_str() {
        RR3_SERVO_TOPIC => frame_handle_servo(bus, &req.args),
        RR3_FACE_TOPIC => frame_handle_face(bus, ctl, &req.args),
        other => Err(format!("未知主题: \"{other}\"")),
    }
}

/// 绑定注入（rr3.set_bindings，**控制面**，批N4 件七(c) / 批N9 件五）：宿主桥连接
/// 建立时下发描述符解析的引脚真值（`{"i2c_sda": "21", ...}`——值全字符串，rr-hal/
/// rr-registry 契约同形）。逐键过 [`rr3_board_bind_entry`] 键域/值域校验：未知键/
/// 坏值 fail-closed 整体不落（暂存区不半写入，Err 折 result.error 回帧）；全过才
/// 覆盖写板侧单点（[`crate::state::rr3_board_binds_set`]）。部分注入合法（缺键 =
/// None，消费点各自判齐）。成功即打印十键值——板侧引脚真值的**观测面**（boot 横幅
/// 已去引脚号，此打印是唯一真值观测点）。随后**装配对账**（批N9 件五）：注入值逐键
/// 判等具名装配真值（[`crate::board::rr3_bindings_check`]），不齐 eprintln 点名
/// ＋累计 `binding_mismatch`（fail-visible，不拒收不拒 boot），不齐数随应答
/// `mismatch` 字段回带供宿主实录判读。
fn rr3_handle_bindings(bus: &Arc<Mutex<Bus>>, args: &Value) -> Result<Value, String> {
    let obj = args
        .as_object()
        .ok_or_else(|| format!("args 非法: {args}（须对象）"))?;
    let mut nb = Rr3BoardBindings::default();
    for (k, v) in obj {
        let s = v
            .as_str()
            .ok_or_else(|| format!("{k} 值非法: {v}（须字符串）"))?;
        rr3_board_bind_entry(&mut nb, k, s)?;
    }
    rr3_board_binds_set(nb.clone());
    println!(
        "[periph] 绑定注入 ✓ i2c_sda={} i2c_scl={} adc_rail_pin={} uart2_tx={} uart2_rx={} ledc_pan={} ledc_tilt={} i2s_bclk={} i2s_sd={} i2s_ws={}",
        nb.i2c_sda.map_or("-".into(), |v| v.to_string()),
        nb.i2c_scl.map_or("-".into(), |v| v.to_string()),
        nb.adc_rail_pin.map_or("-".into(), |v| v.to_string()),
        nb.uart2_tx.map_or("-".into(), |v| v.to_string()),
        nb.uart2_rx.map_or("-".into(), |v| v.to_string()),
        nb.ledc_pan.map_or("-".into(), |v| v.to_string()),
        nb.ledc_tilt.map_or("-".into(), |v| v.to_string()),
        nb.i2s_bclk.map_or("-".into(), |v| v.to_string()),
        nb.i2s_sd.map_or("-".into(), |v| v.to_string()),
        nb.i2s_ws.map_or("-".into(), |v| v.to_string()),
    );
    let bad = rr3_bindings_check(&nb);
    if bad > 0 {
        // saturating：本计数是运维观测面，宿主反复注入不符绑定也不该把它加溢出
        // （debug 档 `+=` 溢出直接 panic 在真板上 = 复位；release 档回绕最坏让
        // 观测账归零）。饱和到 u32::MAX 仍是"一直不符"的正确语义。
        let mut b = lock_bus(bus);
        b.st.binding_mismatch = b.st.binding_mismatch.saturating_add(bad as u32);
    }
    Ok(json!({ "applied": true, "mismatch": bad }))
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
                // **不写前记账**：角度只由运行面在作业成功后镜像硬件真值（与行面
                // PAN/TILT 同口径）——读 `angle_deg` 永不复述没到达的角。
                let mut b = lock_bus(bus);
                b.st.wander_on = false; // 外部点名优先，自主扫摆让位
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
        let base = expr_eye_base(expr);
        let previous = std::mem::replace(&mut ctl.prev_expr, expr.to_string());
        {
            let mut b = lock_bus(bus);
            b.st.expr_mode = true;
            b.st.frame_mode = false;
            b.st.base = base;
            b.st.last_text = format!("EXPR {expr}");
            b.st.anim_reset();
            b.st.drawn = base;
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

/// Response 回帧（组帧机制在 periph-mech，平台面只做"失败点名不重试"）：result
/// 在位（业务失败折 {"error": 文案}——宿主解析契约），trace_id 原样回带、src=本板
/// 位形、dst=调用方、timestamp 单调时基。发送失败 eprintln 点名——宿主 io 失败
/// 自会重开连接重发。
pub(crate) fn frame_send_response(
    io: &mut impl Rr3UartPort,
    req: &FrameReq,
    result: Result<Value, String>,
    clock: &impl Rr3ClockPort,
) {
    if let Err(e) = periph_mech::frame::frame_send_response(io, req, result, clock) {
        eprintln!("[periph] {e}");
    }
}
