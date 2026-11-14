//! [hardware] 标量串化、注入文本排序与 host 档派生。

use super::super::*;
use crate::profile::rr3_profile_parse;
use std::collections::BTreeMap;

/// [hardware] 段（schema 3 只余非引脚键）：字符串/整数标量收、整数字符串化、
/// 缺段可选；坏类型逐条点名。物理键的"出现即红"在 boot 链裁（门 n），解析层
/// 不带契约真值不越权。
#[test]
fn hardware_section_is_optional_scalars_stringified() {
    // 缺段：合法（纯 sim/无硬件格形态），hal_bindings 为空文本
    let p = rr3_profile_parse(GOOD).expect("缺 [hardware] 段必须合法");
    assert!(p.hardware.is_empty(), "{:?}", p.hardware);

    // 字符串 + 整数混合：整数按十进制字符串化（[hardware] 不设键白名单，
    // 键位语义归 rr-hal 描述符解析，rrd 只管装配形状）
    let text = format!("{GOOD}\n[hardware]\nspi_device = \"/dev/spidev0.0\"\nlink_metric = 100\n");
    let p = rr3_profile_parse(&text).expect("标量 [hardware] 必须合法");
    assert_eq!(
        p.hardware.get("spi_device").map(String::as_str),
        Some("/dev/spidev0.0")
    );
    assert_eq!(
        p.hardware.get("link_metric").map(String::as_str),
        Some("100")
    );

    // 布尔/浮点/数组/表逐条点名，一次全收集
    let text =
        format!("{GOOD}\n[hardware]\nflag = true\nratio = 0.5\nlines = [4]\nsub = {{ k = 1 }}\n");
    let errs = rr3_profile_parse(&text).expect_err("坏类型必须报错");
    assert_eq!(errs.len(), 4, "{errs:?}");
    for key in ["flag", "ratio", "lines", "sub"] {
        assert!(
            errs.iter().any(|e| e.contains(&format!("hardware.{key} "))),
            "{errs:?}"
        );
    }

    // 空键点名
    let text = format!("{GOOD}\n[hardware]\n\"\" = 1\n");
    let errs = rr3_profile_parse(&text).expect_err("空键必须报错");
    assert!(errs.iter().any(|e| e.contains("键不得为空串")), "{errs:?}");
}

/// [hardware] → 注入文本：BTreeMap 升序确定性、整数已是字符串形状、空表空串。
#[test]
fn hardware_bindings_text_is_sorted_and_escaped() {
    assert_eq!(rr3_bindings_text(&BTreeMap::new()), "", "空表 → 空串");
    let mut hw = BTreeMap::new();
    hw.insert("spi_device".to_string(), "0".to_string());
    hw.insert("link_ifname".to_string(), "wl\"x\\".to_string());
    let text = rr3_bindings_text(&hw);
    // BTreeMap 升序：link_ifname < spi_device
    assert_eq!(
        text, "link_ifname = \"wl\\\"x\\\\\"\nspi_device = \"0\"\n",
        "升序 + 转义（双引号/反斜杠）"
    );
}

/// 派生器正例（host 档）：[pins] 派生视图 ∪ [hardware] 非引脚键合成注入文本——
/// 取值与旧 [hardware] 逐字节对齐（pwm<N> 派生 pwm_chip=N、行号取 physical[0]、
/// addr 原样），键名不变，rr-hal descriptor.rs 解析端零改动；四角色全派生 +
/// 多条 pin 共键同值（led/relay 同 gpiochip0）不误报。
#[test]
fn boot_inputs_derives_and_merges_hal_bindings() {
    let root = Root::new("derive");
    let text = format!(
        "{GOOD}\n[hardware]\nalsa_device = \"default\"\nlink_ifname = \"wlan0\"\n\
         [[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpiochip0\"\nphysical = [4]\n\
         direction = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\nmux_group = []\n\
         [[pin]]\nrole = \"act.relay.gpio\"\ncontroller = \"gpiochip0\"\nphysical = [17]\n\
         direction = \"out\"\nrail = \"none\"\nfailsafe = \"driven_low\"\nmux_group = []\n\
         [[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\naddr = \"0x40\"\n\
         direction = \"in\"\nmux_group = []\n"
    );
    let p = root.write_profile("derive.toml", &text);
    let boot = boot_inputs(&p).expect("满配装配单应通过");
    assert_eq!(
        boot.hal_bindings,
        "alsa_device = \"default\"\n\
         gpio_chip = \"0\"\n\
         gpio_led_line = \"4\"\n\
         gpio_relay_line = \"17\"\n\
         i2c_bus = \"1\"\n\
         ina219_addr = \"0x40\"\n\
         link_ifname = \"wlan0\"\n\
         pwm_channel = \"0\"\n\
         pwm_chip = \"0\"\n",
        "派生视图与非引脚键按 BTreeMap 升序合并，取值与旧 [hardware] 一致"
    );
    assert!(boot.role_missing.is_empty(), "{:?}", boot.role_missing);
}

/// SPI 词形派生正例（批N3 件六，敞口 18 行收口）：spidev<N> 词形（总线号）+
/// channel（片选号）合成 /dev/spidev<N>.<M>——与链④真件档旧 [hardware] 直给值
/// 逐字节对齐（profiles/distro.toml 迁移前 = "/dev/spidev0.0"），注入文本形状不变。
#[test]
fn boot_inputs_derives_spi_device_wordform() {
    let root = Root::new("derive-spi");
    let text = format!(
        "{GOOD}\n[[pin]]\nrole = \"ui.face.spi\"\ncontroller = \"spidev0\"\nchannel = 0\n\
         direction = \"out\"\nmux_group = []\n"
    );
    let p = root.write_profile("derive-spi.toml", &text);
    let boot = boot_inputs(&p).expect("SPI 显示 pin 应通过装载");
    assert_eq!(
        boot.hal_bindings,
        "pwm_channel = \"0\"\n\
         pwm_chip = \"0\"\n\
         spi_device = \"/dev/spidev0.0\"\n",
        "spidev0 + channel 0 派生 /dev/spidev0.0，与旧 [hardware] 直给值逐字节对齐"
    );
}
