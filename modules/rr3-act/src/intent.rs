//! intent/result 订阅执行面（0.0.3 T3，方案 Q1 裁定）：brain 产出结构化
//! `actions`（元素 `{topic, args}`，contracts/events/brain-intent-result.schema.json），
//! 本模块广播认领——**两道归属门**后才执行：
//!
//! ① 归属门：topic ∈ 本模块 exposes（不越权——发布侧 `rr3_reg_pub_check`
//!    四道门的精神在订阅执行面的对偶：别人的主题一个不碰）；
//! ② 开放门：topic ∈ 意图执行面**开放集**——该集由 config `[rr3-act.intent]
//!    open_topics` 注入、组合件读入透传（0.0.5 批M MC-10 A-9："某形态不开某令"
//!    = 改数据不改码）。开放集外的**已归属**主题（led/effect）到此拒执行；开放
//!    集只认本模块**有执行臂**的在册主题（relay/set、servo/command），boot 时已
//!    fail-closed 校验（越界值拒 boot）。
//!
//! 执行失败（参数裁决拒 / HAL 写失败）是**业务性失败**，逐条折日志继续收下一帧
//! （事件语义无应答回路）；传输性失败（发布 relay/state、servo/state 写不出去）
//! 由调用方按 `exit(1)` 口径收口（组合件主循环的发布 `?`）。
//!
//! 归属（0.0.5 批C② 原子重铺，0.0.5 方案 §12.1）：本文件是总线接入编排面
//! （两道门 + 执行编排），**留组合件**；裁决与执行机制在 servo/led/relay 原子件。

use serde_json::Value;

use super::boot::Rr3ActAxisState;
use super::led::RR3_LED_EFFECT_TOPIC;
use super::relay::{self, RR3_RELAY_SET_TOPIC};
use super::rpc::{
    rr3_low_power_reject, rr3_relay_err_text, rr3_relay_parse_args, rr3_servo_gate,
    rr3_servo_parse_args,
};
use super::servo::{Rr3ServoCfg, RR3_SERVO_COMMAND_TOPIC};

/// 本模块 exposes 的全集（module.toml [rpc].exposes 同源——归属门的真值；
/// 字面量零内联，主题事实源在各自原子件的常量）
const EXPOSED: [&str; 3] = [
    RR3_SERVO_COMMAND_TOPIC,
    RR3_LED_EFFECT_TOPIC,
    RR3_RELAY_SET_TOPIC,
];

/// 单条动作的执行结果：relay 落通断、servo 落角度——调用方按变体发对应的
/// state 流（两条驱动路 RPC/intent 共用同一条状态流的口径不变）。relay 变体
/// 携带 `(生效, 前一)`（0.0.5 批I A-7：组合件据此做同态抑制）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3IntentOutcome {
    Relay(bool, bool),
    Servo(u16),
}

/// 处理 intent 载荷的 `actions` 字段：逐条两道门 + 执行。
/// 返回每条的处置结果（Ok = 生效后果；Err = 拒因），由调用方逐条落日志——
/// 纯函数不碰总线，sim 设备可直接断言落效。
///
/// `low_blocked` = 低电禁动闸门闭锁（MC-5 §20 ② ②）：**执行面**（servo/relay）
/// 与 RPC 路同判拒动；**妆面 led 不受辖**（仍走开放门=在册未开放的既有口径）。
/// `axis` = 执行轴状态（批N3 件三）：servo 动作过拒动判据（§6 硬约束）——广播
/// 无 motion 键 = auto（语音触发的判向/转向无逐步人工确认，按点动申报是撒谎），
/// 未标定轴在此被代码拒动。
/// `open_topics` = 意图执行面开放集（A-9，config 注入）：只有列在其中的**已归属**
/// 主题才进入执行臂；未列（含 led/effect）一律由开放门拒执行。
pub fn rr3_actions_handle(
    actions: &Value,
    gpio: &mut dyn rr_hal_api::Rr3GpioOut,
    servo: &mut dyn rr_hal_api::Rr3PwmServo,
    servo_cfg: &Rr3ServoCfg,
    low_blocked: bool,
    axis: &Rr3ActAxisState,
    open_topics: &[String],
) -> Vec<Result<Rr3IntentOutcome, String>> {
    let Some(list) = actions.as_array() else {
        return vec![Err(format!("actions 必须是数组，收到 {actions}"))];
    };
    list.iter()
        .map(|item| rr3_action_handle(item, gpio, servo, servo_cfg, low_blocked, axis, open_topics))
        .collect()
}

/// 开放集展示串（`relay/set/servo/command`；拒因文案的唯一渲染点）。空集显示
/// 为 `(空)`——"某形态意图执行面全关"是合法数据，不是错。
fn rr3_open_topics_join(open_topics: &[String]) -> String {
    if open_topics.is_empty() {
        return "(空)".to_string();
    }
    open_topics.join("/")
}

/// 单条动作：形状裁决 → 两道门 → 执行（低电闸门只辖执行面；servo 动作再过
/// 拒动判据——批N3 件三，与 RPC 路同判）。B9：门面/执行臂各自成函数，本函数只
/// 留归属分派。
fn rr3_action_handle(
    item: &Value,
    gpio: &mut dyn rr_hal_api::Rr3GpioOut,
    servo: &mut dyn rr_hal_api::Rr3PwmServo,
    servo_cfg: &Rr3ServoCfg,
    low_blocked: bool,
    axis: &Rr3ActAxisState,
    open_topics: &[String],
) -> Result<Rr3IntentOutcome, String> {
    let (topic, args) = rr3_item_gate(item, open_topics)?;
    match topic {
        RR3_RELAY_SET_TOPIC => rr3_relay_arm(args, gpio, low_blocked),
        RR3_SERVO_COMMAND_TOPIC => rr3_servo_arm(args, servo, servo_cfg, low_blocked, axis),
        // 开放集已 boot 校验只含上述两条执行臂主题；此臂为配置/执行面不一致的兜底
        other => Err(format!(
            "意图开放集含 {other} 但本模块无对应执行臂（配置与执行面不一致）"
        )),
    }
}

