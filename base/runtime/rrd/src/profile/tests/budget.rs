//! 功耗与内存预算的单测（批B14 自 `tests.rs` 按被试族切出）。
//!
//! 覆盖 ⑨ 预算判据的实体反例组、装载链红绿三态（超 cap、power pin 未覆盖、
//! 同组收窄）与内存预算（逐原子求和、栈/堆分别越界、边界等值）。夹具（`Root`、
//! `GOOD`、`ESP32_GOOD`、`boot_inputs`）仍在父模块，经 `use super::*` 取用。

use super::*;

/// 校验⑨ 判据实体反例组（批N2 首件基-36 立，门 c ⑨-c 补并发口径，判据进 ①b）：
/// 正例（servo 五键申报形状 Σ700 ≤ cap 2000 ⇒ 绿）＋反例——同域加总超 cap ⇒ 红、
/// rail 不在 domains ⇒ 红、critical=false 不进 Σ；并发口径三态——显式空组 =
/// 全体同时（最坏缺省）、恰一组归属 = 组内互斥取 max（收窄须是数据，F-12）、
/// 多组归属 = 不收窄（宁超算不漏算）。
#[test]
fn demand_budget_check_nine() {
    let rail5v = || Rr3Domain {
        name: "rail5v".into(),
        current_cap_ma: 2000,
        ..Default::default()
    };
    let d = |rail: &str, peak: i64, critical: bool, groups: &[&str]| Rr3AssembleDemand {
        rail: rail.into(),
        peak_ma: peak,
        sustained_ma: peak.clamp(1, 120),
        critical,
        concurrency_group: groups.iter().map(|g| g.to_string()).collect(),
    };

    // 正例：servo 五键申报形状（rail5v peak 700 sustained 120 critical，空组）⇒ 无红
    let ok = vec![(
        "rr3-act/servo".to_string(),
        vec![d("rail5v", 700, true, &[])],
    )];
    assert!(
        rr3_demand_budget(&ok, &[rail5v()]).is_empty(),
        "Σ700 ≤ cap 2000 必须绿"
    );

    // 反例①：两件同域加总超 cap（Σ2400 > 2000）⇒ 红点名域 + 最坏值 + cap + 申报者
    let over = vec![
        (
            "rr3-act/servo".to_string(),
            vec![d("rail5v", 700, true, &[])],
        ),
        (
            "rr3-x/dummy".to_string(),
            vec![d("rail5v", 1700, true, &[])],
        ),
    ];
    let errs = rr3_demand_budget(&over, &[rail5v()]);
    assert!(
        errs.iter().any(|e| e.contains("电流预算超限")
            && e.contains("rail5v")
            && e.contains("2400")
            && e.contains("2000")
            && e.contains("rr3-act/servo")
            && e.contains("rr3-x/dummy")),
        "{errs:?}"
    );

    // 反例②：rail 不在 domains ⇒ 红（域缺即 cap 缺，预算无分母）
    let ghost = vec![("rr3-x/dummy".to_string(), vec![d("rail9v", 1, true, &[])])];
    let errs = rr3_demand_budget(&ghost, &[rail5v()]);
    assert!(
        errs.iter()
            .any(|e| e.contains("rail9v") && e.contains("不在 [[supplies.domain]]")),
        "{errs:?}"
    );

    // 反例③：critical=false 不进 Σ（判据只算 critical=true）——5000 mA 也绿
    let noncrit = vec![(
        "rr3-x/dummy".to_string(),
        vec![d("rail5v", 5000, false, &[])],
    )];
    assert!(
        rr3_demand_budget(&noncrit, &[rail5v()]).is_empty(),
        "critical=false 不得进加总"
    );

    // 并发口径（F-12：不重叠必须是数据）——恰一组归属 = 组内互斥取 max：
    // 三件各 700 同挂 "axis" 组 ⇒ 最坏 = max(700) = 700 ≤ 2000 绿（不加总 2100）
    let grouped = vec![
        (
            "rr3-a/servo".to_string(),
            vec![d("rail5v", 700, true, &["axis"])],
        ),
        (
            "rr3-b/servo".to_string(),
            vec![d("rail5v", 700, true, &["axis"])],
        ),
        (
            "rr3-c/servo".to_string(),
            vec![d("rail5v", 700, true, &["axis"])],
        ),
    ];
    assert!(
        rr3_demand_budget(&grouped, &[rail5v()]).is_empty(),
        "同组互斥按 max 计（700 ≤ 2000）必须绿"
    );
    // 多组归属 = 不收窄（宁超算不漏算）：条目同时挂两组 ⇒ 全额进 Σ
    let multi = vec![
        (
            "rr3-a/x".to_string(),
            vec![d("rail5v", 700, true, &["g1", "g2"])],
        ),
        ("rr3-b/y".to_string(), vec![d("rail5v", 700, true, &["g1"])]),
        ("rr3-c/z".to_string(), vec![d("rail5v", 700, true, &["g2"])]),
    ];
    let errs = rr3_demand_budget(&multi, &[rail5v()]);
    assert!(
        errs.iter().any(|e| e.contains("2100")),
        "多组归属不收窄，全额加总 2100 > 2000 ⇒ 红 {errs:?}"
    );
}

