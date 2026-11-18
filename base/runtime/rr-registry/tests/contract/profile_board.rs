//! Pi 4B 板端点档（W4/P0-C）：双轴 pin 与板侧硬件的差异面。

use super::*;

/// Pi 4B 板端点（W4/P0-C）：`profiles/pi4b-board.toml` 钉**差异面**——rtos 薄板档
/// （host 单轴描述符不能容纳双轴，见文件头 N43-1 审查裁定）、chip=raspberry-pi-4、
/// [[pin]] pan/tilt 成对（pwm0 通道 0/1、GPIO12/13）、[hardware] 板侧四键、恰一格
/// act.servo（bindings/enabled/present 三源一致）。完整装载链由 rrd 的 boot drill
/// （pi4b_board_profile_boot_inputs_drill）覆盖，本测试只钉形状与线束。
#[test]
fn pi4b_board_profile_pins_w4_harness() {
    let root = repo_root();
    let table = rr_registry::rr3_slot_parse(&slots_toml());
    let text = std::fs::read_to_string(root.join("profiles/pi4b-board.toml"))
        .expect("profiles/pi4b-board.toml 必须存在（W4/P0-C）");
    let v: toml::Value = toml::from_str(&text).expect("pi4b-board.toml 必须可解析");

    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "pi4b-board schema_version 漂移"
    );
    assert_tier(
        &v,
        "rtos",
        "pi4b-board 档位漂移（W4 只把 Pi 当板：rtos 薄板档，host 单轴描述符不能容纳双轴）",
    );
    assert_chip(&v, "raspberry-pi-4", "pi4b-board");
    assert_eq!(
        v.get("os")
            .and_then(|o| o.get("route"))
            .and_then(toml::Value::as_str),
        Some("distro"),
        "pi4b-board route 漂移（Raspberry Pi OS 属发行版快路轨）"
    );

    // [hardware] 板侧总线四键：Pi 40-pin 真值（GPIO2/3 I2C1、GPIO14/15 UART0）。
    let hardware = v
        .get("hardware")
        .and_then(toml::Value::as_table)
        .expect("pi4b-board 缺 [hardware] 段（板侧总线真值）");
    for (k, expected) in [
        ("i2c_sda", "2"),
        ("i2c_scl", "3"),
        ("uart2_tx", "14"),
        ("uart2_rx", "15"),
    ] {
        assert_eq!(
            hardware.get(k).and_then(toml::Value::as_str),
            Some(expected),
            "pi4b-board hardware.{k} 漂移（W4 接线表 §9.2）"
        );
    }

    // 形态面：恰一格 act.servo，bindings/enabled_slots/present 三源一致。
    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("pi4b-board 缺 [bindings]");
    assert_eq!(
        bindings.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["act.servo"],
        "pi4b-board 绑定面应恰一格 act.servo（板端点只承载舵机）"
    );
    assert_eq!(
        bindings.get("act.servo").and_then(toml::Value::as_str),
        Some("rr3-act"),
        "pi4b-board bindings.act.servo 漂移"
    );
    let enabled = v
        .get("enabled_slots")
        .and_then(toml::Value::as_table)
        .expect("pi4b-board 缺 [enabled_slots]");
    assert_eq!(
        enabled.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["rr3-act"],
        "pi4b-board 启用模块面应恰 rr3-act"
    );
    assert_eq!(
        str_list(&v, "enabled_slots.rr3-act"),
        vec!["act.servo".to_string()],
        "pi4b-board enabled_slots 应恰 act.servo"
    );
    let present = v
        .get("slots")
        .and_then(|s| s.get("present"))
        .and_then(toml::Value::as_array)
        .expect("pi4b-board 缺 [slots].present");
    assert_eq!(
        present
            .iter()
            .filter_map(toml::Value::as_str)
            .collect::<Vec<_>>(),
        vec!["act.servo"],
        "pi4b-board 在位声明应恰 act.servo"
    );

    // [[pin]] 双轴成对：role/per_instance 形状走校验①；线束值逐项钉死。
    assert_profile_pins_schema3(&v, &root, &["act.axis.pwm", "act.axis.pwm"]);
    let pins = v
        .get("pin")
        .and_then(toml::Value::as_array)
        .expect("pi4b-board 缺 [[pin]]");
    assert_eq!(
        pins.len(),
        2,
        "pi4b-board [[pin]] 应恰两枚（pan/tilt 成对）"
    );
    for (i, (instance, channel, physical)) in
        [("pan", 0, 12), ("tilt", 1, 13)].into_iter().enumerate()
    {
        let p = &pins[i];
        assert_eq!(
            p.get("instance").and_then(toml::Value::as_str),
            Some(instance),
            "pi4b-board [[pin]]#{i} instance 漂移"
        );
        assert_eq!(
            p.get("controller").and_then(toml::Value::as_str),
            Some("pwm0"),
            "pi4b-board [[pin]]#{i} controller 漂移（pwmchip0 双通道）"
        );
        assert_eq!(
            p.get("channel").and_then(toml::Value::as_integer),
            Some(channel),
            "pi4b-board [[pin]]#{i} channel 漂移（pwm0/pwm1 通道目录）"
        );
        assert_eq!(
            p.get("physical").and_then(toml::Value::as_array).map(|a| a
                .iter()
                .filter_map(toml::Value::as_integer)
                .collect::<Vec<_>>()),
            Some(vec![physical]),
            "pi4b-board [[pin]]#{i} physical 漂移（GPIO12/GPIO13）"
        );
        assert_eq!(
            p.get("rail").and_then(toml::Value::as_str),
            Some("rail5v"),
            "pi4b-board [[pin]]#{i} rail 漂移（独立 UBEC 5V 轨）"
        );
        assert_eq!(
            p.get("failsafe").and_then(toml::Value::as_str),
            Some("release"),
            "pi4b-board [[pin]]#{i} failsafe 漂移（不保持激磁）"
        );
    }

    // L-10 tier↔tier_scope：rtos 档引用的格其 tier_scope 必须含 rtos。
    {
        let slot = "act.servo";
        let scope = table.rr3_slot_tier_scope(slot).unwrap_or_default();
        assert!(
            scope.iter().any(|t| t == "rtos"),
            "pi4b-board（rtos 档）引用格 {slot} 的 tier_scope = {scope:?} 不含 rtos（L-10）"
        );
    }
}
