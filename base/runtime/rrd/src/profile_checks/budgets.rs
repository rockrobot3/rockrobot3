// SPDX-License-Identifier: Apache-2.0
//! 装配扫描、预算判据（校验⑨⑩）与缺角色/缺格三源合一。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rr_registry::{Rr3AssembleFacts, Rr3RoleTable, Rr3SlotTable};

use crate::profile::{
    rr3_assemble_scan, rr3_demand_budget, rr3_demand_expand, rr3_memory_budget, rr3_power_gaps,
    Rr3Profile,
};

/// 装配扫描产物：模块名 → [(件名, facts)]（预算判据分子；坏形进 broken 旁路）
type Rr3Scan = Vec<(String, Vec<(String, Rr3AssembleFacts)>)>;

/// 装配扫描：逐模块读 atoms/*/assemble.toml；坏形/注入缺进 broken（该模块
/// Degraded，不 fatal），合法 facts 进 scanned（预算判据分子）。
pub(crate) fn rr3_scan_modules(
    root: &Path,
    profile: &Rr3Profile,
) -> (Rr3Scan, Vec<(String, String)>) {
    let mut broken = Vec::new();
    let mut scanned = Vec::new();
    for m in profile.enabled_slots.keys() {
        let (atoms, msgs) = rr3_assemble_scan(&root.join("modules").join(m));
        scanned.push((m.clone(), atoms));
        for msg in msgs {
            broken.push((m.clone(), msg));
        }
    }
    (scanned, broken)
}

/// 校验⑨（电流预算/覆盖）与校验⑩（内存预算）：分子来自装配扫描 facts，
/// 分母来自装配单；坏形件不进加总（可见面 = assemble_broken 点名）。
pub(crate) fn rr3_check_budgets(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    scanned: &Rr3Scan,
) -> Vec<String> {
    let mut errs = Vec::new();
    let declared = rr3_demand_expand(profile, roles, scanned);
    errs.extend(rr3_demand_budget(&declared, &profile.supplies.domains));
    errs.extend(rr3_power_gaps(profile, roles, scanned));
    let stack_sum: i64 = scanned
        .iter()
        .flat_map(|(_, atoms)| atoms.iter())
        .map(|(_, f)| f.task_stack_bytes)
        .sum();
    let heap_sum: i64 = scanned
        .iter()
        .flat_map(|(_, atoms)| atoms.iter())
        .map(|(_, f)| f.heap_peak_bytes)
        .sum();
    errs.extend(rr3_memory_budget(
        stack_sum,
        heap_sum,
        &profile.supplies.budget,
        &profile.platform_tier,
    ));
    errs
}

/// kind=logic 角色按定义无 [pins] 物理绑定，不进缺角色点名面。
fn rr3_role_is_logic(roles: &Rr3RoleTable, role: &str) -> bool {
    roles.rr3_role_kind(role).as_deref() == Some("logic")
}

/// 校验②（后果 = 降不拒）：present ∩ enabled_slots 的缺角色清单。
///
/// 判据按**绑定组**（roles.toml `group`，缺省 = 角色名本身）：
/// - 组内全为 logic 角色 → 无物理绑定面，跳过（现状四格能力面角色）；
/// - 否则组内至少一条角色在 [pins] 有绑定即满足——同组角色是同一物理位置的
///   可替代实现（ui.face 的 SPI 点阵 / I2C OLED），装配单按板二选一。
///
/// 缺组点名以组名为第二列（组名缺省 = 角色名，既有可观测输出逐字不变）。
pub(crate) fn rr3_role_gaps(profile: &Rr3Profile, roles: &Rr3RoleTable) -> Vec<(String, String)> {
    let mut gaps = Vec::new();
    let enabled_all: BTreeSet<String> = profile.enabled_slots.values().flatten().cloned().collect();
    for slot in &enabled_all {
        if !profile.slots_present.contains(slot) {
            continue;
        }
        for (group, members) in rr3_slot_groups(roles, slot) {
            if !rr3_group_covered(profile, roles, &members) {
                gaps.push((slot.clone(), group));
            }
        }
    }
    gaps.sort();
    gaps
}

/// 格位内按绑定组归并（BTreeMap 定序；组名缺省已在 roles 解析层补成角色名）。
fn rr3_slot_groups(roles: &Rr3RoleTable, slot: &str) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for role in roles.rr3_role_slot_names(slot) {
        let group = roles.rr3_role_group(&role).unwrap_or_else(|| role.clone());
        groups.entry(group).or_default().push(role);
    }
    groups
}

/// 组覆盖判定：全 logic = 无需 [pins]；否则组内任一角色有绑定即覆盖。
fn rr3_group_covered(profile: &Rr3Profile, roles: &Rr3RoleTable, members: &[String]) -> bool {
    if members.iter().all(|r| rr3_role_is_logic(roles, r)) {
        return true;
    }
    members
        .iter()
        .any(|r| profile.pins.iter().any(|p| p.role == *r))
}

