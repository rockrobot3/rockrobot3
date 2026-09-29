//! manifest / topics 校验。规则语义全部来自 `module.schema.toml` 的 `[rules]`。
//!
//! 规则清单（方案 §4 表格逐行落地）：
//! - `module_name`：`^rr3-[a-z]+$`
//! - `semver`：点分数字
//! - `type` / `lang` / `restart_policy`：枚举，取自 `[rules]`
//! - `capability`：`^[a-z]+(\.[a-z_]+)+$`
//! - `topic`：`rr3-<模块>/<域>/<事件>`
//! - `own_topic`：先过 `topic`，再要求前缀等于本 manifest 的 `manifest.name`
//! - `none`：放行

use std::path::{Path, PathBuf};
use toml::Value;

use crate::schema::{
    rr3_registry_field_specs, rr3_registry_get_by_path, rr3_registry_rule_list,
};

const RR3_TOPIC_HEAD: &str = "rr3-";

/// 一次 manifest 校验的结果
#[derive(Debug, Default)]
pub struct Rr3ValidationReport {
    pub errors: Vec<String>,
}

impl Rr3ValidationReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// 小写段（`[a-z]+`）：manifest.name 去前缀后的部分
fn rr3_registry_lower_alpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase())
}

/// capability 段：小写字母 + 下划线，非空
fn rr3_registry_lower_us(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// `^[a-z]+(\.[a-z_]+)+$`
fn rr3_registry_capability_shape(s: &str) -> bool {
    let segs: Vec<&str> = s.split('.').collect();
    segs.len() >= 2 && segs.iter().all(|s| rr3_registry_lower_us(s))
}

/// `rr3-<模块>/<域>/<事件>`，三段；首段带 rr3- 前缀
fn rr3_registry_topic_shape_ok(name: &str) -> bool {
    let segs: Vec<&str> = name.split('/').collect();
    if segs.len() != 3 || !segs[0].starts_with(RR3_TOPIC_HEAD) {
        return false;
    }
    let stem = &segs[0][RR3_TOPIC_HEAD.len()..];
    rr3_registry_lower_alpha(stem)
        && segs[1..].iter().all(|s| rr3_registry_lower_us(s))
}

/// 点分数字版本
fn rr3_registry_semver_ok(s: &str) -> bool {
    s.contains('.')
        && !s.starts_with('.')
        && !s.ends_with('.')
        && s.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// 单条规则检查（上下文无关规则）。`own_topic` 需 manifest.name，由
/// `rr3_registry_validate_manifest` 特判，不进这里。
pub fn rr3_registry_check(rule: &str, value: &str) -> Vec<String> {
    let bad = |m: String| vec![m];
    match rule {
        "none" => vec![],
        "module_name" => {
            let stem = value.strip_prefix(RR3_TOPIC_HEAD).unwrap_or("");
            if rr3_registry_lower_alpha(stem) {
                vec![]
            } else {
                bad(format!("manifest.name 非法: {value}（要求 ^rr3-[a-z]+$）"))
            }
        }
        "semver" => {
            if rr3_registry_semver_ok(value) {
                vec![]
            } else {
                bad(format!("semver 非法: {value}（要求点分数字）"))
            }
        }
        "type" | "lang" | "restart_policy" => {
            let allowed = rr3_registry_rule_list(rule).unwrap_or_default();
            if allowed.iter().any(|a| a == value) {
                vec![]
            } else {
                bad(format!("{rule} 越界: {value}（允许 {allowed:?}）"))
            }
        }
        "capability" => {
            if rr3_registry_capability_shape(value) {
                vec![]
            } else {
                bad(format!("capability 非法: {value}（要求 ^[a-z]+(\\.[a-z_]+)+$）"))
            }
        }
        "topic" => {
            if rr3_registry_topic_shape_ok(value) {
                vec![]
            } else {
                bad(format!("topic 非法: {value}（要求 rr3-<模块>/<域>/<事件>）"))
            }
        }
        other => bad(format!("未知规则名（schema 与代码不同步）: {other}")),
    }
}

fn rr3_registry_type_err(path: &str, kind: &str, v: &Value) -> String {
    format!("字段 {path} 类型应为 {kind}，实际为 {v:?}")
}

/// 从 manifest 路径向上定位 `contracts/topics.toml`
fn rr3_registry_contract_path(manifest: &Path) -> PathBuf {
    manifest
        .ancestors()
        .find(|a| a.join("contracts/topics.toml").is_file())
        .map(|root| root.join("contracts/topics.toml"))
        .unwrap_or_else(|| PathBuf::from("contracts/topics.toml"))
}

/// 校验单个 module.toml（含与 contracts/topics.toml 的交叉校验）
pub fn rr3_registry_validate_manifest(path: &Path) -> Rr3ValidationReport {
    let mut report = Rr3ValidationReport::default();

    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            report.errors.push(format!("无法读取 manifest {}: {e}", path.display()));
            return report;
        }
    };
    let root: Value = match toml::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            report.errors.push(format!("manifest TOML 解析失败: {e}"));
            return report;
        }
    };

    let self_name = rr3_registry_get_by_path(&root, "manifest.name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let contract_path = rr3_registry_contract_path(path);
    let contract_text = std::fs::read_to_string(&contract_path);
    let registered: Vec<String> = match &contract_text {
        Ok(t) => crate::validate::rr3_registry_topic_names(t),
        Err(_) => {
            report.errors.push(format!(
                "无法读取主题契约 {}（manifest 交叉校验需要它）",
                contract_path.display()
            ));
            vec![]
        }
    };

    // 1) 逐字段：存在性 + 类型 + 规则
    for spec in rr3_registry_field_specs() {
        let found = rr3_registry_get_by_path(&root, &spec.path);
        let Some(value) = found else {
            if spec.required {
                report.errors.push(format!("缺少必填字段 {}", spec.path));
            }
            continue;
        };

        let kind_ok = match spec.kind.as_str() {
            "string" => match value.as_str() {
                Some(s) => {
                    if let Some(rule) = &spec.rule {
                        report.errors.extend(rr3_registry_check(rule, s));
                    }
                    true
                }
                None => false,
            },
            "bool" => value.as_bool().is_some(),
            "int" => value.as_integer().is_some(),
            "list" => match (value.as_array(), spec.rule.as_deref()) {
                (Some(arr), Some(rule)) => {
                    for (idx, item) in arr.iter().enumerate() {
                        // F1：列表项非字符串是 manifest 类型错误，必须报告而非被 filter_map 静默
                        // 丢弃（否则同段的合法项会掩盖它，fail-open）。
                        let Some(s) = item.as_str() else {
                            report.errors.push(format!(
                                "字段 {} 第 {} 项应为字符串，实际为 {item:?}",
                                spec.path,
                                idx + 1
                            ));
                            continue;
                        };
                        if rule == "own_topic" {
                            report.errors.extend(rr3_registry_check("topic", s));
                            let want = format!("{self_name}/");
                            if !self_name.is_empty() && !s.starts_with(&want) {
                                report.errors.push(format!(
                                    "越权主题 {s}（前缀必须是 {want}）"
                                ));
                            }
                            // F2：events.publishes 与 rpc.exposes 都必须在 contracts/topics.toml
                            // 注册（CI 检查③），未注册的 RPC 名不再放行。
                            // F3：去掉 `!registered.is_empty()` 护栏——空注册表不得让成员检查
                            // 被空洞满足。
                            let check_registered =
                                spec.path == "events.publishes" || spec.path == "rpc.exposes";
                            if check_registered && !registered.iter().any(|t| t == s) {
                                report.errors.push(format!("未注册主题 {s}"));
                            }
                        } else {
                            report.errors.extend(rr3_registry_check(rule, s));
                        }
                    }
                    true
                }
                (Some(_), None) => true,
                _ => false,
            },
            other => panic!("schema 未知 kind {other}（契约与代码不同步）"),
        };
        if !kind_ok {
            report.errors.push(rr3_registry_type_err(&spec.path, &spec.kind, value));
        }
    }

    // 2) 跨字段：heartbeat < watchdog（方案 §4 [health] 约束）
    let hb = rr3_registry_get_by_path(&root, "health.heartbeat_ms").and_then(Value::as_integer);
    let wd = rr3_registry_get_by_path(&root, "health.watchdog_timeout_ms")
        .and_then(Value::as_integer);
    if let (Some(hb), Some(wd)) = (hb, wd) {
        if hb >= wd {
            report.errors.push(format!("heartbeat_ms({hb}) 必须小于 watchdog_timeout_ms({wd})"));
        }
    }

    // 3) 依赖必须命中 base 内建能力（v0.0.1 无跨模块能力注册表）
    let base = rr3_registry_rule_list("base_capabilities").unwrap_or_default();
    for field in ["depends.required", "depends.optional"] {
        if let Some(arr) = rr3_registry_get_by_path(&root, field).and_then(Value::as_array) {
            for (idx, item) in arr.iter().enumerate() {
                // F1：非字符串依赖项同样是 manifest 类型错误，必须报告
                let Some(cap) = item.as_str() else {
                    report.errors.push(format!(
                        "字段 {field} 第 {} 项应为字符串，实际为 {item:?}",
                        idx + 1
                    ));
                    continue;
                };
                if !base.iter().any(|b| b == cap) {
                    report.errors.push(format!("{field} 依赖了未知能力 {cap}"));
                }
            }
        }
    }

    // 4) subscribes 必须已注册（订阅方不要求前缀归属）
    // F3：去掉 `!registered.is_empty()` 护栏——空注册表不得让订阅成员检查被空洞满足。
    // 非字符串项的类型错误已由逐字段循环（rule=topic）报告，这里只做成员检查。
    if let Some(arr) = rr3_registry_get_by_path(&root, "events.subscribes").and_then(Value::as_array)
    {
        for topic in arr.iter().filter_map(Value::as_str) {
            if !registered.iter().any(|t| t == topic) {
                report.errors.push(format!("events.subscribes 引用未注册主题 {topic}"));
            }
        }
    }

    report
}

