//! Pi 4B 宿主档：classic/S3 桥板差异面与 host-only（N44）形状。

use super::*;

/// Pi 4B 宿主 × classic 板（W2/T1）：`profiles/pi4b-host-esp32-classic.toml` 钉**差异面**
/// （host 档 / chip / 宿主两键 + 板侧十一键同源 / 十一格绑定 / 恰一条 [[pin]] = MAX7219）。
/// 完整形状由 default/distro 测试与 rrd 装载门共同覆盖；线束与档位漂移即红。
#[test]
fn pi4b_host_esp32_classic_profile_pins_t1_harness() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("profiles/pi4b-host-esp32-classic.toml"))
        .expect("profiles/pi4b-host-esp32-classic.toml 必须存在（W2/T1）");
    let v: toml::Value = toml::from_str(&text).expect("pi4b-host-esp32-classic.toml 必须可解析");

    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "pi4b-host-esp32-classic schema_version 漂移"
    );
    assert_tier(
        &v,
        "host",
        "pi4b-host-esp32-classic 档位漂移（T1 = Pi 宿主档）",
    );
    assert_chip(&v, "raspberry-pi-4", "pi4b-host-esp32-classic");
    assert_eq!(
        v.get("os")
            .and_then(|o| o.get("route"))
            .and_then(toml::Value::as_str),
        Some("distro"),
        "pi4b-host-esp32-classic route 漂移（Raspberry Pi OS 属发行版快路轨）"
    );

    let hardware = v
        .get("hardware")
        .and_then(toml::Value::as_table)
        .expect("pi4b-host-esp32-classic 缺 [hardware] 段");
    // 宿主两键：真机实测值（USB 声卡 CARD 名 / Pi 网口）。
    assert_eq!(
        hardware.get("alsa_device").and_then(toml::Value::as_str),
        Some("plughw:CARD=Device"),
        "pi4b-host-esp32-classic alsa_device 漂移（Pi 真机 USB 声卡短名，不钉卡号）"
    );
    assert_eq!(
        hardware.get("link_ifname").and_then(toml::Value::as_str),
        Some("wlan0"),
        "pi4b-host-esp32-classic link_ifname 漂移"
    );
    // 板侧十一键：与 profiles/esp32-mini.toml 逐值对齐（classic 板同一线束）。
    for (k, expected) in [
        ("i2c_sda", "21"),
        ("i2c_scl", "22"),
        ("adc_rail_pin", "34"),
        ("uart2_tx", "18"),
        ("uart2_rx", "19"),
        ("ledc_pan", "32"),
        ("ledc_tilt", "33"),
        ("i2s_bclk", "4"),
        ("i2s_sd", "27"),
        ("i2s_ws", "26"),
        ("cred_store", "nvs:rr3"),
    ] {
        assert_eq!(
            hardware.get(k).and_then(toml::Value::as_str),
            Some(expected),
            "pi4b-host-esp32-classic hardware.{k} 漂移（应与 classic 线束同源）"
        );
    }

    // 形态面：十格绑定 + 十格在位（2026-10-31 实机取证收窄；net.bt 是 placeholder 不进生产装配面）。
    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("pi4b-host-esp32-classic 缺 [bindings]");
    assert_eq!(bindings.len(), 10, "pi4b-host-esp32-classic 绑定面应恰十格");
    for (slot, provider) in [
        ("act.servo", "rr3-act"),
        ("ui.face", "rr3-interact"),
        ("audio.playback", "rr3-interact"),
        ("audio.capture", "rr3-sense"),
        ("cognition.llm", "rr3-brain"),
    ] {
        assert_eq!(
            bindings.get(slot).and_then(toml::Value::as_str),
            Some(provider),
            "pi4b-host-esp32-classic bindings.{slot} 漂移"
        );
    }
    let present = v
        .get("slots")
        .and_then(|s| s.get("present"))
        .and_then(toml::Value::as_array)
        .expect("pi4b-host-esp32-classic 缺 [slots].present");
    assert_eq!(
        present.len(),
        10,
        "pi4b-host-esp32-classic 在位声明应恰十格"
    );

    // [[pin]] 恰一条：Pi 本机唯一的本地外设 = MAX7219 点阵（ui.face.spi，spidev0/ch0，
    // 装载期派生 spi_device）。其余格（act.servo 等）无本地 pin → 校验② role_missing 降级，
    // 板侧真值走 [hardware] 桥注入。
    let pins = v
        .get("pin")
        .and_then(toml::Value::as_array)
        .expect("pi4b-host-esp32-classic 应有恰一条 [[pin]]（ui.face.spi）");
    assert_eq!(
        pins.len(),
        1,
        "pi4b-host-esp32-classic [[pin]] 应恰一条（Pi 本机只接 MAX7219 点阵）"
    );
    let p0 = &pins[0];
    assert_eq!(
        p0.get("role").and_then(toml::Value::as_str),
        Some("ui.face.spi"),
        "pi4b-host-esp32-classic [[pin]] role 漂移"
    );
    assert_eq!(
        p0.get("controller").and_then(toml::Value::as_str),
        Some("spidev0"),
        "pi4b-host-esp32-classic [[pin]] controller 漂移（host 词形前缀 spidev，见平台表 [controller_wordform]）"
    );
    assert_eq!(
        p0.get("channel").and_then(toml::Value::as_integer),
        Some(0),
        "pi4b-host-esp32-classic [[pin]] channel 漂移（CE0）"
    );
}

