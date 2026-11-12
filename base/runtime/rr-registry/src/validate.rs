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
//!
//! 申报面退役门（0.0.5 批J C14 首例，ADR-0019 修订 2 ②）：`contracts/topics.toml`
//! 显式 `status = "retired"` 的主题——号位保留、义务解除——不得被 publishes /
//! rpc.exposes / subscribes 申报引用，引用即拒并点名。

use std::path::{Path, PathBuf};
use toml::Value;

use crate::schema::{rr3_field_specs, rr3_get_by_path, rr3_rule_list, Rr3FieldSpec};

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
fn rr3_lower_alpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase())
}

/// capability 段：小写字母 + 下划线，非空
fn rr3_lower_us(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// `^[a-z]+(\.[a-z_]+)+$`
fn rr3_capability_shape(s: &str) -> bool {
    let segs: Vec<&str> = s.split('.').collect();
    segs.len() >= 2 && segs.iter().all(|s| rr3_lower_us(s))
}

/// `rr3-<模块>/<域>/<事件>`，三段；首段带 rr3- 前缀
fn rr3_topic_shape_ok(name: &str) -> bool {
    let segs: Vec<&str> = name.split('/').collect();
    if segs.len() != 3 || !segs[0].starts_with(RR3_TOPIC_HEAD) {
        return false;
    }
    let stem = &segs[0][RR3_TOPIC_HEAD.len()..];
    rr3_lower_alpha(stem) && segs[1..].iter().all(|s| rr3_lower_us(s))
}

/// 点分数字版本
fn rr3_semver_ok(s: &str) -> bool {
    s.contains('.')
        && !s.starts_with('.')
        && !s.ends_with('.')
        && s.split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// 单条规则检查（上下文无关规则）。`own_topic` 需 manifest.name，由
/// `rr3_manifest_gate` 特判，不进这里。
pub fn rr3_registry_check(rule: &str, value: &str) -> Vec<String> {
    match rr3_rule_pass(rule, value) {
        Some(true) => Vec::new(),
        Some(false) => vec![rr3_rule_err(rule, value)],
        None => vec![format!("未知规则名（schema 与代码不同步）: {rule}")],
    }
}

/// 规则裁决总入口：词形类（[`rr3_rule_shape`]）先查，未命中再查枚举词表；
/// `None` = 规则名不在册（调用方点名）。
fn rr3_rule_pass(rule: &str, value: &str) -> Option<bool> {
    if let Some(ok) = rr3_rule_shape(rule, value) {
        return Some(ok);
    }
    rr3_rule_enum(rule, value)
}

/// 词形类规则（值自身判据，与词表无关）。
fn rr3_rule_shape(rule: &str, value: &str) -> Option<bool> {
    match rule {
        "module_name" => Some(rr3_lower_alpha(
            value.strip_prefix(RR3_TOPIC_HEAD).unwrap_or(""),
        )),
        "semver" => Some(rr3_semver_ok(value)),
        "capability" => Some(rr3_capability_shape(value)),
        // 原子件名（atom.schema.toml，0.0.5 批B①）：^[a-z][a-z_]*$，复用 capability 段规则
        "atom_name" => Some(rr3_lower_us(value)),
        "topic" => Some(rr3_topic_shape_ok(value)),
        _ => None,
    }
}

/// 枚举类规则（`[rules]` 词表成员检查）；`none` 恒放行。
fn rr3_rule_enum(rule: &str, value: &str) -> Option<bool> {
    match rule {
        "none" => Some(true),
        "type" | "lang" | "restart_policy" => Some(
            rr3_rule_list(rule)
                .unwrap_or_default()
                .iter()
                .any(|a| a == value),
        ),
        _ => None,
    }
}

/// 规则失配文案（一句一规则，与判据同表单点；新增规则两处同批）。
fn rr3_rule_err(rule: &str, value: &str) -> String {
    match rule {
        "module_name" => format!("manifest.name 非法: {value}（要求 ^rr3-[a-z]+$）"),
        "semver" => format!("semver 非法: {value}（要求点分数字）"),
        "type" | "lang" | "restart_policy" => {
            format!(
                "{rule} 越界: {value}（允许 {:?}）",
                rr3_rule_list(rule).unwrap_or_default()
            )
        }
        "capability" => format!("capability 非法: {value}（要求 ^[a-z]+(\\.[a-z_]+)+$）"),
        "atom_name" => format!("atom.name 非法: {value}（要求 ^[a-z][a-z_]*$）"),
        "topic" => format!("topic 非法: {value}（要求 rr3-<模块>/<域>/<事件>）"),
        other => format!("未知规则名（schema 与代码不同步）: {other}"),
    }
}

fn rr3_type_err(path: &str, kind: &str, v: &Value) -> String {
    format!("字段 {path} 类型应为 {kind}，实际为 {v:?}")
}

/// manifest 未知字段反向断言（0.0.5 收口批 E'，fail-closed）：schema 正向遍历只查
/// "必填在位、类型与规则合法"，manifest 里多出来的键（typo 如 `uses.slost`）无人看，
/// 静默放行等于契约外私开载荷面、依赖静默丢失。本函数从字段规格重建允许键树
/// （path 一律两段式：段 → 键集，schema 测试已钉首段形状），对 manifest 逐层对账——
/// 顶层段集、段内键集双对账，多一个键即红。atom 与 module 两侧共用，不建第二份实现。
pub fn rr3_no_unknown_keys(
    root: &Value,
    specs: &[crate::schema::Rr3FieldSpec],
    what: &str,
) -> Vec<String> {
    let mut errs = Vec::new();
    let (allow, top_scalars) = rr3_allow_tree(specs);
    let Some(table) = root.as_table() else {
        errs.push(format!("{what} 顶层必须是表（[段] 结构），实际为 {root:?}"));
        return errs;
    };
    for (name, value) in table {
        if let Some(keys) = allow.get(name.as_str()) {
            rr3_chk_section(name, value, keys, what, &mut errs);
        } else if !top_scalars.contains(name.as_str()) {
            // 顶层标量键（契约登记在案）不做段内对账，其余未登记段即红
            errs.push(format!(
                "{what} 含未登记段 [{name}]（schema_version 契约无此段——typo 或契约未演进）"
            ));
        }
    }
    errs
}

/// 从字段规格重建允许键树：段 → 键集；一段式 path = 顶层标量键
/// （如 atom manifest 的 schema_version），不进段树单独放行。
#[allow(clippy::type_complexity)]
fn rr3_allow_tree(
    specs: &[crate::schema::Rr3FieldSpec],
) -> (
    std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>>,
    std::collections::BTreeSet<&str>,
) {
    let mut allow: std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>> =
        Default::default();
    let mut top_scalars: std::collections::BTreeSet<&str> = Default::default();
    for s in specs {
        match s.path.split_once('.') {
            Some((section, key)) => {
                allow.entry(section).or_default().insert(key);
            }
            None => {
                top_scalars.insert(s.path.as_str());
            }
        }
    }
    (allow, top_scalars)
}

/// 单个段的形状与段内键对账（段必须是表，键必须在规格树内）。
fn rr3_chk_section(
    name: &str,
    value: &Value,
    keys: &std::collections::BTreeSet<&str>,
    what: &str,
    errs: &mut Vec<String>,
) {
    let Some(section) = value.as_table() else {
        errs.push(format!("{what} 段 [{name}] 必须是表，实际为 {value:?}"));
        return;
    };
    for k in section.keys() {
        if !keys.contains(k.as_str()) {
            errs.push(format!(
                "{what} 含未登记字段 {name}.{k}（typo 或契约未演进——未知字段不报错即静默丢语义）"
            ));
        }
    }
}

/// 从 manifest 路径向上定位 `contracts/topics.toml`
fn rr3_contract_path(manifest: &Path) -> PathBuf {
    manifest
        .ancestors()
        .find(|a| a.join("contracts/topics.toml").is_file())
        .map(|root| root.join("contracts/topics.toml"))
        .unwrap_or_else(|| PathBuf::from("contracts/topics.toml"))
}

/// 校验单个 module.toml（与 contracts/topics.toml 交叉校验）——旧签名、旧行为
pub fn rr3_manifest_gate(path: &Path) -> Rr3ValidationReport {
    let mut report = Rr3ValidationReport::default();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            report
                .errors
                .push(format!("无法读取 manifest {}: {e}", path.display()));
            return report;
        }
    };
    let contract_path = rr3_contract_path(path);
    let contract_text = match std::fs::read_to_string(&contract_path) {
        Ok(t) => t,
        Err(e) => {
            report.errors.push(format!(
                "无法读取主题契约 {}（manifest 交叉校验需要它）: {e}",
                contract_path.display()
            ));
            return report;
        }
    };
    let set = crate::topics::rr3_topic_parse(&contract_text);
    // 收紧（不是放宽）：契约解析后空表直接拒，原行为靠 F3 护栏隐式覆盖。
    if set.rr3_topic_is_empty() {
        report.errors.push(format!(
            "主题契约 {} 解析后为空：拒绝校验任何 manifest（ADR-0006）",
            contract_path.display()
        ));
        return report;
    }
    report.errors = rr3_validate_inner(&text, &set);
    report
}

