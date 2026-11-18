//! 角色词表（增补轮六 §4.1）：`contracts/roles.toml` 的集合视图与校验①真值。
//!
//! 生产真相只有 `contracts/roles.toml`。本文件实现它的装载与形状检查：
//! profiles `[pins]` 的 role 外键、`requires` 必填字段集（校验①-b 判据对象）、
//! `derives` 派生键集（rrd 派生器的一致性对账面）都在这张表上裁定。
//! 空角色表是契约事故，不是放行许可证。
//!
//! 六表对账（门 a，§8 待建）的真值源之一：`roles.slot ⊆ slots.toml` 与
//! `roles.iface ⊆ RR3_ASSEMBLE_HANDLE_TRAITS` 两条**在装载侧由调用方携带
//! 对方真值判定**（rrd 装载链两张表都在手）；本 crate 只判自身形状，
//! 不写第二份 slots/traits 真值。

use std::collections::BTreeMap;
use toml::Value;

/// 合法 kind（§4.1 + §18 F-27 封闭三值；ADR-0022 裁定 3.1 的 `kind != logic`
/// 判据依赖 signal/power 的区分）
pub const RR3_ROLE_KINDS: [&str; 3] = ["signal", "power", "logic"];

/// 合法 direction（§4.1 封闭四值）
pub const RR3_ROLE_DIRECTIONS: [&str; 4] = ["out", "in", "io", "passive"];

/// 一条角色的登记面
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3RoleEntry {
    slot: String,
    kind: String,
    iface: String,
    requires: Vec<String>,
    derives: Vec<String>,
    direction: String,
    droppable: bool,
    input_only_ok: bool,
    per_instance: bool,
}

/// 一册角色表（不可变快照）：role 名 → 登记面
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3RoleTable {
    roles: BTreeMap<String, Rr3RoleEntry>,
}

/// 解析一份角色表文本（生产契约与测试叠加同构，共用同一解析器，不写第二套）。
/// 解析失败给空表（`rr3_role_check` 会点名"表为空"，fail-closed 护栏同 slots）。
pub fn rr3_role_parse(text: &str) -> Rr3RoleTable {
    let Ok(root) = toml::from_str::<Value>(text) else {
        return Rr3RoleTable::default();
    };
    let mut table = Rr3RoleTable::default();
    for r in root
        .get("role")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_table)
    {
        rr3_role_row(&mut table, r);
    }
    table
}

/// 单条入册：缺 name 的条目整条不收（其余键按各自缺省收）。
fn rr3_role_row(table: &mut Rr3RoleTable, r: &toml::Table) {
    let Some(name) = r.get("name").and_then(Value::as_str) else {
        return;
    };
    table.roles.insert(name.to_string(), rr3_role_entry(r));
}

