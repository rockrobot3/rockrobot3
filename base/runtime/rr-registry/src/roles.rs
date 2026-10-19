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
/// 解析失败给空表（`rr3_role_table_check` 会点名"表为空"，fail-closed 护栏同 slots）。
pub fn rr3_registry_role_table_from_text(text: &str) -> Rr3RoleTable {
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
        if let Some(name) = r.get("name").and_then(Value::as_str) {
            table.roles.insert(
                name.to_string(),
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
                },
            );
        }
    }
    table
}

impl Rr3RoleTable {
    pub fn rr3_role_table_is_empty(&self) -> bool {
        self.roles.is_empty()
    }

    /// 查册：角色名是否在册（校验①-a 的判定入口）
    pub fn rr3_role_table_contains(&self, role: &str) -> bool {
        self.roles.contains_key(role)
    }

    /// 该角色归属的格位（校验② 的 role→slot 归并键；不在册 = None）
    pub fn rr3_role_table_slot(&self, role: &str) -> Option<String> {
        self.roles.get(role).map(|e| e.slot.clone())
    }

    pub fn rr3_role_table_kind(&self, role: &str) -> Option<String> {
        self.roles.get(role).map(|e| e.kind.clone())
    }

    pub fn rr3_role_table_iface(&self, role: &str) -> Option<String> {
        self.roles.get(role).map(|e| e.iface.clone())
    }

    /// 该角色的 [pins] 必填字段集（校验①-b "requires 每项必须在场"的判据源）
    pub fn rr3_role_table_requires(&self, role: &str) -> Option<Vec<String>> {
        self.roles.get(role).map(|e| e.requires.clone())
    }

    /// 该角色向描述符派生视图供给的键集（rrd 派生器的一致性对账面）
    pub fn rr3_role_table_derives(&self, role: &str) -> Option<Vec<String>> {
        self.roles.get(role).map(|e| e.derives.clone())
    }

    pub fn rr3_role_table_direction(&self, role: &str) -> Option<String> {
        self.roles.get(role).map(|e| e.direction.clone())
    }

    pub fn rr3_role_table_droppable(&self, role: &str) -> Option<bool> {
        self.roles.get(role).map(|e| e.droppable)
    }

    pub fn rr3_role_table_input_only_ok(&self, role: &str) -> Option<bool> {
        self.roles.get(role).map(|e| e.input_only_ok)
    }

    /// per_instance：true ⇒ [pins] 条目必须带 instance（pin_id = role.instance），
    /// false ⇒ 禁填 instance（填即红，校验①-b）
    pub fn rr3_role_table_per_instance(&self, role: &str) -> Option<bool> {
        self.roles.get(role).map(|e| e.per_instance)
    }

    pub fn rr3_role_table_names(&self) -> Vec<String> {
        self.roles.keys().cloned().collect()
    }