/// 直接校验一段 manifest 文本 + 调用方给定的主题集。
/// rrd 用它把「契约 + 仅测试的叠加层」合成一张所有权表，再校验 wire 上的文本，
/// 且不落临时文件（spec §6）。
pub fn rr3_manifest_chk(text: &str, topics: &crate::topics::Rr3TopicSet) -> Rr3ValidationReport {
    let mut report = Rr3ValidationReport::default();
    if topics.rr3_topic_is_empty() {
        report
            .errors
            .push("主题集为空：拒绝校验任何 manifest（空注册表不是放行许可证）".to_string());
        return report;
    }
    report.errors = rr3_validate_inner(text, topics);
    report
}

/// 校验内核：与拆分前的逐段逻辑等价，唯一变化是主题名来源改成入参
/// （`registered` 列表成员检查 → `topics.rr3_topic_contains`）。
/// 规则清单见本文件模块头的注释，一字未动。
fn rr3_validate_inner(text: &str, topics: &crate::topics::Rr3TopicSet) -> Vec<String> {
    let mut report = Rr3ValidationReport::default();

    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            report.errors.push(format!("manifest TOML 解析失败: {e}"));
            return report.errors;
        }
    };

    let self_name = rr3_get_by_path(&root, "manifest.name")
        .and_then(Value::as_str)
        .unwrap_or("");

    // 0) 未知字段反向断言（收口批 E'，fail-closed）——先于逐字段：契约外的键不进任何正路
    report.errors.extend(rr3_no_unknown_keys(
        &root,
        &rr3_field_specs(),
        "module manifest",
    ));

    // 1) 逐字段：存在性 + 类型 + 规则
    for spec in rr3_field_specs() {
        rr3_chk_field(&spec, &root, self_name, topics, &mut report.errors);
    }

    // 2) 跨字段：heartbeat < watchdog（方案 §4 [health] 约束）
    let hb = rr3_get_by_path(&root, "health.heartbeat_ms").and_then(Value::as_integer);
    let wd = rr3_get_by_path(&root, "health.watchdog_timeout_ms").and_then(Value::as_integer);
    if let (Some(hb), Some(wd)) = (hb, wd) {
        if hb >= wd {
            report.errors.push(format!(
                "heartbeat_ms({hb}) 必须小于 watchdog_timeout_ms({wd})"
            ));
        }
    }

    // 3) subscribes 必须已注册（订阅方不要求前缀归属）
    rr3_chk_subs(&root, topics, &mut report.errors);

    report.errors
}

