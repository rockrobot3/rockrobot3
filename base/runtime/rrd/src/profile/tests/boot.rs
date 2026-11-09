//! 装载输入面单测（批B15 自 `tests.rs` 按被试族切出）。
//!
//! 覆盖执行轴档案投影（单轴/双轴/坏件/副本/轴错配）、探针注入与 binding 归属、
//! `[hardware]` 标量串化与排序转义、SPI 词形推导，以及引脚/域/证据三道装载判据。

use super::*;

/// 轴档案文本帮手（十四键，值面参照件二 §4.4 示例档）：pin/calibrated/hard_stop
/// 三键参数化——pin 走 GOOD 的 pin_id 语法（外键对账 F-4），状态两键即执行轴
/// 投影的输入面。
fn axis_text(pin: &str, calibrated: bool, hard_stop: &str) -> String {
    axis_text_range(pin, calibrated, hard_stop, 60, 120)
}

/// 行程可变的投影输入面（件五双轴用例：pan/tilt 各给一行程，注入文本可辨源）。
fn axis_text_range(pin: &str, calibrated: bool, hard_stop: &str, lo: u16, hi: u16) -> String {
    format!(
        "schema = 1\npin = \"{pin}\"\ncenter_deg = 90\nlo_deg = {lo}\nhi_deg = {hi}\ndir_sign = 1\nslew_deg_s = 100\nsettle_ms = 200\nhold = \"release\"\ncurrent_ceil_ma = 750\nwarmup_ms = 0\nhard_stop = \"{hard_stop}\"\ncalibrated = {calibrated}\nsource = \"manual\"\n"
    )
}

/// 双轴装配单（ESP32_GOOD + tilt pin）：rtos 档不派生 [hardware]，双 pin 可在册
/// （host 档派生键同值校验会把双轴拒在门外——host 模块实例是单轴单描述符，模型
/// 自洽；板侧双轴真形貌在 rtos 档，板侧描述符消费归 0.0.7）。channel/physical 与
/// pan 错开（③-a/③-b 双占各守一维）。件五消歧用例的输入面。
fn esp32_good_tilt() -> String {
    format!(
        "{ESP32_GOOD}\n[[pin]]\nrole = \"act.axis.pwm\"\ninstance = \"tilt\"\n\
         controller = \"ledc0\"\nchannel = 1\nphysical = [33]\ndirection = \"out\"\n\
         rail = \"rail5v\"\nfailsafe = \"release\"\nmux_group = []\n"
    )
}

/// 执行轴状态投影·恰一份（批N3 件三建，件五扩形）：合法档案装载成功 → 注入
/// 文本按 `calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>` 投影，
/// 点名清单干净——rr3-act 拒动判据与行程裁决的输入面就是这五键的档案真值。
#[test]
fn axis_state_projects_when_exactly_one_profile_loads() {
    let root = Root::new("axis-one");
    let p = root.write_profile("full.toml", GOOD);
    root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
    let boot = boot_inputs(&p).expect("恰一份合法档案不改变 boot 绿面");
    assert_eq!(
        boot.axis_state, "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90",
        "恰一份合法档案必须投影执行轴状态（rr3-act 拒动判据的输入面）"
    );
    assert!(boot.axis_notes.is_empty(), "{:?}", boot.axis_notes);
}

/// 执行轴状态投影·坏件（批N3 件三）：档案形状错 → 注入文本空串 = 不注入
/// （模块侧折保守缺省，§6 复位语义④），点名清单逐条入账且**不拒 boot**——
/// 档案装载是降级面，与缺格/缺角色同档。
#[test]
fn axis_state_stays_empty_and_notes_broken_profile() {
    let root = Root::new("axis-broken");
    let p = root.write_profile("full.toml", GOOD);
    root.write_axis(&p, "pan", "schema = 1\npin = \"act.axis.pwm.pan\"\n");
    let boot = boot_inputs(&p).expect("坏档案不拒 boot（降级面，与缺格同档）");
    assert!(
        boot.axis_state.is_empty(),
        "坏档案不得投影注入文本——缺真相按最窄面起跑"
    );
    assert!(
        boot.axis_notes
            .iter()
            .any(|n| n.contains("pan.toml") && n.contains("坏形")),
        "坏件必须点名到件: {:?}",
        boot.axis_notes
    );
}

