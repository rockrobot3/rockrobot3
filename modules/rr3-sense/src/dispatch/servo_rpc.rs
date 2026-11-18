//! 舵机命令面：pan/tilt 角度解码与命令旁路推送。

use rr_hal::backend::Rr3AudioCmdProducer;
use rr_hal::backend::Rr3PeriphCmd;
use serde_json::{json, Value};

/// 舵机命令面（A9，桥单所有者）：args `{"pan":0..=180}` / `{"tilt":0..=180}`，
/// 至少一键；逐轴推入持桥音频源的命令旁路（板侧执行前回执）。`cmd_tx` 缺位 =
/// 本进程不是桥所有者（音频后端非 esp32）——fail-closed 点名拒，不另开 UART。
pub(super) fn rr3_sense_servo(
    tx: Option<&Rr3AudioCmdProducer>,
    args: &Value,
) -> Result<Value, String> {
    let (pan, tilt, n) = rr3_servo_axes(args)?;
    if n == 0 {
        return Err("舵机命令需 pan/tilt 至少一键（0..=180）".into());
    }
    let Some(tx) = tx else {
        return Err("舵机命令面未就绪：本进程不持有外设桥（音频后端须为 esp32）".into());
    };
    rr3_servo_push(tx, pan, tilt);
    Ok(json!({ "queued": n }))
}

/// 逐轴推入命令旁路（缺键 = 本轴不下发）。
fn rr3_servo_push(tx: &Rr3AudioCmdProducer, pan: Option<u16>, tilt: Option<u16>) {
    if let Some(p) = pan {
        tx.rr3_cmd_push(Rr3PeriphCmd::ServoPan(p));
    }
    if let Some(t) = tilt {
        tx.rr3_cmd_push(Rr3PeriphCmd::ServoTilt(t));
    }
}

/// 双轴角度解码（缺键 = 本轴不下发）与下发键数。
fn rr3_servo_axes(args: &Value) -> Result<(Option<u16>, Option<u16>, u32), String> {
    let pan = rr3_axis_deg(args, "pan")?;
    let tilt = rr3_axis_deg(args, "tilt")?;
    let n = u32::from(pan.is_some()) + u32::from(tilt.is_some());
    Ok((pan, tilt, n))
}

/// 单轴角度门：缺键/Null = 本轴不下发；非整数或 >180 逐键点名（板侧行程裁决前
/// 的协议级 fail-closed，不预钳不猜）。
pub(super) fn rr3_axis_deg(args: &Value, key: &str) -> Result<Option<u16>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => rr3_deg_of(v, key),
    }
}

/// 单轴角度值域：整数且 ≤180（越界逐键点名）。
fn rr3_deg_of(v: &Value, key: &str) -> Result<Option<u16>, String> {
    let n = v
        .as_u64()
        .ok_or_else(|| format!("{key} 应为整数角度（0..=180）"))?;
    rr3_deg_cap(key, n)?;
    Ok(Some(n as u16))
}

/// 单轴角度上界（0..=180，越界逐键点名）。
fn rr3_deg_cap(key: &str, n: u64) -> Result<(), String> {
    if n > 180 {
        return Err(format!("{key} 越界 0..=180: {n}"));
    }
    Ok(())
}
