//! Pi 4B 单板全链档（W5-1）：`profiles/pi4b-solo.toml` 的形状钉。
//!
//! 与 pi4b-host 的差别是补齐全链缺的两格（audio.capture/vad 与 act.servo），
//! 且**不**依赖外部板：无板侧十一键、无桥。形状漂移即红。

use super::*;

/// 档基线：schema 5 / host 全承载 / Pi 4 / distro 快路轨。
fn solo_basics(v: &toml::Value) {
    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "pi4b-solo schema_version 漂移"
    );
    assert_tier(v, "host", "pi4b-solo 档位漂移（Pi 单板 = host 全承载）");
    assert_chip(v, "raspberry-pi-4", "pi4b-solo");
    assert_eq!(
        v.get("os")
            .and_then(|o| o.get("route"))
            .and_then(toml::Value::as_str),
        Some("distro"),
        "pi4b-solo route 漂移（Raspberry Pi OS 属发行版快路轨）"
    );
}

/// 宿主两键（USB 声卡 CARD 名 + 网口）；不得出现板侧十一键（单板无桥）。
fn solo_hardware(v: &toml::Value) {
    let hardware = v
        .get("hardware")
        .and_then(toml::Value::as_table)
        .expect("pi4b-solo 缺 [hardware] 段");
    assert_eq!(
        hardware.get("alsa_device").and_then(toml::Value::as_str),
        Some("plughw:CARD=Device"),
        "pi4b-solo alsa_device 漂移"
    );
    assert_eq!(
        hardware.get("link_ifname").and_then(toml::Value::as_str),
        Some("wlan0"),
        "pi4b-solo link_ifname 漂移"
    );
    for k in [
        "uart2_tx",
        "uart2_rx",
        "ledc_pan",
        "ledc_tilt",
        "cred_store",
    ] {
        assert!(
            hardware.get(k).is_none(),
            "pi4b-solo 不得出现板侧键 {k}（单板无桥）"
        );
    }
}

/// 十格全链绑定，逐格对表。
fn solo_bindings(v: &toml::Value) {
    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("pi4b-solo 缺 [bindings]");
    assert_eq!(bindings.len(), 10, "pi4b-solo 绑定面应恰十格");
    for (slot, module) in [
        ("act.servo", "rr3-act"),
        ("audio.capture", "rr3-sense"),
        ("audio.vad", "rr3-sense"),
        ("audio.playback", "rr3-interact"),
        ("ui.face", "rr3-interact"),
        ("net.wifi", "rr3-connect"),
        ("cognition.dialog", "rr3-brain"),
        ("cognition.asr", "rr3-brain"),
        ("cognition.llm", "rr3-brain"),
        ("cognition.tts", "rr3-brain"),
    ] {
        assert_eq!(
            bindings.get(slot).and_then(toml::Value::as_str),
            Some(module),
            "pi4b-solo bindings.{slot} 漂移"
        );
    }
}

/// 两条 [[pin]]：pan 单轴 + OLED I2C 显示；不得登记 tilt。
fn solo_pins(v: &toml::Value) {
    let pins = v
        .get("pin")
        .and_then(toml::Value::as_array)
        .expect("pi4b-solo 缺 [[pin]]");
    assert_eq!(pins.len(), 2, "pi4b-solo 应恰两条 [[pin]]（pan + 显示）");
    let pan = pins
        .iter()
        .find(|p| p.get("role").and_then(toml::Value::as_str) == Some("act.axis.pwm"))
        .expect("缺 act.axis.pwm 引脚");
    assert_eq!(
        pan.get("instance").and_then(toml::Value::as_str),
        Some("pan"),
        "舵机轴应为 pan"
    );
    assert_eq!(
        pan.get("controller").and_then(toml::Value::as_str),
        Some("pwm0")
    );
    assert!(
        pins.iter()
            .all(|p| p.get("instance").and_then(toml::Value::as_str) != Some("tilt")),
        "host 档单轴，不得登记 tilt"
    );
    let face = pins
        .iter()
        .find(|p| p.get("role").and_then(toml::Value::as_str) == Some("ui.face.i2c"))
        .expect("缺 ui.face.i2c 引脚（W5-3 OLED 换装）");
    assert_eq!(
        face.get("controller").and_then(toml::Value::as_str),
        Some("i2c1")
    );
    assert_eq!(
        face.get("addr").and_then(toml::Value::as_str),
        Some("0x3c"),
        "SSD1306 7-bit 从地址漂移（真机探测档 0x3C）"
    );
    assert_eq!(
        face.get("device").and_then(toml::Value::as_str),
        Some("SSD1306"),
        "显示器件型号词漂移（devices.toml 在册行）"
    );
}

/// enabled_slots / present / bindings 三处一致。
fn solo_consistency(v: &toml::Value) {
    let enabled = v
        .get("enabled_slots")
        .and_then(toml::Value::as_table)
        .expect("pi4b-solo 缺 [enabled_slots]");
    let mut from_enabled: Vec<String> = enabled
        .values()
        .flat_map(|arr| arr.as_array().cloned().unwrap_or_default())
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect();
    from_enabled.sort();
    let mut present: Vec<String> = v
        .get("slots")
        .and_then(|s| s.get("present"))
        .and_then(toml::Value::as_array)
        .expect("pi4b-solo 缺 [slots].present")
        .iter()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect();
    present.sort();
    assert_eq!(from_enabled, present, "enabled_slots 与 present 不一致");
    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("pi4b-solo 缺 [bindings]");
    let mut bound: Vec<String> = bindings.keys().cloned().collect();
    bound.sort();
    assert_eq!(bound, present, "bindings 与 present 不一致");
}

#[test]
fn pi4b_solo_profile_is_complete_single_board_chain() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("profiles/pi4b-solo.toml"))
        .expect("profiles/pi4b-solo.toml 必须存在（W5-1）");
    let v: toml::Value = toml::from_str(&text).expect("pi4b-solo.toml 必须可解析");
    solo_basics(&v);
    solo_hardware(&v);
    solo_bindings(&v);
    solo_pins(&v);
    solo_consistency(&v);
}
