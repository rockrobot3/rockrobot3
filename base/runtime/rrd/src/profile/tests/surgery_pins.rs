//! 格位缺席、平台词汇与引脚冲突判据单测（批B13 自 `surgery.rs` 按被试族切出）。
//!
//! 覆盖 `device_absent` 申报、platform 表缺档/空表、器件与角色词汇越界，以及
//! 引脚唯一性四类判据（同物理脚、同控制器通道、同总线地址、冲突集 mux 组）。

use super::*;

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
    let no_wf = platform_esp32_text().replace("[controller_wordform]", "[controller_wordform_x]");
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
    let e = rr3_profile_from_text_with_platform("host-empty-wf", &good_esp, platform_esp32_text())
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
