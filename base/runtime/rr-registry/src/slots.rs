//! 能力格坐标系（ADR-0012 裁定 1）：`contracts/slots.toml` 的集合视图与查册门真值。
//!
//! 生产真相只有 `contracts/slots.toml`。查册门（ADR-0012 裁定 5②）只在本文件实现一次：
//! manifest 能力引用与 profile `[bindings]` 双侧都查同一张表；基础层与
//! `base_capabilities` 钉死等价（等价断言在 `tests/contract.rs` 的 CI 全集层），
//! 依赖仲裁（ADR-0009）与格位查册因此共用同一份 base 真值，不允许第二处。
//! 空格位表是契约事故，不是放行许可证。
//!
//! 格状态与档位适用性（0.0.5 批J C17 裁定 (a)，schema_version 2）：每格另带
//! `status`（正常/占位/retired 三态——三态说这张格有没有件）与 `tier_scope`
//! （host/rtos/bare 的子集——适用性说这档要不要这张格，ADR-0012 增补注记的
//! base-supply/base-bus 分族即 base 层的 tier_scope 真值）。两个轴分判，不混判。

use std::collections::BTreeMap;
use toml::Value;

/// 合法层名（裁定 1：三层坐标系；层集变更 = 破坏性变更 = 升 schema_version）
const RR3_SLOT_LAYERS: [&str; 3] = ["base", "body", "cognition"];

/// 合法格状态（C17 裁定 (a) 三态标记；状态集变更 = 破坏性变更 = 升 schema_version）
const RR3_SLOT_STATUS: [&str; 3] = ["normal", "placeholder", "retired"];

/// 合法承载档（ADR-0018 修订 1 三档；`tier_scope` 是这三者的非空子集）
const RR3_SLOT_TIERS: [&str; 3] = ["host", "rtos", "bare"];

/// 一格的登记面：层 + 状态 + 档位适用集
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3SlotEntry {
    layer: String,
    status: String,
    tier_scope: Vec<String>,
}

/// 一册格位表（不可变快照）：slot 名 → 登记面
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3SlotTable {
    slots: BTreeMap<String, Rr3SlotEntry>,
}

/// 解析一份格位文本（生产契约与测试叠加同构，共用同一解析器，不写第二套）
pub fn rr3_registry_slot_table_from_text(text: &str) -> Rr3SlotTable {
    let Ok(root) = toml::from_str::<Value>(text) else {
        return Rr3SlotTable::default();
    };
    let mut table = Rr3SlotTable::default();
    for s in root
        .get("slot")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_table)
    {
        if let Some(name) = s.get("name").and_then(Value::as_str) {
            table.slots.insert(
                name.to_string(),
                Rr3SlotEntry {
                    layer: s
                        .get("layer")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    status: s
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    tier_scope: s
                        .get("tier_scope")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                },
            );
        }
    }
    table
}

impl Rr3SlotTable {
    pub fn rr3_slot_table_is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 查册：能力名是否为表内格位（查册门的唯一判定入口）
    pub fn rr3_slot_table_contains(&self, slot: &str) -> bool {
        self.slots.contains_key(slot)
    }

    /// 该格位所在层（不在册 = None）
    pub fn rr3_slot_table_layer(&self, slot: &str) -> Option<String> {
        self.slots.get(slot).map(|e| e.layer.clone())
    }

    /// 该格位的状态（不在册 = None；schema_version=2 起在册必非空）
    pub fn rr3_slot_table_status(&self, slot: &str) -> Option<String> {
        self.slots.get(slot).map(|e| e.status.clone())
    }

    /// 该格位适用的承载档（不在册 = None）
    pub fn rr3_slot_table_tier_scope(&self, slot: &str) -> Option<Vec<String>> {
        self.slots.get(slot).map(|e| e.tier_scope.clone())
    }

    pub fn rr3_slot_table_names(&self) -> Vec<String> {
        self.slots.keys().cloned().collect()
    }