/// 形状 + 两道门（B9 自 action_handle 抽出）：topic/args 形状、归属门、
/// 开放门。通过即返回 (topic, args)。
fn rr3_item_gate<'a>(
    item: &'a Value,
    open_topics: &[String],
) -> Result<(&'a str, &'a Value), String> {
    let (topic, args) = rr3_item_shape(item)?;
    rr3_gates(topic, open_topics)?;
    Ok((topic, args))
}

/// 两道归属门（B9 自 item_gate 抽出）：exposes 归属门 + 意图开放门（A-9）。
fn rr3_gates(topic: &str, open_topics: &[String]) -> Result<(), String> {
    if !EXPOSED.contains(&topic) {
        return Err(format!("越权拒执行 {topic}：不在本模块 exposes"));
    }
    rr3_topic_open(topic, open_topics)
}

/// 开放门（A-9）：开放集由 config 注入（组合件读入透传），不再硬编码 match；
/// 已归属但不在开放集的主题（led/effect）到此拒执行。
fn rr3_topic_open(topic: &str, open_topics: &[String]) -> Result<(), String> {
    if !open_topics.iter().any(|t| t.as_str() == topic) {
        return Err(format!(
            "已归属但意图执行面未开放 {topic}（当前开放集 {}）",
            rr3_open_topics_join(open_topics)
        ));
    }
    Ok(())
}

/// 单条动作的形状裁决：`topic` 须为字符串、`args` 须在场。
fn rr3_item_shape(item: &Value) -> Result<(&str, &Value), String> {
    let topic = rr3_item_topic(item)?;
    let args = rr3_item_args(item)?;
    Ok((topic, args))
}

/// 动作 topic 形状（须为字符串）。
fn rr3_item_topic(item: &Value) -> Result<&str, String> {
    item.get("topic")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("动作缺 topic（字符串），收到 {item}"))
}

/// 动作 args 形状（须在场）。
fn rr3_item_args(item: &Value) -> Result<&Value, String> {
    item.get("args")
        .ok_or_else(|| format!("动作缺 args（对象），收到 {item}"))
}

/// relay 执行臂（B9 自 action_handle 抽出）
fn rr3_relay_arm(
    args: &Value,
    gpio: &mut dyn rr_hal_api::Rr3GpioOut,
    low_blocked: bool,
) -> Result<Rr3IntentOutcome, String> {
    if low_blocked {
        return Err(rr3_low_power_reject(RR3_RELAY_SET_TOPIC));
    }
    let on = rr3_relay_parse_args(args)?;
    let (on, prev) = relay::rr3_relay_apply(gpio, on).map_err(|e| rr3_relay_err_text(&e))?;
    Ok(Rr3IntentOutcome::Relay(on, prev))
}

/// servo 执行臂（B9 自 action_handle 抽出）：低电闸门 → 拒动判据 → 参数裁决
/// → 行程钳位 → HAL 写。
fn rr3_servo_arm(
    args: &Value,
    servo: &mut dyn rr_hal_api::Rr3PwmServo,
    servo_cfg: &Rr3ServoCfg,
    low_blocked: bool,
    axis: &Rr3ActAxisState,
) -> Result<Rr3IntentOutcome, String> {
    let angle = rr3_servo_target(args, servo_cfg, low_blocked, axis)?;
    rr3_servo_commit(servo, angle)
}

/// 判据 + 裁决段：低电闸门 → 拒动判据 → 参数裁决 → 行程钳位，回落角。
fn rr3_servo_target(
    args: &Value,
    servo_cfg: &Rr3ServoCfg,
    low_blocked: bool,
    axis: &Rr3ActAxisState,
) -> Result<u16, String> {
    rr3_servo_ready(args, low_blocked, axis)?;
    let (angle, _move_ms) = rr3_servo_parse_args(args, servo_cfg)?;
    // 安全核钳位最后包一道（批D₃ 裁定 5）：与 RPC 路同口径，落角前不越行程
    Ok(rr_core::control::rr3_travel_clamp(
        angle,
        servo_cfg.angle_min_deg,
        servo_cfg.angle_max_deg,
    ))
}

/// 闸门段：低电禁动 + 拒动硬约束（§6，批N3 件三——广播无 motion 键 = auto，
/// 未标定轴被代码拒动；判据与 RPC 路同源 rr-hal-api 单真值点）。
fn rr3_servo_ready(args: &Value, low_blocked: bool, axis: &Rr3ActAxisState) -> Result<(), String> {
    if low_blocked {
        return Err(rr3_low_power_reject(RR3_SERVO_COMMAND_TOPIC));
    }
    rr3_servo_gate(args, axis)
}

/// 落角段：HAL 写失败按业务性失败折拒因（同 RPC 路的后端写失败口径）。
fn rr3_servo_commit(
    servo: &mut dyn rr_hal_api::Rr3PwmServo,
    angle: u16,
) -> Result<Rr3IntentOutcome, String> {
    servo
        .set_angle(angle)
        .map_err(|e| format!("舵机写入失败: {e}"))?;
    Ok(Rr3IntentOutcome::Servo(angle))
}

#[cfg(test)]
mod tests;
