//! descriptor 单测（文件头：本文件只放测试，生产面在 `descriptor.rs`）。
//!
//! 拆分动因：`descriptor.rs` 548 行、内联测试自 395 行起，先按仓内同形搬出。

use super::*;

/// rrd 注入器产物形状的全键正例：十四键逐字段落位（值全字符串——注入器已
/// 字符串化；原十三键 = 十 host 键 + 批N4 三键，批N17 件二随键集补 cred_store；
/// 批N9 七键不在本测试覆盖面，键域由 entry/rr-ipc 侧单测钉）。
#[test]
fn full_descriptor_parses_all_fourteen_keys() {
    let text = concat!(
        "alsa_device = \"default\"\n",
        "pwm_chip = \"0\"\n",
        "pwm_channel = \"0\"\n",
        "gpio_chip = \"0\"\n",
        "gpio_led_line = \"4\"\n",
        "gpio_relay_line = \"17\"\n",
        "i2c_bus = \"1\"\n",
        "ina219_addr = \"0x40\"\n",
        "link_ifname = \"wlan0\"\n",
        "spi_device = \"/dev/spidev0.0\"\n",
        "i2c_sda = \"21\"\n",
        "i2c_scl = \"22\"\n",
        "adc_rail_pin = \"34\"\n",
        "cred_store = \"nvs:rr3\"\n",
    );
    let b = rr3_bindings_parse(text).expect("全键正例必须解析");
    assert_eq!(b.alsa_device.as_deref(), Some("default"));
    assert_eq!(b.pwm_chip, Some(0));
    assert_eq!(b.pwm_channel, Some(0));
    assert_eq!(b.gpio_chip, Some(0));
    assert_eq!(b.gpio_led_line, Some(4));
    assert_eq!(b.gpio_relay_line, Some(17));
    assert_eq!(b.i2c_bus, Some(1));
    assert_eq!(b.ina219_addr, Some(0x40));
    assert_eq!(b.link_ifname.as_deref(), Some("wlan0"));
    assert_eq!(b.spi_device.as_deref(), Some("/dev/spidev0.0"));
    assert_eq!(b.i2c_sda, Some(21));
    assert_eq!(b.i2c_scl, Some(22));
    assert_eq!(b.adc_rail_pin, Some(34));
    assert_eq!(b.cred_store.as_deref(), Some("nvs:rr3"));
}

/// 空文本 = 空绑定（合法，装配单未给 [hardware]；纯 sim 形态照常起）。
#[test]
fn empty_text_is_empty_bindings() {
    assert_eq!(
        rr3_bindings_parse("").expect("空文本必须合法"),
        Rr3HalBindings::default()
    );
}

/// fail-closed 点名：未知键、坏值（十进制域/裸十六进制/空串/越 u8/非 nvs:
/// 前缀）、非字符串值，一次全收集不静默放行。
#[test]
fn unknown_keys_and_bad_values_are_all_named() {
    let text = concat!(
        "pwm_chip = \"abc\"\n",
        "i2c_bus = \"-1\"\n",
        "ina219_addr = \"40\"\n", // 裸写有十进制歧义，只认 0x 前缀
        "alsa_device = \"  \"\n", // trim 后空 = 不认
        "mystery_key = \"x\"\n",
        "link_ifname = 3\n",             // 非字符串标量
        "i2c_sda = \"256\"\n",           // 板侧键取值域 = u8，越域即红
        "cred_store = \"flash:boot\"\n", // 非 nvs: 前缀，批N17 件二
    );
    let errs = rr3_bindings_parse(text).expect_err("坏值必须报错");
    assert_eq!(errs.len(), 8, "{errs:?}");
    assert!(
        errs.iter().any(|e| e.contains("pwm_chip 取值不认")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("i2c_bus 取值不认")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("ina219_addr 取值不认")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("alsa_device 取值不认")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("mystery_key 未在册")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("link_ifname 应为字符串标量")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("i2c_sda 取值不认")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("cred_store 取值不认")),
        "{errs:?}"
    );
}

/// compose 分层：描述符为底、overrides 同键覆盖赢、overrides 也能补描述符没给的键。
#[test]
fn compose_overrides_win_over_descriptor() {
    let desc = "pwm_chip = \"0\"\ngpio_led_line = \"4\"\n";
    let mut ov = BTreeMap::new();
    ov.insert("pwm_chip".to_string(), "1".to_string());
    ov.insert("link_ifname".to_string(), "mlan0".to_string());
    let b = rr3_bindings_compose(Some(desc), &ov).expect("合成必须成功");
    assert_eq!(b.pwm_chip, Some(1), "同键应急覆盖赢");
    assert_eq!(b.gpio_led_line, Some(4), "描述符未被覆盖的键原样保留");
    assert_eq!(
        b.link_ifname.as_deref(),
        Some("mlan0"),
        "覆盖可补描述符缺键"
    );
}

/// compose fail-closed：坏描述符合并成一条 Err；overrides 未知键/坏值点名；
/// 无描述符 + 空 overrides = 空绑定（不是某板缺省）。
#[test]
fn compose_fails_closed_and_names_the_cause() {
    let e = rr3_bindings_compose(Some("pwm_chip = \"x\"\n"), &BTreeMap::new())
        .expect_err("坏描述符必须败");
    assert!(
        e.contains("描述符解析失败") && e.contains("pwm_chip"),
        "{e}"
    );

    let mut ov = BTreeMap::new();
    ov.insert("nope".to_string(), "1".to_string());
    let e = rr3_bindings_compose(None, &ov).expect_err("未知覆盖键必须败");
    assert!(
        e.contains("应急覆盖无效") && e.contains("nope 未在册"),
        "{e}"
    );

    let mut ov = BTreeMap::new();
    ov.insert("pwm_chip".to_string(), "1.5".to_string());
    let e = rr3_bindings_compose(None, &ov).expect_err("坏覆盖值必须败");
    assert!(
        e.contains("应急覆盖无效") && e.contains("pwm_chip 取值不认"),
        "{e}"
    );

    let b = rr3_bindings_compose(None, &BTreeMap::new()).expect("空合成必须合法");
    assert_eq!(
        b,
        Rr3HalBindings::default(),
        "无描述符无覆盖 = 空绑定（fail-closed 不回落某板）"
    );
}
