//! 表的只读视图（平台名、控制器与冲突集查询）。

use super::*;

impl Rr3PlatformTable {
    pub fn rr3_platform_name(&self) -> &str {
        &self.platform
    }

    /// 在册控制器名（升序，BTreeMap 序）
    pub fn rr3_ctl_names(&self) -> Vec<String> {
        self.controllers.keys().cloned().collect()
    }

    /// 控制器通道上界（③-b 判 (controller, channel) 不双占的分母；0 = 无通道维）
    pub fn rr3_ctl_channels(&self, name: &str) -> Option<i64> {
        self.controllers.get(name).map(|e| e.channels)
    }

    /// ⑤-b 判据源：模拟测点必须落此集
    pub fn rr3_adc_pins(&self) -> &BTreeSet<i64> {
        &self.capabilities.adc_pins
    }

    /// ⑤-a 判据源：direction=out 且 input_only_ok=false 的 physical 不得落此集
    pub fn rr3_input_only_pins(&self) -> &BTreeSet<i64> {
        &self.capabilities.input_only_pins
    }

    pub fn rr3_no_open_drain(&self) -> &BTreeSet<i64> {
        &self.capabilities.no_open_drain
    }

    /// ④-b 判据源：冲突矩阵（每条 = 一组互斥成员 + 组名 + 可读理由）
    pub fn rr3_conflicts(&self) -> &[Rr3ConflictEntry] {
        &self.conflicts
    }

    /// ⑧-a 判定：三元 (controller, role, instance) 是否有具名构造（非 per_instance
    /// 角色的 instance 传空串，与数据文件"第三段留空"同形）
    pub fn rr3_ctor_contains(&self, controller: &str, role: &str, instance: &str) -> bool {
        self.construction
            .contains_key(&format!("{controller}:{role}:{instance}"))
    }

    /// ⑧ 对账反向用：某三元键的具名构造路径
    pub fn rr3_ctor_get(&self, key: &str) -> Option<&str> {
        self.construction.get(key).map(String::as_str)
    }

    pub fn rr3_ctor_keys(&self) -> Vec<String> {
        self.construction.keys().cloned().collect()
    }

    /// host 词形前缀查询（实-6 (a)，批N4 件七）：派生键 → 控制器名前缀。查不到 =
    /// 本平台无该键词形（空表或未录）——rrd 派生器拿 None 必须 fail-closed 红，
    /// 不许回退硬编码前缀（这正是下移的纪律面）。
    pub fn rr3_wordform(&self, key: &str) -> Option<&str> {
        self.controller_wordform.get(key).map(String::as_str)
    }

    /// 词形表全量（BTreeMap 升序引用）——装载期 ①-f 词形漂移判据遍历用：表里
    /// 有什么判什么，rrd 零硬编码词形清单（新增词形键 = 改数据，判据自动扩）。
    pub fn rr3_wordform_map(&self) -> &BTreeMap<String, String> {
        &self.controller_wordform
    }
}

impl Rr3ConflictEntry {
    pub fn rr3_conflict_group(&self) -> &str {
        &self.group
    }

    pub fn rr3_conflict_pins(&self) -> &BTreeSet<i64> {
        &self.pins
    }

    pub fn rr3_conflict_ctls(&self) -> &BTreeSet<String> {
        &self.controllers
    }

