//! 装配单解析面单测（批B15 自 `tests.rs` 按被试族切出）。
//!
//! 覆盖 `rr3_profile_parse` 的正例、错误全收集与点名、缺段/空组、日志环上限、
//! 不可解析 TOML 的单错归一，以及整册解析后 manifest 解析与缺失面。

use super::*;

#[test]
fn good_profile_parses() {
    let p = rr3_profile_parse(GOOD).expect("应能解析");
    assert_eq!(p.bindings.len(), 3);
    assert_eq!(p.enabled_slots.len(), 3);
    assert_eq!(p.enabled_slots["rr3-pa"], vec!["act.servo".to_string()]);
    // 描述符八段（schema_version=4）：[platform]/[supplies]/[slots]/[[pin]] 逐字段收齐
    assert_eq!(p.platform_tier, "host");
    // 平台事实表外键（§4.6，校验③④⑤⑧判据前件选册）
    assert_eq!(p.platform_chip, "rk3566");
    assert!(p.supplies.watchdog_present);
    assert_eq!(p.supplies.watchdog_timeout_ms, 15000);
    assert_eq!(p.supplies.watchdog_feeder, "rrd");
    assert_eq!(p.supplies.tick_period_ms, 200);
    assert_eq!(p.supplies.tick_jitter_ms, 100);
    assert_eq!(p.supplies.safety_core_interval_ms, 1000);
    // [supplies.budget]（schema 4，校验⑩ 分母）：host 档显式不适用态收齐
    assert!(p.supplies.budget.not_applicable);
    assert_eq!(p.supplies.budget.task_stack_budget_bytes, 0);
    assert_eq!(p.supplies.budget.heap_total_bytes, 0);
    assert_eq!(p.supplies.budget.heap_reserved_bytes, 0);
    assert_eq!(
        p.slots_present,
        ["act.servo", "cognition.llm", "power.charger"]
    );
    assert!(
        p.slots_device_absent.is_empty(),
        "GOOD 无缺席登记 → 空 map（D3 合法态是可选键）"
    );
    // [[pin]]：逐字段收齐，pin_id 按 per_instance 语法拼装（device 批N3 件八进场，
    // GOOD 的 servo pin 无 I2C 面不填 = 空串；env pin 满配语义全字段收齐）
    assert_eq!(p.pins.len(), 2, "{:?}", p.pins);
    let pin = &p.pins[0];
    assert_eq!(pin.rr3_pin_id(), "act.axis.pwm.pan");
    assert_eq!(pin.controller, "pwm0");
    assert_eq!(pin.channel, Some(0));
    assert_eq!(pin.physical, vec![12]);
    assert_eq!(pin.direction, "out");
    assert_eq!(pin.rail, "rail5v");
    assert_eq!(pin.failsafe, "release");
    assert!(pin.mux_group.is_empty());
    assert_eq!(pin.device, "", "GOOD servo pin 无器件面，device = 空串");
    // 第二条 pin：power.charger.meter（I2C 满配，device 键非空）
    let env = &p.pins[1];
    assert_eq!(env.role, "power.charger.meter");
    assert_eq!(env.controller, "i2c1");
    assert_eq!(env.addr, "0x70");
    assert_eq!(env.device, "INA219");
    assert_eq!(env.direction, "in");
    // [os].route 被无视（运行期不读），但它的存在不构成错误
}

#[test]
fn parse_errors_are_all_collected_and_named() {
    let text = GOOD
        .replace("schema_version = 5", "schema_version = 9")
        .replace("\"act.servo\" = \"rr3-pa\"", "\"act.servo\" = \"\"")
        .replace("rr3-pb = [\"power.charger\"]", "rr3-pb = [\"\"]")
        .replace("rr3-pc = [\"cognition.llm\"]", "rr3-pc = 3");
    let errs = rr3_profile_parse(&text).expect_err("形状不合必须报错");
    assert_eq!(errs.len(), 4, "{errs:?}");
    assert!(
        errs.iter().any(|e| e.contains("schema_version")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("bindings.act.servo")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("enabled_slots.rr3-pb 的每一项")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("enabled_slots.rr3-pc 应为字符串数组")),
        "{errs:?}"
    );
}

