// SPDX-License-Identifier: Apache-2.0
//! rr3-act 协议层（0.0.8 W-A 批5 拆分，ADR-0017 五层表协议层）。
//!
//! 点名执行两 handler（servo/command 低电闸门 → 拒动判据 → 参数裁决 → HAL 落角；
//! led/effect 效果裁决 → 状态机切换＋同步落电平）＋ 载荷组装与参数下钻面（0.0.5
//! 批I A-8，ADR-0019 修订 4：json! 组装与 Value 下钻上移承载层，件面 serde_json
//! 清零；错误文案逐字，每句落在唯一一处）＋ 低电禁动拒因文案＋ `rr3-act/ops/list`
//! 快照组体。本层零总线触碰（发布/应答归机制层 dispatcher）——纯裁决与组体。

use rr_core::control::rr3_travel_clamp;
use rr_hal_api::{Rr3GpioOut, Rr3PwmServo};
use serde_json::{json, Value};

use crate::boot::{Rr3ActAxisState, Rr3ActCfg};
use crate::led::{rr3_led_effects_join, Rr3Led, Rr3LedEffect};
use crate::relay;
use crate::servo::{self, Rr3ServoCfg};

/// 本模块 exposes 的 ops 槽快照读口（批N4 件九二，module.toml [rpc].exposes 同源）：
/// 白名单组体应答，槽名与 assemble.toml 槽表申报同源（门 d covers 申报的对账面）
pub(crate) const RR3_OPS_LIST_TOPIC: &str = "rr3-act/ops/list";

/// 低电禁动拒因（MC-5，§20 ② ③）：点名是哪条执行令被闸门拒了、解禁口径、
/// 以及"不自动重放被拒的令"（拒因回显，由上层决定是否重发）。
pub(crate) fn rr3_low_power_reject(what: &str) -> String {
    format!("低电禁动：{what} 拒动（回阈 + 迟滞后解禁，不自动重放）")
}

/// 动作形态词拒收文案（error wire 契约字面量——面板与 legacy 依赖字面；批N34
/// 件二自 mech 上移：机制层零堆枚举 [`super::servo::Rr3ServoMotionError`]，
/// 渲染归编排层与行程/时限同批N24 件三先例，字面量逐字保持）。
pub(crate) fn rr3_motion_reject(word: &str) -> String {
    format!("不认的动作形态 {word:?}（运行命令面只认 jog/auto）")
}

/// 动作形态门（§6 硬约束；RPC 与意图两路同源）：词面裁决 → 单真值点判据。
pub(crate) fn rr3_servo_gate(args: &Value, axis: &Rr3ActAxisState) -> Result<(), String> {
    let motion = servo::rr3_servo_motion(args.get("motion").and_then(Value::as_str))
        .map_err(|servo::Rr3ServoMotionError::UnknownWord(word)| rr3_motion_reject(word))?;
    rr_hal_api::rr3_motion_allowed(axis.hard_stop, axis.calibrated, motion)
        .map_err(|e| format!("动作形态 {} 被拒: {e}", motion.rr3_act_motion_name()))
}

/// 点名处置三元组：(应答体, servo 待发新角（servo 面才有）, relay `(生效, 前一)`
/// （relay 面才有——A-7 同态抑制的组合件侧原料）)。Err = 业务性失败折 result.error。
pub(crate) type Rr3CallOutcome = Result<(Value, Option<u16>, Option<(bool, bool)>), String>;

/// servo/command 点名：低电闸门 → 拒动判据（批N3 件三）→ 参数裁决 → HAL 落角 →
/// 回 (应答体, 待发的 state 角度, relay 无)。`blocked` = 低电禁动闸门闭锁（MC-5：
/// 闸门先于一切裁决，闭锁期一律拒动，拒因明确点名低电）；`axis` = 执行轴状态——
/// 动作形态先裁（§6 硬约束：未标定轴 Auto 拒，判据 rr-hal-api 单真值点），
/// 参数值域后裁（形态都不许，参数值没有意义）。
pub(crate) fn rr3_handle_servo(
    servo_dev: &mut dyn Rr3PwmServo,
    cfg: &Rr3ServoCfg,
    args: &Value,
    blocked: bool,
    axis: &Rr3ActAxisState,
) -> Rr3CallOutcome {
    if blocked {
        return Err(rr3_low_power_reject(servo::RR3_SERVO_COMMAND_TOPIC));
    }
    let (angle, move_ms) = rr3_servo_apply(servo_dev, cfg, args, axis)?;
    rr3_servo_outcome(angle, move_ms)
}

/// servo 点名应答三元组（应答体 + 待发 state 角度 + relay 无）。
fn rr3_servo_outcome(angle: u16, move_ms: u64) -> Rr3CallOutcome {
    Ok((
        json!({ "angle_deg": angle, "move_ms": move_ms }),
        Some(angle),
        None,
    ))
}