/// 执行轴状态投影·同 pin 副本（批N3 件三建，件五收口）：两份合法档案同一 pin
/// → 注入文本空串并点名副本——同 pin 副本不是消歧对象是病灶（恰一份合法才投影
/// 的 boot 链防线；写侧同款拒面在 axis_write 件四，读侧点名指清理）。
#[test]
fn axis_state_stays_empty_and_notes_duplicate_pin_profiles() {
    let root = Root::new("axis-two");
    let p = root.write_profile("full.toml", GOOD);
    root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
    root.write_axis(&p, "pan-copy", &axis_text("act.axis.pwm.pan", true, "yes"));
    let boot = boot_inputs(&p).expect("两份合法档案不改变 boot 绿面");
    assert!(
        boot.axis_state.is_empty(),
        "同 pin 副本不得投影注入文本——恰一份合法才投影"
    );
    assert!(
        boot.axis_notes
            .iter()
            .any(|n| n.contains("2 份合法档案（副本）") && n.contains("先清理副本")),
        "同 pin 副本必须点名病灶: {:?}",
        boot.axis_notes
    );
}

/// 执行轴状态投影·双轴消歧（批N3 件五，收"多轴未消歧"挂账）：pan + tilt 两份
/// 合法档案，轴读数 = pan（env 缺省同源）→ 只投影 pan 的行程真值（五键文本
/// 可辨源：pan [60,120] ≠ tilt [80,100]）；点名清单干净——注入的行程必须属于
/// 被驱动的那根轴。
#[test]
fn axis_state_projects_driven_axis_when_two_axes_present() {
    let root = Root::new("axis-dual");
    let p = root.write_profile("dual.toml", &esp32_good_tilt());
    root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
    root.write_axis(
        &p,
        "tilt",
        &axis_text_range("act.axis.pwm.tilt", true, "yes", 80, 100),
    );
    let boot = boot_inputs(&p).expect("双轴两份合法档案不改变 boot 绿面");
    assert_eq!(
        boot.axis_state, "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90",
        "双轴在场只投影被驱动轴（env 缺省 pan）的行程真值"
    );
    assert!(boot.axis_notes.is_empty(), "{:?}", boot.axis_notes);
}

/// 执行轴状态投影·轴读数与档案 instance 错配（批N3 件五）：装配单只挂 tilt
/// pin、档案只有 tilt，轴读数 = 缺省 pan → 单份也对轴，"注入 tilt 行程、驱动
/// pan 轴"是错配，宁可空串折保守缺省并点名。
#[test]
fn axis_state_stays_empty_when_axis_env_misses_every_instance() {
    let text = GOOD.replace("instance = \"pan\"", "instance = \"tilt\"");
    let root = Root::new("axis-mismatch");
    let p = root.write_profile("tilt-only.toml", &text);
    root.write_axis(
        &p,
        "tilt",
        &axis_text_range("act.axis.pwm.tilt", true, "yes", 80, 100),
    );
    let boot = boot_inputs(&p).expect("轴错配不改变 boot 绿面（降级面）");
    assert!(
        boot.axis_state.is_empty(),
        "轴读数对不上任何 instance 不得投影——错配行程比无行程更危险"
    );
    assert!(
        boot.axis_notes
            .iter()
            .any(|n| n.contains("无一 instance = \"pan\"")),
        "错配必须点名轴读数与 instance 集: {:?}",
        boot.axis_notes
    );
}

/// MC-4 门禁用例①：注入非 linux 生产探针（有硬件面的格位恒缺）→ 这些格位进缺格
/// 清单，且**点名到件与键**（(模块, 格位) 成对，模块不错配到别的格）——这条钉的
/// 是"缺格 → Degraded 点名件与键"的检出真值（探测拿不到答案不当作在位的下游）。
/// 无硬件面可反证的格位（cognition.llm）不在缺格清单：那是"没有反证"不是"拿不到答案"。
#[test]
fn injected_absent_probe_names_missing_module_and_slot() {
    let root = Root::new("probe-absent");
    let p = root.write_profile("absent.toml", GOOD);
    let boot = boot_inputs_with_probe(&p, &crate::probe::Rr3AbsentProbe)
        .expect("缺格不是致命项，boot 输入必须成功返回");
    assert_eq!(
        boot.missing,
        vec![
            ("rr3-pa".to_string(), "act.servo".to_string()),
            ("rr3-pb".to_string(), "power.charger".to_string()),
        ],
        "有硬件面的格位必须点名件与键、升序去重（软件格 cognition.llm 不在内）: {:?}",
        boot.missing
    );
}

