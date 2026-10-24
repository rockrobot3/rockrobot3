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
use crate::led::{Rr3Led, Rr3LedEffect, rr3_led_effects_join};
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
pub(crate) fn rr3_servo_motion_reject(word: &str) -> String {
    format!("不认的动作形态 {word:?}（运行命令面只认 jog/auto）")
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
    // 拒动硬约束（§6，批N3 件三）：词缺席 = auto（保守——不申报按自动）；
    // 判据拒收回 InvalidParam，折业务性失败点名言（批N34 件二：机制层零堆
    // 枚举，词面文案在编排层渲染——契约字面量逐字不变）
    let motion = servo::rr3_servo_check_motion(args.get("motion").and_then(Value::as_str))
        .map_err(|servo::Rr3ServoMotionError::UnknownWord(word)| rr3_servo_motion_reject(word))?;
    rr_hal_api::rr3_act_motion_allowed(axis.hard_stop, axis.calibrated, motion)
        .map_err(|e| format!("动作形态 {} 被拒: {e}", motion.rr3_act_motion_name()))?;
    let (angle, move_ms) = rr3_servo_parse_args(args, cfg)?;
    // 安全核钳位最后包一道（批D₃ 裁定 5：安全核永远包住控制图）——上游裁决
    // 被绕过时输出也不越行程；输入合法时恒等，行为守恒
    let angle = rr3_travel_clamp(angle, cfg.angle_min_deg, cfg.angle_max_deg);
    // HAL 写失败按业务性失败折 error（同 interact 的后端写失败口径）
    servo_dev
        .set_angle(angle)
        .map_err(|e| format!("舵机写入失败: {e}"))?;
    eprintln!("servo: angle={angle} move_ms={move_ms}（sim 即时到位；实机按时限走反射环闭环）");
    Ok((
        json!({ "angle_deg": angle, "move_ms": move_ms }),
        Some(angle),
        None,
    ))
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
        applied.rr3_led_effect_as_str(),
        previous.rr3_led_effect_as_str()
    );
    Ok((
        json!({
            "effect": applied.rr3_led_effect_as_str(),
            "previous": previous.rr3_led_effect_as_str(),
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
pub fn rr3_relay_apply_error_text(e: &relay::Rr3RelayApplyError) -> String {
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
                .map(Rr3LedEffect::rr3_led_effect_as_str)
                .collect::<Vec<_>>(),
        },
        "servo": {
            "move_timeout_ms": cfg.servo.move_timeout_ms,
            "state_min_interval_ms": cfg.servo.state_min_interval_ms,
        },
    })
}

/// servo/command 参数下钻（Value→纯值）：缺参/类型错/越 u16 在这里点名；
/// 行程与时限的值域判定留件（`servo::rr3_servo_check_travel` /
/// `servo::rr3_servo_check_timeout`），点名顺序与上移前逐字同序。
pub fn rr3_servo_parse_args(args: &Value, cfg: &Rr3ServoCfg) -> Result<(u16, u64), String> {
    let angle = args
        .get("angle_deg")
        .ok_or_else(|| "缺 angle_deg（int，行程内角度）".to_string())?;
    let angle = angle
        .as_u64()
        .ok_or_else(|| format!("angle_deg 必须是非负整数，收到 {angle}"))?;
    let angle = u16::try_from(angle).map_err(|_| format!("angle_deg 超出 u16 范围: {angle}"))?;
    // 行程/时限的值域判定留件（零堆枚举，批N24 件三），文案在此渲染逐字
    // （act 册 §9 契约字面量，每句落在唯一一处）
    servo::rr3_servo_check_travel(angle, cfg).map_err(|e| match e {
        servo::Rr3ServoTravelError::OutOfRange {
            angle_deg,
            min_deg,
            max_deg,
        } => format!("angle_deg={angle_deg} 超出行程 [{min_deg}, {max_deg}]"),
    })?;
    let move_ms = match args.get("move_ms") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .ok_or_else(|| format!("move_ms 必须是非负整数，收到 {v}"))?,
        ),
    };
    let move_ms = servo::rr3_servo_check_timeout(move_ms, cfg).map_err(|e| match e {
        servo::Rr3ServoTimeoutError::ZeroMs => "move_ms 必须大于 0".to_string(),
        servo::Rr3ServoTimeoutError::BeyondLimit { ms, limit } => {
            format!("move_ms={ms} 超出时限上限 {limit}（不给堵转留机会）")
        }
    })?;
    Ok((angle, move_ms))
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
mod tests {
    use super::*;

    fn servo_cfg() -> servo::Rr3ServoCfg {
        servo::Rr3ServoCfg {
            angle_min_deg: 0,
            angle_max_deg: 180,
            move_timeout_ms: 800,
            state_min_interval_ms: 50,
        }
    }

