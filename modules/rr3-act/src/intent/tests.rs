//! intent 的单测（自同名单测模块原样抽出，判据不变）。

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

/// 已标定轴态——既有用例的统一第 7 实参基线（auto 放行的正面；行程
/// lo/hi/center 为件五五键形状的样本值，jog/auto 走位用例角度在其内）
fn axis_calibrated() -> Rr3ActAxisState {
    Rr3ActAxisState {
        hard_stop: rr_hal_api::Rr3ActHardStop::Yes,
        calibrated: true,
        lo_deg: 60,
        hi_deg: 120,
        center_deg: 90,
    }
}

/// 整包调用（十余处同款实参表）：servo_cfg/开放集/轴态三实参固定，仅低电
/// 闸门位与载荷随用例变化
fn act_call(
    payload: &Value,
    low_blocked: bool,
    gpio: &mut Rr3SimGpioOut,
    servo: &mut Rr3SimServo,
) -> Vec<Result<Rr3IntentOutcome, String>> {
    rr3_actions_handle(
        payload,
        gpio,
        servo,
        &servo_cfg(),
        low_blocked,
        &axis_calibrated(),
        &open_all(),
    )
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
        let out = rr3_actions_handle(
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
    let out = act_call(
        &json!([{"topic": "rr3-act/relay/set", "args": {"on": true}}]),
        LOW_OPEN,
        &mut gpio,
        &mut servo,
    );
    assert_eq!(
        out,
        vec![Ok(Rr3IntentOutcome::Relay(true, false))],
        "{out:?}"
    );
    assert!(gpio.rr3_sim_gpio_level(), "吸合必须落到 GPIO 电平");

    let out = act_call(
        &json!([{"topic": "rr3-act/relay/set", "args": {"on": false}}]),
        LOW_OPEN,
        &mut gpio,
        &mut servo,
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
        let out = act_call(
            &json!([{"topic": "rr3-act/servo/command", "args": {"angle_deg": deg}}]),
            LOW_OPEN,
            &mut gpio,
            &mut servo,
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
        let out = act_call(&json!([bad]), false, &mut gpio, &mut servo);
        assert!(out[0].is_err(), "非法 servo 动作必须拒: {out:?}");
    }
    assert_eq!(servo.rr3_sim_servo_angle(), 90, "全部被拒，舵机不得动");
}

#[test]
fn foreign_topic_is_rejected_by_exposes_gate() {
    let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
    let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
    let out = act_call(
        &json!([{"topic": "rr3-power/battery/state", "args": {}}]),
        LOW_OPEN,
        &mut gpio,
        &mut servo,
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
    let out = act_call(
        &json!([{ "topic": "rr3-act/led/effect", "args": {} }]),
        LOW_OPEN,
        &mut gpio,
        &mut servo,
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
    let out = rr3_actions_handle(
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
    let out = rr3_actions_handle(
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
    let out = rr3_actions_handle(
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
    let out = act_call(&json!("开灯"), false, &mut gpio, &mut servo);
    assert!(out[0].as_ref().unwrap_err().contains("数组"));
    // 元素缺 topic / 缺 args
    let out = act_call(&json!([{"args": {}}]), false, &mut gpio, &mut servo);
    assert!(out[0].as_ref().unwrap_err().contains("topic"));
    let out = act_call(
        &json!([{"topic": "rr3-act/relay/set"}]),
        LOW_OPEN,
        &mut gpio,
        &mut servo,
    );
    assert!(out[0].as_ref().unwrap_err().contains("args"));
    // 参数裁决照常把门（非真布尔）
    let out = act_call(
        &json!([{"topic": "rr3-act/relay/set", "args": {"on": 1}}]),
        LOW_OPEN,
        &mut gpio,
        &mut servo,
    );
    assert!(out[0].as_ref().unwrap_err().contains("布尔"));
    assert!(!gpio.rr3_sim_gpio_level(), "全部被拒，GPIO 不得变化");
    assert_eq!(servo.rr3_sim_servo_angle(), 90, "全部被拒，舵机不得动");
}

#[test]
fn multiple_actions_execute_in_order() {
    let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
    let mut servo = Rr3SimServo::rr3_sim_servo_new(90);
    let out = act_call(
        &json!([
            {"topic": "rr3-act/relay/set", "args": {"on": true}},
            {"topic": "rr3-act/servo/command", "args": {"angle_deg": 45}},
            {"topic": "rr3-act/relay/set", "args": {"on": false}},
        ]),
        LOW_OPEN,
        &mut gpio,
        &mut servo,
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
        let out = rr3_actions_handle(
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