/// MC-4 门禁用例②：探测件只对一格报缺 → 只有那一格的点名模块进缺格，
/// 其余同在位——证明缺格判定确实**逐格读探测结果**，不是整单连坐。
#[test]
fn injected_selective_probe_marks_only_its_slot() {
    struct OnlyServoAbsent;
    impl crate::probe::Rr3Probe for OnlyServoAbsent {
        fn rr3_probe_slot(&self, slot: &str) -> bool {
            slot != "act.servo"
        }
    }
    let root = Root::new("probe-selective");
    let p = root.write_profile("selective.toml", GOOD);
    let boot = boot_inputs_with_probe(&p, &OnlyServoAbsent).expect("部分缺格不致命");
    assert_eq!(
        boot.missing,
        vec![("rr3-pa".to_string(), "act.servo".to_string())],
        "只有探到缺的那一格点名件与键: {:?}",
        boot.missing
    );
}

/// 缺格：bindings 移除任一格 → (模块, 格位) 进缺格清单，其余不受牵连
#[test]
fn removed_binding_is_missing_for_its_module_only() {
    let root = Root::new("missing");
    let p = root.write_profile(
        "missing.toml",
        &GOOD.replace("\"power.charger\" = \"rr3-pb\"\n", ""),
    );
    let boot = boot_inputs(&p).expect("缺格不是致命项");
    assert_eq!(
        boot.missing,
        vec![("rr3-pb".to_string(), "power.charger".to_string())],
        "只有点名模块缺格"
    );
    assert_eq!(
        boot.manifests.len(),
        3,
        "缺格模块的 manifest 仍在解析结果里（供日志点名）"
    );
}

/// enabled_slots 声明了格位但 binding 指向别的模块：对点名模块算缺格，同时构成"binding 未被认领"
/// 的结构不自洽 → 致命（fail-closed，装配单各说一套必须当场听见）
#[test]
fn binding_claimed_by_another_module_is_fatal() {
    let root = Root::new("cross");
    let text = GOOD.replace(
        "\"power.charger\" = \"rr3-pb\"",
        "\"power.charger\" = \"rr3-pc\"",
    );
    let p = root.write_profile("cross.toml", &text);
    let e = boot_inputs(&p).expect_err("binding 被他人认领必须致命");
    assert!(e.contains("未被 enabled_slots 认领"), "{e}");
}

/// 基础层格位进 bindings = 违反"基础层由基座内建承载"（slots.toml 注记）→ 致命
#[test]
fn base_layer_binding_is_fatal() {
    let root = Root::new("base");
    let text = GOOD.replace("[bindings]\n", "[bindings]\n\"bus.publish\" = \"rr3-pa\"\n");
    let p = root.write_profile("base.toml", &text);
    let e = boot_inputs(&p).expect_err("基础层 binding 必须致命");
    assert!(e.contains("bindings 不得绑定基础层格位 bus.publish"), "{e}");
}

/// 查册门：enabled_slots/bindings 引用未在册格位 → 致命并点名（真契约上跑）
#[test]
fn unknown_slot_is_fatal_and_named() {
    let root = Root::new("unknown");
    let text = GOOD.replace("rr3-pc = [\"cognition.llm\"]", "rr3-pc = [\"sensor.wifi\"]");
    let p = root.write_profile("unknown.toml", &text);
    let e = boot_inputs(&p).expect_err("未在册格位必须致命");
    assert!(
        e.contains("enabled_slots 模块 rr3-pc 的格位 sensor.wifi 未在册"),
        "{e}"
    );
}

/// 基础层格位进 enabled_slots 只记账、不算缺格（power.battery 恒在位，default 机同款形状）
#[test]
fn base_layer_enabled_slot_is_never_missing() {
    let root = Root::new("baseslot");
    let text = GOOD.replace(
        "rr3-pc = [\"cognition.llm\"]",
        "rr3-pc = [\"cognition.llm\", \"power.battery\"]",
    );
    let p = root.write_profile("baseslot.toml", &text);
    let boot = boot_inputs(&p).expect("基础层 enabled_slots 格位不缺格");
    assert!(boot.missing.is_empty(), "{:?}", boot.missing);
}

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

