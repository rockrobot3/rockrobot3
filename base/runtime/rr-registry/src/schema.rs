//! 加载 `module.schema.toml` 字段契约。schema 与代码同仓，编译期即随二进制内嵌。

use toml::Value;

/// manifest 契约 schema 版本（对应 `module.schema.toml` 的 `schema_version` 键；
/// 与信封契约的 `x-schema-version` 是两套键，见 `contracts/README.md` 演进规则 2）
pub const RR3_MANIFEST_SCHEMA_VERSION: u32 = 1;

const RR3_MODULE_SCHEMA_TOML: &str = include_str!("module.schema.toml");

/// 单个 manifest 字段的契约描述
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3FieldSpec {
    pub path: String,
    pub kind: String,
    pub required: bool,
    pub rule: Option<String>,
}

fn rr3_registry_str_field(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key).and_then(Value::as_str).map(str::to_string)
}

/// 解析内嵌 schema，返回字段规格列表（顺序与契约文件一致）
pub fn rr3_registry_field_specs() -> Vec<Rr3FieldSpec> {
    let root: Value = toml::from_str(RR3_MODULE_SCHEMA_TOML)
        .unwrap_or_else(|e| panic!("module.schema.toml 解析失败: {e}"));
    let fields = root
        .get("field")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("module.schema.toml 缺少 [[field]] 数组"));
    fields
        .iter()
        .filter_map(Value::as_table)
        .map(|f| Rr3FieldSpec {
            path: rr3_registry_str_field(f, "path").expect("field.path 必填"),
            kind: rr3_registry_str_field(f, "kind").expect("field.kind 必填"),
            required: f.get("required").and_then(Value::as_bool).unwrap_or(false),
            rule: rr3_registry_str_field(f, "rule"),
        })
        .collect()
}

/// schema 的 `[rules]` 表（字符串列表类规则）
pub fn rr3_registry_rule_list(name: &str) -> Option<Vec<String>> {
    let root: Value = toml::from_str(RR3_MODULE_SCHEMA_TOML).ok()?;
    let arr = root.get("rules")?.get(name)?.as_array()?;
    Some(
        arr.iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    )
}

/// 按点分路径取 manifest 值（`"events.publishes"` → 两级 table 遍历）
pub fn rr3_registry_get_by_path<'a>(root: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in dotted.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_get_by_path_resolves_nested_list() {
        let v: Value =
            toml::from_str("[events]\npublishes = [\"rr3-sense/audio/vad_start\"]\n").unwrap();
        let got = rr3_registry_get_by_path(&v, "events.publishes").expect("嵌套路径必须解析到");
        assert_eq!(got.as_array().unwrap().len(), 1);
    }

    #[test]
    fn schema_get_by_path_rejects_missing_segment() {
        let v: Value = toml::from_str("[manifest]\nname = \"rr3-x\"\n").unwrap();
        assert!(rr3_registry_get_by_path(&v, "manifest.nope").is_none());
        assert!(rr3_registry_get_by_path(&v, "nope.deep.deeper").is_none());
    }

    #[test]
    fn schema_rule_lists_are_present() {
        assert_eq!(rr3_registry_rule_list("type").unwrap(), ["module", "brain"]);
        assert_eq!(rr3_registry_rule_list("lang").unwrap(), ["rust", "python"]);
        assert_eq!(rr3_registry_rule_list("restart_policy").unwrap().len(), 3);
        assert!(rr3_registry_rule_list("base_capabilities")
            .unwrap()
            .contains(&"bus.publish".to_string()));
        assert!(rr3_registry_rule_list("nonexistent_rule").is_none());
    }

    #[test]
    fn schema_field_paths_are_all_unique_and_prefixed_by_section() {
        let specs = rr3_registry_field_specs();
        let mut seen: Vec<&str> = specs.iter().map(|s| s.path.as_str()).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "字段路径不得重复");
        let sections = [
            "manifest",
            "provides",
            "depends",
            "events",
            "rpc",
            "resources",
            "health",
            "upgrades",
        ];
        for s in &specs {
            let head = s.path.split('.').next().unwrap();
            assert!(sections.contains(&head), "未知段 {} in {}", head, s.path);
        }
    }
}