/// servo 落角段：拒动判据 → 参数裁决 → 行程钳位 → HAL 写。返回落角与申报时限。
fn rr3_servo_apply(
    servo_dev: &mut dyn Rr3PwmServo,
    cfg: &Rr3ServoCfg,
    args: &Value,
    axis: &Rr3ActAxisState,
) -> Result<(u16, u64), String> {
    let (angle, move_ms) = rr3_servo_plan(cfg, args, axis)?;
    // HAL 写失败按业务性失败折 error（同 interact 的后端写失败口径）
    rr3_servo_land(servo_dev, angle, move_ms)?;
    Ok((angle, move_ms))
}

/// 裁决段：拒动判据 → 参数裁决 → 行程钳位（安全核永远包住控制图，输入合法时恒等）。
fn rr3_servo_plan(
    cfg: &Rr3ServoCfg,
    args: &Value,
    axis: &Rr3ActAxisState,
) -> Result<(u16, u64), String> {
    rr3_servo_gate(args, axis)?;
    let (angle, move_ms) = rr3_servo_parse_args(args, cfg)?;
    Ok((
        rr3_travel_clamp(angle, cfg.angle_min_deg, cfg.angle_max_deg),
        move_ms,
    ))
}

/// 落角末段：HAL 写 + 台账一行（写失败折业务性拒因）。
fn rr3_servo_land(servo_dev: &mut dyn Rr3PwmServo, angle: u16, move_ms: u64) -> Result<(), String> {
    servo_dev
        .set_angle(angle)
        .map_err(|e| format!("舵机写入失败: {e}"))?;
    eprintln!("servo: angle={angle} move_ms={move_ms}（sim 即时到位；实机按时限走反射环闭环）");
    Ok(())
}

/// led/effect 点名：效果裁决 → 状态机切换 + 同步落电平（批I A-1：写失败折日志
/// 不退——妆面件；时基由组合件单调时钟注入，批I A-2）→ 回 (应答体含生效/前效果, servo/relay 无)。
/// `effects` = 本形态白名单子集（A-9：config 注入，非内联）。
pub(crate) fn rr3_handle_led(
    led_ctrl: &mut Rr3Led,
    gpio: &mut dyn Rr3GpioOut,
    args: &Value,
    now_ms: u64,
    effects: &[Rr3LedEffect],
) -> Rr3CallOutcome {
    let effect = rr3_led_parse_args(args, effects)?;
    let (applied, previous, level) = led_ctrl.rr3_led_apply(now_ms, effect);
    gpio.set(level)
        .unwrap_or_else(|e| eprintln!("rr3-act-led: 写电平失败: {e}"));
    eprintln!(
        "led: {}（前态 {}）",
        applied.rr3_led_effect_str(),
        previous.rr3_led_effect_str()
    );
    Ok((
        json!({
            "effect": applied.rr3_led_effect_str(),
            "previous": previous.rr3_led_effect_str(),
        }),
        None,
        None,
    ))
}

// ── 载荷组装与参数下钻面（0.0.5 批I A-8，ADR-0019 修订 4：json! 组装与 Value
// 下钻上移承载层，件面 serde_json 清零；错误文案逐字，每句落在唯一一处）──

/// relay/set 参数裁决：只认真布尔——`{"on": 1}`、`{"on": "true"}` 都拒，
/// 通断没有"近似真"。（自 relay 原子件上移）
pub fn rr3_relay_parse_args(args: &Value) -> Result<bool, String> {
    let Some(v) = args.get("on") else {
        return Err("缺 on（bool，true=吸合 false=断开）".into());
    };
    v.as_bool()
        .ok_or_else(|| format!("on 必须是布尔，收到 {v}"))
}

/// relay/set 应答体：回显生效后的通断
pub fn rr3_relay_result(on: bool) -> Value {
    json!({ "on": on })
}

/// relay 机制错误的人读文案（批N24 件三，L-3 形状清理）：机制层零堆枚举
/// （`Rr3RelayApplyError`，HAL 错原样随行），文案在编排层渲染——act 册 §9
/// 契约字面量逐字保持（面板与 legacy 依赖字面），每句落在唯一一处。
pub fn rr3_relay_err_text(e: &relay::Rr3RelayApplyError) -> String {
    match e {
        relay::Rr3RelayApplyError::ReadBack(e) => format!("继电器读回失败: {e}"),
        relay::Rr3RelayApplyError::Write(e) => format!("继电器写入失败: {e}"),
    }
}

/// relay/state 摘要体：变化即发，seq 是本状态流自己的计数
/// （contracts/events/act-relay_state.schema.json 的形状事实源）
pub fn rr3_relay_state_body(seq: u64, on: bool) -> Value {
    json!({ "seq": seq, "on": on })
}

