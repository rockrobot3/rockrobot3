//! 能力格坐标系（ADR-0012 裁定 1）：`contracts/slots.toml` 的集合视图与查册门真值。
//!
//! 生产真相只有 `contracts/slots.toml`。查册门（ADR-0012 裁定 5②）只在本文件实现一次：
//! manifest 能力引用与 profile `[bindings]` 双侧都查同一张表；基础层与
//! `base_capabilities` 钉死等价（等价断言在 `tests/contract.rs` 的 CI 全集层），
//! 依赖仲裁（ADR-0009）与格位查册因此共用同一份 base 真值，不允许第二处。
//! 空格位表是契约事故，不是放行许可证。

use std::collections::BTreeMap;
use toml::Value;

/// 合法层名（裁定 1：三层坐标系；层集变更 = 破坏性变更 = 升 schema_version）
const RR3_SLOT_LAYERS: [&str; 3] = ["base", "body", "cognition"];

/// 一册格位表（不可变快照）：slot 名 → 层
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3SlotTable {
    layers: BTreeMap<String, String>,
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
            table.layers.insert(
                name.to_string(),
                s.get("layer")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            );
        }
    }
    table
}

impl Rr3SlotTable {
    pub fn rr3_slot_table_is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// 查册：能力名是否为表内格位（查册门的唯一判定入口）
    pub fn rr3_slot_table_contains(&self, slot: &str) -> bool {
        self.layers.contains_key(slot)
    }

    /// 该格位所在层（不在册 = None）
    pub fn rr3_slot_table_layer(&self, slot: &str) -> Option<String> {
        self.layers.get(slot).cloned()
    }

    pub fn rr3_slot_table_names(&self) -> Vec<String> {
        self.layers.keys().cloned().collect()
    }

    /// 基础层格位（升序）。查册门把本列表与 `base_capabilities` 钉死等价。
    pub fn rr3_slot_table_base(&self) -> Vec<String> {
        self.layers
            .iter()
            .filter(|(_, l)| l.as_str() == "base")
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// 自身形状检查：非空 + 每格能力名过同一规则函数（`rr3_registry_check`）+ 层名在册
    pub fn rr3_slot_table_check(&self) -> Vec<String> {
        if self.rr3_slot_table_is_empty() {
            return vec!["格位表为空：必须至少登记一个 [[slot]]（ADR-0012 裁定 1）".to_string()];
        }
        let mut errs = Vec::new();
        for (name, layer) in &self.layers {
            errs.extend(crate::validate::rr3_registry_check("capability", name));
            if !RR3_SLOT_LAYERS.contains(&layer.as_str()) {
                errs.push(format!(
                    "slot {name} 的 layer 非法: {layer}（只允许 {RR3_SLOT_LAYERS:?}）"
                ));
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

    /// 契约必须解析成 20 格，三层分布 5/11/4（与 contract.rs 的形状 pin 同源、彼此独立）
    #[test]
    fn contract_text_parses_into_expected_shape() {
        let table = rr3_registry_slot_table_from_text(CONTRACT);
        assert_eq!(table.layers.len(), 20, "契约格位数");
        assert!(!table.rr3_slot_table_is_empty());
        assert_eq!(table.rr3_slot_table_base().len(), 5, "基础层格数");
        assert!(table.rr3_slot_table_contains("audio.capture"));
        assert!(table.rr3_slot_table_contains("bus.rpc"));
        assert!(!table.rr3_slot_table_contains("audio"), "单段名不是格位");
        assert_eq!(
            table.rr3_slot_table_layer("cognition.dialog").as_deref(),
            Some("cognition")
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
[[slot]]
name = "audio.capture"
layer = "telepathy"
"#,
        );
        let errs = bad.rr3_slot_table_check();
        assert!(
            errs.iter().any(|e| e.contains("capability 非法")),
            "{errs:?}"
        );
        assert!(errs.iter().any(|e| e.contains("layer 非法")), "{errs:?}");
    }
}