/// Pi 4B 宿主 × ESP32-S3 N16R8 板（W2/T2）：`profiles/pi4b-host-esp32s3.toml` 钉
/// **差异面**（板侧十一键为 S3 真值；宿主侧与 classic 档同源；十一格绑定/在位；
/// 恰一条 [[pin]] = MAX7219）。与 classic 档的差异只允许发生在 [hardware] 板侧键。
#[test]
fn pi4b_host_esp32s3_profile_pins_t2_harness() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("profiles/pi4b-host-esp32s3.toml"))
        .expect("profiles/pi4b-host-esp32s3.toml 必须存在（W2/T2）");
    let v: toml::Value = toml::from_str(&text).expect("pi4b-host-esp32s3.toml 必须可解析");

    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "pi4b-host-esp32s3 schema_version 漂移"
    );
    assert_tier(&v, "host", "pi4b-host-esp32s3 档位漂移（T2 = Pi 宿主档）");
    assert_eq!(
        v.get("platform")
            .and_then(|p| p.get("chip"))
            .and_then(toml::Value::as_str),
        Some("raspberry-pi-4"),
        "pi4b-host-esp32s3 chip 漂移"
    );
    assert_eq!(
        v.get("os")
            .and_then(|o| o.get("route"))
            .and_then(toml::Value::as_str),
        Some("distro"),
        "pi4b-host-esp32s3 route 漂移"
    );

    let hardware = v
        .get("hardware")
        .and_then(toml::Value::as_table)
        .expect("pi4b-host-esp32s3 缺 [hardware] 段");
    assert_eq!(
        hardware.get("alsa_device").and_then(toml::Value::as_str),
        Some("plughw:CARD=Device"),
        "pi4b-host-esp32s3 alsa_device 漂移（Pi 真机 USB 声卡短名）"
    );
    assert_eq!(
        hardware.get("link_ifname").and_then(toml::Value::as_str),
        Some("wlan0"),
        "pi4b-host-esp32s3 link_ifname 漂移"
    );
    // 板侧十一键：S3 N16R8 真值（与 profiles/esp32s3-mini.toml 逐值对齐）。
    s3_hardware_keys(hardware, "pi4b-host-esp32s3", "应与 S3 线束同源");

    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("pi4b-host-esp32s3 缺 [bindings]");
    assert_eq!(
        bindings.len(),
        10,
        "pi4b-host-esp32s3 绑定面应恰十格（net.bt placeholder 不进生产装配面）"
    );
    let present = v
        .get("slots")
        .and_then(|s| s.get("present"))
        .and_then(toml::Value::as_array)
        .expect("pi4b-host-esp32s3 缺 [slots].present");
    assert_eq!(present.len(), 10, "pi4b-host-esp32s3 在位声明应恰十格");

    let pins = v
        .get("pin")
        .and_then(toml::Value::as_array)
        .expect("pi4b-host-esp32s3 应有恰一条 [[pin]]（ui.face.spi）");
    assert_eq!(pins.len(), 1, "pi4b-host-esp32s3 [[pin]] 应恰一条");
    assert_eq!(
        pins[0].get("role").and_then(toml::Value::as_str),
        Some("ui.face.spi"),
        "pi4b-host-esp32s3 [[pin]] role 漂移"
    );
}

