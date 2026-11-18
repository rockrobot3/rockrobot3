//! 装配在册表 pin：handle traits、描述符键与注册表键集同批。

use super::*;

/// G4 在册表钉死（ADR-0019 修订 1 ③，批J）：九能力面 trait × 描述符键逐项点名。
/// 两表真值源 = rr-hal-api（trait 面）/ rr-hal descriptor.rs（[hardware] 键集）；
/// 本测试钉的是装配校验在 rr-registry 的这份副本——扩 trait/扩键都走契约级评审，
/// 届时两处同批改，单改一处此测试当场红。
#[test]
fn assemble_registry_tables_are_pinned() {
    assert_eq!(
        rr_registry::RR3_ASSEMBLE_HANDLE_TRAITS,
        &[
            "Rr3GpioOut",
            "Rr3PwmServo",
            "Rr3AudioSource",
            "Rr3AudioSink",
            "Rr3Power",
            "Rr3Link",
            "Rr3Display",
            "Rr3Sensor",
            "Rr3DigitalIn",
        ]
    );
    assert_eq!(
        rr_registry::RR3_ASSEMBLE_DESC_KEYS,
        &[
            "alsa_device",
            "pwm_chip",
            "pwm_channel",
            "gpio_chip",
            "gpio_led_line",
            "gpio_relay_line",
            "i2c_bus",
            "ina219_addr",
            "link_ifname",
            "spi_device",
            // P0-3/P0-4：ESP32 外设桥 UART 设备路径（组合档宿主侧真值，描述符 [hardware].periph_uart）
            "periph_uart",
            // 批N4 件七(b) 基-24 桥上配置腿：板侧配置三键（板侧裸寄存器自愈路径
            // 引脚真值，无 role 对应故不在 [pins] 派生键集，rtos 档 [hardware] 直给）
            "i2c_sda",
            "i2c_scl",
            "adc_rail_pin",
            // 批N9 件五（MC-7 缺③进位补齐）：板侧主驱动七键（外设引脚真值，消费
            // 语义 = 主驱动装配对账锚，同上不在 [pins] 派生键集）
            "uart2_tx",
            "uart2_rx",
            "ledc_pan",
            "ledc_tilt",
            "i2s_bclk",
            "i2s_sd",
            "i2s_ws",
            // 批N17 件三（N-10 ② 键半边）：凭据存储源标记（非 GPIO，板侧读数面
            // 可见；boot 读链不消费＝批N14 架构事实，非引脚不进对账锚）
            "cred_store",
            // 批B62：INA219 分流校准两键（分流电阻毫欧 / 电流 LSB 微安，无 role
            // 对应故不在 [pins] 派生键集，[hardware] 直给）
            "ina219_shunt_mohm",
            "ina219_ilsb_ua",
        ]
    );
}

/// G4 仓扫描（ADR-0019 修订 1 ③，批J）：modules/ 下每个 atoms/*/assemble.toml
/// 都必须过形状级真值（schema v2 / 在册表 / 未知字段点名），且 ops 声明在模块
/// config.toml 上无注入缺——真值数据（battery/charger/bt 三份）与 config 漂移
/// 由此测试当场拦下。数量钉 17：新增件须走 rr3-mkmod 五件套，删除须走 ADR。
#[test]
fn repo_assemble_manifests_validate_and_injection_complete() {
    let modules = repo_root().join("modules");
    let mut mnames: Vec<PathBuf> = std::fs::read_dir(&modules)
        .expect("读不到 modules/ 目录")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    mnames.sort();
    let mut count = 0usize;
    for m in &mnames {
        let atoms = m.join("atoms");
        let Ok(entries) = std::fs::read_dir(&atoms) else {
            continue; // 无原子件目录的模块形态（两件式豁免同款边界）
        };
        let mut anames: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        anames.sort();
        for atom in &anames {
            let path = atom.join("assemble.toml");
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} 读不到: {e}（五件套④缺件）", path.display()));
            let facts = rr_registry::rr3_assemble_parse(&text)
                .unwrap_or_else(|errs| panic!("{} 坏形: {errs:?}", path.display()));
            let cfg = std::fs::read_to_string(m.join("config.toml")).unwrap_or_else(|e| {
                panic!(
                    "{} 缺 config.toml: {e}（模块开机即读，随包必带）",
                    m.display()
                )
            });
            let gaps = rr_registry::rr3_injection_gaps(&facts, &cfg);
            assert!(
                gaps.is_empty(),
                "{} 注入缺: {gaps:?}（config 键与装配声明漂移）",
                path.display()
            );
            count += 1;
        }
    }
    assert_eq!(
        count, 17,
        "装配清单数量漂移：新增须走 rr3-mkmod 五件套，删除须走 ADR"
    );
}

