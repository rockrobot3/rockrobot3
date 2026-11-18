//! 格位面查册门、档位一致性与结构自洽判据。

use std::collections::BTreeSet;

use rr_registry::Rr3SlotTable;

use crate::profile::Rr3Profile;

/// 格位引用查册门三扇合一：bindings/enabled_slots、[slots].present、
/// [slots].device_absent（判定口径见各子函数）。
pub(crate) fn rr3_check_slot_refs(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = rr3_check_bind_slots(profile, table);
    errs.extend(rr3_check_present(profile, table));
    errs.extend(rr3_check_absent(profile, table));
    errs
}

/// bindings/enabled_slots 的查册门（bindings 另拒基础层格位——基础层恒在位）
fn rr3_check_bind_slots(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();
    rr3_bind_rows_check(profile, table, &mut errs);
    // enabled_slots 的格位必须查册命中（基础层允许——恒在位，只记账不绑定）
    for (module, slots) in &profile.enabled_slots {
        for slot in slots {
            if !table.rr3_slot_contains(slot) {
                errs.push(format!(
                    "enabled_slots 模块 {module} 的格位 {slot} 未在册（contracts/slots.toml 无此格）"
                ));
            }
        }
    }
    errs
}

/// bindings 格位查册：未在册/基础层绑定两判。
fn rr3_bind_rows_check(profile: &Rr3Profile, table: &Rr3SlotTable, errs: &mut Vec<String>) {
    for slot in profile.bindings.keys() {
        if !table.rr3_slot_contains(slot) {
            errs.push(format!(
                "bindings 格位 {slot} 未在册（contracts/slots.toml 无此格，装配单不得自造格位）"
            ));
        } else if table.rr3_slot_layer(slot).as_deref() == Some("base") {
            errs.push(format!(
                "bindings 不得绑定基础层格位 {slot}（基础层由基座内建承载，恒在位）"
            ));
        }
    }
}

/// [slots].present 查册门：声明集只认 rr-registry 那一册；基础层进 present 属
/// 自相矛盾（恒在位），点名拒。
fn rr3_check_present(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();
    for slot in &profile.slots_present {
        rr3_present_slot(slot, table, &mut errs);
    }
    errs
}

/// present 单格两判：在册性 + 基础层禁入。
fn rr3_present_slot(slot: &str, table: &Rr3SlotTable, errs: &mut Vec<String>) {
    if !table.rr3_slot_contains(slot) {
        errs.push(format!(
            "[slots].present 格位 {slot} 未在册（contracts/slots.toml 无此格）"
        ));
    } else if table.rr3_slot_layer(slot).as_deref() == Some("base") {
        errs.push(format!(
            "[slots].present 不得声明基础层格位 {slot}（基础层由基座内建承载，恒在位）"
        ));
    }
}

/// [slots].device_absent 查册门（D3 裁定 5）：格名在册 + 只登记 base 层格
/// （非 base 格的缺席表达 = 不进 present，写进本表属第二套语法即红）。
fn rr3_check_absent(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();
    for slot in profile.slots_device_absent.keys() {
        rr3_absent_slot(slot, table, &mut errs);
    }
    errs
}

/// device_absent 单格两判：在册性 + 只认 base 层。
fn rr3_absent_slot(slot: &str, table: &Rr3SlotTable, errs: &mut Vec<String>) {
    if !table.rr3_slot_contains(slot) {
        errs.push(format!(
            "[slots].device_absent 格位 {slot} 未在册（contracts/slots.toml 无此格）"
        ));
    } else if table.rr3_slot_layer(slot).as_deref() != Some("base") {
        errs.push(format!(
            "[slots].device_absent 只登记基础层格位，{slot} 非 base 层（非基础层格的缺席表达 = 不进 [slots].present，写进本表属第二套语法）"
        ));
    }
}