/// 单字段检查：存在性（必填）→ 类型 → 规则；类型错只报一次并给出实际值。
/// 顺序与拆分前逐段等价。
fn rr3_chk_field(
    spec: &Rr3FieldSpec,
    root: &Value,
    self_name: &str,
    topics: &crate::topics::Rr3TopicSet,
    errs: &mut Vec<String>,
) {
    let Some(value) = rr3_get_by_path(root, &spec.path) else {
        if spec.required {
            errs.push(format!("缺少必填字段 {}", spec.path));
        }
        return;
    };

    if !rr3_kind_ok(spec, value, self_name, topics, errs) {
        errs.push(rr3_type_err(&spec.path, &spec.kind, value));
    }
}

/// 类型与规则的联合裁决：string/bool/int/list 四类，未知 kind = 契约漂移（panic）。
/// string 与 list 的规则检查在本层下沉，bool/int 只看形状。
fn rr3_kind_ok(
    spec: &Rr3FieldSpec,
    value: &Value,
    self_name: &str,
    topics: &crate::topics::Rr3TopicSet,
    errs: &mut Vec<String>,
) -> bool {
    match spec.kind.as_str() {
        "string" => rr3_kind_str(spec, value, errs),
        "bool" => value.as_bool().is_some(),
        "int" => value.as_integer().is_some(),
        "list" => rr3_chk_list(spec, value, self_name, topics, errs),
        other => panic!("schema 未知 kind {other}（契约与代码不同步）"),
    }
}