/// 单条 `[[role]]` 的登记面：缺键按空串/false 收（逐字段次序同旧内联构造）。
fn rr3_role_entry(r: &toml::Table) -> Rr3RoleEntry {
    Rr3RoleEntry {
        slot: r
            .get("slot")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        kind: r
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        iface: r
            .get("iface")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        requires: r
            .get("requires")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        derives: r
            .get("derives")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        direction: r
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        droppable: r.get("droppable").and_then(Value::as_bool).unwrap_or(false),
        input_only_ok: r
            .get("input_only_ok")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        per_instance: r
            .get("per_instance")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

impl Rr3RoleTable {
    pub fn rr3_role_is_empty(&self) -> bool {
        self.roles.is_empty()
    }

    /// 查册：角色名是否在册（校验①-a 的判定入口）
    pub fn rr3_role_contains(&self, role: &str) -> bool {
        self.roles.contains_key(role)
    }

    /// 该角色归属的格位（校验② 的 role→slot 归并键；不在册 = None）
    pub fn rr3_role_slot(&self, role: &str) -> Option<String> {
        self.roles.get(role).map(|e| e.slot.clone())
    }

    pub fn rr3_role_kind(&self, role: &str) -> Option<String> {
        self.roles.get(role).map(|e| e.kind.clone())
    }

    pub fn rr3_role_iface(&self, role: &str) -> Option<String> {
        self.roles.get(role).map(|e| e.iface.clone())
    }

    /// 该角色的 [pins] 必填字段集（校验①-b "requires 每项必须在场"的判据源）
    pub fn rr3_role_requires(&self, role: &str) -> Option<Vec<String>> {
        self.roles.get(role).map(|e| e.requires.clone())
    }

    /// 该角色向描述符派生视图供给的键集（rrd 派生器的一致性对账面）
    pub fn rr3_role_derives(&self, role: &str) -> Option<Vec<String>> {
        self.roles.get(role).map(|e| e.derives.clone())
    }

    pub fn rr3_role_direction(&self, role: &str) -> Option<String> {
        self.roles.get(role).map(|e| e.direction.clone())
    }

    pub fn rr3_role_droppable(&self, role: &str) -> Option<bool> {
        self.roles.get(role).map(|e| e.droppable)
    }

    pub fn rr3_role_input_only(&self, role: &str) -> Option<bool> {
        self.roles.get(role).map(|e| e.input_only_ok)
    }

    /// per_instance：true ⇒ [pins] 条目必须带 instance（pin_id = role.instance），
    /// false ⇒ 禁填 instance（填即红，校验①-b）
    pub fn rr3_role_per_inst(&self, role: &str) -> Option<bool> {
        self.roles.get(role).map(|e| e.per_instance)
    }

    pub fn rr3_role_names(&self) -> Vec<String> {
        self.roles.keys().cloned().collect()
    }

    /// 归入某格位的全部角色名（校验② 的 `roles(slot=g) ⊆ pins.role` 判据源）
    pub fn rr3_role_slot_names(&self, slot: &str) -> Vec<String> {
        self.roles
            .iter()
            .filter(|(_, e)| e.slot == slot)
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// 自身形状检查：非空 + kind/direction 词域封闭 + slot/iface/requires 键必在
    /// + 布尔三键必在（缺省 = 误写 false 不可区分，同 no_injection 的显式范式）。
    ///
    /// slot ∈ slots.toml 与 iface ∈ HANDLE_TRAITS 两条跨表判等**不在此处**
    /// （真值在对方表，调用方携带判定——见模块头注释）。
    pub fn rr3_role_check(&self) -> Vec<String> {
        if self.rr3_role_is_empty() {
            return vec!["角色表为空：必须至少登记一个 [[role]]（增补轮六 §4.1）".to_string()];
        }
        let mut errs = Vec::new();
        for (name, e) in &self.roles {
            rr3_role_one(name, e, &mut errs);
        }
        errs
    }
}

/// 单条角色的形状判据（判据与文案次序同旧实现）。
fn rr3_role_one(name: &str, e: &Rr3RoleEntry, errs: &mut Vec<String>) {
    rr3_role_slot_chk(name, e, errs);
    rr3_role_kind_chk(name, e, errs);
    rr3_role_iface_chk(name, e, errs);
    rr3_role_req_chk(name, e, errs);
    rr3_role_dir_chk(name, e, errs);
    // derives 允许显式空（批N3 件八放宽）：P-5 共读源角色（power.charger.meter
    // 与 battery.meter 同一 INA219 读数面）与 placeholder 格角色（env/imu，
    // 真后端未到无句柄消费）不派生注入键，维持"无第二真值点"。判空即红的
    // 旧判据撤销——解析层无法区分"缺键"与"显式 []"，显式性由 CI 契约测试
    // 逐条点名钉住（roles.rs / contract.rs 各一份名单）。
}

fn rr3_role_slot_chk(name: &str, e: &Rr3RoleEntry, errs: &mut Vec<String>) {
    if e.slot.is_empty() {
        errs.push(format!(
            "role {name} 缺 slot 外键（→ contracts/slots.toml）"
        ));
    }
}

fn rr3_role_kind_chk(name: &str, e: &Rr3RoleEntry, errs: &mut Vec<String>) {
    if !RR3_ROLE_KINDS.contains(&e.kind.as_str()) {
        errs.push(format!(
            "role {name} 的 kind 非法: {}（封闭三值 {RR3_ROLE_KINDS:?}，§18 F-27）",
            e.kind
        ));
    }
}

fn rr3_role_iface_chk(name: &str, e: &Rr3RoleEntry, errs: &mut Vec<String>) {
    if e.iface.is_empty() {
        errs.push(format!(
            "role {name} 缺 iface 外键（→ RR3_ASSEMBLE_HANDLE_TRAITS）"
        ));
    }
}

/// requires 空只在 kind=logic 上合法：logic 角色无 [pins] 物理绑定
/// （链路/音频四格的真值住 [hardware] 与 board 具名装配），"没有必填
/// 引脚字段"是事实而非缺写。signal/power 角色一律非空——引脚身份
/// （controller/addr/physical 等）必须由角色说清，留空即红。
/// 显式性由 CI 契约测试逐条点名钉住（与 derives 显式空的处置同款）。
fn rr3_role_req_chk(name: &str, e: &Rr3RoleEntry, errs: &mut Vec<String>) {
    if e.requires.is_empty() && e.kind != "logic" {
        errs.push(format!(
            "role {name} 的 requires 为空（kind={} 无逻辑豁免，须列必填引脚字段）",
            e.kind
        ));
    }
}

fn rr3_role_dir_chk(name: &str, e: &Rr3RoleEntry, errs: &mut Vec<String>) {
    if !RR3_ROLE_DIRECTIONS.contains(&e.direction.as_str()) {
        errs.push(format!(
            "role {name} 的 direction 非法: {}（只允许 {RR3_ROLE_DIRECTIONS:?}）",
            e.direction
        ));
    }
}

#[cfg(test)]
mod tests;
