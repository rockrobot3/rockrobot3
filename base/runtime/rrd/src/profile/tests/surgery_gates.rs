//! 装配单结构门与真档钻取单测（批B13 自 `surgery.rs` 按被试族切出）。
//!
//! 覆盖 `rr3_boot_inputs` 的形状判据（[hardware] 物理键、SPI 稳态、role、新段、
//! tier、wiring 交叉）与四份真仓装配单的装载钻取（default/distro/esp32-mini/pi4b）。

use super::*;

/// 门 n 稳态判据（§8，power P-13）：[hardware] 出现物理键即红——GOOD 无电表
/// pin，i2c_bus 无派生者，按稳态文案点名
#[test]
fn gate_n_hardware_physical_key_is_fatal() {
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
fn gate_n_coexistence_names_both_values() {
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
fn gate_n_spi_device_steady_state_red() {
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
fn gate_n_spi_device_coexistence_names_both_values() {
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
fn role_missing_is_listed_not_fatal() {
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
fn boot_inputs_rejects_out_of_whitelist_slot_contract() {
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
fn new_sections_are_required_and_named() {
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
fn platform_and_supplies_shape_errors_are_named() {
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
fn new_sections_unknown_keys_are_named() {
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
fn bare_tier_without_watchdog_is_fatal() {
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
fn tier_outside_slot_tier_scope_is_fatal() {
    let root = Root::new("l10");
    // tier 换 bare 同批翻转 [supplies.budget] 为 bare 档形状（分档自洽，
    // bare_tier_without_watchdog_is_fatal 注记同源）
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

/// 校验⑦ 跨档反例（后果 = 拒；boot 链把 [pins]/domains/devices 三份真值接起来
/// 判，wiring.rs 单测已钉判定逻辑，这里钉的是跨档接线）：signal 越出
/// pin_id ∪ {vcc,gnd}（F-3 自造语法）、net 越出 domains（F-5）、mate 越出
/// devices mate 集（§4.1b 门 o）逐条点名；命中集全绿由末段正例钉住。
#[test]
fn wiring_cross_file_gates_are_fatal() {
    // ⑦-a：自造语法 pwm:12 既非 pin_id 也非 vcc/gnd（F-2 同款病，接线侧再现）
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

    // ⑦-b：自造域名（F-5：文档示例值会被装机档抄）
    let root = Root::new("wiring-7b");
    let p = root.write_profile("wiring-7b.toml", GOOD);
    root.write_wiring(
        &p,
        &GOOD_WIRING.replace("net = \"rail5v\"", "net = \"rail24\""),
    );
    let e = boot_inputs(&p).expect_err("自造域名必须致命");
    assert!(e.contains("校验⑦-b") && e.contains("rail24"), "{e}");

    // ⑦-d：对端型号不在 devices.toml mate 集（词形错误——在册的是 SG90-3P）
    let root = Root::new("wiring-7d");
    let p = root.write_profile("wiring-7d.toml", GOOD);
    root.write_wiring(
        &p,
        &GOOD_WIRING.replace("mate = \"none\"", "mate = \"SG90-9G\""),
    );
    let e = boot_inputs(&p).expect_err("mate 不在册必须致命");
    assert!(e.contains("校验⑦-d") && e.contains("SG90-9G"), "{e}");

    // 命中集全绿：signal 写 [pins] 真值 pin_id、net/mate 写在册值——⑦ 三判据同过
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

/// 接线档装载门（fail-closed，后果 = 拒）：留空反例（§5 缺失语义列——rated_a
/// 无不适值可言，缺即红）与闭合面破（feeds 悬空引用）都在 boot 即拒并点名
#[test]
fn wiring_shape_and_closure_gates_are_fatal_at_boot() {
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

/// 真仓 default 机 drill：profiles/default.toml + hardware/default/wiring.toml
/// 过完整装载门——⑦-a 的四个 pin_id（act.axis.pwm.pan/act.led.gpio/
/// act.relay.gpio/power.battery.meter）、⑦-b 的三域名（batt/rail5v/rail3v3）、
/// ⑦-d 的 SG90-3P 全部命中仓内真值（跨档对账的三份真值两两闭合）。
#[test]
fn default_profile_wiring_drill() {
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
fn distro_profile_wiring_drill() {
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
fn esp32_mini_profile_boot_inputs_drill() {
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
fn pi4b_board_profile_boot_inputs_drill() {
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
fn pi4b_host_profile_boot_inputs_drill() {
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

/// [slots].present 声明缺是缺格三源合一第一源：present 移除某格 →
/// (模块, 格位) 进缺格清单（缺格不是致命项，其余模块不牵连）
#[test]
fn absent_slot_declaration_is_missing_source() {
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
fn present_registry_gate_rejects_unknown_and_base_slots() {
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