/// 字符串字段：规则在场则下沉到 `rr3_registry_check`，非串一律不合法。
fn rr3_kind_str(spec: &Rr3FieldSpec, value: &Value, errs: &mut Vec<String>) -> bool {
    let Some(s) = value.as_str() else {
        return false;
    };
    if let Some(rule) = &spec.rule {
        errs.extend(rr3_registry_check(rule, s));
    }
    true
}

/// 列表字段：无规则只看数组形状；有规则逐项过 `rr3_chk_item`。非字符串项必须
/// 点名（F1：filter_map 静默丢弃会让同段合法项掩盖类型错，fail-open）。
fn rr3_chk_list(
    spec: &Rr3FieldSpec,
    value: &Value,
    self_name: &str,
    topics: &crate::topics::Rr3TopicSet,
    errs: &mut Vec<String>,
) -> bool {
    let Some(arr) = value.as_array() else {
        return false;
    };
    let Some(rule) = spec.rule.as_deref() else {
        return true;
    };
    for (idx, item) in arr.iter().enumerate() {
        let Some(s) = item.as_str() else {
            errs.push(format!(
                "字段 {} 第 {} 项应为字符串，实际为 {item:?}",
                spec.path,
                idx + 1
            ));
            continue;
        };
        rr3_chk_item(spec, rule, s, self_name, topics, errs);
    }
    true
}

/// 列表项规则：own_topic 含前缀归属、主题注册与 retired 三重判定；
/// 其余规则一律走 `rr3_registry_check`（一份判据一份真值）。
fn rr3_chk_item(
    spec: &Rr3FieldSpec,
    rule: &str,
    s: &str,
    self_name: &str,
    topics: &crate::topics::Rr3TopicSet,
    errs: &mut Vec<String>,
) {
    if rule != "own_topic" {
        errs.extend(rr3_registry_check(rule, s));
        return;
    }
    errs.extend(rr3_registry_check("topic", s));
    rr3_chk_own_prefix(s, self_name, errs);
    rr3_chk_declared(spec, s, topics, errs);
}

/// own_topic 的前缀归属：主题必须以 `<manifest.name>/` 开头。
fn rr3_chk_own_prefix(s: &str, self_name: &str, errs: &mut Vec<String>) {
    let want = format!("{self_name}/");
    if !self_name.is_empty() && !s.starts_with(&want) {
        errs.push(format!("越权主题 {s}（前缀必须是 {want}）"));
    }
}

/// 申报面注册与退役：publishes/rpc.exposes 必须在主题集注册、不得是 retired。
fn rr3_chk_declared(
    spec: &Rr3FieldSpec,
    s: &str,
    topics: &crate::topics::Rr3TopicSet,
    errs: &mut Vec<String>,
) {
    // F2：events.publishes 与 rpc.exposes 都必须在主题集
    // （contracts/topics.toml，CI 检查③）注册，未注册的 RPC 名不再放行。
    // F3：主题集为空的内核入口已由两个包装 fail-closed（rr3_topic_
    // is_empty 护栏）——空注册表不得让成员检查被空洞满足。
    let check_registered = spec.path == "events.publishes" || spec.path == "rpc.exposes";
    if !check_registered {
        return;
    }
    if !topics.rr3_topic_contains(s) {
        errs.push(format!("未注册主题 {s}"));
        return;
    }
    // C14 门改判（0.0.5 批J）：retired 主题 = 号位保留、义务解除，
    // 申报发布/暴露即拒（ADR-0019 修订 2 ②）。
    if topics.rr3_topic_status(s).as_deref() == Some("retired") {
        errs.push(format!(
            "retired 主题 {s} 不得被申报（发布/暴露）——号位保留、义务解除（ADR-0019 修订 2 ②）"
        ));
    }
}