    pub fn rr3_conflict_why(&self) -> &str {
        &self.why
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // include_str! 以本文件所在目录为基准：src → rr-registry → runtime → base → 仓库根
    const ESP32: &str = include_str!("../../../../contracts/platform/esp32.toml");
    const ESP32S3: &str = include_str!("../../../../contracts/platform/esp32s3.toml");
    const RK3566: &str = include_str!("../../../../contracts/platform/rk3566.toml");
    const RPI4: &str = include_str!("../../../../contracts/platform/raspberry-pi-4.toml");

    /// 四份在册平台表全部可解析（演练两份的"空集显式"是裁定②的形状，不是缺段）
    #[test]
    fn contract_tables_parse() {
        let esp = rr3_plat_parse(ESP32).expect("esp32 表须可解析");
        assert_eq!(esp.rr3_platform_name(), "esp32");
        let s3 = rr3_plat_parse(ESP32S3).expect("esp32s3 表须可解析");
        assert_eq!(s3.rr3_platform_name(), "esp32s3");
        let rk = rr3_plat_parse(RK3566).expect("rk3566 表须可解析");
        assert_eq!(rk.rr3_platform_name(), "rk3566");
        let rpi = rr3_plat_parse(RPI4).expect("rpi4 表须可解析");
        assert_eq!(rpi.rr3_platform_name(), "raspberry-pi-4");
    }

    /// esp32 表形状 pin（与 board.rs 引脚真值段同源；改数据须同批改这里）
    #[test]
    fn esp32_shape_is_pinned() {
        let esp = rr3_plat_parse(ESP32).expect("esp32 表须可解析");
        assert_eq!(
            esp.rr3_ctl_names(),
            vec!["adc1", "gpio0", "i2c0", "ledc0", "uart2", "vspi"]
        );
        assert_eq!(esp.rr3_ctl_channels("ledc0"), Some(16));
        assert_eq!(esp.rr3_ctl_channels("gpio0"), Some(0));
        // ⑤-b：GPIO34（ADC_RAIL_PIN）在 ADC 集。GPIO32/33 双职能（LEDC ch0/ch1 +
        // ADC1_CH4/CH5，ESP32 IO 矩阵）——芯片事实两集同在，"现役占用是否冲突"
        // 由绑定/构造对账判（⑧），能力面不裁现役。
        assert!(esp.rr3_adc_pins().contains(&34));
        assert!(esp.rr3_adc_pins().contains(&32));
        // ⑤-a：GPIO34 input-only；GPIO32 可输出（LEDC ch0 在用）
        assert!(esp.rr3_input_only_pins().contains(&34));
        assert!(!esp.rr3_input_only_pins().contains(&32));
        // 实-6 (a)：esp32 为 rtos 档，词形派生面空表显式（非缺段——缺段在
        // missing_sections_and_keys_are_named 反例判）
        assert!(esp.rr3_wordform_map().is_empty());
        // ④-b：I2C0 硬线 {21,22} 与 UART2 {18,19} 两组互斥集
        assert!(esp
            .rr3_conflicts()
            .iter()
            .any(|c| c.rr3_conflict_group() == "i2c0"
                && c.rr3_conflict_pins() == &BTreeSet::from([21, 22])));
        assert!(esp
            .rr3_conflicts()
            .iter()
            .any(|c| c.rr3_conflict_group() == "uart2"
                && c.rr3_conflict_pins() == &BTreeSet::from([18, 19])));
        // ⑧-a：板侧现役两个构造三元键（GPIO32 pan / GPIO33 tilt）
        assert!(esp.rr3_ctor_contains("ledc0", "act.axis.pwm", "pan"));
        assert!(esp.rr3_ctor_contains("ledc0", "act.axis.pwm", "tilt"));
        assert_eq!(esp.rr3_ctor_get("ledc0:act.axis.pwm:pan"), Some("ledc"));
    }

    /// 演练两份的形状 pin：空集显式 + 构造表与现役 profile 对账
    #[test]
    fn rehearsal_shapes_are_pinned() {
        let rk = rr3_plat_parse(RK3566).expect("rk3566 表须可解析");
        assert!(rk.rr3_adc_pins().is_empty());
        assert!(rk.rr3_conflicts().is_empty());
        // default.toml [pins] 现役四绑定逐条有构造
        assert!(rk.rr3_ctor_contains("pwm0", "act.axis.pwm", "pan"));
        assert!(rk.rr3_ctor_contains("gpiochip0", "act.led.gpio", ""));
        assert!(rk.rr3_ctor_contains("gpiochip0", "act.relay.gpio", ""));
        assert!(rk.rr3_ctor_contains("i2c1", "power.battery.meter", ""));
        let rpi = rr3_plat_parse(RPI4).expect("rpi4 表须可解析");
        assert!(rpi.rr3_adc_pins().is_empty());
        assert!(rpi.rr3_conflicts().is_empty());
        // W4/P0-C 回填（批N43）：Pi-as-board 双轴舵机两枚构造（pwmchip0 通道 0/1）
        assert!(rpi.rr3_ctor_contains("pwm0", "act.axis.pwm", "pan"));
        assert!(rpi.rr3_ctor_contains("pwm0", "act.axis.pwm", "tilt"));
        assert_eq!(
            rpi.rr3_ctor_get("pwm0:act.axis.pwm:tilt"),
            Some("sysfs-pwm")
        );
        assert_eq!(rpi.rr3_ctl_channels("pwm0"), Some(2));
        assert!(rpi.rr3_ctl_names().contains(&"spidev0".to_string()));
        assert_eq!(rpi.rr3_ctl_channels("spidev0"), Some(1));
        // 实-6 (a)：host 形态两份词形派生表同形同值（Linux sysfs 命名习惯）——
        // rr3 派生器的词形前件真值点，四键封闭
        for t in [rk, rpi] {
            assert_eq!(
                t.rr3_wordform_map(),
                &BTreeMap::from([
                    ("pwm_chip".to_string(), "pwm".to_string()),
                    ("gpio_chip".to_string(), "gpiochip".to_string()),
                    ("i2c_bus".to_string(), "i2c".to_string()),
                    ("spi_device".to_string(), "spidev".to_string()),
                ])
            );
        }
    }

    /// 缺失语义（裁定①）：最小文本一次点名全部五段；单键缺与段缺同红
    #[test]
    fn missing_sections_and_keys_are_named() {
        // 五段全缺的最小文本：controller / capabilities / conflict / construction /
        // controller_wordform 各点名一条
        let errs =
            rr3_plat_parse("schema_version = 1\nplatform = \"t\"\n").expect_err("缺五段必须红");
        for needle in [
            "controller",
            "capabilities",
            "conflict",
            "construction",
            "controller_wordform",
        ] {
            assert!(
                errs.iter().any(|e| e.contains(needle)),
                "{needle}: {errs:?}"
            );
        }

        // 缺 [capabilities]（施工图 Task 3 Step 3 反例原文形状）
        let no_caps = ESP32.replace("[capabilities]", "[capabilities_typo]");
        let errs = rr3_plat_parse(&no_caps).expect_err("缺 [capabilities] 须红");
        assert!(errs.iter().any(|e| e.contains("capabilities")), "{errs:?}");

        // [capabilities] 缺单键：显式空集的纪律——缺键 ≠ 空
        let no_adc = ESP32.replace("adc_pins = [32, 33, 34, 35, 36, 39]", "");
        let errs = rr3_plat_parse(&no_adc).expect_err("缺 adc_pins 须红");
        assert!(errs.iter().any(|e| e.contains("adc_pins")), "{errs:?}");
    }

    /// 形状坏逐条点名：kind 出域 / 构造键非三元 / 构造 controller 不在册 /
    /// 冲突集双空 / 组名重复 / output 规则不在册 / 脚号负数
    #[test]
    fn bad_shapes_are_named() {
        let bad_kind = ESP32.replace("kind = \"pwm\"", "kind = \"dma\"");
        let errs = rr3_plat_parse(&bad_kind).expect_err("kind 出域须红");
        assert!(
            errs.iter().any(|e| e.contains("kind") && e.contains("dma")),
            "{errs:?}"
        );

        let bad_key = ESP32.replace(
            "\"ledc0:act.axis.pwm:pan\" = \"ledc\"",
            "\"ledc0.act.axis.pwm.pan\" = \"ledc\"",
        );
        let errs = rr3_plat_parse(&bad_key).expect_err("构造键非三元须红");
        assert!(errs.iter().any(|e| e.contains("三元串")), "{errs:?}");

        let bad_ctl = ESP32.replace(
            "\"ledc0:act.axis.pwm:pan\" = \"ledc\"",
            "\"ledc9:act.axis.pwm:pan\" = \"ledc\"",
        );
        let errs = rr3_plat_parse(&bad_ctl).expect_err("构造 controller 不在册须红");
        assert!(errs.iter().any(|e| e.contains("ledc9")), "{errs:?}");

        let empty_conflict = ESP32.replace(
            "pins = [21, 22]\ngroup = \"i2c0\"",
            "pins = []\ngroup = \"i2c0\"",
        );
        let errs = rr3_plat_parse(&empty_conflict).expect_err("冲突集双空须红");
        assert!(errs.iter().any(|e| e.contains("无判据对象")), "{errs:?}");

        let dup_group = ESP32.replace("group = \"uart2\"", "group = \"i2c0\"");
        let errs = rr3_plat_parse(&dup_group).expect_err("组名重复须红");
        assert!(errs.iter().any(|e| e.contains("重复")), "{errs:?}");

        let bad_rule = ESP32.replace(
            "output_capable_pins = \"complement(input_only_pins)\"",
            "output_capable_pins = \"everything\"",
        );
        let errs = rr3_plat_parse(&bad_rule).expect_err("output 规则不在册须红");
        assert!(errs.iter().any(|e| e.contains("不在册")), "{errs:?}");

        let neg_pin = ESP32.replace(
            "input_only_pins = [34, 35, 36, 39]",
            "input_only_pins = [-1]",
        );
        let errs = rr3_plat_parse(&neg_pin).expect_err("负脚号须红");
        assert!(errs.iter().any(|e| e.contains("非负")), "{errs:?}");

        // 实-6 (a) 词形段三反例：键出域 / 值空串 / 段缺（esp32 空表换段名即段缺）
        let bad_wf_key = ESP32.replace(
            "[controller_wordform]",
            "[controller_wordform]\ngpio_line = \"gpio\"",
        );
        let errs = rr3_plat_parse(&bad_wf_key).expect_err("词形键出域须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("controller_wordform") && e.contains("gpio_line")),
            "{errs:?}"
        );

        let empty_wf_val = RPI4.replace("pwm_chip = \"pwm\"", "pwm_chip = \"\"");
        let errs = rr3_plat_parse(&empty_wf_val).expect_err("词形前缀空串须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("pwm_chip") && e.contains("不得为空串")),
            "{errs:?}"
        );

        let no_wf = RK3566.replace("[controller_wordform]", "[controller_wordform_x]");
        let errs = rr3_plat_parse(&no_wf).expect_err("词形段缺须红");
        assert!(
            errs.iter().any(|e| e.contains("缺 [controller_wordform]")),
            "{errs:?}"
        );
        // 顶层未知键点名同步照出（改名后的 controller_wordform_x 是顶层未知键）
        assert!(errs.iter().any(|e| e.contains("顶层未知键")), "{errs:?}");
    }
}
