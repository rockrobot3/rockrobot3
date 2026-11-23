// SPDX-License-Identifier: Apache-2.0
//! 装配单结构门与真档钻取单测（批B13 自 `surgery.rs` 按被试族切出）。
//!
//! 覆盖 `rr3_boot_inputs` 的形状判据（[hardware] 物理键、SPI 稳态、role、新段、
//! tier、wiring 交叉）与四份真仓装配单的装载钻取（default/distro/esp32-mini/pi4b）。

use super::*;

/// 门 n 稳态判据（§8，power P-13）：[hardware] 出现物理键即红——GOOD 无电表
/// pin，i2c_bus 无派生者，按稳态文案点名
#[test]
fn gate_n_key_fatal() {
    let root = Root::new("gate-n-static");
    let text = format!("{GOOD}\n[hardware]\ni2c_bus = 1\n");
    let p = root.write_profile("gate-n-static.toml", &text);
    let e = boot_inputs(&p).expect_err("[hardware] 物理键必须致命");
    assert!(
        e.contains("hardware.i2c_bus 是引脚侧派生键") && e.contains("门 n 稳态判据"),
        "{e}"
    );
}

/// 门 n 两处并存判据：pin 实际派生该键时点名两份取值（[hardware] 值 vs 派生值）
#[test]
fn gate_n_names_values() {
    let root = Root::new("gate-n-both");
    let text = format!("{GOOD}\n[hardware]\npwm_chip = 5\n");
    let p = root.write_profile("gate-n-both.toml", &text);
    let e = boot_inputs(&p).expect_err("两处并存必须致命");
    assert!(
        e.contains("hardware.pwm_chip = \"5\" 与 [pins] 派生值 \"0\" 两处并存"),
        "{e}"
    );
}

/// 门 n 稳态判据（批N3 件六，敞口 18 行收口）：`spi_device` 进物理键集后，
/// [hardware] 再出现即红——GOOD 无显示 pin，按稳态文案点名（SPI 器件走 schema 2
/// 旧路直给的迁移残留，正是敞口 18 行登记的病形）
#[test]
fn gate_n_spi_state_red() {
    let root = Root::new("gate-n-spi");
    let text = format!("{GOOD}\n[hardware]\nspi_device = \"/dev/spidev0.0\"\n");
    let p = root.write_profile("gate-n-spi.toml", &text);
    let e = boot_inputs(&p).expect_err("[hardware] spi_device 必须致命");
    assert!(
        e.contains("hardware.spi_device 是引脚侧派生键") && e.contains("门 n 稳态判据"),
        "{e}"
    );
}

/// 门 n 两处并存判据（spi_device 形）：显示 pin 已派生该键时 [hardware] 残留直给
/// → 点名两份取值（两值恰同也是红——并存本身就是第二真值点，L-23）
#[test]
fn gate_n_spi_values() {
    let root = Root::new("gate-n-spi-both");
    let text = format!(
        "{GOOD}\n[[pin]]\nrole = \"ui.face.spi\"\ncontroller = \"spidev0\"\nchannel = 0\n\
             direction = \"out\"\nmux_group = []\n\
             [hardware]\nspi_device = \"/dev/spidev0.0\"\n"
    );
    let p = root.write_profile("gate-n-spi-both.toml", &text);
    let e = boot_inputs(&p).expect_err("两处并存必须致命");
    assert!(
        e.contains(
            "hardware.spi_device = \"/dev/spidev0.0\" 与 [pins] 派生值 \"/dev/spidev0.0\" 两处并存"
        ),
        "{e}"
    );
}

/// 校验②（后果 = 降不拒）：格位在位且被启用但角色无 [[pin]] 绑定 → (格位, 角色)
/// 进 role_missing 清单随 boot 带出，模块照常起——缺角色是缺能力，不是拒 boot。
/// GOOD 补了 power.charger.meter pin（批N3 件八：power.charger 有归属角色了，满配须有
/// pin——本测试删 servo pin 后清单纯 act.servo 一条；power.charger pin 在场不误报）
#[test]
fn role_miss_not_fatal() {
    let root = Root::new("role-missing");
    let text = GOOD.replace(
            "\n[[pin]]\nrole = \"act.axis.pwm\"\ninstance = \"pan\"\ncontroller = \"pwm0\"\nchannel = 0\nphysical = [12]\ndirection = \"out\"\nrail = \"rail5v\"\nfailsafe = \"release\"\nmux_group = []\n",
            "",
        );
    let p = root.write_profile("role-missing.toml", &text);
    let boot = boot_inputs(&p).expect("缺角色不是致命项");
    assert_eq!(
        boot.role_missing,
        vec![("act.servo".to_string(), "act.axis.pwm".to_string()),],
        "present ∩ enabled_slots 的格位缺角色逐条点名（power.charger pin 在场不误报）"
    );
    assert!(boot.missing.is_empty(), "{:?}", boot.missing);
}