/// Pi 4B 独立 host-only（N44）：`profiles/pi4b-host.toml` 钉**差异面**——host 档、
/// 无外部板（[hardware] 零板侧键）、本地末梢只有 MAX7219（[[pin]] ui.face.spi）
/// 与 USB 声卡回放；act/sense 无末梢，bindings/enabled/present 三源一致为七格
/// 三模块（connect/interact/brain）。net.bt 是 placeholder 不进生产装配面。
/// 完整装载链由 rrd 的 boot drill 覆盖。
#[test]
fn pi4b_host_profile_shape_n44_harness() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("profiles/pi4b-host.toml"))
        .expect("profiles/pi4b-host.toml 必须存在（N44）");
    let v: toml::Value = toml::from_str(&text).expect("pi4b-host.toml 必须可解析");

    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "pi4b-host schema_version 漂移"
    );
    assert_tier(&v, "host", "pi4b-host 档位漂移（N44 = Pi 本机 host-only）");
    assert_eq!(
        v.get("platform")
            .and_then(|p| p.get("chip"))
            .and_then(toml::Value::as_str),
        Some("raspberry-pi-4"),
        "pi4b-host chip 漂移"
    );
    assert_eq!(
        v.get("os")
            .and_then(|o| o.get("route"))
            .and_then(toml::Value::as_str),
        Some("distro"),
        "pi4b-host route 漂移"
    );

    let hardware = v
        .get("hardware")
        .and_then(toml::Value::as_table)
        .expect("pi4b-host 缺 [hardware] 段");
    assert_eq!(
        hardware.get("alsa_device").and_then(toml::Value::as_str),
        Some("plughw:CARD=Device"),
        "pi4b-host alsa_device 漂移（Pi 真机 USB 声卡短名）"
    );
    assert_eq!(
        hardware.get("link_ifname").and_then(toml::Value::as_str),
        Some("wlan0"),
        "pi4b-host link_ifname 漂移"
    );
    for k in [
        "i2c_sda",
        "i2c_scl",
        "adc_rail_pin",
        "uart2_tx",
        "uart2_rx",
        "ledc_pan",
        "ledc_tilt",
        "i2s_bclk",
        "i2s_sd",
        "i2s_ws",
        "cred_store",
    ] {
        assert!(
            hardware.get(k).is_none(),
            "pi4b-host 无外部板，[hardware] 不得出现板侧键 {k}"
        );
    }

    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("pi4b-host 缺 [bindings]");
    assert_eq!(
        bindings.len(),
        7,
        "pi4b-host 绑定面应恰七格（net.bt placeholder 不进生产装配面）"
    );
    for slot in ["act.servo", "audio.capture", "audio.vad"] {
        assert!(
            bindings.get(slot).is_none(),
            "pi4b-host 本机无末梢，不得绑定 {slot}"
        );
    }

    let enabled = v
        .get("enabled_slots")
        .and_then(toml::Value::as_table)
        .expect("pi4b-host 缺 [enabled_slots]");
    let modules: Vec<&str> = enabled.keys().map(String::as_str).collect();
    assert_eq!(
        modules,
        vec!["rr3-brain", "rr3-connect", "rr3-interact"],
        "pi4b-host 启用模块应恰 connect/interact/brain"
    );

    let present = v
        .get("slots")
        .and_then(|s| s.get("present"))
        .and_then(toml::Value::as_array)
        .expect("pi4b-host 缺 [slots].present");
    assert_eq!(present.len(), 7, "pi4b-host 在位声明应恰七格");

    let pins = v
        .get("pin")
        .and_then(toml::Value::as_array)
        .expect("pi4b-host 应有恰一条 [[pin]]（ui.face.spi）");
    assert_eq!(pins.len(), 1, "pi4b-host [[pin]] 应恰一条");
    assert_eq!(
        pins[0].get("role").and_then(toml::Value::as_str),
        Some("ui.face.spi"),
        "pi4b-host [[pin]] role 漂移"
    );
    assert_eq!(
        pins[0].get("controller").and_then(toml::Value::as_str),
        Some("spidev0"),
        "pi4b-host [[pin]] controller 漂移（MAX7219 CE0）"
    );
    assert_eq!(
        pins[0].get("channel").and_then(toml::Value::as_integer),
        Some(0),
        "pi4b-host [[pin]] channel 漂移（CE0）"
    );
}