/// 平台登记表三份真文件可解析 + 关键形状钉死（基-22：校验 ④⑤⑧ 的判据前件；
/// 细 pin 在 rr-registry/src/platform.rs mod tests，此层做 CI ②③ 的同源复核——
/// 任一侧漂移即红）。演练两份的"空集显式"是裁定②的形状，不是缺段。
#[test]
fn platform_tables_parse_and_shape_is_pinned() {
    let esp = rr_registry::rr3_plat_parse(
        &std::fs::read_to_string(repo_root().join("contracts/platform/esp32.toml"))
            .expect("contracts/platform/esp32.toml 必须存在（批N1 基-22 产物）"),
    )
    .expect("esp32 平台表须可解析");
    assert_eq!(esp.rr3_platform_name(), "esp32");
    assert_eq!(
        esp.rr3_ctl_names(),
        vec!["adc1", "gpio0", "i2c0", "ledc0", "uart2", "vspi"]
    );
    assert!(esp.rr3_adc_pins().contains(&34));
    assert!(esp.rr3_input_only_pins().contains(&34));
    assert!(esp
        .rr3_conflicts()
        .iter()
        .any(|c| c.rr3_conflict_group() == "i2c0"));
    assert!(esp.rr3_ctor_contains("ledc0", "act.axis.pwm", "pan"));

    let s3 = rr_registry::rr3_plat_parse(
        &std::fs::read_to_string(repo_root().join("contracts/platform/esp32s3.toml"))
            .expect("contracts/platform/esp32s3.toml 必须存在（W1/T2）"),
    )
    .expect("esp32s3 平台表须可解析");
    assert_eq!(s3.rr3_platform_name(), "esp32s3");
    assert_eq!(
        s3.rr3_ctl_names(),
        vec!["adc1", "gpio0", "i2c0", "ledc0", "uart2"]
    );
    assert_eq!(s3.rr3_ctl_channels("ledc0"), Some(8));
    assert_eq!(s3.rr3_ctl_channels("adc1"), Some(10));
    assert!(s3.rr3_adc_pins().contains(&1));
    assert!(s3.rr3_input_only_pins().is_empty());
    assert!(s3
        .rr3_conflicts()
        .iter()
        .any(|c| c.rr3_conflict_group() == "i2c0"));
    assert!(s3
        .rr3_conflicts()
        .iter()
        .any(|c| c.rr3_conflict_group() == "uart2"));
    assert!(s3.rr3_ctor_contains("ledc0", "act.axis.pwm", "pan"));
    assert!(s3.rr3_ctor_contains("ledc0", "act.axis.pwm", "tilt"));
    assert!(s3.rr3_wordform_map().is_empty());

    let rk = rr_registry::rr3_plat_parse(
        &std::fs::read_to_string(repo_root().join("contracts/platform/rk3566.toml"))
            .expect("contracts/platform/rk3566.toml 必须存在（演练实例，取值待器件核对）"),
    )
    .expect("rk3566 平台表须可解析");
    assert_eq!(rk.rr3_platform_name(), "rk3566");
    assert!(rk.rr3_adc_pins().is_empty());
    assert!(rk.rr3_conflicts().is_empty());
    assert!(rk.rr3_ctor_contains("pwm0", "act.axis.pwm", "pan"));
    assert!(rk.rr3_ctor_contains("i2c1", "power.battery.meter", ""));

    let rpi = rr_registry::rr3_plat_parse(
        &std::fs::read_to_string(repo_root().join("contracts/platform/raspberry-pi-4.toml"))
            .expect("contracts/platform/raspberry-pi-4.toml 必须存在（演练实例，待真板对账）"),
    )
    .expect("rpi4 平台表须可解析");
    assert_eq!(rpi.rr3_platform_name(), "raspberry-pi-4");
    assert!(rpi.rr3_adc_pins().is_empty());
    assert!(rpi.rr3_conflicts().is_empty());
    // W4/P0-C 回填（批N43）：Pi-as-board 双轴舵机的具名构造两枚（pwm0 通道 0/1）。
    assert!(rpi.rr3_ctor_contains("pwm0", "act.axis.pwm", "pan"));
    assert!(rpi.rr3_ctor_contains("pwm0", "act.axis.pwm", "tilt"));
    assert_eq!(rpi.rr3_ctor_keys().len(), 2);
    assert_eq!(rpi.rr3_ctl_channels("pwm0"), Some(2));
}

