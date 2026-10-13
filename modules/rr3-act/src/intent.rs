//! intent/result 订阅执行面（0.0.3 T3，方案 Q1 裁定）：brain 产出结构化
//! `actions`（元素 `{topic, args}`，contracts/events/brain-intent-result.schema.json），
//! 本模块广播认领——**两道归属门**后才执行：
//!
//! ① 归属门：topic ∈ 本模块 exposes（不越权——发布侧 `rr3_reg_publish_check`
//!    四道门的精神在订阅执行面的对偶：别人的主题一个不碰）；
//! ② 开放门：topic ∈ 意图执行面已开放集（`relay/set` 0.0.3 T3 起开放；
//!    `servo/command` 批F2 随判向-转向编排开放——判据 8 的"注入'向左/向右'
//!    类意图后舵机转到对应档位"走这条执行路；led 仍封闭，扩面随真实需要
//!    append，不预先占位）。
//!
//! 执行失败（参数裁决拒 / HAL 写失败）是**业务性失败**，逐条折日志继续收下一帧
//! （事件语义无应答回路）；传输性失败（发布 relay/state、servo/state 写不出去）
//! 由调用方按 `exit(1)` 口径收口（组合件主循环的发布 `?`）。
//!
//! 归属（0.0.5 批C② 原子重铺，0.0.5 方案 §12.1）：本文件是总线接入编排面
//! （两道门 + 执行编排），**留组合件**；裁决与执行机制在 servo/led/relay 原子件。

use serde_json::Value;

use super::led::RR3_LED_EFFECT_TOPIC;
use super::relay::{self, RR3_RELAY_SET_TOPIC};
use super::servo::{Rr3ServoCfg, RR3_SERVO_COMMAND_TOPIC};
use super::{rr3_low_power_reject, rr3_relay_parse_args, rr3_servo_parse_args};

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
pub fn rr3_intent_actions_handle(
    actions: &Value,
    gpio: &mut dyn rr_hal::Rr3GpioOut,
    servo: &mut dyn rr_hal::Rr3PwmServo,
    servo_cfg: &Rr3ServoCfg,
    low_blocked: bool,
) -> Vec<Result<Rr3IntentOutcome, String>> {
    let Some(list) = actions.as_array() else {
        return vec![Err(format!("actions 必须是数组，收到 {actions}"))];
    };
    list.iter()
        .map(|item| rr3_intent_action_handle(item, gpio, servo, servo_cfg, low_blocked))
        .collect()
}