/// 门 c 装载拒链·红例①（§5 ⑨ 后果 = 拒：扩算后 Σ critical > cap ⇒ boot Err）：
/// GOOD 单轴（servo 700，角色 pin 归因实例 1）+ 两件显示件（oled/face 各 700，
/// 无对应角色 pin 走归因地板 1，空组最坏口径）⇒ 2100 > 2000，红条点名域、
/// 最坏值与申报者——"舵机堵转叠加满亮屏"这一 ADR-0021 裁定 2 立项红例在
/// 装载链可达（多件同域叠加逐件全额计）。
#[test]
fn demand_budget_chain_boot_rejects_over_cap() {
    let root = Root::new("budget-red");
    root.stage_display_atom("face", "[]");
    root.stage_display_atom("oled", "[]");
    let p = root.write_profile("budget-red.toml", GOOD);
    let e = boot_inputs(&p).expect_err("扩算 2100 > cap 2000 必须致命");
    assert!(
        e.contains("电流预算超限")
            && e.contains("rail5v")
            && e.contains("2100")
            && e.contains("2000")
            && e.contains("rr3-pa/servo")
            && e.contains("rr3-pa/oled")
            && e.contains("rr3-pa/face"),
        "{e}"
    );
}

/// 门 c 装载拒链·红例②（§5 ⑨ 缺失语义：件未申报 demands 而角色 kind=power ⇒ 红）：
/// 覆写 servo 为无申报——act.axis.pwm（全仓唯一 kind=power 角色）挂轨 rail5v 而
/// rr3-pa 申报面无 rail5v 需求，红条点名 pin、模块与判据原文。
#[test]
fn demand_budget_chain_boot_rejects_uncovered_power_pin() {
    let root = Root::new("budget-undeclared");
    root.stage_servo_undeclared();
    let p = root.write_profile("undeclared.toml", GOOD);
    let e = boot_inputs(&p).expect_err("kind=power pin 无申报覆盖必须致命");
    assert!(
        e.contains("件未申报 demands") && e.contains("rail5v") && e.contains("rr3-pa"),
        "{e}"
    );
}

/// 门 c 装载拒链·绿例（F-12：收窄必须是数据）：同一红例布局（servo+oled+face），
/// 两件显示件申报同挂 "axis" 组 ⇒ 组内互斥取 max = 700，Σ = 700（servo 未分组
/// 全额）+ 700 = 1400 ≤ 2000，boot 过——与红例①对照钉住"同布局、只多一条申报
/// 数据，红转绿"，收窄是申报者自证声明，判据只认数据不验物理。
#[test]
fn demand_budget_chain_boot_accepts_group_narrowing() {
    let root = Root::new("budget-grouped");
    root.stage_display_atom("face", "[\"axis\"]");
    root.stage_display_atom("oled", "[\"axis\"]");
    let p = root.write_profile("budget-grouped.toml", GOOD);
    let boot = boot_inputs(&p).expect("组收窄 1400 ≤ 2000 必须过");
    assert!(boot.missing.is_empty(), "{:?}", boot.missing);
}