/// 六份组合件清单真文件：申报面定形 pin（N-11，§8 门 j 前件）——schema 位（组合件
/// 面版本域 = 1，与件级 2 同名不同域）、name 与目录名同源、atoms ⊗ 链集合、空面
/// 明写 no_injection；外加三枚合成反例钉死判据本身。
/// 2026-10-19 批N4 件五对表：rr3-interact 组合件级 ops 七槽首启用（表情节拍＋自检
/// 期望表情＋面板五限额，消费点在组合件面），不再是零注入面——单独具名 pin 槽集；
/// 其余五份维持零注入面断言不变。
#[test]
fn composite_declaration_face_is_pinned() {
    let expected: &[(&str, &[&str])] = &[
        ("rr3-act", &["servo", "led", "relay"]),
        ("rr3-brain", &["dialog", "asr", "llm", "tts"]),
        ("rr3-connect", &["wifi", "bt"]),
        ("rr3-interact", &["face", "player"]),
        ("rr3-power", &["battery", "charger"]),
        ("rr3-sense", &["capture", "vad", "env", "imu"]),
    ];
    for (name, atoms) in expected {
        let path = repo_root().join(format!("modules/{name}/composite.toml"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} 读不到: {e}", path.display()));
        let facts = rr_registry::rr3_composite_parse(&text)
            .unwrap_or_else(|errs| panic!("{} 坏形: {errs:?}", path.display()));
        assert_eq!(facts.name, *name, "{}: name 与目录名不同源", path.display());
        assert_eq!(facts.unit, "process");
        assert_eq!(
            facts.atoms,
            *atoms,
            "{}: atoms ⊗ 链漂移（增删原子件须走 ADR 并同批改本 pin）",
            path.display()
        );
        if *name == "rr3-interact" {
            // 批N4 件五：组合件级 ops 七槽首启用（I-17/I-18②），句柄仍零、
            // no_injection 不再写——增删槽须同批改本 pin 与门 d 分母基线。
            assert!(
                facts.handles.is_empty() && !facts.no_injection,
                "{}: 组合件申报面漂移（rr3-interact 非零注入面：七 ops 槽在册）",
                path.display()
            );
            let slots: Vec<&str> = facts.ops.iter().map(|o| o.slot.as_str()).collect();
            assert_eq!(
                slots,
                [
                    "face_tick_ms",
                    "selftest_face_expect",
                    "panel_max_conns",
                    "panel_read_timeout_ms",
                    "panel_header_limit",
                    "panel_body_limit",
                    "panel_call_timeout_ms",
                ],
                "{}: 组合件级 ops 七槽漂移（批N4 件五 I-17/I-18②，增删须同批改本 pin）",
                path.display()
            );
            continue;
        }
        assert!(
            facts.handles.is_empty() && facts.ops.is_empty() && facts.no_injection,
            "{}: 组合件申报面漂移（N-11：零注入面组合件必须显式申报）",
            path.display()
        );
    }

    let prologue =
        "schema_version = 1\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\n";
    // 反例 1：两数组均空却漏写 no_injection → L-5 红
    let errs = rr_registry::rr3_composite_parse(&format!("{prologue}handles = []\nops = []\n"))
        .expect_err("空面漏写声明必须红");
    assert!(
        errs.iter().any(|e| e.contains("未明写 no_injection")),
        "{errs:?}"
    );
    // 反例 2：unit 出域 → 红
    let errs = rr_registry::rr3_composite_parse(
        &prologue.replace("unit = \"process\"", "unit = \"thread\""),
    )
    .expect_err("unit 出域必须红");
    assert!(
        errs.iter()
            .any(|e| e.contains("unit") && e.contains("thread")),
        "{errs:?}"
    );
    // 反例 3：写件级域的版本值 2 → 红，且点名"同名不同域"（schema 位裁定）
    assert_eq!(rr_registry::RR3_COMPOSITE_SCHEMA_VERSION, 1);
    let errs = rr_registry::rr3_composite_parse(
        "schema_version = 2\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\nno_injection = true\n",
    )
    .expect_err("组合件面写件级版本值必须红");
    assert!(errs.iter().any(|e| e.contains("同名不同域")), "{errs:?}");
}

/// descriptor.rs 键集与 rr-registry 在册表"两处同批"机判（C4 撤销后的改判收口，
/// 全案 §9 N1 行："descriptor.rs 键集改判与 rr-registry 在册表同批"）。十键批
/// `spi_device` 与批N4 件七(b) 板侧三键 `i2c_sda`/`i2c_scl`/`adc_rail_pin` 进场后，
/// rr-hal `Rr3HalBindings` 字段集与 RR3_ASSEMBLE_DESC_KEYS 必须逐名一致——单边扩键
/// 即红（在册表是 handles[].desc_key 的判等域，注入端键名不同源 = 装配申报被静默
/// 丢键）。rr-registry 不依赖 rr-hal，按契约测试惯例读真值文本扫描（与
/// roles/devices/platform 的 include_str! 同路）。
#[test]
fn descriptor_keys_and_registry_desc_keys_are_the_same_batch() {
    let desc = std::fs::read_to_string(repo_root().join("base/runtime/rr-hal/src/descriptor.rs"))
        .expect("rr-hal descriptor.rs 必须存在");
    for k in rr_registry::RR3_ASSEMBLE_DESC_KEYS.iter() {
        assert!(
            desc.contains(&format!("pub {k}: Option<")),
            "描述符键 {k} 在 descriptor.rs 的 Rr3HalBindings 无同名字段（两处同批被打破，扩键须契约级评审并同批）"
        );
    }
    // 反向：descriptor.rs 的 Option 字段总数 = 在册表长度（防 descriptor 单边多键）
    let field_count = desc
        .lines()
        .filter(|l| l.contains("pub ") && l.contains(": Option<"))
        .count();
    assert_eq!(
        field_count,
        rr_registry::RR3_ASSEMBLE_DESC_KEYS.len(),
        "descriptor.rs Rr3HalBindings 字段数（{field_count}）与在册表（{}）不一致——扩键两处同批",
        rr_registry::RR3_ASSEMBLE_DESC_KEYS.len()
    );
}