    /// 归入某格位的全部角色名（校验② 的 `roles(slot=g) ⊆ pins.role` 判据源）
    pub fn rr3_role_table_names_for_slot(&self, slot: &str) -> Vec<String> {
        self.roles
            .iter()
            .filter(|(_, e)| e.slot == slot)
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// 自身形状检查：非空 + kind/direction 词域封闭 + slot/iface/requires 键必在
    /// + 布尔三键必在（缺省 = 误写 false 不可区分，同 no_injection 的显式范式）。
    /// slot ∈ slots.toml 与 iface ∈ HANDLE_TRAITS 两条跨表判等**不在此处**
    /// （真值在对方表，调用方携带判定——见模块头注释）。
    pub fn rr3_role_table_check(&self) -> Vec<String> {
        if self.rr3_role_table_is_empty() {
            return vec!["角色表为空：必须至少登记一个 [[role]]（增补轮六 §4.1）".to_string()];
        }
        let mut errs = Vec::new();
        for (name, e) in &self.roles {
            if e.slot.is_empty() {
                errs.push(format!(
                    "role {name} 缺 slot 外键（→ contracts/slots.toml）"
                ));
            }
            if !RR3_ROLE_KINDS.contains(&e.kind.as_str()) {
                errs.push(format!(
                    "role {name} 的 kind 非法: {}（封闭三值 {RR3_ROLE_KINDS:?}，§18 F-27）",
                    e.kind
                ));
            }
            if e.iface.is_empty() {
                errs.push(format!(
                    "role {name} 缺 iface 外键（→ RR3_ASSEMBLE_HANDLE_TRAITS）"
                ));
            }
            if e.requires.is_empty() {
                errs.push(format!(
                    "role {name} 的 requires 为空（无必填字段须显式 []，留空不可区分）"
                ));
            }
            if !RR3_ROLE_DIRECTIONS.contains(&e.direction.as_str()) {
                errs.push(format!(
                    "role {name} 的 direction 非法: {}（只允许 {RR3_ROLE_DIRECTIONS:?}）",
                    e.direction
                ));
            }
            // derives 允许显式空（批N3 件八放宽）：P-5 共读源角色（power.charger.meter
            // 与 battery.meter 同一 INA219 读数面）与 placeholder 格角色（env/imu，
            // 真后端未到无句柄消费）不派生注入键，维持"无第二真值点"。判空即红的
            // 旧判据撤销——解析层无法区分"缺键"与"显式 []"，显式性由 CI 契约测试
            // 逐条点名钉住（roles.rs / contract.rs 各一份名单）。
        }
        errs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // include_str! 以本文件所在目录为基准：src → rr-registry → runtime → base → 仓库根
    const CONTRACT: &str = include_str!("../../../../contracts/roles.toml");

    /// 契约必须解析成 10 条角色，逐条点名断言（与 contract.rs 的形状 pin 同源、彼此独立）
    #[test]
    fn contract_text_parses_into_expected_shape() {
        let table = rr3_registry_role_table_from_text(CONTRACT);
        assert_eq!(
            table.roles.len(),
            10,
            "契约角色数（批N3 件八器件三角色进场后十条）"
        );
        assert_eq!(
            table.rr3_role_table_names(),
            vec![
                "act.axis.pwm",
                "act.led.gpio",
                "act.relay.gpio",
                "power.battery.meter",
                "power.charger.meter",
                "power.current.sense",
                "power.rail.sense",
                "sensor.env.meter",
                "sensor.imu.meter",
                "ui.face.spi",
            ]
        );
        // slot 外键与 per_instance 语义（校验①-b 的两侧）
        assert_eq!(
            table.rr3_role_table_slot("act.axis.pwm").as_deref(),
            Some("act.servo")
        );
        assert_eq!(
            table.rr3_role_table_per_instance("act.axis.pwm"),
            Some(true)
        );
        assert_eq!(
            table.rr3_role_table_per_instance("act.led.gpio"),
            Some(false)
        );
        assert_eq!(table.rr3_role_table_contains("act.axis.enc"), false);
        // requires / derives 与派生器对账
        assert_eq!(
            table.rr3_role_table_requires("act.axis.pwm"),
            Some(vec![
                "controller".to_string(),
                "channel".to_string(),
                "physical".to_string(),
                "rail".to_string(),
                "failsafe".to_string(),
            ])
        );
        assert_eq!(
            table.rr3_role_table_derives("act.led.gpio"),
            Some(vec!["gpio_chip".to_string(), "gpio_led_line".to_string(),])
        );
        assert_eq!(
            table.rr3_role_table_kind("power.battery.meter").as_deref(),
            Some("signal")
        );
        assert_eq!(
            table.rr3_role_table_droppable("act.relay.gpio"),
            Some(false)
        );
        assert_eq!(
            table.rr3_role_table_names_for_slot("act.servo"),
            vec!["act.axis.pwm"]
        );
        // 批N2 件六新角色：域测点两角色同挂 power.rail（per_instance 语义、
        // requires/derives 同 power.battery.meter 形状）
        assert_eq!(
            table.rr3_role_table_slot("power.rail.sense").as_deref(),
            Some("power.rail")
        );
        assert_eq!(
            table.rr3_role_table_per_instance("power.rail.sense"),
            Some(true)
        );
        assert_eq!(
            table.rr3_role_table_droppable("power.current.sense"),
            Some(false)
        );
        assert_eq!(
            table.rr3_role_table_requires("power.rail.sense"),
            Some(vec!["controller".to_string(), "addr".to_string()])
        );
        assert_eq!(
            table.rr3_role_table_names_for_slot("power.rail"),
            vec!["power.current.sense", "power.rail.sense"]
        );
        assert_eq!(table.rr3_role_table_contains("power.switch"), false);
        // 批N3 件六新角色：SPI 显示面词形（slot=ui.face，iface=Rr3Display，
        // requires=controller+channel → derives=spi_device 派生对——敞口 18 行收口）
        assert_eq!(
            table.rr3_role_table_slot("ui.face.spi").as_deref(),
            Some("ui.face")
        );
        assert_eq!(
            table.rr3_role_table_per_instance("ui.face.spi"),
            Some(false)
        );
        assert_eq!(table.rr3_role_table_droppable("ui.face.spi"), Some(true));
        assert_eq!(
            table.rr3_role_table_requires("ui.face.spi"),
            Some(vec!["controller".to_string(), "channel".to_string()])
        );
        assert_eq!(
            table.rr3_role_table_derives("ui.face.spi"),
            Some(vec!["spi_device".to_string()])
        );
        assert_eq!(
            table.rr3_role_table_direction("ui.face.spi").as_deref(),
            Some("out")
        );
        // 批N3 件八新角色：I2C 读数器件三角色（requires 首引 device 键、derives
        // 显式空 = 不派生注入键——P-5 共读源与 placeholder 格两形态，见 roles.toml 注记）
        assert_eq!(
            table.rr3_role_table_slot("power.charger.meter").as_deref(),
            Some("power.charger")
        );
        assert_eq!(
            table.rr3_role_table_iface("power.charger.meter").as_deref(),
            Some("Rr3Power")
        );
        assert_eq!(
            table.rr3_role_table_requires("power.charger.meter"),
            Some(vec![
                "controller".to_string(),
                "addr".to_string(),
                "device".to_string()
            ])
        );
        assert_eq!(
            table.rr3_role_table_derives("power.charger.meter"),
            Some(vec![])
        );
        assert_eq!(
            table.rr3_role_table_droppable("power.charger.meter"),
            Some(false)
        );
        assert_eq!(
            table.rr3_role_table_per_instance("power.charger.meter"),
            Some(false)
        );
        assert_eq!(
            table.rr3_role_table_slot("sensor.env.meter").as_deref(),
            Some("sensor.env")
        );
        assert_eq!(
            table.rr3_role_table_iface("sensor.env.meter").as_deref(),
            Some("Rr3Sensor")
        );
        assert_eq!(
            table.rr3_role_table_slot("sensor.imu.meter").as_deref(),
            Some("sensor.imu")
        );
        assert_eq!(
            table.rr3_role_table_iface("sensor.imu.meter").as_deref(),
            Some("Rr3Sensor")
        );
        assert_eq!(
            table.rr3_role_table_derives("sensor.imu.meter"),
            Some(vec![])
        );
        assert_eq!(
            table
                .rr3_role_table_direction("sensor.env.meter")
                .as_deref(),
            Some("in")
        );
        assert!(
            table.rr3_role_table_check().is_empty(),
            "{:?}",
            table.rr3_role_table_check()
        );
    }

    /// 坏文本给空表，空表必须报错（fail-open 护栏）
    #[test]
    fn unparsable_text_is_not_a_pass_port() {
        let table = rr3_registry_role_table_from_text("this = is not = valid = toml");
        assert!(table.rr3_role_table_is_empty());
        assert!(!table.rr3_role_table_check().is_empty());
    }

    /// 形状检查：kind 越界 / direction 越界 / 缺 slot / 缺 iface / requires 空
    /// 逐条点名（§4.1 字段域封闭判据的装载侧实现；derives 空 = 显式不派生合法，
    /// 批N3 件八放宽——见 rr3_role_table_check 注记，无对应红例）
    #[test]
    fn shape_violations_are_named() {
        let bad = rr3_registry_role_table_from_text(
            r#"
[[role]]
name = "act.axis.pwm"
kind = "magical"
iface = "Rr3PwmServo"
requires = ["controller"]
derives = ["pwm_chip"]
direction = "sideways"
droppable = true
input_only_ok = false
per_instance = true
[[role]]
name = "act.led.gpio"
slot = "act.led"
kind = "signal"
requires = ["controller"]
derives = []
direction = "out"
droppable = false
input_only_ok = false
per_instance = false
"#,
        );
        let errs = bad.rr3_role_table_check();
        assert!(
            errs.iter().any(|e| e.contains("act.axis.pwm 缺 slot 外键")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("kind 非法: magical")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("direction 非法: sideways")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("act.led.gpio 缺 iface 外键")),
            "{errs:?}"
        );
    }
}