#[test]
fn missing_sections_and_empty_enabled_slots_are_errors() {
    let errs = rr3_profile_parse("schema_version = 5\n").expect_err("缺段必须报错");
    assert!(
        errs.iter().any(|e| e.contains("缺少 [bindings]")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("缺少 [enabled_slots]")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("缺少 [runtime]")),
        "[runtime] 必填段缺也必须点名（schema 5 起，批N5 件二 F-30）: {errs:?}"
    );
    // [supplies] 段缺时 [supplies.budget] 的点名不级联（budget 错误只在
    // supplies 段在场时裁）——budget 段缺的留空反例归
    // budget_tier_consistency_errors_are_named。

    let errs = rr3_profile_parse("schema_version = 5\n[bindings]\n\n[enabled_slots]\n")
        .expect_err("空 enabled_slots 必须报错");
    assert!(
        errs.iter().any(|e| e.contains("[enabled_slots] 为空")),
        "{errs:?}"
    );
}

/// [runtime].log_ring_cap 判据（批N5 件二 F-30"装载期缺即红"）：键缺/段缺/零/负/
/// 类型错逐条点名；合法值如实收进结构体（换成 64 仍解析成功——证明容量是
/// 装配单数据不是解析器写死）。
#[test]
fn runtime_log_ring_cap_missing_or_nonpositive_is_red_and_value_flows() {
    // 段缺（GOOD 定点删除）
    let errs = rr3_profile_parse(&GOOD.replace("[runtime]\nlog_ring_cap = 512\n", ""))
        .expect_err("缺 [runtime] 必须报错");
    assert!(
        errs.iter().any(|e| e.contains("缺少 [runtime]")),
        "{errs:?}"
    );
    // 键缺
    let errs = rr3_profile_parse(&GOOD.replace("log_ring_cap = 512", ""))
        .expect_err("缺 log_ring_cap 必须报错");
    assert!(
        errs.iter()
            .any(|e| e.contains("runtime.log_ring_cap 缺失或不是整数")),
        "{errs:?}"
    );
    // 0 与负数（正整数判据）
    for bad in ["log_ring_cap = 0", "log_ring_cap = -8"] {
        let errs = rr3_profile_parse(&GOOD.replace("log_ring_cap = 512", bad))
            .expect_err("非正容量必须报错");
        assert!(
            errs.iter().any(|e| e.contains("须为正整数")),
            "{bad} 必须点名正整数判据: {errs:?}"
        );
    }
    // 类型错（字符串不是整数）
    let errs = rr3_profile_parse(&GOOD.replace("log_ring_cap = 512", "log_ring_cap = \"many\""))
        .expect_err("字符串容量必须报错");
    assert!(
        errs.iter()
            .any(|e| e.contains("runtime.log_ring_cap 缺失或不是整数")),
        "{errs:?}"
    );
    // 未知键点名
    let errs = rr3_profile_parse(&GOOD.replace("[runtime]", "[runtime]\nlog_level = \"debug\""))
        .expect_err("未知键必须报错");
    assert!(
        errs.iter()
            .any(|e| e.contains("[runtime] 未知键 log_level")),
        "{errs:?}"
    );
    // 合法值如实收进结构体（64 ≠ 编译期兜底 512——数据流证明）
    let p = rr3_profile_parse(&GOOD.replace("log_ring_cap = 512", "log_ring_cap = 64"))
        .expect("64 是合法容量");
    assert_eq!(p.log_ring_cap, 64, "容量必须来自装配单数据");
    assert_eq!(
        rr3_profile_parse(GOOD).expect("GOOD 合法").log_ring_cap,
        512
    );
}

#[test]
fn unparsable_toml_is_single_error() {
    let errs = rr3_profile_parse("[bindings\n").expect_err("必须报错");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].starts_with("装配单 TOML 解析失败"), "{errs:?}");
}

/// 满配：manifest 路径按 <root>/modules/<名>/module.toml 解析，缺格为空
#[test]
fn full_profile_resolves_manifests_and_finds_no_missing() {
    let root = Root::new("full");
    let p = root.write_profile("full.toml", GOOD);
    let boot = boot_inputs(&p).expect("满配装配单应通过");
    assert!(boot.missing.is_empty(), "{:?}", boot.missing);
    assert_eq!(boot.manifests.len(), 3);
    // 比对基准必须是**规范化后**的根目录：boot_inputs 拿到的 manifest 路径派生自
    // canonicalize 过的装配单路径（macOS 的 /tmp 是 /private/tmp 的符号链接），
    // 拿夹具的原始路径比对就会红在系统布局上，不是红在被测行为。
    let modules_root = std::fs::canonicalize(&root.dir)
        .expect("夹具根目录必须可规范化")
        .display()
        .to_string();
    for (i, name) in ["rr3-pa", "rr3-pb", "rr3-pc"].iter().enumerate() {
        assert_eq!(
            boot.manifests[i],
            Path::new(&modules_root)
                .join("modules")
                .join(name)
                .join("module.toml"),
            "manifest 路径必须按 modules-root 解析且升序"
        );
    }
}
