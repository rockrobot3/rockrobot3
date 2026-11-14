//! binding 归属与格位查册：缺格、跨模块认领、基础层禁入。

use super::super::*;

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