/// subscribes 成员检查：必须已注册（订阅方不要求前缀归属）。
/// 非字符串项的类型错误已由逐字段循环（rule=topic）报告，这里只做成员检查；
/// C14 门改判（0.0.5 批J）：retired 主题不得被申报订阅（号位保留、义务解除）。
fn rr3_chk_subs(root: &Value, topics: &crate::topics::Rr3TopicSet, errs: &mut Vec<String>) {
    if let Some(arr) = rr3_get_by_path(root, "events.subscribes").and_then(Value::as_array) {
        for topic in arr.iter().filter_map(Value::as_str) {
            rr3_subs_one(topic, topics, errs);
        }
    }
}

/// 单条订阅：必须已注册；retired 主题不得被申报订阅。
fn rr3_subs_one(topic: &str, topics: &crate::topics::Rr3TopicSet, errs: &mut Vec<String>) {
    if !topics.rr3_topic_contains(topic) {
        errs.push(format!("events.subscribes 引用未注册主题 {topic}"));
    } else if topics.rr3_topic_status(topic).as_deref() == Some("retired") {
        errs.push(format!(
            "events.subscribes 引用 retired 主题 {topic}——号位保留、义务解除（ADR-0019 修订 2 ②）"
        ));
    }
}

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
    if topics.is_none_or(|v| v.is_empty()) {
        errs.push("主题注册表为空：必须至少登记一个 [[topic]]（ADR-0006）".to_string());
    }

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
    if !known_owners.iter().any(|o| o == owner) && !declared_owners.iter().any(|o| o == owner) {
        errs.push(format!("topic {name} 的 owner 未登记: {owner}"));
    } else if !name.starts_with(&format!("{owner}/"))
        || name.split('/').next().unwrap_or("") != owner
    {
        // 前缀归属 + owner==首段 双重不变量（二者不等价时也要报错）
        errs.push(format!("topic {name} 不在 owner {owner} 命名空间内"));
    }
}

#[cfg(test)]
mod text_entry_tests {
    use super::*;
    use crate::topics::rr3_topic_parse;

    // include_str! 以本文件所在目录（src/）为基准：src → rr-registry → runtime → base → 仓库根
    const CONTRACT: &str = include_str!("../../../../contracts/topics.toml");
    const OVERLAY: &str = include_str!("../../tests/fixtures/sim-topics.toml");

    fn merged() -> crate::topics::Rr3TopicSet {
        rr3_topic_parse(CONTRACT)
            .rr3_topic_merge(&rr3_topic_parse(OVERLAY))
            .expect("夹具叠加层不该撞名")
    }

    fn sima(publishes: &str, hb: u64, wd: u64) -> String {
        format!(
            r#"[manifest]
schema_version = 1
name = "rr3-sima"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/sim_peer"

[events]
publishes = ["{publishes}"]
subscribes = ["rr3-runtime/module/state"]

[resources]
restart_policy = "always"

[health]
heartbeat_ms = {hb}
watchdog_timeout_ms = {wd}
"#
        )
    }

    /// 只在叠加层里的 topic：合并后放行，未合并按未注册主题拒
    #[test]
    fn overlay_topic_passes_only_when_merged() {
        let base = rr3_topic_parse(CONTRACT);
        let r1 = rr3_manifest_chk(&sima("rr3-sima/ping/state", 200, 1000), &base);
        assert!(
            r1.errors.iter().any(|e| e.contains("未注册主题")),
            "{:?}",
            r1.errors
        );

        let r2 = rr3_manifest_chk(&sima("rr3-sima/ping/state", 200, 1000), &merged());
        assert!(r2.ok(), "合并后必须放行: {:?}", r2.errors);
    }