/// 格位契约形状门进运行期装载（0.0.5 批J 深挖收口）：status/tier_scope 越界的
/// slots.toml 在 boot 即拒并点名——不能只靠 CI 契约测试兜底（装机现场改坏的
/// 契约同样不得带病起）。
#[test]
fn boot_rej_contract() {
    let root = Root::new("slot-shape");
    let bad_slots = root.dir.join("slots-bad.toml");
    std::fs::write(
        &bad_slots,
        r#"
[[slot]]
name = "act.servo"
layer = "body"
status = "deprecated"
tier_scope = ["quantum"]
"#,
    )
    .unwrap();
    let p = root.write_profile("ok.toml", GOOD);
    // slots 册由本用例自备（被测对象就是它），其余各册照常走真值——接线档由
    // 推导函数定位（write_profile 已同批 stage）
    let wiring = crate::wiring::rr3_wiring_path(&p).expect("夹具装配单必须能推导接线档");
    let errs = rr3_boot_inputs(
        &p,
        &bad_slots,
        Path::new(RR3_CONTRACT_ROLES_PATH),
        Path::new(RR3_CONTRACT_DEVICES_PATH),
        &wiring,
        Path::new(rr_registry::RR3_CONTRACT_PLATFORM_PATH),
    )
    .expect_err("越界契约必须拒");
    assert!(errs.contains("status 非法"), "{errs}");
    assert!(errs.contains("tier_scope 含非法档 quantum"), "{errs}");
}

/// schema_version=2 三新段必填：缺段逐条点名（fail-closed，一次收集完）
#[test]
fn new_required_named() {
    let errs = rr3_profile_parse("schema_version = 2\n").expect_err("缺新段必须报错");
    for want in [
        "缺少 [platform]",
        "缺少 [supplies]",
        "缺少 [slots]",
        "缺少 [bindings]",
        "缺少 [enabled_slots]",
    ] {
        assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
    }
}

/// [platform]/[supplies] 形状：tier 值域、类型逐键检查，坏值一次全收集
#[test]
fn platform_named() {
    let text = GOOD
        .replace("tier = \"host\"", "tier = \"nano\"")
        .replace("watchdog_present = true", "watchdog_present = 1")
        .replace(
            "watchdog_timeout_ms = 15000",
            "watchdog_timeout_ms = \"15s\"",
        )
        .replace("watchdog_feeder = \"rrd\"", "watchdog_feeder = \"\"");
    let errs = rr3_profile_parse(&text).expect_err("形状不合必须报错");
    assert_eq!(errs.len(), 4, "{errs:?}");
    for want in [
        "platform.tier",
        "supplies.watchdog_present",
        "supplies.watchdog_timeout_ms",
        "supplies.watchdog_feeder",
    ] {
        assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
    }
}

/// 三新段未知键逐条点名（一条不静默放行）
#[test]
fn new_keys_named() {
    let text = GOOD
        .replace("tier = \"host\"", "tier = \"host\"\nfoo = 1")
        .replace(
            "watchdog_present = true",
            "watchdog_present = true\nbar = 1",
        )
        .replace("[slots]\n", "[slots]\nbaz = []\n");
    let errs = rr3_profile_parse(&text).expect_err("未知键必须报错");
    assert_eq!(errs.len(), 3, "{errs:?}");
    for want in [
        "[platform] 未知键 foo",
        "[supplies] 未知键 bar",
        "[slots] 未知键 baz",
    ] {
        assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
    }
}

