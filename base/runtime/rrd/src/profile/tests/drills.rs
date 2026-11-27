// SPDX-License-Identifier: Apache-2.0
//! 真仓装配单钻取：default/distro/esp32-mini/esp32s3-mini/pi4b 各档过完整装载门，
//! 并钉 default↔distro 接线镜像等价（只允许注释差异）。
//!
//! 判据是"仓内真值两两闭合"：装配单 + 接线档 + devices.toml 三份真值同时命中；
//! 镜像/装配单漂移即红。真板侧运行不在本文件（触发条件见各用例注记）。
//!
//! 拆分动因：`surgery_gates.rs` 555 行，结构门与真档钻取分属两簇。

use std::path::Path;

use super::*;

/// 真仓 default 机 drill：profiles/default.toml + hardware/default/wiring.toml
/// 过完整装载门——⑦-a 的四个 pin_id（act.axis.pwm.pan/act.led.gpio/
/// act.relay.gpio/power.battery.meter）、⑦-b 的三域名（batt/rail5v/rail3v3）、
/// ⑦-d 的 SG90-3P 全部命中仓内真值（跨档对账的三份真值两两闭合）。
#[test]
fn def_wiring_drill() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let p = repo.join("profiles/default.toml");
    let wiring = crate::wiring::rr3_wiring_path(&p).expect("真仓装配单可推导");
    assert!(
        wiring.exists(),
        "真仓接线档必须随批N1 落位: {}",
        wiring.display()
    );
    let boot = boot_inputs(&p).expect("default 机装配单+接线档必须过完整装载门（host 档）");
    assert_eq!(boot.manifests.len(), 6, "enabled_slots 六模块（升序）");
}

/// 真仓 distro 机 drill：profiles/distro.toml + hardware/distro/wiring.toml
/// （与 default 同板同接线的镜像）过完整装载门——镜像双轨各自对各自的装配单
/// 闭合（distro.toml 的 pin/域集真值 + 同一 devices.toml mate 集），镜像漂移
/// 即此测试红；装机 deb 面由此件随 rr3-pack 入包（Z-4）。
#[test]
fn distro_wiring_drill() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let p = repo.join("profiles/distro.toml");
    let boot = boot_inputs(&p).expect("distro 机装配单+接线档必须过完整装载门（host 档）");
    assert_eq!(boot.manifests.len(), 6, "enabled_slots 六模块（升序）");
}

/// F-B 桥接微型形态装配演练（0.0.5 批K H4/R3-D6）：对仓内真件
/// `profiles/esp32-mini.toml` + 真 contracts/slots.toml 走完整 boot 输入装配——
/// 查册/形状门、G6 档位一致性、L-10 tier↔tier_scope 全过；三格自洽
/// （missing 空 = present/bindings/enabled_slots 三源一致）、装配段扫描零缺口、
/// boot 队列恰为两模块、[hardware] 板侧配置三键直给进注入文本（批N4 件七(b)）。
/// 真板侧运行（限位/时限/饱和
/// 通告、桥断仍守护）触发条件 = 0.0.7 轻承载档真板，本测试只证装配链层
/// "裁剪 = 改数据"（CI 全集层形状 pin 在 rr-registry contract.rs）。
#[test]
fn esp32_boot_drill() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let p = repo.join("profiles/esp32-mini.toml");
    let boot = boot_inputs(&p).expect("esp32-mini 装配单必须过完整装载门（rtos 档）");
    let mods: Vec<String> = boot
        .manifests
        .iter()
        .map(|m| {
            m.parent()
                .and_then(Path::file_name)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        mods,
        vec!["rr3-act", "rr3-sense"],
        "boot 队列恰为 F-B 两模块（enabled_slots 升序，BTreeMap 确定性）"
    );
    assert!(
        boot.missing.is_empty(),
        "三格自洽（present/bindings/enabled_slots 三源一致）: {:?}",
        boot.missing
    );
    assert!(
        boot.assemble_broken.is_empty(),
        "真件装配段扫描零缺口: {:?}",
        boot.assemble_broken
    );
    assert!(
        boot.role_missing.is_empty(),
        "三格全有角色绑定（[[pin]] 真值点在位）: {:?}",
        boot.role_missing
    );
    // [hardware] 板侧配置三键（批N4 件七(b) 基-24 桥上配置腿）：rtos 档原样
    // 直给进注入文本（BTreeMap 升序确定性——三键逐名在文，值全字符串形状）
    assert!(
        boot.hal_bindings.contains("adc_rail_pin = \"34\"")
            && boot.hal_bindings.contains("i2c_scl = \"22\"")
            && boot.hal_bindings.contains("i2c_sda = \"21\""),
        "esp32-mini [hardware] 板侧配置三键必须进 RR3_HAL_BINDINGS 注入文本: {:?}",
        boot.hal_bindings
    );
}

