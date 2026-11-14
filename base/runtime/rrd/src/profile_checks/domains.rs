//! 注入路径唯一（门 n）、供电轨与电力域面的校验判据。

use std::collections::{BTreeMap, BTreeSet};

use rr_registry::Rr3RoleTable;

use crate::profile::{Rr3Domain, Rr3PinEntry, Rr3Profile, RR3_PIN_DERIVED_KEYS};

/// 门 n（§8 注入路径唯一）：[hardware] 出现引脚侧派生键一律红——pin 实际派生
/// 该键时点名两份取值，否则按稳态判据点名。
pub(crate) fn rr3_check_hw_keys(
    profile: &Rr3Profile,
    derived: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut errs = Vec::new();
    for k in RR3_PIN_DERIVED_KEYS {
        if let Some(hv) = profile.hardware.get(k) {
            rr3_hw_key_note(k, hv, derived, &mut errs);
        }
    }
    errs
}

/// 单键两判：pin 也派生该键（两份取值并存）/ 只有 [hardware] 直给（稳态判据）。
fn rr3_hw_key_note(k: &str, hv: &str, derived: &BTreeMap<String, String>, errs: &mut Vec<String>) {
    match derived.get(k) {
        Some(dv) => errs.push(format!(
            "hardware.{k} = \"{hv}\" 与 [pins] 派生值 \"{dv}\" 两处并存（门 n：注入路径唯一，[pins] 派生或 [hardware] 直给二者取一）"
        )),
        None => errs.push(format!(
            "hardware.{k} 是引脚侧派生键（schema 3 起唯一来源 = [pins] 派生，[hardware] 不得直给——门 n 稳态判据）"
        )),
    }
}

/// 装载期校验⑥-a/⑥-b：功率面角色必须挂真实供电轨，轨名必须在册；
/// signal 面缺 rail 合法，"none" 是显式不适值。
pub(crate) fn rr3_check_rails(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    domain_names: &BTreeSet<String>,
) -> Vec<String> {
    let mut errs = Vec::new();
    for pin in &profile.pins {
        rr3_rail_pin(pin, roles, domain_names, &mut errs);
    }
    errs
}

/// 单引脚轨判据：功率面缺轨/写 none 即红；其余轨名须在域名单在册。
fn rr3_rail_pin(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    domain_names: &BTreeSet<String>,
    errs: &mut Vec<String>,
) {
    let kind = roles.rr3_role_kind(&pin.role);
    let rail_unlisted = pin.rail.is_empty() || pin.rail == "none";
    if rail_unlisted {
        if kind.as_deref() == Some("power") {
            errs.push(format!(
                "pins[{}] rail {} 但角色 {} kind = power（校验⑥-b：功率面必须挂真实供电轨，缺 rail 或 none 即红）",
                pin.rr3_pin_id(),
                if pin.rail.is_empty() { "缺失" } else { "= none" },
                pin.role
            ));
        }
    } else if !domain_names.contains(pin.rail.as_str()) {
        errs.push(format!(
            "pins[{}] rail = {} 未在 [[supplies.domain]] 在册（校验⑥-a；域名单源在该形态的域声明，§18 F-5）",
            pin.rr3_pin_id(),
            pin.rail
        ));
    }
}

/// 装载期校验⑥-c/⑥-d/⑥-e 的域面：武装需测点与龄期上界、evidence 与 armed
/// 成对且形制合法、三个绑定引用位必须是 [pins] 的 pin_id、可切域必有开关位。
pub(crate) fn rr3_check_domains(profile: &Rr3Profile) -> Vec<String> {
    let mut errs = Vec::new();
    for d in &profile.supplies.domains {
        errs.extend(rr3_check_domain(d, &profile.pins));
    }
    errs
}