/// 校验①-a：role 未在 roles.toml 在册 → 致命点名（真契约上跑）
#[test]
fn pin_unknown_role_is_fatal() {
    let root = Root::new("pin-role");
    let text = GOOD.replace("role = \"act.axis.pwm\"", "role = \"act.axis.magical\"");
    let p = root.write_profile("pin-role.toml", &text);
    let e = boot_inputs(&p).expect_err("自造角色必须致命");
    assert!(e.contains("角色 act.axis.magical 未在册"), "{e}");
}

/// 校验①-b per_instance 两侧：true 角色缺 instance / false 角色填 instance 都红
#[test]
fn pin_per_instance_rules_are_fatal_both_sides() {
    let root = Root::new("pin-perinst");
    // act.axis.pwm per_instance=true，删 instance → 必填缺
    let text = GOOD.replace("instance = \"pan\"\n", "");
    let p = root.write_profile("perinst-missing.toml", &text);
    let e = boot_inputs(&p).expect_err("per_instance=true 缺 instance 必须致命");
    assert!(e.contains("instance 必填"), "{e}");

    // act.led.gpio per_instance=false，填 instance → 禁填
    let text = format!(
        "{GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ninstance = \"x\"\n\
         controller = \"gpiochip0\"\nphysical = [4]\ndirection = \"out\"\nfailsafe = \"hold\"\n"
    );
    let p = root.write_profile("perinst-extra.toml", &text);
    let e = boot_inputs(&p).expect_err("per_instance=false 填 instance 必须致命");
    assert!(e.contains("instance 禁填"), "{e}");
}

/// 校验①-b 留空反例（§5 缺失语义）：requires 必填字段删键即红——channel 以
/// None = 缺、failsafe 以空串 = 缺，一次收集、逐条点名，一条不静默放行
#[test]
fn pin_requires_left_blank_is_fatal() {
    let root = Root::new("pin-blank");
    let text = GOOD
        .replace("channel = 0\n", "")
        .replace("failsafe = \"release\"\n", "");
    let p = root.write_profile("pin-blank.toml", &text);
    let e = boot_inputs(&p).expect_err("requires 字段留空必须致命");
    assert!(e.contains("缺 channel"), "{e}");
    assert!(e.contains("缺 failsafe"), "{e}");
}

/// 校验① direction 相等：pin.direction 与角色 direction 各说一套 → 致命点名
#[test]
fn pin_direction_mismatch_is_fatal() {
    let root = Root::new("pin-direction");
    let text = GOOD.replace("direction = \"out\"", "direction = \"in\"");
    let p = root.write_profile("pin-direction.toml", &text);
    let e = boot_inputs(&p).expect_err("direction 与角色不一致必须致命");
    assert!(
        e.contains("direction = in 与角色 act.axis.pwm direction = out 不一致"),
        "{e}"
    );
}

/// [[pin]] 形状：未知键/坏类型/缺身份键逐条点名，一次收集完
#[test]
fn pin_section_shape_errors_are_named() {
    let text =
        format!("{GOOD}\n[[pin]]\nrole = \"\"\ndirection = \"out\"\nwat = 1\nphysical = [\"x\"]\n");
    // GOOD 现役两条 pin，追加的坏 pin 序号 = #2
    let errs = rr3_profile_parse(&text).expect_err("形状不合必须报错");
    assert!(
        errs.iter().any(|e| e.contains("pin #2.role 缺失或为空")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("pin #2 未知键 wat")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("pin #2.physical 的每一项都必须是整数")),
        "{errs:?}"
    );
    // 缺 role / 缺 direction（身份键缺席，解析期即红）
    let errs = rr3_profile_parse(format!("{GOOD}\n[[pin]]\ncontroller = \"pwm1\"\n").as_str())
        .expect_err("缺 role/direction 必须报错");
    assert!(
        errs.iter().any(|e| e.contains("pin #2 缺 role")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("pin #2 缺 direction")),
        "{errs:?}"
    );
}