/// 单条动作：形状裁决 → 两道门 → 执行（低电闸门只辖执行面）
fn rr3_intent_action_handle(
    item: &Value,
    gpio: &mut dyn rr_hal::Rr3GpioOut,
    servo: &mut dyn rr_hal::Rr3PwmServo,
    servo_cfg: &Rr3ServoCfg,
    low_blocked: bool,
) -> Result<Rr3IntentOutcome, String> {
    let topic = item
        .get("topic")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("动作缺 topic（字符串），收到 {item}"))?;
    let args = item
        .get("args")
        .ok_or_else(|| format!("动作缺 args（对象），收到 {item}"))?;
    if !EXPOSED.contains(&topic) {
        return Err(format!("越权拒执行 {topic}：不在本模块 exposes"));
    }
    match topic {
        RR3_RELAY_SET_TOPIC => {
            if low_blocked {
                return Err(rr3_low_power_reject(RR3_RELAY_SET_TOPIC));
            }
            let on = rr3_relay_parse_args(args)?;
            let (on, prev) = relay::rr3_relay_apply(gpio, on)?;
            Ok(Rr3IntentOutcome::Relay(on, prev))
        }
        RR3_SERVO_COMMAND_TOPIC => {
            if low_blocked {
                return Err(rr3_low_power_reject(RR3_SERVO_COMMAND_TOPIC));
            }
            let (angle, _move_ms) = rr3_servo_parse_args(args, servo_cfg)?;
            // 安全核钳位最后包一道（批D₃ 裁定 5）：与 RPC 路同口径，落角前不越行程
            let angle = rr_core::control::rr3_travel_clamp(
                angle,
                servo_cfg.angle_min_deg,
                servo_cfg.angle_max_deg,
            );
            // HAL 写失败按业务性失败折拒因（同 RPC 路的后端写失败口径）
            servo
                .set_angle(angle)
                .map_err(|e| format!("舵机写入失败: {e}"))?;
            Ok(Rr3IntentOutcome::Servo(angle))
        }
        other => Err(format!(
            "已归属但意图执行面未开放 {other}（当前只认 {RR3_RELAY_SET_TOPIC}/{RR3_SERVO_COMMAND_TOPIC}）"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::{Rr3SimGpioOut, Rr3SimServo};
    use serde_json::json;

    fn servo_cfg() -> Rr3ServoCfg {
        Rr3ServoCfg {
            angle_min_deg: 0,
            angle_max_deg: 180,
            move_timeout_ms: 800,
            state_min_interval_ms: 50,
        }
    }

    /// 低电闸门开放态（既有用例的统一第 5 实参：不闭锁——两路对拍基线）
    const LOW_OPEN: bool = false;

    #[test]
    fn valid_relay_action_drives_gpio() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": true}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
        );
        assert_eq!(
            out,
            vec![Ok(Rr3IntentOutcome::Relay(true, false))],
            "{out:?}"
        );
        assert!(gpio.rr3_sim_gpio_level(), "吸合必须落到 GPIO 电平");

        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": false}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
        );
        assert_eq!(out, vec![Ok(Rr3IntentOutcome::Relay(false, true))]);
        assert!(!gpio.rr3_sim_gpio_level(), "断开必须落到 GPIO 电平");
    }

    /// 批F2 开放面：servo/command 动作经行程裁决落到 sim 舵机，结果是角度变体。
    #[test]
    fn valid_servo_action_drives_sim_servo() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        for (deg, want) in [
            (45u64, Rr3IntentOutcome::Servo(45)),
            (90, Rr3IntentOutcome::Servo(90)),
            (135, Rr3IntentOutcome::Servo(135)),
        ] {
            let out = rr3_intent_actions_handle(
                &json!([{"topic": "rr3-act/servo/command", "args": {"angle_deg": deg}}]),
                &mut gpio,
                &mut servo,
                &servo_cfg(),
                LOW_OPEN,
            );
            assert_eq!(out, vec![Ok(want)], "deg={deg}");
        }
        assert_eq!(servo.rr3_sim_servo_angle(), 135, "最后一条动作的落角");
        assert!(!gpio.rr3_sim_gpio_level(), "servo 动作不碰 GPIO");
    }

    /// servo 动作同样吃行程/类型裁决（同一 parse 函数）：越界折拒因不落角。
    #[test]
    fn servo_action_is_judged_by_travel_and_type() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        for bad in [
            json!({"topic": "rr3-act/servo/command", "args": {"angle_deg": 200}}),
            json!({"topic": "rr3-act/servo/command", "args": {"angle_deg": "45"}}),
            json!({"topic": "rr3-act/servo/command", "args": {}}),
        ] {
            let out = rr3_intent_actions_handle(
                &json!([bad]),
                &mut gpio,
                &mut servo,
                &servo_cfg(),
                false,
            );
            assert!(out[0].is_err(), "非法 servo 动作必须拒: {out:?}");
        }
        assert_eq!(servo.rr3_sim_servo_angle(), 90, "全部被拒，舵机不得动");
    }

    #[test]
    fn foreign_topic_is_rejected_by_exposes_gate() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-power/battery/state", "args": {}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
        );
        let err = out[0].as_ref().unwrap_err();
        assert!(err.contains("越权"), "归属门拒因要点名越权: {err}");
        assert!(err.contains("exposes"), "拒因要点名归属依据: {err}");
        assert!(!gpio.rr3_sim_gpio_level(), "被拒动作不得碰 GPIO");
        assert_eq!(servo.rr3_sim_servo_angle(), 90, "被拒动作不得碰舵机");
    }

    #[test]
    fn exposed_but_closed_topic_is_rejected_by_open_gate() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        // led/effect 仍封闭（扩面随真实需要 append）；relay/servo 已开放
        let out = rr3_intent_actions_handle(
            &json!([{ "topic": "rr3-act/led/effect", "args": {} }]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
        );
        let err = out[0].as_ref().unwrap_err();
        assert!(
            err.contains("未开放"),
            "归属但在册未开放的动作要由开放门拒: {err}"
        );
        assert!(err.contains("relay/set"), "拒因点名当前开放集: {err}");
    }

    #[test]
    fn malformed_actions_are_named_not_fatal() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        // actions 不是数组
        let out =
            rr3_intent_actions_handle(&json!("开灯"), &mut gpio, &mut servo, &servo_cfg(), false);
        assert!(out[0].as_ref().unwrap_err().contains("数组"));
        // 元素缺 topic / 缺 args
        let out = rr3_intent_actions_handle(
            &json!([{"args": {}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            false,
        );
        assert!(out[0].as_ref().unwrap_err().contains("topic"));
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set"}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
        );
        assert!(out[0].as_ref().unwrap_err().contains("args"));
        // 参数裁决照常把门（非真布尔）
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": 1}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
        );
        assert!(out[0].as_ref().unwrap_err().contains("布尔"));
        assert!(!gpio.rr3_sim_gpio_level(), "全部被拒，GPIO 不得变化");
        assert_eq!(servo.rr3_sim_servo_angle(), 90, "全部被拒，舵机不得动");
    }

    #[test]
    fn multiple_actions_execute_in_order() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        let out = rr3_intent_actions_handle(
            &json!([
                {"topic": "rr3-act/relay/set", "args": {"on": true}},
                {"topic": "rr3-act/servo/command", "args": {"angle_deg": 45}},
                {"topic": "rr3-act/relay/set", "args": {"on": false}},
            ]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
        );
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], Ok(Rr3IntentOutcome::Relay(true, false)));
        assert_eq!(out[1], Ok(Rr3IntentOutcome::Servo(45)));
        assert_eq!(out[2], Ok(Rr3IntentOutcome::Relay(false, true)));
        assert_eq!(servo.rr3_sim_servo_angle(), 45, "舵机动作按序落效");
        assert!(!gpio.rr3_sim_gpio_level(), "按序执行后最终态=最后一条");
    }

    /// MC-5 低电禁动闸门·intent 路两腿对拍（§20 ② ②）：
    /// 腿一·确定性——同一闭锁输入两跑逐条相等；
    /// 腿二·正确性——执行面（servo/relay）拒动且拒因点名"低电禁动"，
    /// 妆面（led）不因低电禁动而拒（仍走开放门=在册未开放），解禁后照常执行。
    #[test]
    fn low_gate_blocks_effector_not_face_on_intent_path() {
        let args = json!([
            {"topic": "rr3-act/relay/set", "args": {"on": true}},
            {"topic": "rr3-act/servo/command", "args": {"angle_deg": 45}},
            {"topic": "rr3-act/led/effect", "args": {}},
        ]);

        // 腿一·确定性：闭锁态两跑逐条相等（无隐藏状态）
        let run = |low_blocked: bool| {
            let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
            let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
            let out =
                rr3_intent_actions_handle(&args, &mut gpio, &mut servo, &servo_cfg(), low_blocked);
            (out, gpio.rr3_sim_gpio_level(), servo.rr3_sim_servo_angle())
        };
        assert_eq!(run(true), run(true), "闭锁态两跑必须逐拍相等");
        assert_eq!(run(false), run(false), "开放态两跑必须逐拍相等");

        // 腿二·正确性：闭锁只辖执行面（servo/relay 拒动），妆面 led 不因低电被禁
        let (blocked, gpio_lvl, servo_deg) = run(true);
        assert!(blocked[0].is_err(), "闭锁：relay 必须拒: {blocked:?}");
        assert!(
            blocked[0].as_ref().unwrap_err().contains("低电禁动"),
            "relay 拒因点名低电禁动: {blocked:?}"
        );
        assert!(blocked[1].is_err(), "闭锁：servo 必须拒: {blocked:?}");
        assert!(
            blocked[1].as_ref().unwrap_err().contains("低电禁动"),
            "servo 拒因点名低电禁动: {blocked:?}"
        );
        assert!(
            blocked[2].is_err() && blocked[2].as_ref().unwrap_err().contains("未开放"),
            "led 不因低电被禁：拒因是开放门（在册未开放），非低电禁动: {blocked:?}"
        );
        assert!(!gpio_lvl, "闭锁：relay 不得落到 GPIO");
        assert_eq!(servo_deg, 90, "闭锁：舵机不得动");

        // 解禁后（回阈+迟滞的开放态）照常执行，执行面复位生效
        let (open, gpio_lvl, servo_deg) = run(false);
        assert_eq!(
            open[0],
            Ok(Rr3IntentOutcome::Relay(true, false)),
            "解禁：relay 照常执行"
        );
        assert_eq!(
            open[1],
            Ok(Rr3IntentOutcome::Servo(45)),
            "解禁：servo 照常执行"
        );
        assert!(gpio_lvl, "解禁：relay 落到 GPIO");
        assert_eq!(servo_deg, 45, "解禁：舵机落角");
    }
}
