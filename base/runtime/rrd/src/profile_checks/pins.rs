//! 装载期校验①（[[pin]] 逐条）、host 档词形派生与派生漂移检查。

use std::collections::BTreeMap;

use rr_registry::{Rr3PlatformTable, Rr3RoleTable, Rr3SlotTable};

use super::derive::{rr3_pin_derive_value, rr3_pin_has, rr3_wordform_tail};
use crate::profile::{Rr3PinEntry, Rr3Profile, RR3_PIN_KEYS};

/// 装载期校验①（[[pin]] 逐条）+ host 档词形派生；返回 (错误, 派生视图)。
/// rtos/bare 档不派生（板侧描述符消费归 0.0.7 批G/批D1），pins 校验照跑。
pub(crate) fn rr3_check_pins(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    table: &Rr3SlotTable,
    platform: &Rr3PlatformTable,
) -> (Vec<String>, BTreeMap<String, String>) {
    let mut errs = Vec::new();
    let mut derived = BTreeMap::new();
    // host 档词形前件守卫（实-6 (a)）：词形表空 = 平台没提供派生前件，fail-closed
    rr3_wordform_guard(profile, platform, &mut errs);
    for pin in &profile.pins {
        let (pin_errs, derives) = rr3_check_pin(pin, roles, table);
        errs.extend(pin_errs);
        errs.extend(rr3_pins_derive(
            profile,
            pin,
            derives,
            platform,
            &mut derived,
        ));
    }
    (errs, derived)
}

/// host 档派生一段：非 host 档或该 pin 未声明 derives 时零波及。
fn rr3_pins_derive(
    profile: &Rr3Profile,
    pin: &Rr3PinEntry,
    derives: Option<Vec<String>>,
    platform: &Rr3PlatformTable,
    derived: &mut BTreeMap<String, String>,
) -> Vec<String> {
    let Some(derives) = derives else {
        return Vec::new();
    };
    if profile.platform_tier != "host" {
        return Vec::new();
    }
    rr3_derive_pin(pin, &derives, platform, derived)
}

fn rr3_wordform_guard(profile: &Rr3Profile, platform: &Rr3PlatformTable, errs: &mut Vec<String>) {
    if profile.platform_tier == "host" && platform.rr3_wordform_map().is_empty() {
        errs.push(
            "host 档平台事实表 controller_wordform 为空（词形派生前件缺失，host 档必须选带词形表的主机平台册，fail-closed）"
                .into(),
        );
    }
}

/// 单条 pin 的静态校验（角色查册 / per_instance / requires / direction /
/// role.slot）；角色未在册时直接返回单条点名（不级联刷屏），Second 返回 derived
/// 声明供 host 档派生。四类判据各自具名，追加次序不变。
fn rr3_check_pin(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    table: &Rr3SlotTable,
) -> (Vec<String>, Option<Vec<String>>) {
    let mut errs = Vec::new();
    let label = format!("pins[{}]", pin.rr3_pin_id());
    let Some(derives) = rr3_pin_role_gate(pin, roles, &label, &mut errs) else {
        return (errs, None);
    };
    rr3_pin_inst_check(pin, roles, &label, &mut errs);
    rr3_pin_req_check(pin, roles, &label, &mut errs);
    rr3_pin_dir_check(pin, roles, &label, &mut errs);
    rr3_pin_slot_check(pin, roles, table, &mut errs);
    (errs, Some(derives))
}

/// 角色在册门：未在册即点名并中止本条（不级联刷屏）。
fn rr3_pin_role_gate(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    label: &str,
    errs: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Some(derives) = roles.rr3_role_derives(&pin.role) else {
        errs.push(format!(
            "{label} 角色 {} 未在册（contracts/roles.toml 无此角色，装配单不得自造角色）",
            pin.role
        ));
        return None;
    };
    Some(derives)
}

/// per_instance 规则：true ⇒ instance 必填；false ⇒ instance 禁填。
fn rr3_pin_inst_check(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    label: &str,
    errs: &mut Vec<String>,
) {
    let Some(per_inst) = roles.rr3_role_per_inst(&pin.role) else {
        return;
    };
    // per_instance 与 instance 在场必须一致（true 必填 / false 禁填）。
    if per_inst != pin.instance.is_empty() {
        return;
    }
    errs.push(rr3_inst_msg(pin, label, per_inst));
}

/// per_instance 违规文案：true 缺 instance / false 多填 instance。
fn rr3_inst_msg(pin: &Rr3PinEntry, label: &str, per_inst: bool) -> String {
    if per_inst {
        format!(
            "{label} 角色 {} per_instance = true，instance 必填（多实例角色的身份一半在 instance）",
            pin.role
        )
    } else {
        format!(
            "{label} 角色 {} per_instance = false，instance 禁填（填了 pin_id 就有两个真值点）",
            pin.role
        )
    }
}