/// 取 topics.toml 中登记的 owner 名列表
pub fn rr3_registry_topic_owners(text: &str) -> Vec<String> {
    match toml::from_str::<Value>(text) {
        Ok(root) => owner_names_in(&root),
        Err(_) => vec![],
    }
}

fn owner_names_in(root: &Value) -> Vec<String> {
    table_field_strings(root, "owner", "name")
}

/// 取 topics.toml 中登记的 topic 名列表
pub fn rr3_registry_topic_names(text: &str) -> Vec<String> {
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
pub fn rr3_registry_validate_topics(text: &str, known_owners: &[String]) -> Vec<String> {
    let mut errs = vec![];
    let Ok(root) = toml::from_str::<Value>(text) else {
        return vec!["topics.toml 解析失败".to_string()];
    };
    let allowed_kinds = rr3_registry_rule_list("topic_allowed_kinds").unwrap_or_default();
    let declared_owners = owner_names_in(&root);
    let topics = root.get("topic").and_then(Value::as_array);

    // F3：注册表为空（0 个 [[topic]]）是契约事故，不是放行许可证。若坐视空表，
    // 下游成员检查（publishes/rpc.exposes/subscribes）会被空洞满足而 fail-open。
    // owner 维度的登记检查保持原样，仅新增这条零主题护栏。
    if topics.map_or(true, |v| v.is_empty()) {
        errs.push("主题注册表为空：必须至少登记一个 [[topic]]（ADR-0006）".to_string());
    }

    for entry in topics.into_iter().flatten() {
        let Some(t) = entry.as_table() else { continue };
        let Some(name) = t.get("name").and_then(Value::as_str) else {
            errs.push("topic 缺少 name".to_string());
            continue;
        };
        let owner = t.get("owner").and_then(Value::as_str).unwrap_or("");
        let kind = t.get("kind").and_then(Value::as_str).unwrap_or("");

        errs.extend(rr3_registry_check("topic", name));
        if !known_owners.iter().any(|o| o == owner) && !declared_owners.iter().any(|o| o == owner)
        {
            errs.push(format!("topic {name} 的 owner 未登记: {owner}"));
        } else if !name.starts_with(&format!("{owner}/"))
            || name.split('/').next().unwrap_or("") != owner
        {
            // 前缀归属 + owner==首段 双重不变量（二者不等价时也要报错）
            errs.push(format!("topic {name} 不在 owner {owner} 命名空间内"));
        }
        if !allowed_kinds.iter().any(|k| k == kind) {
            errs.push(format!("topic {name} 的 kind 非法: {kind}"));
        }
    }
    errs
}
