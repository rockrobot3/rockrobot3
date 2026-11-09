//! 装配单手术面单测（B9 结构收口，自 `profile.rs` 测试尾按语义面切出）。
//!
//! 覆盖 TOML 键行手术的红绿判据：只动目标行、fail-closed 四拒、同名多义拒、
//! 值与证据更新后重解析等值。被测面 = [`crate::profile_edit`]（机制）经
//! [`crate::profile`] 重导出。

use super::*;
use crate::profile::{rr3_domain_apply, rr3_key_line_apply};

    // ── TOML 键行手术面单测（批N22 件一）：红绿判据 = 只动目标行、fail-closed 四拒 ──

    /// 手术夹具：三域块（带注释与 [supplies] 段头，形状 = default.toml 缩减）。
    /// 手术函数是文本级纯函数，不吃完整档——注释保留与邻块原封是它的判据域。
    const SURGERY_TEXT: &str = r#"# 电力网络声明
[supplies]
tick_period_ms = 200

[[supplies.domain]]
name = "batt" # 电池域
nominal_mv = 3700
severe_mv = 3300
armed = false
evidence = "none"

[[supplies.domain]]
name = "rail5v"
nominal_mv = 5000
severe_mv = 4400
armed = false
evidence = "none"

[[supplies.domain]]
name = "rail3v3"
nominal_mv = 3300
severe_mv = 3000
armed = false
evidence = "none"
"#;

    /// 正例：只动目标域的目标键行——注释保留、邻域同名键行原封、块序不变
    #[test]
    fn domain_keys_apply_touches_target_block_only() {
        let out =
            rr3_domain_apply(SURGERY_TEXT, "batt", &[("severe_mv", "3200")]).expect("合法手术应过");
        // 目标行已改
        assert!(out.contains("severe_mv = 3200"), "{out}");
        // 邻域同名键行原封
        assert!(out.contains("severe_mv = 4400"), "rail5v 不得波及: {out}");
        assert!(out.contains("severe_mv = 3000"), "rail3v3 不得波及: {out}");
        // 注释与块序保留
        assert!(out.contains('#'), "注释须保留: {out}");
        assert!(out.contains("# 电池域"), "{out}");
        let batt = out.split("rail5v").next().unwrap_or_default();
        assert!(
            batt.contains("name = \"batt\"") && batt.contains("severe_mv = 3200"),
            "batt 块内自洽: {batt}"
        );
    }

    /// 正例：armed + evidence 两键行一次手术（写口联动写形状）
    #[test]
    fn domain_keys_apply_pairs_armed_and_evidence() {
        let out = rr3_domain_apply(
            SURGERY_TEXT,
            "rail5v",
            &[("armed", "true"), ("evidence", "\"实测/0002\"")],
        )
        .expect("合法联动手术应过");
        let rail5v = out.split("rail3v3").next().unwrap_or_default();
        assert!(rail5v.contains("armed = true"), "{rail5v}");
        assert!(rail5v.contains("evidence = \"实测/0002\""), "{rail5v}");
        // batt 块的 armed 原封
        let batt = out.split("rail5v").next().unwrap_or_default();
        assert!(batt.contains("armed = false"), "batt 不得波及: {batt}");
    }

    /// 缺域拒：域名不在册不带病落盘
    #[test]
    fn domain_keys_apply_rejects_missing_domain() {
        let e = rr3_domain_apply(SURGERY_TEXT, "rail9v", &[("severe_mv", "1")])
            .expect_err("缺域必须拒");
        assert!(e.contains("rail9v") && e.contains("缺域拒"), "{e}");
    }

    /// 多义拒：同名两块 = 装载期域名唯一判据破，手术面不替解析器裁决
    #[test]
    fn domain_keys_apply_rejects_ambiguous_domain() {
        let dup = format!("{SURGERY_TEXT}\n[[supplies.domain]]\nname = \"batt\"\nnominal_mv = 1\nsevere_mv = 2\narmed = false\nevidence = \"none\"\n");
        let e = rr3_domain_apply(&dup, "batt", &[("severe_mv", "1")]).expect_err("同名多义必须拒");
        assert!(e.contains("命中 2 个"), "{e}");
    }

    /// 键行缺拒：块内缺目标键（档被改病）不插入不猜
    #[test]
    fn domain_keys_apply_rejects_missing_key_line() {
        let broken = SURGERY_TEXT.replace("severe_mv = 3300\n", "");
        let e = rr3_domain_apply(&broken, "batt", &[("severe_mv", "3200")])
            .expect_err("键行缺失必须拒");
        assert!(e.contains("severe_mv 缺失"), "{e}");
    }

    /// 平铺键行手术（轴档案用）：替换正例 + 词边界（`hi_deg` 不得命中他行）+
    /// 缺行拒
    #[test]
    fn toml_key_line_apply_flat_and_word_boundary() {
        let axis = "schema = 1\npin = \"act.axis.pwm.pan\"\nhi_deg = 120\nslew_deg_s = 100\n";
        let out = rr3_key_line_apply(axis, "hi_deg", "135").expect("平铺手术应过");
        assert!(out.contains("hi_deg = 135"), "{out}");
        assert!(out.contains("slew_deg_s = 100"), "{out}");
        // 词边界：后随非 '=' 不命中——"deg" 无命中即拒而非误伤
        let e = rr3_key_line_apply(axis, "deg", "1").expect_err("无命中必须拒");
        assert!(e.contains("deg 缺失"), "{e}");
        // 尾行替换保尾换行
        let out = rr3_key_line_apply(axis, "slew_deg_s", "90").expect("应过");
        assert!(out.ends_with('\n'), "尾换行须保留: {out:?}");
    }

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

    /// [slots].device_absent 正例（D3 裁定 5，批N2 件三）：base 层格带理由登记
    /// 器件缺席 = 校验⑥合法态——boot 过、不进缺格判定（"恒在位"是通道承诺不是
    /// 器件承诺，缺席显式申报即认）。
    #[test]
    fn device_absent_declares_base_slot_absence_legally() {
        let root = Root::new("absent-ok");
        let ok = GOOD.replace(
            "present = [\"act.servo\", \"cognition.llm\", \"power.charger\"]",
            "present = [\"act.servo\", \"cognition.llm\", \"power.charger\"]\n\n[slots.device_absent]\n\"power.battery\" = \"无电池形态（市电/桥上灌供电），校验⑥认它为合法态\"",
        );
        let p = root.write_profile("absent-ok.toml", &ok);
        let boot = boot_inputs(&p).expect("base 层格带理由登记缺席必须过");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
        let parsed = rr3_profile_parse(&ok).expect("正例文本应能解析");
        assert_eq!(
            parsed
                .slots_device_absent
                .get("power.battery")
                .map(String::as_str),
            Some("无电池形态（市电/桥上灌供电），校验⑥认它为合法态")
        );
    }

    /// [slots].device_absent 查册门三红：未在册格名、非 base 层格（缺席表达 =
    /// 不进 present，写进本表属第二套语法即红，F-2 同型）、理由留空（no_injection
    /// 范式——缺席不写理由 = 未申报的静默退化）。
    #[test]
    fn device_absent_gate_rejects_unknown_nonbase_and_empty_reason() {
        let root = Root::new("absent-unknown");
        let unknown = GOOD.replace(
            "present = [\"act.servo\", \"cognition.llm\", \"power.charger\"]",
            "present = [\"act.servo\", \"cognition.llm\", \"power.charger\"]\n\n[slots.device_absent]\n\"sensor.nope\" = \"不存在的格\"",
        );
        let p = root.write_profile("absent-unknown.toml", &unknown);
        let e = boot_inputs(&p).expect_err("device_absent 未在册格位必须致命");
        assert!(e.contains("device_absent 格位 sensor.nope 未在册"), "{e}");

        let root = Root::new("absent-body");
        let body = GOOD.replace(
            "present = [\"act.servo\", \"cognition.llm\", \"power.charger\"]",
            "present = [\"act.servo\", \"cognition.llm\", \"power.charger\"]\n\n[slots.device_absent]\n\"act.servo\" = \"板上无舵机\"",
        );
        let p = root.write_profile("absent-body.toml", &body);
        let e = boot_inputs(&p).expect_err("device_absent 声明非 base 层格必须致命");
        assert!(
            e.contains("只登记基础层格位") && e.contains("act.servo"),
            "{e}"
        );

        let root = Root::new("absent-empty");
        let empty = GOOD.replace(
            "present = [\"act.servo\", \"cognition.llm\", \"power.charger\"]",
            "present = [\"act.servo\", \"cognition.llm\", \"power.charger\"]\n\n[slots.device_absent]\n\"power.battery\" = \"\"",
        );
        let p = root.write_profile("absent-empty.toml", &empty);
        let e = boot_inputs(&p).expect_err("device_absent 理由留空必须致命");
        assert!(e.contains("理由必须是非空字符串"), "{e}");
    }

    /// 装配单文件不存在 = 致命，文案是绝对化口径（canonicalize 的 ENOENT）。
    /// 接线路径随意给：装配单 canonicalize 先败，轮不到它（有 Root 帮手的用例都
    /// 走真推导，本条只钉读不到装配单的文案口径）。
    #[test]
    fn missing_profile_file_is_fatal() {
        let e = rr3_boot_inputs(
            Path::new("/nonexistent/rr3-nope.toml"),
            Path::new(RR3_CONTRACT_SLOTS_PATH),
            Path::new(RR3_CONTRACT_ROLES_PATH),
            Path::new(RR3_CONTRACT_DEVICES_PATH),
            Path::new("/nonexistent/rr3-nope-wiring.toml"),
            Path::new("/nonexistent/rr3-nope-platform"),
        )
        .expect_err("读不到装配单必须致命");
        assert!(e.contains("无法解析为绝对路径"), "{e}");
    }

    /// 校验③④⑤⑧判据前件的 fail-closed（§4.6 裁定①）：[platform].chip 选不到表 =
    /// 拒 boot，不许降级成"无表 = 无冲突/无词表"（宽容 = ④⑤⑧ 静默不判，F-6 同病）
    #[test]
    fn platform_table_missing_for_chip_is_fatal() {
        let root = Root::new("plat-missing");
        let p = root.write_profile("plat-missing.toml", GOOD);
        let wiring = crate::wiring::rr3_wiring_path(&p).expect("夹具装配单必须能推导接线档");
        let e = rr3_boot_inputs(
            &p,
            Path::new(RR3_CONTRACT_SLOTS_PATH),
            Path::new(RR3_CONTRACT_ROLES_PATH),
            Path::new(RR3_CONTRACT_DEVICES_PATH),
            &wiring,
            &root.dir.join("platform-empty"),
        )
        .expect_err("chip 选不到平台表必须拒 boot");
        assert!(
            e.contains("读不到平台事实表") && e.contains("rk3566"),
            "{e}"
        );
    }

    /// 平台事实表词形段缺失 = 致命（实-6 (a)，批N4 件七，裁定①）：host 词形派生
    /// 前件随平台表形状强制在册，缺段在 rr-registry 解析即红、boot 链原样上抛
    /// 拒 boot（宽容 = 派生静默不判，F-6 同病）。
    #[test]
    fn platform_wordform_section_missing_is_fatal() {
        let good_esp = GOOD.replace("chip = \"rk3566\"", "chip = \"esp32\"");
        let no_wf =
            platform_esp32_text().replace("[controller_wordform]", "[controller_wordform_x]");
        let e = rr3_profile_from_text_with_platform("wf-missing", &good_esp, &no_wf)
            .expect_err("平台表缺词形段必须拒 boot");
        assert!(
            e.contains("controller_wordform") && e.contains("形状不合"),
            "{e}"
        );
    }

    /// host 档 + 空词形表 = 守卫红（实-6 (a) fail-closed）：rtos 册（esp32）空表
    /// 显式合法，但 host 档误选即前件缺失——逐键红之外总守卫一条，点名选册错误。
    #[test]
    fn host_tier_with_empty_wordform_table_is_fatal() {
        let good_esp = GOOD.replace("chip = \"rk3566\"", "chip = \"esp32\"");
        let e =
            rr3_profile_from_text_with_platform("host-empty-wf", &good_esp, platform_esp32_text())
                .expect_err("host 档选空词形表必须拒 boot");
        assert!(e.contains("controller_wordform 为空"), "{e}");
    }

    /// ①-f 反例（实-6 (a) 下移后补齐——此前四对词形硬编码在 rrd、本判据零反例）：
    /// 控制器词形指明 i2c 总线族，但角色 derives 只声明 GPIO 族键——派生视图会漏
    /// 键，装载即红。词形清单遍历自平台表（rk3566 真册），i2c1 ∈ ⑧-b 词表在册。
    #[test]
    fn controller_wordform_family_off_role_derives_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"i2c1\"\n\
             physical = [26]\ndirection = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\n\
             mux_group = []\n"
        );
        let root = Root::new("wf-drift");
        let p = root.write_profile("wf-drift.toml", &text);
        let e = boot_inputs(&p).expect_err("词形族与角色 derives 各说一套必须拒 boot");
        assert!(e.contains("派生漂移") && e.contains("i2c_bus"), "{e}");
    }

    /// [platform].chip 留空反例（F-6 缺字段可被留空绕过的封堵面）：chip 是平台
    /// 事实外键，"合法不适用"不存在——留空即红
    #[test]
    fn platform_chip_left_blank_is_fatal() {
        let text = GOOD.replace("chip = \"rk3566\"", "");
        let errs = rr3_profile_parse(&text).expect_err("chip 留空必须报错");
        assert!(errs.iter().any(|e| e.contains("缺 chip")), "{errs:?}");

        let text = GOOD.replace("chip = \"rk3566\"", "chip = \"\"");
        let errs = rr3_profile_parse(&text).expect_err("chip 空串必须报错");
        assert!(
            errs.iter().any(|e| e.contains("chip 不得为空串")),
            "{errs:?}"
        );
    }

    /// 校验⑤-b 反例（F-9）：模拟测点落非 ADC 脚——GPIO23 读回 0 mV ⇒ 档位恒真或
    /// 恒假。ESP32_GOOD 的电表 pin physical = [34] ∈ adc_pins 是绿基线，替换成
    /// 23 即红（测点引用面 = 域 sense 的 pin_id 外键）。
    #[test]
    fn analog_sense_pin_off_adc_set_is_rejected() {
        let bad = ESP32_GOOD.replace("physical = [34]", "physical = [23]");
        let e = rr3_profile_from_text_with_platform("adc-off", &bad, platform_esp32_text())
            .expect_err("测点落非 ADC 脚必须拒 boot");
        assert!(e.contains("ADC 能力集") && e.contains("校验⑤-b"), "{e}");
    }

    /// 校验⑤-a 反例：direction = out（角色 input_only_ok = false）落 input-only
    /// 脚集——GPIO35 无输出驱动能力，写 out 即假账。换到 35 避开电表 34 的 ③-a
    /// 双占级联，红因单一。
    #[test]
    fn output_on_input_only_pin_is_rejected() {
        let bad = ESP32_GOOD.replace("physical = [32]", "physical = [35]");
        let e = rr3_profile_from_text_with_platform("input-only", &bad, platform_esp32_text())
            .expect_err("output 落 input-only 脚必须拒 boot");
        assert!(e.contains("input-only") && e.contains("校验⑤-a"), "{e}");
    }

    /// 校验⑧-b 反例：controller 不在平台在册控制器词表——装配单不得自造控制器
    /// （ledc9 词形合法但词表无此名，词表不是词形）。
    #[test]
    fn controller_off_platform_vocabulary_is_rejected() {
        let bad = ESP32_GOOD.replace("controller = \"ledc0\"", "controller = \"ledc9\"");
        let e = rr3_profile_from_text_with_platform("ctl-word", &bad, platform_esp32_text())
            .expect_err("自造控制器必须拒 boot");
        assert!(e.contains("在册控制器词表") && e.contains("校验⑧-b"), "{e}");
    }

    /// 校验③-a 反例：同一 physical 元素两条 pin 双占（启用集 = [pins] 全集）
    #[test]
    fn two_pins_same_physical_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpiochip0\"\n\
             physical = [12]\ndirection = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\n\
             mux_group = []\n"
        );
        let root = Root::new("dup-phys");
        let p = root.write_profile("dup-phys.toml", &text);
        let e = boot_inputs(&p).expect_err("physical 双占必须拒 boot");
        assert!(
            e.contains("校验③-a") && e.contains("physical 元素 12"),
            "{e}"
        );
    }

    /// 校验③-b 反例：同一 (controller, channel) 双占——F-7 的"脚 + 通道"两维，
    /// 双轴两通道即证；占同通道不同脚照红。
    #[test]
    fn two_pins_same_controller_channel_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"act.axis.pwm\"\ninstance = \"tilt\"\n\
             controller = \"pwm0\"\nchannel = 0\nphysical = [13]\ndirection = \"out\"\n\
             rail = \"rail5v\"\nfailsafe = \"release\"\nmux_group = []\n"
        );
        let root = Root::new("dup-chan");
        let p = root.write_profile("dup-chan.toml", &text);
        let e = boot_inputs(&p).expect_err("(controller, channel) 双占必须拒 boot");
        assert!(
            e.contains("校验③-b") && e.contains("(controller pwm0, channel 0)"),
            "{e}"
        );
    }

    /// 校验③-c 反例（批N3 件八精化判据）：同一 (bus, addr) 共址而 device 缺省——
    /// 共址须同 device 且型号在册，器件身份不可判（缺省）即红（P-5 共读源必须显式
    /// 登记器件身份；"none" 是显式不适值，不占地址维不误报）。GOOD 只有 servo 一条
    /// pin，这里成对追加两条同 (i2c1, 0x40) 的 meter 才构成共址。
    #[test]
    fn two_pins_same_bus_addr_without_device_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndirection = \"in\"\nmux_group = []\n\
             [[pin]]\nrole = \"power.charger.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndirection = \"in\"\nmux_group = []\n"
        );
        let root = Root::new("dup-addr");
        let p = root.write_profile("dup-addr.toml", &text);
        let e = boot_inputs(&p).expect_err("共址而 device 缺省必须拒 boot");
        assert!(
            e.contains("校验③-c") && e.contains("(bus i2c1, addr 0x40)"),
            "{e}"
        );
    }

    /// 校验③-c 正例（P-5 共读源形状）：battery 与 charger 同 (i2c1, 0x40) 同
    /// device = INA219 共用同一读数源——合法不红（一个物理器件服务两个角色不等于
    /// 地址冲突；device 非空且在册 = 门 o 装载侧执法同时过）。
    #[test]
    fn two_pins_same_bus_addr_same_device_is_allowed() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndevice = \"INA219\"\ndirection = \"in\"\nmux_group = []\n\
             [[pin]]\nrole = \"power.charger.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndevice = \"INA219\"\ndirection = \"in\"\nmux_group = []\n"
        );
        let root = Root::new("shared-addr");
        let p = root.write_profile("shared-addr.toml", &text);
        let boot = boot_inputs(&p).expect("同址同器件（共读源）应通过");
        assert!(
            boot.hal_bindings.contains("i2c_bus = \"1\""),
            "battery.meter 派生键照常注入（charger derives=[] 不派生）: {:?}",
            boot.hal_bindings
        );
        assert_eq!(
            boot.hal_bindings.matches("ina219_addr").count(),
            1,
            "共读源角色不重复派生（derives=[] 维持注入单真值点）"
        );
    }

    /// 校验③-c 反例（器件身份不同 = 真冲突）：同 (i2c1, 0x40) 而两条 device 各说
    /// 一套——坑 7 的区分位（地址有应答 ≠ 型号正确），两条各点名一条。
    #[test]
    fn two_pins_same_bus_addr_mismatched_device_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndevice = \"INA219\"\ndirection = \"in\"\nmux_group = []\n\
             [[pin]]\nrole = \"power.charger.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndevice = \"SSD1306\"\ndirection = \"in\"\nmux_group = []\n"
        );
        let root = Root::new("addr-mix");
        let p = root.write_profile("addr-mix.toml", &text);
        let e = boot_inputs(&p).expect_err("共址而 device 不同必须拒 boot");
        assert!(
            e.contains("校验③-c") && e.contains("(bus i2c1, addr 0x40)"),
            "{e}"
        );
    }

    /// 门 o 装载侧执法反例：device 非空但型号不在器件词表——装配单不得自造型号词
    /// （单条 pin 也判，与共址无关）。
    #[test]
    fn device_off_vocabulary_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndevice = \"INMP441\"\ndirection = \"in\"\nmux_group = []\n"
        );
        let root = Root::new("dev-word");
        let p = root.write_profile("dev-word.toml", &text);
        let e = boot_inputs(&p).expect_err("自造型号词必须拒 boot");
        assert!(e.contains("门 o") && e.contains("INMP441"), "{e}");
    }

    /// 校验④-b 反例：两条 pin 的 physical 命中平台 conflicts 同一子集（esp32 组
    /// i2c0 = {21, 22}）而 mux_group 都未申报该组 ⇒ 漏声明即红；组名取自平台表
    /// （装机档不得自起名，否则 mux_group 退回自愿填写）。
    #[test]
    fn conflict_subset_without_mux_group_is_rejected() {
        let text = format!(
            "{ESP32_GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpio0\"\n\
             physical = [22]\ndirection = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\n\
             mux_group = []\n"
        )
        .replace("physical = [32]", "physical = [21]");
        let e = rr3_profile_from_text_with_platform("conflict-miss", &text, platform_esp32_text())
            .expect_err("命中 conflicts 子集而未申报同组必须拒 boot");
        assert!(
            e.contains("mux_group") && e.contains("conflicts") && e.contains("校验④-b"),
            "{e}"
        );
        // 漏声明的两条各点名一条（成员成对才成形，点名到每一条漏报者）
        assert_eq!(e.matches("校验④-b").count(), 2, "{e}");
    }

    /// 校验④-b 正例：同一冲突子集的两条 pin 都申报平台组名（mux_group = ["i2c0"]）
    /// → 放行。平台表超出现役绑定的成员（18/19 组）不算红——芯片事实 ≠ 现役占用，
    /// 单向含入语义。
    #[test]
    fn conflict_subset_with_mux_group_passes() {
        let text = format!(
            "{ESP32_GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpio0\"\n\
             physical = [22]\ndirection = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\n\
             mux_group = [\"i2c0\"]\n"
        )
        .replace("physical = [32]", "physical = [21]")
        .replace(
            "mux_group = []\n\n[[pin]]\nrole = \"power.battery.meter\"",
            "mux_group = [\"i2c0\"]\n\n[[pin]]\nrole = \"power.battery.meter\"",
        );
        rr3_profile_from_text_with_platform("conflict-ok", &text, platform_esp32_text())
            .expect("同组申报的两条命中者必须放行");
    }