/// G6 档位一致性（ADR-0018 修订 1）：tier=bare 且无看门狗 → 致命点名；
/// bare + 看门狗在场 → 一致，放行。tier 换档同批翻转 [supplies.budget] 为
/// bare 档形状（分档自洽：not_applicable = false + 三数键，parse 层判据）。
#[test]
fn bare_tier_fatal() {
    let root = Root::new("g6");
    let bad = GOOD
            .replace("tier = \"host\"", "tier = \"bare\"")
            .replace("watchdog_present = true", "watchdog_present = false")
            .replace(
                "[supplies.budget]\nnot_applicable = true",
                "[supplies.budget]\nnot_applicable = false\ntask_stack_budget_bytes = 65536\nheap_total_bytes = 327680\nheap_reserved_bytes = 16384",
            );
    let p = root.write_profile("bare-nowd.toml", &bad);
    let e = boot_inputs(&p).expect_err("无看门狗的 bare 档必须被拒");
    assert!(e.contains("无看门狗的板不允许归 bare 档"), "{e}");

    let ok = GOOD.replace("tier = \"host\"", "tier = \"bare\"").replace(
            "[supplies.budget]\nnot_applicable = true",
            "[supplies.budget]\nnot_applicable = false\ntask_stack_budget_bytes = 65536\nheap_total_bytes = 327680\nheap_reserved_bytes = 16384",
        );
    let p = root.write_profile("bare-wd.toml", &ok);
    boot_inputs(&p).expect("bare 档 + 看门狗在场应放行");
}

/// L-10 tier↔tier_scope 交叉校验（0.0.5 批K，ADR-0012 增补注记分族的装配侧
/// 执法）：bare 档引用 bus.publish（tier_scope = host/rtos）→ 拒 boot 点名；
/// 引用 scope 全档的 power.battery → 放行。enabled_slots 是唯一允许基础层
/// 格位的引用面，反例经它构造。
#[test]
fn tier_slot_tier_fatal() {
    let root = Root::new("l10");
    // tier 换 bare 同批翻转 [supplies.budget] 为 bare 档形状（分档自洽，
    // bare_tier_fatal 注记同源）
    let bad = GOOD
            .replace("tier = \"host\"", "tier = \"bare\"")
            .replace(
                "[supplies.budget]\nnot_applicable = true",
                "[supplies.budget]\nnot_applicable = false\ntask_stack_budget_bytes = 65536\nheap_total_bytes = 327680\nheap_reserved_bytes = 16384",
            )
            .replace(
                "rr3-pa = [\"act.servo\"]",
                "rr3-pa = [\"act.servo\"]\nrr3-pd = [\"bus.publish\"]",
            );
    let p = root.write_profile("bare-bus.toml", &bad);
    let e = boot_inputs(&p).expect_err("bare 档引用 bus 格必须被拒");
    assert!(
        e.contains("tier↔tier_scope 交叉校验失败") && e.contains("bus.publish"),
        "{e}"
    );

    let ok = GOOD
            .replace("tier = \"host\"", "tier = \"bare\"")
            .replace(
                "[supplies.budget]\nnot_applicable = true",
                "[supplies.budget]\nnot_applicable = false\ntask_stack_budget_bytes = 65536\nheap_total_bytes = 327680\nheap_reserved_bytes = 16384",
            )
            .replace(
                "rr3-pa = [\"act.servo\"]",
                "rr3-pa = [\"act.servo\"]\nrr3-pd = [\"power.battery\"]",
            );
    let p = root.write_profile("bare-battery.toml", &ok);
    boot_inputs(&p).expect("bare 档引用 scope 全档的 power.battery 应放行");
}

// ⑦-a：自造语法 pwm:12 既非 pin_id 也非 vcc/gnd（F-2 同款病，接线侧再现）
fn wiring_signal_gate() {
    let root = Root::new("wiring-7a");
    let p = root.write_profile("wiring-7a.toml", GOOD);
    root.write_wiring(
        &p,
        &GOOD_WIRING.replace(
            "signal = [\"vcc\", \"gnd\"]",
            "signal = [\"vcc\", \"pwm:12\"]",
        ),
    );
    let e = boot_inputs(&p).expect_err("signal 自造语法必须致命");
    assert!(e.contains("校验⑦-a") && e.contains("pwm:12"), "{e}");
}

// ⑦-b：自造域名（F-5：文档示例值会被装机档抄）
fn wiring_net_gate() {
    let root = Root::new("wiring-7b");
    let p = root.write_profile("wiring-7b.toml", GOOD);
    root.write_wiring(
        &p,
        &GOOD_WIRING.replace("net = \"rail5v\"", "net = \"rail24\""),
    );
    let e = boot_inputs(&p).expect_err("自造域名必须致命");
    assert!(e.contains("校验⑦-b") && e.contains("rail24"), "{e}");
}