/// requires 逐项在场（§5 缺失语义：空串/None/空数组 = 缺；"none" 算在场）。
fn rr3_pin_req_check(pin: &Rr3PinEntry, roles: &Rr3RoleTable, label: &str, errs: &mut Vec<String>) {
    for f in roles.rr3_role_requires(&pin.role).unwrap_or_default() {
        rr3_pin_req_one(&f, pin, label, errs);
    }
}

/// 单条 requires 两判：字段名词表外 / 必填位缺失。
fn rr3_pin_req_one(f: &str, pin: &Rr3PinEntry, label: &str, errs: &mut Vec<String>) {
    if !RR3_PIN_KEYS.contains(&f) {
        errs.push(format!(
            "角色 {} 的 requires 引用未知字段 {f}（[pins] 字段词表外——门 a 六表对账第 5 张的漂移）",
            pin.role
        ));
    } else if !rr3_pin_has(pin, f) {
        errs.push(format!(
            "{label} 缺 {f}（角色 {} requires 必填，留空即红——§5 缺失语义；\"合法不适用\"须显式写 none）",
            pin.role
        ));
    }
}

/// direction 相等（pin 的物理方向与角色声明各说一套 = 数据矛盾）。
fn rr3_pin_dir_check(pin: &Rr3PinEntry, roles: &Rr3RoleTable, label: &str, errs: &mut Vec<String>) {
    if let Some(d) = roles.rr3_role_direction(&pin.role) {
        if pin.direction != d {
            errs.push(format!(
                "{label} direction = {} 与角色 {} direction = {d} 不一致（校验①）",
                pin.direction, pin.role
            ));
        }
    }
}

/// role.slot ∈ slots（roles/slots 两表对账的装载侧执法）。
fn rr3_pin_slot_check(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    table: &Rr3SlotTable,
    errs: &mut Vec<String>,
) {
    if let Some(slot) = roles.rr3_role_slot(&pin.role) {
        if !table.rr3_slot_contains(&slot) {
            errs.push(format!(
                "角色 {} 的 slot {slot} 未在格位契约在册（roles/slots 两表对账）",
                pin.role
            ));
        }
    }
}

/// host 档派生：derives 声明逐键取值（多实例共键须同值）+ 词形漂移检查。
/// 词形清单住平台表——表里有什么判什么，rrd 零硬编码词形清单。
fn rr3_derive_pin(
    pin: &Rr3PinEntry,
    derives: &[String],
    platform: &Rr3PlatformTable,
    derived: &mut BTreeMap<String, String>,
) -> Vec<String> {
    let mut errs = Vec::new();
    let label = format!("pins[{}]", pin.rr3_pin_id());
    for d in derives {
        rr3_derive_one(pin, d, &label, platform, derived, &mut errs);
    }
    rr3_drift_check(pin, derives, &label, platform, &mut errs);
    errs
}

/// 单条派生键取值并落账：多实例共键须同值（同一资源维）。
fn rr3_derive_one(
    pin: &Rr3PinEntry,
    d: &str,
    label: &str,
    platform: &Rr3PlatformTable,
    derived: &mut BTreeMap<String, String>,
    errs: &mut Vec<String>,
) {
    let v = match rr3_pin_derive_value(pin, d, platform) {
        Ok(v) => v,
        Err(e) => {
            errs.push(format!("{label} 派生 {d} 失败: {e}"));
            return;
        }
    };
    match derived.entry(d.to_string()) {
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(v);
        }
        std::collections::btree_map::Entry::Occupied(slot) => {
            if slot.get() != &v {
                errs.push(format!(
                    "{label} 派生键 {d} 与他条取值不同（{} vs {v}）——多实例共键须同值（同一资源维）",
                    slot.get()
                ));
            }
        }
    }
}

/// 词形漂移（①-f）：控制器词形指明总线族，该族派生键必须被角色声明。
/// derives 显式空的角色跳过（"不派生"是声明不是漏声明）。
fn rr3_drift_check(
    pin: &Rr3PinEntry,
    derives: &[String],
    label: &str,
    platform: &Rr3PlatformTable,
    errs: &mut Vec<String>,
) {
    if derives.is_empty() {
        return;
    }
    rr3_drift_scan(pin, derives, label, platform, errs);
}

/// 逐词形族比对：命中且未声明 = 派生漂移，点名。
fn rr3_drift_scan(
    pin: &Rr3PinEntry,
    derives: &[String],
    label: &str,
    platform: &Rr3PlatformTable,
    errs: &mut Vec<String>,
) {
    for (key, prefix) in platform.rr3_wordform_map() {
        if rr3_drift_hit(pin, derives, key, prefix) {
            errs.push(format!(
                "{label} controller 词形 {} 指明 {key}，但角色 {} derives 未声明（派生漂移）",
                pin.controller, pin.role
            ));
        }
    }
}

/// 词形命中该族、而角色 derives 未声明该键 = 派生漂移。
fn rr3_drift_hit(pin: &Rr3PinEntry, derives: &[String], key: &str, prefix: &str) -> bool {
    rr3_wordform_tail(&pin.controller, prefix).is_some() && !derives.iter().any(|d| d == key)
}