/// 缺格三源合一（方案 §15）：实测缺（探测）+ 声明缺（present）+ 注入缺
/// （enabled_slots 声明但无指向本模块的 binding）；基础层恒在位跳过。
pub(crate) fn rr3_slot_gaps(
    profile: &Rr3Profile,
    table: &Rr3SlotTable,
    roles: &Rr3RoleTable,
    probe: &dyn crate::probe::Rr3Probe,
) -> Vec<(String, String)> {
    let i2c_bind = rr3_i2c_bind(profile, roles);
    let mut missing = rr3_probe_gaps(profile, &i2c_bind, probe);
    rr3_declared_gaps(profile, table, &mut missing);
    missing.sort();
    missing
}

/// L-14 器件级探测的 (bus, addr) 组装：格位 → 该格角色在 [pins] 的 I2C 绑定
/// （controller 词形 i2c<N> → N，addr 非 none 非空才有效）。无绑定 → None →
/// 探针回落总线级五面判据（无 [pins] 形态兼容面）。
fn rr3_i2c_bind(profile: &Rr3Profile, roles: &Rr3RoleTable) -> BTreeMap<String, (u32, String)> {
    let mut i2c_bind = BTreeMap::new();
    let enabled_all: BTreeSet<String> = profile.enabled_slots.values().flatten().cloned().collect();
    for slot in &enabled_all {
        for role in roles.rr3_role_slot_names(slot) {
            for pin in profile.pins.iter().filter(|p| p.role == role) {
                let Some(n) = pin
                    .controller
                    .strip_prefix("i2c")
                    .and_then(|rest| rest.parse::<u32>().ok())
                else {
                    continue;
                };
                if !pin.addr.is_empty() && pin.addr != "none" {
                    i2c_bind
                        .entry(slot.clone())
                        .or_insert_with(|| (n, pin.addr.clone()));
                }
            }
        }
    }
    i2c_bind
}

/// 探测先行（MC-4）：bindings 声明的格位逐格探测；探测自身故障按"探测不到"
/// 处理（fail-closed，方案 §4）——缺格三源合一的"实测缺"即出自本环。
fn rr3_probe_gaps(
    profile: &Rr3Profile,
    i2c_bind: &BTreeMap<String, (u32, String)>,
    probe: &dyn crate::probe::Rr3Probe,
) -> Vec<(String, String)> {
    let mut missing = Vec::new();
    for (slot, module) in &profile.bindings {
        let (bus, addr) = rr3_bind_pair(i2c_bind, slot);
        if !probe.rr3_probe_slot_bound(slot, bus, addr) {
            missing.push((module.clone(), slot.clone()));
        }
    }
    missing
}

/// 格位的器件级 (bus, addr)：无绑定回落总线级五面判据（None, None）。
fn rr3_bind_pair<'a>(
    i2c_bind: &'a BTreeMap<String, (u32, String)>,
    slot: &str,
) -> (Option<u32>, Option<&'a str>) {
    match i2c_bind.get(slot) {
        Some((n, a)) => (Some(*n), Some(a.as_str())),
        None => (None, None),
    }
}

/// 声明缺/注入缺逐格补齐（去重；板说没有的格不再看 binding——绑了也白绑）
fn rr3_declared_gaps(
    profile: &Rr3Profile,
    table: &Rr3SlotTable,
    missing: &mut Vec<(String, String)>,
) {
    for (module, slots) in &profile.enabled_slots {
        for slot in slots {
            if table.rr3_slot_layer(slot).as_deref() == Some("base") {
                continue;
            }
            let absent = !profile.slots_present.contains(slot);
            if (absent || profile.bindings.get(slot) != Some(module))
                && !missing.contains(&(module.clone(), slot.clone()))
            {
                missing.push((module.clone(), slot.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Rr3PinEntry;

    /// 真契约（与装载链同源）：roles 名词域以生产文件为准，不抄第二份。
    const ROLES: &str = include_str!("../../../../../contracts/roles.toml");

    /// ui.face 两角色同属 ui.face.phys 绑定组：组内一路绑定即满足，缺组点名组名
    /// （W5-3 校验②按组判——SPI 点阵与 I2C OLED 二选一）。
    #[test]
    fn face_group_binding() {
        let roles = rr_registry::rr3_role_parse(ROLES);
        let mut profile = Rr3Profile::default();
        profile
            .enabled_slots
            .insert("rr3-interact".into(), vec!["ui.face".into()]);
        profile.slots_present.push("ui.face".into());
        profile
            .bindings
            .insert("ui.face".into(), "rr3-interact".into());
        assert_eq!(
            rr3_role_gaps(&profile, &roles),
            vec![("ui.face".to_string(), "ui.face.phys".to_string())],
            "组内零绑定须点名组名"
        );
        profile.pins.push(Rr3PinEntry {
            role: "ui.face.i2c".into(),
            controller: "i2c1".into(),
            addr: "0x3c".into(),
            device: "SSD1306".into(),
            direction: "out".into(),
            ..Default::default()
        });
        assert!(
            rr3_role_gaps(&profile, &roles).is_empty(),
            "组内一路绑定（OLED）即满足"
        );
    }
}