/// 单个电力域的五项判据（文案与批N21 件二同源），按武装/绑定/开关三面分立。
fn rr3_check_domain(d: &Rr3Domain, pins: &[Rr3PinEntry]) -> Vec<String> {
    let mut errs = Vec::new();
    let label = format!("supplies.domains[{}]", d.name);
    rr3_arm_check(d, &label, &mut errs);
    rr3_bind_check(d, pins, &label, &mut errs);
    if d.switchable && d.switch == "none" {
        errs.push(format!(
            "{label} switchable = true 但 switch = none（可切域必有开关控制位，两键成对）"
        ));
    }
    errs
}

/// ⑥-c/⑥-e 武装面：武装需测点与龄期上界，evidence 与 armed 成对且形制合法。
fn rr3_arm_check(d: &Rr3Domain, label: &str, errs: &mut Vec<String>) {
    if d.armed {
        rr3_arm_sense(d, label, errs);
        rr3_arm_ev(d, label, errs);
    } else if d.evidence != "none" {
        rr3_arm_off(d, label, errs);
    }
}

/// 未武装却携证据的拒动（证据位与武装位成对，不出现过期引用）。
fn rr3_arm_off(d: &Rr3Domain, label: &str, errs: &mut Vec<String>) {
    errs.push(format!(
        "{label} armed = false 但 evidence = {}（校验⑥-e：未武装不携证据，evidence 显式写 \"none\"——证据位与武装位成对，不出现过期引用）",
        if d.evidence.is_empty() { "缺失" } else { &d.evidence }
    ));
}

/// ⑥-c：无测点/无龄期上界的域不得武装拒动。
fn rr3_arm_sense(d: &Rr3Domain, label: &str, errs: &mut Vec<String>) {
    if d.sense == "none" || d.max_age_ms <= 0 {
        errs.push(format!(
            "{label} armed = true 但 sense = none 或 max_age_ms ≤ 0（校验⑥-c：无测点/无龄期上界的域不得武装拒动——未标定即 armed=false，§3.1 降级语义）"
        ));
    }
}

/// ⑥-e：武装拒动必须引用一条形制合法的真板实测证据。
fn rr3_arm_ev(d: &Rr3Domain, label: &str, errs: &mut Vec<String>) {
    if rr3_ev_unset(d) {
        errs.push(format!(
            "{label} armed = true 但 evidence = none（校验⑥-e：武装拒动必须引用一条真板实测证据，规则住 conventions §6.1；未标定即 armed = false）"
        ));
    } else if !rr_ipc::evidence::rr3_evidence_ref_ok(&d.evidence) {
        errs.push(format!(
            "{label}.evidence = {} 形制不合法（校验⑥-e：应为 <形态>/<四位编号>，如 实测/0001）",
            d.evidence
        ));
    }
}

/// 武装域证据位缺失谓词（缺省或显式 `none` 都算没写）。
fn rr3_ev_unset(d: &Rr3Domain) -> bool {
    d.evidence == "none" || d.evidence.is_empty()
}

/// ⑥-d 绑定位：sense/current_sense/switch 一律写 pin_id（"none" 是合法显式值）。
fn rr3_bind_check(d: &Rr3Domain, pins: &[Rr3PinEntry], label: &str, errs: &mut Vec<String>) {
    for (key, val) in [
        ("sense", &d.sense),
        ("current_sense", &d.current_sense),
        ("switch", &d.switch),
    ] {
        rr3_bind_one(key, val, pins, label, errs);
    }
}

/// 单个绑定位两判：`none` 显式不适跳过 / 必须命中 [pins] 的 pin_id。
fn rr3_bind_one(key: &str, val: &str, pins: &[Rr3PinEntry], label: &str, errs: &mut Vec<String>) {
    if val == "none" {
        return;
    }
    if !pins.iter().any(|p| p.rr3_pin_id() == val) {
        errs.push(format!(
            "{label}.{key} = {val} 不在 [pins] 的 pin_id 集（校验⑥-d：绑定位一律写 pin_id，\"none\" 是合法显式值——第二套语法如 adc:gp34 一律红，F-2）"
        ));
    }
}