    /// 叠加层不放行越权与健康规则（只扩主题名，不放宽判定）
    #[test]
    fn overlay_relaxes_nothing_but_membership() {
        let r = rr3_manifest_chk(&sima("rr3-simb/pong/state", 200, 1000), &merged());
        assert!(
            r.errors.iter().any(|e| e.contains("越权主题")),
            "{:?}",
            r.errors
        );

        let r2 = rr3_manifest_chk(&sima("rr3-sima/ping/state", 200, 200), &merged());
        assert!(
            r2.errors
                .iter()
                .any(|e| e.contains("heartbeat_ms(200) 必须小于 watchdog_timeout_ms(200)")),
            "{:?}",
            r2.errors
        );
    }

    /// C14 申报面退役门（0.0.5 批J）：retired 主题不得被 publishes/rpc.exposes/subscribes
    /// 申报引用；正常主题不受影响（contract 真值上跑，asr/text 与 dialog/turn 已显式 retired）
    #[test]
    fn retired_topics_cannot_be_declared() {
        let set = rr3_topic_parse(CONTRACT);
        let manifest = |publishes: &str, subscribes: &str| {
            format!(
                r#"[manifest]
schema_version = 1
name = "rr3-interact"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/rr3-interact"

[events]
publishes = [{publishes}]
subscribes = [{subscribes}]

[resources]
restart_policy = "always"

[health]
heartbeat_ms = 200
watchdog_timeout_ms = 1000
"#
            )
        };

        // 发布申报 → 拒并点名
        let r = rr3_manifest_chk(&manifest("\"rr3-interact/asr/text\"", ""), &set);
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("retired 主题 rr3-interact/asr/text 不得被申报")),
            "{:?}",
            r.errors
        );

        // RPC 暴露申报（retired 主题不能过 rpc.exposes——用同一点名门）与订阅申报 → 拒
        let r = rr3_manifest_chk(&manifest("", "\"rr3-interact/dialog/turn\""), &set);
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("events.subscribes 引用 retired 主题 rr3-interact/dialog/turn")),
            "{:?}",
            r.errors
        );

        // 正常主题申报不受退役门影响（号位保留语义：非 retired 的在册主题照常放行）
        let r = rr3_manifest_chk(
            &manifest(
                "\"rr3-interact/face/expression\"",
                "\"rr3-runtime/module/state\"",
            ),
            &set,
        );
        assert!(r.ok(), "正常主题不得被误伤: {:?}", r.errors);
    }

    /// 空主题集 = 一条错误，不做后续校验
    #[test]
    fn empty_topic_set_fail_closes() {
        let r = rr3_manifest_chk(
            &sima("rr3-sima/ping/state", 200, 1000),
            &crate::topics::Rr3TopicSet::default(),
        );
        assert_eq!(
            r.errors,
            vec!["主题集为空：拒绝校验任何 manifest（空注册表不是放行许可证）"]
        );
    }

    /// 新旧入口对同一份 fixture 必须给出逐字相同的报错（防搬运丢规则）
    #[test]
    fn new_entry_matches_old_entry_on_repo_fixture() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rr3-sense.toml");
        let old = rr3_manifest_gate(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        let new = rr3_manifest_chk(&text, &rr3_topic_parse(CONTRACT));
        assert!(old.ok(), "fixture 本该合规: {:?}", old.errors);
        assert_eq!(new.errors, old.errors);
    }

    /// 坏 manifest 的三类错误都必须由新入口点名具体字段（spec §9 的机器可验项）
    #[test]
    fn bad_manifests_name_the_field() {
        let m = merged();
        let missing =
            sima("rr3-sima/ping/state", 200, 1000).replace("restart_policy = \"always\"\n", "");
        assert!(rr3_manifest_chk(&missing, &m)
            .errors
            .iter()
            .any(|e| e.contains("缺少必填字段 resources.restart_policy")));

        let bad_policy = sima("rr3-sima/ping/state", 200, 1000).replace(
            "restart_policy = \"always\"",
            "restart_policy = \"sometimes\"",
        );
        assert!(rr3_manifest_chk(&bad_policy, &m)
            .errors
            .iter()
            .any(|e| e.contains("restart_policy 越界")));

        let hb_ge_wd = sima("rr3-sima/ping/state", 1000, 200);
        assert!(rr3_manifest_chk(&hb_ge_wd, &m)
            .errors
            .iter()
            .any(|e| e.contains("heartbeat_ms(1000) 必须小于 watchdog_timeout_ms(200)")));
    }
}