    /// 仓内默认灯效白名单全集（三值）——下钻用例的统一实参基线
    fn led_effects_all() -> Vec<Rr3LedEffect> {
        vec![Rr3LedEffect::On, Rr3LedEffect::Off, Rr3LedEffect::Blink]
    }

    /// 拒动硬约束·RPC 路（§6，批N3 件三）：未标定轴的 servo/command（无 motion
    /// 键 = auto 保守缺省）折 result.error 点名"未标定"；显式 jog 落角；已标定
    /// 轴 auto 照常。与 intent 路同判（判据 rr-hal-api 单真值点）。
    #[test]
    fn rpc_servo_refuses_uncalibrated_auto_but_jogs() {
        let conservative = Rr3ActAxisState::rr3_conservative();
        let calibrated = Rr3ActAxisState {
            hard_stop: rr_hal_api::Rr3ActHardStop::Yes,
            calibrated: true,
            lo_deg: 60,
            hi_deg: 120,
            center_deg: 90,
        };
        let run = |args: &Value, axis: &Rr3ActAxisState| {
            let mut servo_dev = rr_hal::sim::Rr3SimServo::rr3_sim_servo_new(90);
            rr3_handle_servo(&mut servo_dev, &servo_cfg(), args, false, axis)
        };
        // 未标定 + auto（缺省）：拒动，拒因点名动作形态与未标定
        let e = run(&json!({"angle_deg": 90}), &conservative).unwrap_err();
        assert!(e.contains("未标定"), "{e}");
        assert!(e.contains("auto"), "{e}");
        // 未标定 + 显式 jog：点动落角
        let v = run(&json!({"angle_deg": 90, "motion": "jog"}), &conservative).unwrap();
        assert_eq!(v.0["angle_deg"], 90);
        // 已标定 + auto：照常走位
        let v = run(&json!({"angle_deg": 90}), &calibrated).unwrap();
        assert_eq!(v.0["angle_deg"], 90);
        // 标定面词（calib-*）不进运行命令面（mech 白名单窄化）；拒因全链逐字
        // （error wire 契约字面量——批N34 件二渲染归编排层，自 mech contains
        // 断言升级为全链全等钉，面板与 legacy 依赖字面）
        let e = run(
            &json!({"angle_deg": 90, "motion": "calib-openloop"}),
            &calibrated,
        )
        .unwrap_err();
        assert_eq!(
            e,
            "不认的动作形态 \"calib-openloop\"（运行命令面只认 jog/auto）"
        );
    }

    /// relay/set 参数裁决：只认真布尔（自 relay 原子件迁入，文案断言逐字）
    #[test]
    fn relay_parse_accepts_only_real_booleans() {
        assert!(rr3_relay_parse_args(&json!({"on": true})).unwrap());
        assert!(!rr3_relay_parse_args(&json!({"on": false})).unwrap());
        for bad in [
            json!({}),
            json!({"on": 1}),
            json!({"on": "true"}),
            json!({"on": null}),
        ] {
            assert!(rr3_relay_parse_args(&bad).is_err(), "非真布尔必须拒: {bad}");
        }
        let e = rr3_relay_parse_args(&json!({"on": 1})).unwrap_err();
        assert!(e.contains("布尔"), "类型不符要点名: {e}");
    }

    /// relay 应答/摘要体逐字对齐契约 schema（schema 钉死键数，多键即漂移）
    #[test]
    fn relay_body_matches_contract_shape() {
        assert_eq!(rr3_relay_result(true)["on"], true);
        let v = rr3_relay_state_body(3, true);
        assert_eq!(v["seq"], 3);
        assert_eq!(v["on"], true);
        assert_eq!(
            v.as_object().map(|m| m.len()),
            Some(2),
            "schema 钉死两键，多键即漂移: {v}"
        );
    }

    /// servo/command 下钻全链：合法值三条路径（缺 move_ms / 显式时限 / 行程边界）
    #[test]
    fn servo_parse_accepts_angle_with_optional_move_ms() {
        let (angle, ms) = rr3_servo_parse_args(&json!({"angle_deg": 90}), &servo_cfg()).unwrap();
        assert_eq!((angle, ms), (90, 800), "缺 move_ms 取 config 上限");
        let (angle, ms) =
            rr3_servo_parse_args(&json!({"angle_deg": 0, "move_ms": 200}), &servo_cfg()).unwrap();
        assert_eq!((angle, ms), (0, 200), "行程下界合法，时限显式生效");
        let (angle, _) =
            rr3_servo_parse_args(&json!({"angle_deg": 180, "move_ms": 800}), &servo_cfg()).unwrap();
        assert_eq!(angle, 180, "行程上界合法");
    }