/// `rr3-act/ops/list` 应答体（批N4 件九二，面板 /api/act/ops 读口）：ops 槽快照，
/// **白名单组体**——槽名与 assemble.toml 槽表申报同源（led/servo 两组，门 d
/// covers 申报的对账面），config 其余键（intent/reflex 等非槽表键）一律不进应答；
/// 行程真值随轴档案（批N3 件五门 g），不在快照面。灯效子集按词表名成列。
pub(crate) fn rr3_act_ops_snapshot(cfg: &Rr3ActCfg) -> Value {
    json!({
        "led": {
            "blink_period_ms": cfg.led.blink_period_ms,
            "led_effects": cfg
                .led
                .effects
                .iter()
                .map(Rr3LedEffect::rr3_led_effect_str)
                .collect::<Vec<_>>(),
        },
        "servo": {
            "move_timeout_ms": cfg.servo.move_timeout_ms,
            "state_min_interval_ms": cfg.servo.state_min_interval_ms,
        },
    })
}

/// servo/command 参数下钻（Value→纯值）：缺参/类型错/越 u16 在这里点名；
/// 行程与时限的值域判定留件（`servo::rr3_servo_travel` /
/// `servo::rr3_servo_timeout`），点名顺序与上移前逐字同序。
pub fn rr3_servo_parse_args(args: &Value, cfg: &Rr3ServoCfg) -> Result<(u16, u64), String> {
    Ok((rr3_servo_angle(args, cfg)?, rr3_servo_move(args, cfg)?))
}

/// angle_deg：缺参/非整数/越 u16/行程外的点名顺序与上移前逐字同序。
fn rr3_servo_angle(args: &Value, cfg: &Rr3ServoCfg) -> Result<u16, String> {
    let angle = args
        .get("angle_deg")
        .ok_or("缺 angle_deg（int，行程内角度）")?;
    let angle = angle
        .as_u64()
        .ok_or_else(|| format!("angle_deg 必须是非负整数，收到 {angle}"))?;
    let angle = u16::try_from(angle).map_err(|_| format!("angle_deg 超出 u16 范围: {angle}"))?;
    // 行程/时限的值域判定留件（零堆枚举，批N24 件三），文案在此渲染逐字
    // （act 册 §9 契约字面量，每句落在唯一一处）
    servo::rr3_servo_travel(angle, cfg).map_err(
        |servo::Rr3ServoTravelError::OutOfRange {
             angle_deg,
             min_deg,
             max_deg,
         }| format!("angle_deg={angle_deg} 超出行程 [{min_deg}, {max_deg}]"),
    )?;
    Ok(angle)
}

/// move_ms：缺省/Null 走时限缺省；给值则非负整数 + 值域判定（留件）。
fn rr3_servo_move(args: &Value, cfg: &Rr3ServoCfg) -> Result<u64, String> {
    let move_ms = args
        .get("move_ms")
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| format!("move_ms 必须是非负整数，收到 {v}"))
        })
        .transpose()?;
    servo::rr3_servo_timeout(move_ms, cfg).map_err(|e| match e {
        servo::Rr3ServoTimeoutError::ZeroMs => "move_ms 必须大于 0".to_string(),
        servo::Rr3ServoTimeoutError::BeyondLimit { ms, limit } => {
            format!("move_ms={ms} 超出时限上限 {limit}（不给堵转留机会）")
        }
    })
}

/// servo/state 体：seq 逐条 +1（流序），angle_deg 是当前角
pub fn rr3_servo_state_body(seq: u64, angle_deg: u16) -> Value {
    json!({ "seq": seq, "angle_deg": angle_deg })
}

/// led/effect 参数下钻：缺/非串在这里点名；契约值→枚举的映射留件
/// （`Rr3LedEffect::rr3_led_effect_parse`），本形态子集判定用注入的 `effects`
/// （A-9：白名单是数据，不再内联 on/off/blink）。
pub fn rr3_led_parse_args(args: &Value, effects: &[Rr3LedEffect]) -> Result<Rr3LedEffect, String> {
    let Some(e) = args.get("effect") else {
        return Err(format!("缺 effect（{}）", rr3_led_effects_join(effects)));
    };
    let Some(s) = e.as_str() else {
        return Err(format!("effect 必须是字符串，收到 {e}"));
    };
    Rr3LedEffect::rr3_led_effect_parse(s)
        .filter(|eff| effects.contains(eff))
        .ok_or_else(|| {
            format!(
                "不认的灯效 {s:?}（本形态白名单 {}）",
                rr3_led_effects_join(effects)
            )
        })
}

#[cfg(test)]
mod tests;