/// 校验⑥-a/⑥-b 反例（§18 F-5 的自造域名 + 功率面挂 none）：未在册 rail 与
/// kind=power 而 rail=none 都必须拒 boot 并点名
#[test]
fn pin_rail_domain_gate_is_fatal() {
    let root = Root::new("rail-gate");
    // ⑥-a：自造域名（F-5：rail24 不在任何一处在册集——文档示例值会被装机档抄）
    let text = GOOD.replace("rail = \"rail5v\"", "rail = \"rail24\"");
    let p = root.write_profile("rail-unknown.toml", &text);
    let e = boot_inputs(&p).expect_err("自造域名必须致命");
    assert!(
        e.contains("rail = rail24 未在 [[supplies.domain]] 在册"),
        "{e}"
    );

    // ⑥-b：kind=power 角色挂 none（act.axis.pwm 是 power，none 是 signal 侧显式值）
    let text = GOOD.replace("rail = \"rail5v\"", "rail = \"none\"");
    let p = root.write_profile("rail-none-power.toml", &text);
    let e = boot_inputs(&p).expect_err("功率面挂 none 必须致命");
    assert!(e.contains("kind = power"), "{e}");
}

/// 校验⑥-c 反例：armed=true 而 sense=none / max_age_ms=0——未标定即不武装，
/// 无测点/无龄期上界的域不得武装拒动（§3.1 降级语义 + §19 Y-4）。
/// GOOD 的 batt 域恰是 sense=none + max_age_ms=0 的最小反例，armed 一开即双红。
#[test]
fn domain_armed_without_sense_is_fatal() {
    let root = Root::new("armed-gate");
    let text = GOOD.replace(
        "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "switchable = false\narmed = true\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("armed.toml", &text);
    let e = boot_inputs(&p).expect_err("武装而无测点/龄期必须致命");
    assert!(e.contains("校验⑥-c"), "{e}");
}

/// 校验⑥-d 反例：域绑定位写第二套语法（F-2 的 adc:gp34）——不在 pin_id 集
/// 即红，"一律写 pin_id，不再各造语法"
#[test]
fn domain_binding_uses_pin_id_not_adhoc_syntax() {
    let root = Root::new("pid-gate");
    let text = GOOD.replace(
        "sense = \"none\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "sense = \"adc:gp34\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("pid.toml", &text);
    let e = boot_inputs(&p).expect_err("第二套语法必须致命");
    assert!(e.contains("不在 [pins] 的 pin_id 集"), "{e}");
    assert!(e.contains("sense = adc:gp34"), "{e}");
}

/// 校验⑥-e 形状反例一（批N21 件二）：armed=true 而 evidence = none——武装
/// 拒动必须引用一条真板实测证据（规则住 conventions §6.1），"none" 即未
/// 标定，须 armed = false。
#[test]
fn domain_armed_without_evidence_is_fatal() {
    let root = Root::new("evid-none-gate");
    let text = GOOD.replace(
        "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "switchable = false\narmed = true\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("evid-none.toml", &text);
    let e = boot_inputs(&p).expect_err("武装而无证据引用必须致命");
    assert!(e.contains("校验⑥-e"), "{e}");
    assert!(e.contains("evidence = none"), "{e}");
}

/// 校验⑥-e 形状反例二（批N21 件二）：引用形制坏（三位编号）——
/// `<形态>/<四位编号>` 是唯一词形（机制真值 rr-ipc::evidence），装载即拒。
#[test]
fn domain_armed_with_malformed_evidence_ref_is_fatal() {
    let root = Root::new("evid-form-gate");
    let text = GOOD.replace(
        "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "switchable = false\narmed = true\nevidence = \"实测/1\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("evid-form.toml", &text);
    let e = boot_inputs(&p).expect_err("形制坏的引用必须致命");
    assert!(e.contains("形制不合法"), "{e}");
}

/// 校验⑥-e 成对反例三（批N21 件二）：armed=false 却携引用——证据位与
/// 武装位成对（同 switch/switchable 范式），未武装显式写 "none"，不留过期引用。
#[test]
fn domain_disarmed_with_evidence_ref_is_fatal() {
    let root = Root::new("evid-pair-gate");
    let text = GOOD.replace(
        "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "switchable = false\narmed = false\nevidence = \"实测/0001\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("evid-pair.toml", &text);
    let e = boot_inputs(&p).expect_err("未武装携证据必须致命");
    assert!(e.contains("未武装不携证据"), "{e}");
}

/// 校验⑥-e 文件半边（批N21 件二，boot 装载链持有容器根调用）：armed=true 的
/// 合法形制引用，证据文件缺席即红（fail-closed，容器缺不静默放行）；同一
/// 装配单落一份六键齐且 form 与目录一致的证据文件后 boot 过＝绿例——D11
/// 界面门的装载期半边全链打通。绿例顺带钉死 ⑥-c 的武装前提（有测点＋龄期
/// 上界 > 0），证据门不孤立于武装门。
#[test]
fn armed_domain_evidence_file_check_gate() {
    let root = Root::new("evid-file-gate");
    let text = GOOD.replace(
        "sense = \"none\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "sense = \"power.charger.meter\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = true\nevidence = \"实测/0001\"\nmax_age_ms = 30000\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("evid-file.toml", &text);

    // 红例：容器缺——armed 域的引用查无文件即拒
    let e = boot_inputs(&p).expect_err("证据文件缺席必须致命");
    assert!(e.contains("校验⑥-e"), "{e}");
    assert!(e.contains("读不到"), "{e}");

    // 绿例：容器里落一份形制合法的证据文件（frontmatter 六键齐，form 与
    // 形态目录名一致）→ boot 过
    let ev = root.dir.join("docs/evidence/实测/0001.md");
    std::fs::create_dir_all(ev.parent().expect("证据路径必有父目录")).expect("建证据容器");
    std::fs::write(
        &ev,
        concat!(
            "+++\n",
            "form = \"实测\"\n",
            "slot_or_role = \"board.firmware\"\n",
            "method = \"真板实测\"\n",
            "conclusion = \"域武装依据成立\"\n",
            "date = \"2026-10-23\"\n",
            "recorder = \"lei_huang\"\n",
            "+++\n\n正文。\n",
        ),
    )
    .expect("写证据文件");
    boot_inputs(&p).expect("证据文件四道核全过后 boot 必须过");
}

/// 仓内证据容器锁定（批N21 件二，首批真档形制回归）：rrd 装载面消费的容器
/// 根与仓内 `docs/evidence` 是同一处——首批两份证据必须四道核全过，真档
/// 形制漂移即刻在本仓测试红。
#[test]
fn repo_evidence_container_frontline_files_verify() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/evidence");
    rr_ipc::evidence::rr3_evidence_verify(&root, "实测/0001").expect("0001 必须是合法证据");
    rr_ipc::evidence::rr3_evidence_verify(&root, "实测/0002").expect("0002 必须是合法证据");
}

/// [[supplies.domain]] 形状门（解析层）：缺键逐键点名、未知键点名、
/// 缺整个域数组点名——一次收集完，运维一次改对
#[test]
fn domain_section_shape_errors_are_named() {
    // 缺整个域数组（schema 3 起必填）：取 GOOD 的 [supplies] 段头做最小文本
    let minimal_head = GOOD.split("[[supplies.domain]]").next().unwrap_or(GOOD);
    let errs = rr3_profile_parse(minimal_head).expect_err("缺域数组必须报错");
    assert!(
        errs.iter().any(|e| e.contains("缺少 [[supplies.domain]]")),
        "{errs:?}"
    );

    // 域条目缺键 + 未知键逐条点名
    let errs = rr3_profile_parse(&format!(
        "{minimal_head}[[supplies.domain]]\nname = \"batt\"\nnominal_mv = 3700\nghost = 1\n"
    ))
    .expect_err("缺键/未知键必须报错");
    for want in [
        "supplies.domains[0].mild_mv",
        "supplies.domains[0].severe_mv",
        "supplies.domains[0].current_cap_ma",
        "supplies.domains[0].sense",
        "supplies.domains[0].current_sense",
        "supplies.domains[0].switch",
        "supplies.domains[0].switchable",
        "supplies.domains[0].armed",
        "supplies.domains[0].evidence",
        "supplies.domains[0].max_age_ms",
        "supplies.domains[0] 未知键 ghost",
    ] {
        assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
    }
}