    /// 下钻点名：缺参/类型错/越 u16 在承载层；行程内越界由件判定点名
    #[test]
    fn servo_parse_rejects_missing_non_integer_and_out_of_range() {
        let e = rr3_servo_parse_args(&json!({}), &servo_cfg()).unwrap_err();
        assert!(e.contains("angle_deg"), "缺参数要点名: {e}");
        for bad in [
            json!({"angle_deg": -5}),
            json!({"angle_deg": "90"}),
            json!({"angle_deg": 90.5}),
        ] {
            assert!(
                rr3_servo_parse_args(&bad, &servo_cfg()).is_err(),
                "非合法非负整数必须拒: {bad}"
            );
        }
        let e = rr3_servo_parse_args(&json!({"angle_deg": 70000}), &servo_cfg()).unwrap_err();
        assert!(e.contains("u16"), "越 u16 先于行程点名: {e}");
        let e = rr3_servo_parse_args(&json!({"angle_deg": 181}), &servo_cfg()).unwrap_err();
        assert!(e.contains("行程"), "行程内越界由件判定点名: {e}");
    }

    #[test]
    fn servo_parse_rejects_move_ms_beyond_timeout_and_zero() {
        let e = rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 801}), &servo_cfg())
            .unwrap_err();
        assert!(e.contains("时限上限"), "超上限拒收: {e}");
        let e = rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 0}), &servo_cfg())
            .unwrap_err();
        assert!(e.contains("大于 0"), "零时限拒收: {e}");
        let e = rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": "x"}), &servo_cfg())
            .unwrap_err();
        assert!(e.contains("move_ms"), "类型不符点名: {e}");
    }

    /// 错误文案逐字钉死（批N24 件三，L-3 形状清理的对账单测）：机制层改交
    /// 零堆枚举后，编排层渲染的 `String` 必须与改前逐字一致——act 册 §9
    /// 契约字面量（面板与 legacy 依赖字面），零行为改变的直接证据。
    #[test]
    fn servo_parse_error_texts_are_verbatim() {
        assert_eq!(
            rr3_servo_parse_args(&json!({"angle_deg": 181}), &servo_cfg()).unwrap_err(),
            "angle_deg=181 超出行程 [0, 180]"
        );
        assert_eq!(
            rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 801}), &servo_cfg())
                .unwrap_err(),
            "move_ms=801 超出时限上限 800（不给堵转留机会）"
        );
        assert_eq!(
            rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 0}), &servo_cfg())
                .unwrap_err(),
            "move_ms 必须大于 0"
        );
    }

    /// relay 错误文案逐字钉死（批N24 件三）：两腿渲染各一断言，`{e}` 走
    /// `Rr3HalError` Display（HAL 错文本随行原样）。
    #[test]
    fn relay_apply_error_text_is_verbatim() {
        use rr_hal_api::Rr3HalError;
        let e = relay::Rr3RelayApplyError::ReadBack(Rr3HalError::Busy);
        assert_eq!(rr3_relay_apply_error_text(&e), "继电器读回失败: HAL 设备忙");
        let e = relay::Rr3RelayApplyError::Write(Rr3HalError::Busy);
        assert_eq!(rr3_relay_apply_error_text(&e), "继电器写入失败: HAL 设备忙");
    }

    /// servo/state 体 seq/angle 两键钉死（自 servo 原子件迁入）
    #[test]
    fn servo_state_body_carries_seq_and_angle() {
        let v = rr3_servo_state_body(7, 135);
        assert_eq!(v["seq"], 7);
        assert_eq!(v["angle_deg"], 135);
    }

    /// led/effect 下钻：缺/非串/不认逐条点名（A-9：白名单子集由注入实参给）
    #[test]
    fn led_parse_args_rejects_missing_non_string_and_unknown() {
        let all = led_effects_all();
        let e = rr3_led_parse_args(&json!({}), &all).unwrap_err();
        assert!(e.contains("effect"), "缺参数要点名: {e}");
        let e = rr3_led_parse_args(&json!({"effect": 3}), &all).unwrap_err();
        assert!(e.contains("字符串"), "类型不符要点名: {e}");
        let e = rr3_led_parse_args(&json!({"effect": "strobe"}), &all).unwrap_err();
        assert!(e.contains("白名单"), "不认的值点名白名单: {e}");
    }

    /// A-9：注入子集收窄即行为改变——在册但不在子集内的值被拒，报本形态白名单
    #[test]
    fn led_parse_args_narrows_by_injected_subset() {
        let only_on = [Rr3LedEffect::On];
        assert_eq!(
            rr3_led_parse_args(&json!({"effect": "on"}), &only_on).unwrap(),
            Rr3LedEffect::On,
            "子集内合法"
        );
        let e = rr3_led_parse_args(&json!({"effect": "blink"}), &only_on).unwrap_err();
        assert!(e.contains("blink"), "被拒的值要点名: {e}");
        assert!(e.contains("on"), "拒因要点名本形态白名单: {e}");
        // 空子集：一条点名都不认（合法数据，运行期全拒）
        let e = rr3_led_parse_args(&json!({"effect": "on"}), &[]).unwrap_err();
        assert!(e.contains("(空)"), "空白名单点名为空: {e}");
    }
}