/// Pi 4B 板端点装配演练（W4/P0-C，批N43）：`profiles/pi4b-board.toml` + 真契约走
/// 完整 boot 输入装配——校验①②⑨ 全过（N43-1 判据）；板端点恰 rr3-act 一模块、
/// act.servo 一格；[[pin]] pan/tilt 双轴成对（role_missing 空 = 校验②无缺角色）；
/// [hardware] 板侧四键原样进 RR3_HAL_BINDINGS 注入文本（rtos 档直给）。
#[test]
fn pi4b_boot_drill() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let p = repo.join("profiles/pi4b-board.toml");
    let boot = boot_inputs(&p).expect("pi4b-board 装配单必须过完整装载门（rtos 薄板档）");
    let mods: Vec<String> = boot
        .manifests
        .iter()
        .map(|m| {
            m.parent()
                .and_then(Path::file_name)
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(
        mods,
        vec!["rr3-act"],
        "板端点 boot 队列应恰为 rr3-act（enabled_slots 升序，BTreeMap 确定性）"
    );
    assert!(
        boot.missing.is_empty(),
        "present/bindings/enabled_slots 三源一致（act.servo 一格）: {:?}",
        boot.missing
    );
    assert!(
        boot.assemble_broken.is_empty(),
        "rr3-act 真件装配段扫描零缺口: {:?}",
        boot.assemble_broken
    );
    assert!(
        boot.role_missing.is_empty(),
        "双轴 [[pin]] 成对（校验②无缺角色）: {:?}",
        boot.role_missing
    );
    assert!(
        boot.hal_bindings.contains("i2c_sda = \"2\"")
            && boot.hal_bindings.contains("i2c_scl = \"3\"")
            && boot.hal_bindings.contains("uart2_tx = \"14\"")
            && boot.hal_bindings.contains("uart2_rx = \"15\""),
        "pi4b-board [hardware] 板侧四键必须进 RR3_HAL_BINDINGS 注入文本: {:?}",
        boot.hal_bindings
    );
}

/// Pi 4B 独立 host-only 装配演练（N44）：`profiles/pi4b-host.toml` + 真契约走
/// 完整 boot 输入装配——host 档三模块（connect/interact/brain，升序）、八格在位、
/// [[pin]] 仅 MAX7219；[hardware] 两键 + SPI 派生键进注入文本；本机无 act/sense
/// 末梢，故不出现板侧十一键。
#[test]
fn pi4b_host_boot_drill() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let p = repo.join("profiles/pi4b-host.toml");
    let boot = boot_inputs(&p).expect("pi4b-host 装配单必须过完整装载门（host 档）");
    let mods: Vec<String> = boot
        .manifests
        .iter()
        .map(|m| {
            m.parent()
                .and_then(Path::file_name)
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(
        mods,
        vec!["rr3-brain", "rr3-connect", "rr3-interact"],
        "host-only boot 队列应恰 connect/interact/brain（enabled_slots 升序）"
    );
    assert!(
        boot.missing.is_empty(),
        "present/bindings/enabled_slots 三源一致（八格）: {:?}",
        boot.missing
    );
    assert!(
        boot.assemble_broken.is_empty(),
        "三模块真件装配段扫描零缺口: {:?}",
        boot.assemble_broken
    );
    assert!(
        boot.role_missing.is_empty(),
        "ui.face.spi 的 [[pin]] 真值点在位（校验②无缺角色）: {:?}",
        boot.role_missing
    );
    assert!(
        boot.hal_bindings
            .contains("alsa_device = \"plughw:CARD=Device\"")
            && boot.hal_bindings.contains("link_ifname = \"wlan0\"")
            && boot
                .hal_bindings
                .contains("spi_device = \"/dev/spidev0.0\""),
        "pi4b-host [hardware] 两键与 SPI 派生键必须进注入文本: {:?}",
        boot.hal_bindings
    );
}

fn mod_names(boot: &Rr3ProfileBoot) -> Vec<String> {
    boot.manifests
        .iter()
        .map(|m| {
            m.parent()
                .and_then(Path::file_name)
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

fn solo_boot() -> Rr3ProfileBoot {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let p = repo.join("profiles/pi4b-solo.toml");
    boot_inputs(&p).expect("pi4b-solo 装配单必须过完整装载门（host 档）")
}

fn solo_queue_is_five(boot: &Rr3ProfileBoot) {
    assert_eq!(
        mod_names(boot),
        vec![
            "rr3-act",
            "rr3-brain",
            "rr3-connect",
            "rr3-interact",
            "rr3-sense"
        ],
        "solo boot 队列应恰五模块（enabled_slots 升序）"
    );
}

fn solo_sources_agree(boot: &Rr3ProfileBoot) {
    assert!(
        boot.missing.is_empty(),
        "present/bindings/enabled_slots 三源一致（十格）: {:?}",
        boot.missing
    );
}

fn solo_assembly_closed(boot: &Rr3ProfileBoot) {
    assert!(
        boot.assemble_broken.is_empty(),
        "五模块真件装配段扫描零缺口: {:?}",
        boot.assemble_broken
    );
}

fn solo_roles_present(boot: &Rr3ProfileBoot) {
    assert!(
        boot.role_missing.is_empty(),
        "pan 与 ui.face.i2c 的 [[pin]] 真值点在位（校验②无缺角色）: {:?}",
        boot.role_missing
    );
}

fn solo_hal_injected(boot: &Rr3ProfileBoot) {
    assert!(
        boot.hal_bindings
            .contains("alsa_device = \"plughw:CARD=Device\"")
            && boot.hal_bindings.contains("link_ifname = \"wlan0\"")
            && boot.hal_bindings.contains("pwm_chip = \"0\"")
            && boot.hal_bindings.contains("pwm_channel = \"0\"")
            && boot.hal_bindings.contains("i2c_bus = \"1\"")
            && boot.hal_bindings.contains("oled_addr = \"0x3c\""),
        "pi4b-solo [hardware] 两键与派生四键必须进注入文本: {:?}",
        boot.hal_bindings
    );
}

/// Pi 4B 单板全链装配演练（W5-1）：`profiles/pi4b-solo.toml` + 真契约走完整
/// boot 输入装配——host 档五模块（act/brain/connect/interact/sense，升序）、
/// 十格在位、[[pin]] 两条（pan PWM + OLED I2C）；[hardware] 两键 + 派生键
/// （pwm_chip/pwm_channel/i2c_bus/oled_addr）进注入文本。
#[test]
fn pi4b_solo_boot_drill() {
    let boot = solo_boot();
    solo_queue_is_five(&boot);
    solo_sources_agree(&boot);
    solo_assembly_closed(&boot);
    solo_roles_present(&boot);
    solo_hal_injected(&boot);
}

/// ESP32-S3 微型形态装配演练（W1/T2 补档，2026-11-27）：`profiles/esp32s3-mini.toml`
/// + `hardware/esp32s3-mini/wiring.toml` 走完整装载门——此前该档有装配单无接线档，
/// 装载必缺件；本用例钉"接线档在位即可装载"、板侧十一键进注入文本、双轴档案按
/// 缺省轴投影成 `RR3_ACT_AXIS_STATE`。
#[test]
fn esp32s3_boot_drill() {
    let boot = s3_boot();
    s3_sources_agree(&boot);
    s3_hal_injected(&boot);
    assert!(
        !boot.axis_state.is_empty(),
        "双轴档案合法装载后按缺省轴 pan 投影出注入文本；notes={:?}",
        boot.axis_notes
    );
}

fn s3_boot() -> Rr3ProfileBoot {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let p = repo.join("profiles/esp32s3-mini.toml");
    boot_inputs(&p).expect("esp32s3-mini 装配单+接线档必须过完整装载门（rtos 档）")
}

/// 三源一致（present/bindings/enabled_slots）且 boot 队列恰两模块。
fn s3_sources_agree(boot: &Rr3ProfileBoot) {
    let mods: Vec<String> = boot
        .manifests
        .iter()
        .map(|m| {
            m.parent()
                .and_then(Path::file_name)
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(mods, vec!["rr3-act", "rr3-sense"], "S3 boot 队列");
    assert!(boot.missing.is_empty(), "三源一致: {:?}", boot.missing);
    assert!(
        boot.assemble_broken.is_empty(),
        "装配段零缺口: {:?}",
        boot.assemble_broken
    );
    assert!(
        boot.role_missing.is_empty(),
        "双轴角色绑定在位: {:?}",
        boot.role_missing
    );
}

/// `[hardware]` 板侧十一键原样进注入文本（rtos 档直给，经桥 `rr3.set_bindings`）。
fn s3_hal_injected(boot: &Rr3ProfileBoot) {
    for kv in [
        "i2c_sda = \"8\"",
        "i2c_scl = \"9\"",
        "adc_rail_pin = \"1\"",
        "uart2_tx = \"41\"",
        "uart2_rx = \"42\"",
        "ledc_pan = \"4\"",
        "ledc_tilt = \"5\"",
        "i2s_bclk = \"6\"",
        "i2s_sd = \"15\"",
        "i2s_ws = \"7\"",
        "cred_store = \"nvs:rr3\"",
    ] {
        assert!(
            boot.hal_bindings.contains(kv),
            "S3 [hardware] 十一键缺 {kv}: {}",
            boot.hal_bindings
        );
    }
}

/// default↔distro 接线镜像等价（批45 盲区②；2026-11-27 批121）：两档是同一台
/// RK3566 参考机 + 同接线的镜像，差异只允许在注释——解析值不等即红，把"人工
/// 纪律"变成机械对账（改一份忘另一份的漂移当场可见）。
#[test]
fn wiring_mirror_drill() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let a = wiring_value(&repo.join("hardware/default/wiring.toml"));
    let b = wiring_value(&repo.join("hardware/distro/wiring.toml"));
    assert_eq!(
        a, b,
        "default 与 distro 接线解析值必须等价（只允许注释差异）"
    );
}

fn wiring_value(p: &Path) -> toml::Value {
    let text =
        std::fs::read_to_string(p).unwrap_or_else(|e| panic!("读 {} 失败: {e}", p.display()));
    text.parse()
        .unwrap_or_else(|e| panic!("{} 不是合法 TOML: {e}", p.display()))
}
