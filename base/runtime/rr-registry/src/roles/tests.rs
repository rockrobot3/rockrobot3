//! roles 单测（文件头：本文件只放测试，生产面在 `roles.rs`）。
//!
//! 拆分动因：`roles.rs` 516 行、内联测试自 268 行起占近半，先按仓内同形搬出。

use super::*;

// include_str! 以本文件所在目录为基准：roles → src → rr-registry → runtime → base → 仓库根
const CONTRACT: &str = include_str!("../../../../../contracts/roles.toml");

/// 契约解析成 14 条角色，名字面顺序稳定（P1 件四四格能力面角色进场后十四条）。
fn role_names_shape(t: &Rr3RoleTable) {
    assert_eq!(
        t.roles.len(),
        14,
        "契约角色数（P1 件四四格能力面角色进场后十四条）"
    );
    assert_eq!(
        t.rr3_role_names(),
        vec![
            "act.axis.pwm",
            "act.led.gpio",
            "act.relay.gpio",
            "audio.capture.mic",
            "audio.playback.spk",
            "audio.vad.gate",
            "net.wifi.link",
            "power.battery.meter",
            "power.charger.meter",
            "power.current.sense",
            "power.rail.sense",
            "sensor.env.meter",
            "sensor.imu.meter",
            "ui.face.spi",
        ]
    );
}

/// act 三角色：slot 外键、per_instance、requires/derives、droppable 与槽内名单。
fn act_role_shape(t: &Rr3RoleTable) {
    assert_eq!(
        t.rr3_role_slot("act.axis.pwm").as_deref(),
        Some("act.servo")
    );
    assert_eq!(t.rr3_role_per_inst("act.axis.pwm"), Some(true));
    assert_eq!(t.rr3_role_per_inst("act.led.gpio"), Some(false));
    assert!(!t.rr3_role_contains("act.axis.enc"));
    assert_eq!(
        t.rr3_role_requires("act.axis.pwm"),
        Some(vec![
            "controller".to_string(),
            "channel".to_string(),
            "physical".to_string(),
            "rail".to_string(),
            "failsafe".to_string(),
        ])
    );
    assert_eq!(
        t.rr3_role_derives("act.led.gpio"),
        Some(vec!["gpio_chip".to_string(), "gpio_led_line".to_string(),])
    );
    assert_eq!(t.rr3_role_droppable("act.relay.gpio"), Some(false));
    assert_eq!(t.rr3_role_slot_names("act.servo"), vec!["act.axis.pwm"]);
}

/// power 角色面：battery/rail/current 三测点与 charger 的 device 键引入。
fn power_role_shape(t: &Rr3RoleTable) {
    assert_eq!(
        t.rr3_role_kind("power.battery.meter").as_deref(),
        Some("signal")
    );
    // 批N2 件六：域测点两角色同挂 power.rail（per_instance、requires 同 battery 形）
    assert_eq!(
        t.rr3_role_slot("power.rail.sense").as_deref(),
        Some("power.rail")
    );
    assert_eq!(t.rr3_role_per_inst("power.rail.sense"), Some(true));
    assert_eq!(t.rr3_role_droppable("power.current.sense"), Some(false));
    assert_eq!(
        t.rr3_role_requires("power.rail.sense"),
        Some(vec!["controller".to_string(), "addr".to_string()])
    );
    assert_eq!(
        t.rr3_role_slot_names("power.rail"),
        vec!["power.current.sense", "power.rail.sense"]
    );
    assert!(!t.rr3_role_contains("power.switch"));
    // 批N3 件八：I2C 读数器件首引 device 键；derives 显式空 = 不派生注入键
    assert_eq!(
        t.rr3_role_slot("power.charger.meter").as_deref(),
        Some("power.charger")
    );
    assert_eq!(
        t.rr3_role_iface("power.charger.meter").as_deref(),
        Some("Rr3Power")
    );
    assert_eq!(
        t.rr3_role_requires("power.charger.meter"),
        Some(vec![
            "controller".to_string(),
            "addr".to_string(),
            "device".to_string()
        ])
    );
    assert_eq!(t.rr3_role_derives("power.charger.meter"), Some(vec![]));
    assert_eq!(t.rr3_role_droppable("power.charger.meter"), Some(false));
    assert_eq!(t.rr3_role_per_inst("power.charger.meter"), Some(false));
}

