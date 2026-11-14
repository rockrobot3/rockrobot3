//! topics.toml 的 owner/topic 登记与命名空间校验。

use toml::Value;

use super::rules::rr3_registry_check;
use crate::schema::rr3_rule_list;

/// 取 topics.toml 中登记的 owner 名列表
pub fn rr3_topics_owners(text: &str) -> Vec<String> {
    match toml::from_str::<Value>(text) {
        Ok(root) => owner_names_in(&root),
        Err(_) => vec![],
    }
}

fn owner_names_in(root: &Value) -> Vec<String> {
    table_field_strings(root, "owner", "name")
}

/// 取 topics.toml 中登记的 topic 名列表
pub fn rr3_topics_names(text: &str) -> Vec<String> {
    match toml::from_str::<Value>(text) {
        Ok(root) => table_field_strings(&root, "topic", "name"),
        Err(_) => vec![],
    }
}

fn table_field_strings(root: &Value, array_key: &str, field: &str) -> Vec<String> {
    root.get(array_key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_table)
                .filter_map(|t| t.get(field).and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 校验一段 topics.toml 文本：owner 必须登记、topic 必须以 "<owner>/" 开头、kind 必须合法
pub fn rr3_topics_chk(text: &str, known_owners: &[String]) -> Vec<String> {
    let mut errs = vec![];
    let Ok(root) = toml::from_str::<Value>(text) else {
        return vec!["topics.toml 解析失败".to_string()];
    };
    let allowed_kinds = rr3_rule_list("topic_allowed_kinds").unwrap_or_default();
    let declared_owners = owner_names_in(&root);
    let topics = root.get("topic").and_then(Value::as_array);

    // F3：注册表为空（0 个 [[topic]]）是契约事故，不是放行许可证。若坐视空表，
    // 下游成员检查（publishes/rpc.exposes/subscribes）会被空洞满足而 fail-open。
    // owner 维度的登记检查保持原样，仅新增这条零主题护栏。
    rr3_topics_guard(topics, &mut errs);

    for entry in topics.into_iter().flatten() {
        rr3_topic_entry(
            entry,
            known_owners,
            &declared_owners,
            &allowed_kinds,
            &mut errs,
        );
    }
    errs
}

/// 零主题护栏：`[[topic]]` 缺失或为空表即点名（空注册表不是放行许可证）。
fn rr3_topics_guard(topics: Option<&Vec<Value>>, errs: &mut Vec<String>) {
    if topics.is_none_or(|v| v.is_empty()) {
        errs.push("主题注册表为空：必须至少登记一个 [[topic]]（ADR-0006）".to_string());
    }
}

/// 单条 `[[topic]]`：name 必填 → 主题形制 → owner 归属 → kind 在册。
fn rr3_topic_entry(
    entry: &Value,
    known_owners: &[String],
    declared_owners: &[String],
    allowed_kinds: &[String],
    errs: &mut Vec<String>,
) {
    let Some(t) = entry.as_table() else { return };
    let Some(name) = t.get("name").and_then(Value::as_str) else {
        errs.push("topic 缺少 name".to_string());
        return;
    };
    let owner = t.get("owner").and_then(Value::as_str).unwrap_or("");
    let kind = t.get("kind").and_then(Value::as_str).unwrap_or("");

    errs.extend(rr3_registry_check("topic", name));
    rr3_topic_owner_ok(name, owner, known_owners, declared_owners, errs);
    if !allowed_kinds.iter().any(|k| k == kind) {
        errs.push(format!("topic {name} 的 kind 非法: {kind}"));
    }
}

/// owner 归属：登记（调用方白名单或本表 [[owner]]）+ 前缀命名空间双重不变量。
fn rr3_topic_owner_ok(
    name: &str,
    owner: &str,
    known_owners: &[String],
    declared_owners: &[String],
    errs: &mut Vec<String>,
) {
    if !rr3_owner_known(owner, known_owners, declared_owners) {
        errs.push(format!("topic {name} 的 owner 未登记: {owner}"));
    } else if !rr3_owner_ns_ok(name, owner) {
        // 前缀归属 + owner==首段 双重不变量（二者不等价时也要报错）
        errs.push(format!("topic {name} 不在 owner {owner} 命名空间内"));
    }
}

/// owner 已登记：调用方白名单或本表 `[[owner]]` 任一命中。
fn rr3_owner_known(owner: &str, known_owners: &[String], declared_owners: &[String]) -> bool {
    known_owners.iter().any(|o| o == owner) || declared_owners.iter().any(|o| o == owner)
}

/// 前缀归属与 owner==首段双重不变量（两条同时成立才算在命名空间内）。
fn rr3_owner_ns_ok(name: &str, owner: &str) -> bool {
    name.starts_with(&format!("{owner}/")) && name.split('/').next().unwrap_or("") == owner
}