/// 校验⑩ 判据实体反例组（n2c-v10）：not_applicable 绿（host 显式不适用）、
/// 边界等号绿（Σ = 预算不算超）、栈超红 ⑩-a、堆超红 ⑩-b——F-13 形状
/// （堆值不计栈）下只红 ⑩-b 不级联 ⑩-a，两条红文各点名判据号与两侧数值。
#[test]
fn memory_budget_check_entity_green_and_red() {
    let budget = Rr3Budget {
        not_applicable: false,
        task_stack_budget_bytes: 65536,
        heap_total_bytes: 327680,
        heap_reserved_bytes: 16384,
    };
    // host 显式不适用 ⇒ 绿（多大都绿——判据关闭是显式的）
    let host = Rr3Budget {
        not_applicable: true,
        ..Rr3Budget::default()
    };
    assert!(rr3_memory_budget(i64::MAX, i64::MAX, &host, "host").is_empty());
    // 边界等号 ⇒ 绿（≤ 判，> 才红）；栈绿堆不超
    assert!(rr3_memory_budget(65536, 311296, &budget, "rtos").is_empty());
    // ⑩-a 栈超红：点名判据号、两侧数值与档位
    let e = rr3_memory_budget(65537, 0, &budget, "rtos");
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(
        e[0].contains("校验⑩-a")
            && e[0].contains("65537")
            && e[0].contains("65536")
            && e[0].contains("rtos"),
        "{e:?}"
    );
    // ⑩-b 堆超红（F-13 形状：栈值不超 ⇒ 栈绿，只红堆）：可用堆 = 327680 − 16384
    let e = rr3_memory_budget(0, 311297, &budget, "rtos");
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(
        e[0].contains("校验⑩-b")
            && e[0].contains("311297")
            && e[0].contains("311296")
            && e[0].contains("16384"),
        "{e:?}"
    );
    // 双超各一条，互不遮蔽
    let e = rr3_memory_budget(70000, 400000, &budget, "rtos");
    assert_eq!(e.len(), 2, "{e:?}");
}

/// [supplies.budget] 分档自洽反例组（parse 层，ADR-0021 修订 3 / F-6 留空反例）：
/// host 档 not_applicable 缺失或写 false ⇒ 红；host 档并存数字键 ⇒ 红；
/// rtos 档 not_applicable = true ⇒ 红；rtos 档缺三数键 ⇒ 红；段缺 ⇒ 红
/// （DoD 3：每个缺失语义有留空反例）。
#[test]
fn budget_tier_consistency_errors_are_named() {
    // host 档 not_applicable 写 false（合法不适用不显式写 = 留空即红）
    let bad = GOOD.replace("not_applicable = true", "not_applicable = false");
    let errs = rr3_profile_parse(&bad).expect_err("host 档不写不适用必须红");
    assert!(
        errs.iter()
            .any(|e| e.contains("分档自洽校验失败") && e.contains("not_applicable = false")),
        "{errs:?}"
    );

    // host 档并存数字键（与 not_applicable = true 自相矛盾）
    let bad = GOOD.replace(
        "[supplies.budget]\nnot_applicable = true",
        "[supplies.budget]\nnot_applicable = true\ntask_stack_budget_bytes = 65536",
    );
    let errs = rr3_profile_parse(&bad).expect_err("host 档并存数字键必须红");
    assert!(
        errs.iter()
            .any(|e| e.contains("并存数字预算键") && e.contains("task_stack_budget_bytes")),
        "{errs:?}"
    );

    // rtos 档 not_applicable = true（有真栈/堆预算却写不适用）
    let bad = ESP32_GOOD.replace("not_applicable = false", "not_applicable = true");
    let errs = rr3_profile_parse(&bad).expect_err("rtos 档写不适用必须红");
    assert!(
        errs.iter().any(|e| e.contains("tier = rtos/bare 但")),
        "{errs:?}"
    );

    // rtos 档缺三数键（留空反例：必填键缺失逐条点名）
    let bad = ESP32_GOOD
        .replace("task_stack_budget_bytes = 65536\n", "")
        .replace("heap_total_bytes = 327680\n", "")
        .replace("heap_reserved_bytes = 98816\n", "");
    let errs = rr3_profile_parse(&bad).expect_err("rtos 档缺数键必须红");
    assert!(
        errs.iter()
            .any(|e| e.contains("task_stack_budget_bytes 缺失")),
        "{errs:?}"
    );
    assert!(errs.iter().any(|e| e.contains("heap_total_bytes 缺失")));
    assert!(errs.iter().any(|e| e.contains("heap_reserved_bytes 缺失")));

    // 段缺（schema 4 起必填，不许整个子段留空）
    let bad = GOOD.replace("[supplies.budget]\nnot_applicable = true\n\n", "");
    let errs = rr3_profile_parse(&bad).expect_err("缺 [supplies.budget] 必须红");
    assert!(
        errs.iter().any(|e| e.contains("缺少 [supplies.budget]")),
        "{errs:?}"
    );

    // 未知键点名（词表封闭）
    let bad = GOOD.replace(
        "[supplies.budget]\nnot_applicable = true",
        "[supplies.budget]\nnot_applicable = true\nstack_budget = 1",
    );
    let errs = rr3_profile_parse(&bad).expect_err("budget 未知键必须红");
    assert!(
        errs.iter()
            .any(|e| e.contains("[supplies.budget] 未知键 stack_budget")),
        "{errs:?}"
    );

    // rtos 档物理矛盾：total ≤ reserved（可用堆 ≤ 0，⑩-b 无分母）
    let bad = ESP32_GOOD.replace("heap_total_bytes = 327680", "heap_total_bytes = 8192");
    let errs = rr3_profile_parse(&bad).expect_err("total ≤ reserved 必须红");
    assert!(
        errs.iter()
            .any(|e| e.contains("可用堆 = total − reserved ≤ 0")),
        "{errs:?}"
    );
}