// ⑦-d：对端型号不在 devices.toml mate 集（词形错误——在册的是 SG90-3P）
fn wiring_mate_gate() {
    let root = Root::new("wiring-7d");
    let p = root.write_profile("wiring-7d.toml", GOOD);
    root.write_wiring(
        &p,
        &GOOD_WIRING.replace("mate = \"none\"", "mate = \"SG90-9G\""),
    );
    let e = boot_inputs(&p).expect_err("mate 不在册必须致命");
    assert!(e.contains("校验⑦-d") && e.contains("SG90-9G"), "{e}");
}

// 命中集全绿：signal 写 [pins] 真值 pin_id、net/mate 写在册值——⑦ 三判据同过
fn wiring_all_hit_ok() {
    let root = Root::new("wiring-7-ok");
    let p = root.write_profile("wiring-7-ok.toml", GOOD);
    root.write_wiring(
        &p,
        r#"
[[wire]]
connector = "J-S"
hdr_pin = [1, 2, 3]
signal = ["vcc", "act.axis.pwm.pan", "gnd"]
net = "rail5v"
feeds = []
mate = "SG90-3P"
keyed = true
rated_a = 1.0
gnd_common = "star-1"
"#,
    );
    boot_inputs(&p).expect("signal/net/mate 全命中在册集必须放行");
}

/// 校验⑦ 跨档反例（后果 = 拒；boot 链把 [pins]/domains/devices 三份真值接起来
/// 判，wiring.rs 单测已钉判定逻辑，这里钉的是跨档接线）：signal 越出
/// pin_id ∪ {vcc,gnd}（F-3 自造语法）、net 越出 domains（F-5）、mate 越出
/// devices mate 集（§4.1b 门 o）逐条点名；命中集全绿由末段正例钉住。
#[test]
fn wiring_file_fatal() {
    wiring_signal_gate();
    wiring_net_gate();
    wiring_mate_gate();
    wiring_all_hit_ok();
}

/// 接线档装载门（fail-closed，后果 = 拒）：留空反例（§5 缺失语义列——rated_a
/// 无不适值可言，缺即红）与闭合面破（feeds 悬空引用）都在 boot 即拒并点名
#[test]
fn wiring_fatal_boot() {
    let root = Root::new("wiring-blank");
    let p = root.write_profile("wiring-blank.toml", GOOD);
    root.write_wiring(&p, &GOOD_WIRING.replace("rated_a = 1.0\n", ""));
    let e = boot_inputs(&p).expect_err("rated_a 留空必须致命");
    assert!(e.contains("形状不合，拒绝启动"), "{e}");
    assert!(e.contains("rated_a 缺失或不是正数"), "{e}");

    let root = Root::new("wiring-dangle");
    let p = root.write_profile("wiring-dangle.toml", GOOD);
    root.write_wiring(
        &p,
        &GOOD_WIRING.replace("feeds = []", "feeds = [\"J-NOPE\"]"),
    );
    let e = boot_inputs(&p).expect_err("悬空 feeds 必须致命");
    assert!(
        e.contains("wiring[J-T].feeds = J-NOPE") && e.contains("位号集"),
        "{e}"
    );
}

/// [slots].present 声明缺是缺格三源合一第一源：present 移除某格 →
/// (模块, 格位) 进缺格清单（缺格不是致命项，其余模块不牵连）
#[test]
fn absent_miss_source() {
    let root = Root::new("present");
    let text = GOOD.replace(", \"power.charger\"", "");
    let p = root.write_profile("absent.toml", &text);
    let boot = boot_inputs(&p).expect("声明缺不是致命项");
    assert_eq!(
        boot.missing,
        vec![("rr3-pb".to_string(), "power.charger".to_string())],
        "present 声明缺的格位对点名模块记缺格"
    );
}

/// [slots].present 查册门：未在册格位、基础层格位 → 致命点名
#[test]
fn present_rej_slots() {
    let root = Root::new("presentgate");
    let unknown = GOOD.replace(
        "present = [\"act.servo\"",
        "present = [\"sensor.wifi\", \"act.servo\"",
    );
    let p = root.write_profile("present-unknown.toml", &unknown);
    let e = boot_inputs(&p).expect_err("present 未在册格位必须致命");
    assert!(e.contains("格位 sensor.wifi 未在册"), "{e}");

    let base = GOOD.replace(
        "present = [\"act.servo\"",
        "present = [\"bus.publish\", \"act.servo\"",
    );
    let p = root.write_profile("present-base.toml", &base);
    let e = boot_inputs(&p).expect_err("present 声明基础层格位必须致命");
    assert!(e.contains("不得声明基础层格位 bus.publish"), "{e}");
}
