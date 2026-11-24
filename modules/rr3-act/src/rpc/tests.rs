// SPDX-License-Identifier: Apache-2.0
//! rr3-act RPC 面单测（自 `rpc.rs` 内联模块原样外置，判据与文案不变）。

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
fn rpc_refuses_but_jogs() {
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
fn relay_ok_booleans() {
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
fn relay_body_shape() {
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
fn servo_ok_move_ms() {
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
fn servo_rej_miss_range() {
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
fn servo_rej_ms_zero() {
    let e =
        rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 801}), &servo_cfg()).unwrap_err();
    assert!(e.contains("时限上限"), "超上限拒收: {e}");
    let e =
        rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 0}), &servo_cfg()).unwrap_err();
    assert!(e.contains("大于 0"), "零时限拒收: {e}");
    let e =
        rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": "x"}), &servo_cfg()).unwrap_err();
    assert!(e.contains("move_ms"), "类型不符点名: {e}");
}

/// 错误文案逐字钉死（批N24 件三，L-3 形状清理的对账单测）：机制层改交
/// 零堆枚举后，编排层渲染的 `String` 必须与改前逐字一致——act 册 §9
/// 契约字面量（面板与 legacy 依赖字面），零行为改变的直接证据。
#[test]
fn servo_error_verbatim() {
    assert_eq!(
        rr3_servo_parse_args(&json!({"angle_deg": 181}), &servo_cfg()).unwrap_err(),
        "angle_deg=181 超出行程 [0, 180]"
    );
    assert_eq!(
        rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 801}), &servo_cfg()).unwrap_err(),
        "move_ms=801 超出时限上限 800（不给堵转留机会）"
    );
    assert_eq!(
        rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 0}), &servo_cfg()).unwrap_err(),
        "move_ms 必须大于 0"
    );
}

/// relay 错误文案逐字钉死（批N24 件三）：两腿渲染各一断言，`{e}` 走
/// `Rr3HalError` Display（HAL 错文本随行原样）。
#[test]
fn relay_error_verbatim() {
    use rr_hal_api::Rr3HalError;
    let e = relay::Rr3RelayApplyError::ReadBack(Rr3HalError::Busy);
    assert_eq!(rr3_relay_err_text(&e), "继电器读回失败: HAL 设备忙");
    let e = relay::Rr3RelayApplyError::Write(Rr3HalError::Busy);
    assert_eq!(rr3_relay_err_text(&e), "继电器写入失败: HAL 设备忙");
}

/// servo/state 体 seq/angle 两键钉死（自 servo 原子件迁入）
#[test]
fn servo_body_seq_angle() {
    let v = rr3_servo_state_body(7, 135);
    assert_eq!(v["seq"], 7);
    assert_eq!(v["angle_deg"], 135);
}

/// led/effect 下钻：缺/非串/不认逐条点名（A-9：白名单子集由注入实参给）
#[test]
fn led_rej_miss_unknown() {
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
fn led_args_subset() {
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
