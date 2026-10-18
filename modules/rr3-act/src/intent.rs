//! intent/result 订阅执行面（0.0.3 T3，方案 Q1 裁定）：brain 产出结构化
//! `actions`（元素 `{topic, args}`，contracts/events/brain-intent-result.schema.json），
//! 本模块广播认领——**两道归属门**后才执行：
//!
//! ① 归属门：topic ∈ 本模块 exposes（不越权——发布侧 `rr3_reg_publish_check`
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

use super::led::RR3_LED_EFFECT_TOPIC;
use super::relay::{self, RR3_RELAY_SET_TOPIC};
use super::servo::{rr3_servo_check_motion, Rr3ServoCfg, RR3_SERVO_COMMAND_TOPIC};
use super::{rr3_low_power_reject, rr3_relay_parse_args, rr3_servo_parse_args, Rr3ActAxisState};

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
pub fn rr3_intent_actions_handle(
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
        .map(|item| {
            rr3_intent_action_handle(item, gpio, servo, servo_cfg, low_blocked, axis, open_topics)
        })
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

/// servo 动作的形态词下钻（承载层 Value 下钻，值域判定留件 mech）：词缺席 =
/// auto（保守缺省——不申报按自动）。
fn servo_motion_of(args: &Value) -> Result<rr_hal_api::Rr3ActMotion, String> {
    rr3_servo_check_motion(args.get("motion").and_then(Value::as_str))
}

/// 单条动作：形状裁决 → 两道门 → 执行（低电闸门只辖执行面；servo 动作再过
/// 拒动判据——批N3 件三，与 RPC 路同判）
fn rr3_intent_action_handle(
    item: &Value,
    gpio: &mut dyn rr_hal_api::Rr3GpioOut,
    servo: &mut dyn rr_hal_api::Rr3PwmServo,
    servo_cfg: &Rr3ServoCfg,
    low_blocked: bool,
    axis: &Rr3ActAxisState,
    open_topics: &[String],
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
    // 开放门（A-9）：开放集由 config 注入（组合件读入透传），不再硬编码 match；
    // 已归属但不在开放集的主题（led/effect）到此拒执行。
    if !open_topics.iter().any(|t| t.as_str() == topic) {
        return Err(format!(
            "已归属但意图执行面未开放 {topic}（当前开放集 {}）",
            rr3_open_topics_join(open_topics)
        ));
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
            // 拒动硬约束（§6，批N3 件三）：广播无 motion 键 = auto（保守缺省），
            // 未标定轴被代码拒动——判据与 RPC 路同源（rr-hal-api 单真值点）
            let motion = servo_motion_of(args)?;
            rr_hal_api::rr3_act_motion_allowed(axis.hard_stop, axis.calibrated, motion)
                .map_err(|e| format!("动作形态 {} 被拒: {e}", motion.rr3_act_motion_name()))?;
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
        // 开放集已 boot 校验只含上述两条执行臂主题；此臂为配置/执行面不一致的兜底
        other => Err(format!(
            "意图开放集含 {other} 但本模块无对应执行臂（配置与执行面不一致）"
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

    /// 仓内默认开放集（relay/set + servo/command）——既有用例的统一第 6 实参基线
    fn open_all() -> Vec<String> {
        vec![
            RR3_RELAY_SET_TOPIC.to_string(),
            RR3_SERVO_COMMAND_TOPIC.to_string(),
        ]
    }

    /// 已标定轴态——既有用例的统一第 7 实参基线（auto 放行的正面）
    fn axis_calibrated() -> Rr3ActAxisState {
        Rr3ActAxisState {
            hard_stop: rr_hal_api::Rr3ActHardStop::Yes,
            calibrated: true,
        }
    }

    /// 拒动硬约束·intent 路（§6，批N3 件三）：未标定轴的 servo 广播（无 motion
    /// 键 = auto 保守缺省）被代码拒动不落角、拒因点名"未标定"；显式 jog 放行；
    /// 已标定轴 auto 照常（既有基线不动）。
    #[test]
    fn uncalibrated_axis_refuses_auto_but_jogs() {
        let args_auto = json!([{"topic": "rr3-act/servo/command", "args": {"angle_deg": 45}}]);
        let args_jog =
            json!([{"topic": "rr3-act/servo/command", "args": {"angle_deg": 45, "motion": "jog"}}]);
        let conservative = Rr3ActAxisState::rr3_conservative();
        let run = |args: &Value, axis: &Rr3ActAxisState| {
            let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
            let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
            let out = rr3_intent_actions_handle(
                args,
                &mut gpio,
                &mut servo,
                &servo_cfg(),
                LOW_OPEN,
                axis,
                &open_all(),
            );
            (out, servo.rr3_sim_servo_angle())
        };
        // 未标定 + auto（缺省）：拒动，舵机纹丝不动
        let (out, deg) = run(&args_auto, &conservative);
        let err = out[0].as_ref().unwrap_err();
        assert!(err.contains("未标定"), "拒因点名未标定: {err}");
        assert!(err.contains("auto"), "拒因点名动作形态: {err}");
        assert_eq!(deg, 90, "未标定轴 auto 不得落角");
        // 未标定 + 显式 jog：点动是未标定轴唯一合法动作面
        let (out, deg) = run(&args_jog, &conservative);
        assert_eq!(out[0], Ok(Rr3IntentOutcome::Servo(45)), "{out:?}");
        assert_eq!(deg, 45, "jog 落角");
        // 已标定 + auto：照常走位（既有基线语义）
        let (out, deg) = run(&args_auto, &axis_calibrated());
        assert_eq!(out[0], Ok(Rr3IntentOutcome::Servo(45)), "{out:?}");
        assert_eq!(deg, 45);
        // relay 不受轴状态辖（闸门语义只对 servo 加判据）
        let (out, _) = run(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": true}}]),
            &conservative,
        );
        assert_eq!(out[0], Ok(Rr3IntentOutcome::Relay(true, false)), "{out:?}");
    }

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
            &axis_calibrated(),
            &open_all(),
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
            &axis_calibrated(),
            &open_all(),
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
                &axis_calibrated(),
                &open_all(),
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
                &axis_calibrated(),
                &open_all(),
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
            &axis_calibrated(),
            &open_all(),
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
        // led/effect 未列开放集（本形态不开 led 意图路）；relay/servo 在开放集内
        let out = rr3_intent_actions_handle(
            &json!([{ "topic": "rr3-act/led/effect", "args": {} }]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
            &axis_calibrated(),
            &open_all(),
        );
        let err = out[0].as_ref().unwrap_err();
        assert!(
            err.contains("未开放"),
            "归属但不在开放集的动作要由开放门拒: {err}"
        );
        assert!(err.contains("relay/set"), "拒因点名当前开放集: {err}");
    }

    /// A-9 核心证据：注入开放集**收窄**即执行面收窄——只开 relay 时 servo/command
    /// 被开放门拒、不落角；空开放集时已归属主题一律拒（改数据不改码）。
    #[test]
    fn injected_open_set_narrows_intent_surface() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        // 只开 relay/set：servo/command 被开放门拒
        let only_relay = vec![RR3_RELAY_SET_TOPIC.to_string()];
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/servo/command", "args": {"angle_deg": 45}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
            &axis_calibrated(),
            &only_relay,
        );
        let err = out[0].as_ref().unwrap_err();
        assert!(err.contains("未开放"), "收窄后 servo 必须被开放门拒: {err}");
        assert_eq!(servo.rr3_sim_servo_angle(), 90, "未开放主题不得落角");
        // 空开放集：relay 也被拒（拒因点名为空）
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": true}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
            &axis_calibrated(),
            &Vec::new(),
        );
        let err = out[0].as_ref().unwrap_err();
        assert!(err.contains("未开放"), "空开放集一律拒: {err}");
        assert!(err.contains("(空)"), "拒因点名开放集为空: {err}");
        assert!(!gpio.rr3_sim_gpio_level(), "空开放集不得碰 GPIO");
    }

    /// 开放集含本模块无执行臂的主题（配置/执行面不一致）→ 兜底臂点名，不静默
    /// 放行（boot 校验挡住这种配置，本臂是纵深防御）
    #[test]
    fn open_set_without_exec_arm_is_named() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        let with_led = vec![RR3_LED_EFFECT_TOPIC.to_string()];
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/led/effect", "args": {}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
            &axis_calibrated(),
            &with_led,
        );
        let err = out[0].as_ref().unwrap_err();
        assert!(
            err.contains("无对应执行臂"),
            "开放集含无臂主题要点名不一致: {err}"
        );
        assert!(!gpio.rr3_sim_gpio_level(), "被拒动作不得碰 GPIO");
    }

    #[test]
    fn malformed_actions_are_named_not_fatal() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
        // actions 不是数组
        let out = rr3_intent_actions_handle(
            &json!("开灯"),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            false,
            &axis_calibrated(),
            &open_all(),
        );
        assert!(out[0].as_ref().unwrap_err().contains("数组"));
        // 元素缺 topic / 缺 args
        let out = rr3_intent_actions_handle(
            &json!([{"args": {}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            false,
            &axis_calibrated(),
            &open_all(),
        );
        assert!(out[0].as_ref().unwrap_err().contains("topic"));
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set"}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
            &axis_calibrated(),
            &open_all(),
        );
        assert!(out[0].as_ref().unwrap_err().contains("args"));
        // 参数裁决照常把门（非真布尔）
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": 1}}]),
            &mut gpio,
            &mut servo,
            &servo_cfg(),
            LOW_OPEN,
            &axis_calibrated(),
            &open_all(),
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
            &axis_calibrated(),
            &open_all(),
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
    /// 妆面（led）不因低电禁动而拒（仍走开放门=未列开放集），解禁后照常执行。
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
            let out = rr3_intent_actions_handle(
                &args,
                &mut gpio,
                &mut servo,
                &servo_cfg(),
                low_blocked,
                &axis_calibrated(),
                &open_all(),
            );
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
            "led 不因低电被禁：拒因是开放门（未列开放集），非低电禁动: {blocked:?}"
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