/// ui/sensor 角色面：SPI 显示词形与 I2C 读数两器件（iface/direction 逐项）。
fn ui_sensor_role_shape(t: &Rr3RoleTable) {
    // 批N3 件六：slot=ui.face，iface=Rr3Display，requires=controller+channel
    assert_eq!(t.rr3_role_slot("ui.face.spi").as_deref(), Some("ui.face"));
    assert_eq!(t.rr3_role_per_inst("ui.face.spi"), Some(false));
    assert_eq!(t.rr3_role_droppable("ui.face.spi"), Some(true));
    assert_eq!(
        t.rr3_role_requires("ui.face.spi"),
        Some(vec!["controller".to_string(), "channel".to_string()])
    );
    assert_eq!(
        t.rr3_role_derives("ui.face.spi"),
        Some(vec!["spi_device".to_string()])
    );
    assert_eq!(t.rr3_role_direction("ui.face.spi").as_deref(), Some("out"));
    assert_eq!(
        t.rr3_role_slot("sensor.env.meter").as_deref(),
        Some("sensor.env")
    );
    assert_eq!(
        t.rr3_role_iface("sensor.env.meter").as_deref(),
        Some("Rr3Sensor")
    );
    assert_eq!(
        t.rr3_role_slot("sensor.imu.meter").as_deref(),
        Some("sensor.imu")
    );
    assert_eq!(
        t.rr3_role_iface("sensor.imu.meter").as_deref(),
        Some("Rr3Sensor")
    );
    assert_eq!(t.rr3_role_derives("sensor.imu.meter"), Some(vec![]));
    assert_eq!(
        t.rr3_role_direction("sensor.env.meter").as_deref(),
        Some("in")
    );
}

/// P1 件四四格能力面：logic 无引脚绑定，requires/derives 双显式空。
fn capability_role_shape(t: &Rr3RoleTable) {
    for (name, slot, iface, dir) in [
        ("net.wifi.link", "net.wifi", "Rr3Link", "io"),
        ("audio.capture.mic", "audio.capture", "Rr3AudioSource", "in"),
        (
            "audio.playback.spk",
            "audio.playback",
            "Rr3AudioSink",
            "out",
        ),
        ("audio.vad.gate", "audio.vad", "Rr3AudioSource", "in"),
    ] {
        assert_eq!(t.rr3_role_slot(name).as_deref(), Some(slot));
        assert_eq!(t.rr3_role_iface(name).as_deref(), Some(iface));
        assert_eq!(t.rr3_role_kind(name).as_deref(), Some("logic"));
        assert_eq!(t.rr3_role_requires(name), Some(vec![]));
        assert_eq!(t.rr3_role_derives(name), Some(vec![]));
        assert_eq!(t.rr3_role_direction(name).as_deref(), Some(dir));
        assert_eq!(t.rr3_role_per_inst(name), Some(false));
    }
}

/// 契约必须解析成 14 条角色，逐条点名断言（与 contract.rs 的形状 pin 同源、彼此独立）
#[test]
fn contract_text_parses_into_expected_shape() {
    let table = rr3_role_parse(CONTRACT);
    role_names_shape(&table);
    act_role_shape(&table);
    power_role_shape(&table);
    ui_sensor_role_shape(&table);
    capability_role_shape(&table);
    assert!(
        table.rr3_role_check().is_empty(),
        "{:?}",
        table.rr3_role_check()
    );
}

/// kind=logic 的 requires 显式 [] 合法；signal 角色留空仍必须红（形状放宽的边界）
#[test]
fn empty_requires_is_logical_only() {
    let logic = rr3_role_parse(
        "[[role]]\nname = \"a.b\"\nslot = \"net.wifi\"\nkind = \"logic\"\niface = \"Rr3Link\"\nrequires = []\nderives = []\ndirection = \"io\"\n",
    );
    assert!(
        logic.rr3_role_check().is_empty(),
        "{:?}",
        logic.rr3_role_check()
    );
    let signal = rr3_role_parse(
        "[[role]]\nname = \"a.b\"\nslot = \"act.led\"\nkind = \"signal\"\niface = \"Rr3GpioOut\"\nrequires = []\nderives = []\ndirection = \"out\"\n",
    );
    let errs = signal.rr3_role_check();
    assert!(
        errs.iter().any(|e| e.contains("requires 为空")),
        "signal 角色 requires 留空必须点名: {errs:?}"
    );
}

/// 坏文本给空表，空表必须报错（fail-open 护栏）
#[test]
fn unparsable_text_is_not_a_pass_port() {
    let table = rr3_role_parse("this = is not = valid = toml");
    assert!(table.rr3_role_is_empty());
    assert!(!table.rr3_role_check().is_empty());
}

/// 形状检查：kind 越界 / direction 越界 / 缺 slot / 缺 iface / requires 空
/// 逐条点名（§4.1 字段域封闭判据的装载侧实现；derives 空 = 显式不派生合法，
/// 批N3 件八放宽——见 rr3_role_check 注记，无对应红例）
#[test]
fn shape_violations_are_named() {
    let bad = rr3_role_parse(
        r#"
[[role]]
name = "act.axis.pwm"
kind = "magical"
iface = "Rr3PwmServo"
requires = ["controller"]
derives = ["pwm_chip"]
direction = "sideways"
droppable = true
input_only_ok = false
per_instance = true
[[role]]
name = "act.led.gpio"
slot = "act.led"
kind = "signal"
requires = ["controller"]
derives = []
direction = "out"
droppable = false
input_only_ok = false
per_instance = false
"#,
    );
    let errs = bad.rr3_role_check();
    assert!(
        errs.iter().any(|e| e.contains("act.axis.pwm 缺 slot 外键")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("kind 非法: magical")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("direction 非法: sideways")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("act.led.gpio 缺 iface 外键")),
        "{errs:?}"
    );
}