/// 门 c 装载拒链·校验⑩ 红例①（§5 ⑩-a 后果 = 拒）：ESP32_GOOD（rtos 档，
/// task_stack_budget_bytes = 65536）+ 合成内存申报件 98304 B ⇒ boot Err 点名
/// 判据号与两侧数值（申报义务未触发，红例用合成数据——见 stage_memory_atom
/// 诚实注记）。
#[test]
fn memory_budget_chain_boot_rejects_stack_over_budget() {
    let root = Root::new("mem-stack-red");
    root.stage_memory_atom("stackhog", 98304, 0);
    let p = root.write_profile("mem-stack-red.toml", ESP32_GOOD);
    let e = boot_inputs(&p).expect_err("Σ栈 98304 > 65536 必须致命");
    assert!(
        e.contains("校验⑩-a") && e.contains("98304") && e.contains("65536"),
        "{e}"
    );
}

/// 门 c 装载拒链·校验⑩ 红例②（F-13 形状：堆值不计栈 ⇒ 栈绿堆红）：栈申报
/// 16384 B 在预算内（⑩-a 绿），堆申报 327680 B > 可用堆 228864 B（327680 −
/// 98816，N-13 ② 全账后）⇒ 只红 ⑩-b——只加总栈会击穿半边的反例在装载链可达。
#[test]
fn memory_budget_chain_boot_rejects_heap_over_budget_while_stack_green() {
    let root = Root::new("mem-heap-red");
    root.stage_memory_atom("stacklegit", 16384, 0);
    root.stage_memory_atom("heaphog", 0, 327680);
    let p = root.write_profile("mem-heap-red.toml", ESP32_GOOD);
    let e = boot_inputs(&p).expect_err("Σ堆 327680 > 可用 228864 必须致命");
    assert!(e.contains("校验⑩-b") && e.contains("327680"), "{e}");
    assert!(!e.contains("校验⑩-a"), "栈在预算内不得级联红：{e}");
}

/// 门 c 装载拒链·校验⑩ 绿例：Σ栈 = 预算（等号不算超）且堆在可用堆内 ⇒ boot 过
/// ——与红例①对照钉住边界语义（≤ 判，> 才红）。
#[test]
fn memory_budget_chain_boot_accepts_boundary_sums() {
    let root = Root::new("mem-green");
    root.stage_memory_atom("stacklegit", 65536, 228864);
    let p = root.write_profile("mem-green.toml", ESP32_GOOD);
    let boot = boot_inputs(&p).expect("等号边界必须过");
    assert!(boot.missing.is_empty(), "{:?}", boot.missing);
}

/// 件级直加语义钉子（非每实例扩算，assemble.rs 头注同源）：同一内存申报件
/// 在多实例角色下不扩算——GOOD（host）换 ESP32_GOOD 载体不便，此处以
/// ESP32_GOOD 单轴（实例 1）+ 双件同值申报对照红例①直接钉数值（98304 =
/// 件级两件直加），扩算语义由 rr3_demand_expand 的 ⑨ 用例反向对照。
#[test]
fn memory_budget_sums_are_per_atom_not_per_instance() {
    let root = Root::new("mem-peratom");
    root.stage_memory_atom("stacka", 49152, 0);
    root.stage_memory_atom("stackb", 49152, 0);
    let p = root.write_profile("mem-peratom.toml", ESP32_GOOD);
    let e = boot_inputs(&p).expect_err("两件直加 98304 > 65536 必须致命");
    assert!(e.contains("Σ task_stack_bytes = 98304"), "{e}");
}