    /// 基础层格位（升序）。查册门把本列表与 `base_capabilities` 钉死等价。
    pub fn rr3_slot_table_base(&self) -> Vec<String> {
        self.slots
            .iter()
            .filter(|(_, e)| e.layer.as_str() == "base")
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// 自身形状检查：非空 + 每格能力名过同一规则函数（`rr3_registry_check`）
    /// + 层名在册 + status 在册（schema_version=2 起必有）+ tier_scope 非空且档名在册
    pub fn rr3_slot_table_check(&self) -> Vec<String> {
        if self.rr3_slot_table_is_empty() {
            return vec!["格位表为空：必须至少登记一个 [[slot]]（ADR-0012 裁定 1）".to_string()];
        }
        let mut errs = Vec::new();
        for (name, entry) in &self.slots {
            errs.extend(crate::validate::rr3_registry_check("capability", name));
            if !RR3_SLOT_LAYERS.contains(&entry.layer.as_str()) {
                errs.push(format!(
                    "slot {name} 的 layer 非法: {}（只允许 {RR3_SLOT_LAYERS:?}）",
                    entry.layer
                ));
            }
            if entry.status.is_empty() {
                errs.push(format!(
                    "slot {name} 缺 status 登记（schema_version=2 起必有: {RR3_SLOT_STATUS:?}）"
                ));
            } else if !RR3_SLOT_STATUS.contains(&entry.status.as_str()) {
                errs.push(format!(
                    "slot {name} 的 status 非法: {}（只允许 {RR3_SLOT_STATUS:?}）",
                    entry.status
                ));
            }
            if entry.tier_scope.is_empty() {
                errs.push(format!(
                    "slot {name} 缺 tier_scope 登记（至少覆盖一个承载档: {RR3_SLOT_TIERS:?}）"
                ));
            }
            for t in &entry.tier_scope {
                if !RR3_SLOT_TIERS.contains(&t.as_str()) {
                    errs.push(format!(
                        "slot {name} 的 tier_scope 含非法档 {t}（只允许 {RR3_SLOT_TIERS:?}）"
                    ));
                }
            }
        }
        errs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // include_str! 以本文件所在目录为基准：src → rr-registry → runtime → base → 仓库根
    const CONTRACT: &str = include_str!("../../../../contracts/slots.toml");

    /// 契约必须解析成 23 格，三层分布 7/12/4（与 contract.rs 的形状 pin 同源、彼此独立）
    #[test]
    fn contract_text_parses_into_expected_shape() {
        let table = rr3_registry_slot_table_from_text(CONTRACT);
        assert_eq!(table.slots.len(), 23, "契约格位数");
        assert!(!table.rr3_slot_table_is_empty());
        assert_eq!(table.rr3_slot_table_base().len(), 7, "基础层格数");
        assert!(table.rr3_slot_table_contains("audio.capture"));
        assert!(table.rr3_slot_table_contains("bus.rpc"));
        assert!(!table.rr3_slot_table_contains("audio"), "单段名不是格位");
        assert_eq!(
            table.rr3_slot_table_layer("cognition.dialog").as_deref(),
            Some("cognition")
        );
        // C17 三态轴：占位格显式标记，其余 18 格 normal
        assert_eq!(
            table.rr3_slot_table_status("sensor.env").as_deref(),
            Some("placeholder")
        );
        assert_eq!(
            table.rr3_slot_table_status("sensor.imu").as_deref(),
            Some("placeholder")
        );
        // MC-9 裁定 (a)：bt 骨架不 claim 射频，格台账同步记占位（数据位，非脚本豁免）
        assert_eq!(
            table.rr3_slot_table_status("net.bt").as_deref(),
            Some("placeholder")
        );
        assert_eq!(
            table.rr3_slot_table_status("act.servo").as_deref(),
            Some("normal")
        );
        // C17 适用性轴：base-bus 四格 bare 不适用（ADR-0012 增补注记），其余全档
        assert_eq!(
            table.rr3_slot_table_tier_scope("bus.publish"),
            Some(vec!["host".to_string(), "rtos".to_string()])
        );
        assert_eq!(
            table.rr3_slot_table_tier_scope("net.online"),
            Some(vec!["host".to_string(), "rtos".to_string()])
        );
        assert_eq!(
            table.rr3_slot_table_tier_scope("power.battery"),
            Some(vec![
                "host".to_string(),
                "rtos".to_string(),
                "bare".to_string()
            ])
        );
        // 批N2 件二（ADR-0021 裁定 2）：power.rail/power.budget 基础层 append，三档全在
        assert!(table.rr3_slot_table_contains("power.rail"));
        assert!(table.rr3_slot_table_contains("power.budget"));
        assert_eq!(
            table.rr3_slot_table_tier_scope("power.rail"),
            Some(vec![
                "host".to_string(),
                "rtos".to_string(),
                "bare".to_string()
            ])
        );
        assert_eq!(
            table.rr3_slot_table_tier_scope("power.budget"),
            Some(vec![
                "host".to_string(),
                "rtos".to_string(),
                "bare".to_string()
            ])
        );
        assert_eq!(
            table.rr3_slot_table_tier_scope("act.servo"),
            Some(vec![
                "host".to_string(),
                "rtos".to_string(),
                "bare".to_string()
            ])
        );
        assert!(
            table.rr3_slot_table_check().is_empty(),
            "{:?}",
            table.rr3_slot_table_check()
        );
    }

    /// 坏文本给空表，空表必须报错（fail-open 护栏）
    #[test]
    fn unparsable_text_is_not_a_pass_port() {
        let table = rr3_registry_slot_table_from_text("this = is not = valid = toml");
        assert!(table.rr3_slot_table_is_empty());
        assert!(!table.rr3_slot_table_check().is_empty());
    }

    /// 形状检查复用 capability 规则函数：非法名与非法层都被同一条门抓出
    #[test]
    fn shape_check_reuses_the_capability_rule() {
        let bad = rr3_registry_slot_table_from_text(
            r#"
[[slot]]
name = "Bus.Publish"
layer = "base"
status = "normal"
tier_scope = ["host"]
[[slot]]
name = "audio.capture"
layer = "telepathy"
status = "normal"
tier_scope = ["host"]
"#,
        );
        let errs = bad.rr3_slot_table_check();
        assert!(
            errs.iter().any(|e| e.contains("capability 非法")),
            "{errs:?}"
        );
        assert!(errs.iter().any(|e| e.contains("layer 非法")), "{errs:?}");
    }

    /// C17 新键的反向断言：缺 status / 非法 status / 缺 tier_scope / 非法档名逐条点名
    #[test]
    fn status_and_tier_scope_violations_are_named() {
        let bad = rr3_registry_slot_table_from_text(
            r#"
[[slot]]
name = "audio.capture"
layer = "body"
tier_scope = ["host"]
[[slot]]
name = "audio.vad"
layer = "body"
status = "deprecated"
tier_scope = ["host"]
[[slot]]
name = "act.servo"
layer = "body"
status = "normal"
[[slot]]
name = "act.led"
layer = "body"
status = "normal"
tier_scope = ["host", "quantum"]
"#,
        );
        let errs = bad.rr3_slot_table_check();
        assert!(
            errs.iter()
                .any(|e| e.contains("audio.capture 缺 status 登记")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("audio.vad 的 status 非法")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("act.servo 缺 tier_scope 登记")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("act.led 的 tier_scope 含非法档 quantum")),
            "{errs:?}"
        );
    }
}