/// 档位一致性（G6：bare 必须有看门狗）+ tier↔tier_scope 交叉校验（L-10）+
/// 占位/退役格禁入生产装配面（P0-2）。三处引用合并去重逐格查，未在册格由
/// 上面查册门点名，此处只对在册格判 scope，不重复报。
pub(crate) fn rr3_check_tier(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();
    rr3_tier_bare(profile, &mut errs);
    let referenced = rr3_tier_refs(profile);
    rr3_tier_slots(&referenced, profile, table, &mut errs);
    errs
}

/// 被引用格位逐条判：未在册由查册门点名，此处只判退役与档位范围。
fn rr3_tier_slots(
    referenced: &BTreeSet<String>,
    profile: &Rr3Profile,
    table: &Rr3SlotTable,
    errs: &mut Vec<String>,
) {
    for slot in referenced {
        if !table.rr3_slot_contains(slot) {
            continue;
        }
        rr3_slot_retired(slot, table, errs);
        rr3_slot_scope(&profile.platform_tier, slot, table, errs);
    }
}

/// 被引用的格位集：bindings 键 + enabled_slots 值 + slots_present 并集。
fn rr3_tier_refs(profile: &Rr3Profile) -> BTreeSet<String> {
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    referenced.extend(profile.bindings.keys().cloned());
    for slots in profile.enabled_slots.values() {
        referenced.extend(slots.iter().cloned());
    }
    referenced.extend(profile.slots_present.iter().cloned());
    referenced
}

/// G6：无看门狗的板不允许归 bare 档（ADR-0018 修订 1）。
fn rr3_tier_bare(profile: &Rr3Profile, errs: &mut Vec<String>) {
    if profile.platform_tier == "bare" && !profile.supplies.watchdog_present {
        errs.push(
            "档位一致性校验失败：tier = bare 但 supplies.watchdog_present = false——无看门狗的板不允许归 bare 档（ADR-0018 修订 1），拒绝启动"
                .to_string(),
        );
    }
}

/// P0-2 根因门：占位/退役格不得进生产装配面（真件落地后改 status=normal）。
fn rr3_slot_retired(slot: &str, table: &Rr3SlotTable, errs: &mut Vec<String>) {
    if let Some(status) = table.rr3_slot_status(slot) {
        if rr3_slot_dead(&status) {
            errs.push(format!(
                "装配面引用 {status} 格 {slot}（占位/退役格不得进生产装配面；真件落地后改 contracts/slots.toml status=normal 再绑定）"
            ));
        }
    }
}

/// 非生产状态：占位 / 退役。
fn rr3_slot_dead(status: &str) -> bool {
    status == "placeholder" || status == "retired"
}

/// L-10：tier↔tier_scope 交叉校验（该档不适用此格）。
fn rr3_slot_scope(tier: &str, slot: &str, table: &Rr3SlotTable, errs: &mut Vec<String>) {
    let scope = table.rr3_slot_tier_scope(slot).unwrap_or_default();
    if !scope.iter().any(|t| t == tier) {
        errs.push(format!(
            "tier↔tier_scope 交叉校验失败：装配档 {tier} 引用格 {slot}，其 tier_scope = {scope:?} 不含 {tier}（该档不适用此格，ADR-0012 增补注记分族），拒绝启动"
        ));
    }
}

/// 结构自洽：每条 binding 都必须被 enabled_slots 认领（否则是死数据）
pub(crate) fn rr3_check_claims(profile: &Rr3Profile) -> Vec<String> {
    let mut errs = Vec::new();
    for (slot, module) in &profile.bindings {
        let claimed = profile
            .enabled_slots
            .get(module)
            .is_some_and(|slots| slots.contains(slot));
        if !claimed {
            errs.push(format!(
                "bindings 条目 {slot}={module} 未被 enabled_slots 认领（模块 {module} 不在 enabled_slots，或其 enabled_slots 不含 {slot}）"
            ));
        }
    }
    errs
}
