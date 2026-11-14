//! module manifest 的字段/未知键/跨字段/申报面校验主链。

use std::path::{Path, PathBuf};
use toml::Value;

use super::report::Rr3ValidationReport;
use super::rules::rr3_registry_check;
use crate::schema::{rr3_field_specs, rr3_get_by_path, Rr3FieldSpec};

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
    rr3_top_scan(table, &allow, &top_scalars, what, &mut errs);
    errs
}

/// 顶层逐段对账：在册段进段内核，顶层标量键放行，其余未登记段即红。
fn rr3_top_scan(
    table: &toml::Table,
    allow: &std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>>,
    top_scalars: &std::collections::BTreeSet<&str>,
    what: &str,
    errs: &mut Vec<String>,
) {
    for (name, value) in table {
        if let Some(keys) = allow.get(name.as_str()) {
            rr3_chk_section(name, value, keys, what, errs);
        } else {
            rr3_top_unknown(name, top_scalars, what, errs);
        }
    }
}

/// 未进段树的顶层键：在册标量键放行，其余未登记段即红。
fn rr3_top_unknown(
    name: &str,
    top_scalars: &std::collections::BTreeSet<&str>,
    what: &str,
    errs: &mut Vec<String>,
) {
    // 顶层标量键（契约登记在案）不做段内对账，其余未登记段即红
    if top_scalars.contains(name) {
        return;
    }
    errs.push(format!(
        "{what} 含未登记段 [{name}]（schema_version 契约无此段——typo 或契约未演进）"
    ));
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
        rr3_allow_one(s, &mut allow, &mut top_scalars);
    }
    (allow, top_scalars)
}

/// 单条字段规格入树：两段式 path 进段键集，一段式进顶层标量集。
#[allow(clippy::type_complexity)]
fn rr3_allow_one<'a>(
    s: &'a Rr3FieldSpec,
    allow: &mut std::collections::BTreeMap<&'a str, std::collections::BTreeSet<&'a str>>,
    top_scalars: &mut std::collections::BTreeSet<&'a str>,
) {
    match s.path.split_once('.') {
        Some((section, key)) => {
            allow.entry(section).or_default().insert(key);
        }
        None => {
            top_scalars.insert(s.path.as_str());
        }
    }
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
        rr3_section_key_err(name, k, keys, what, errs);
    }
}

/// 段内单个键对账：规格树外点名（typo 或契约未演进）。
fn rr3_section_key_err(
    name: &str,
    k: &str,
    keys: &std::collections::BTreeSet<&str>,
    what: &str,
    errs: &mut Vec<String>,
) {
    if !keys.contains(k) {
        errs.push(format!(
            "{what} 含未登记字段 {name}.{k}（typo 或契约未演进——未知字段不报错即静默丢语义）"
        ));
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
    let Some(text) = rr3_manifest_text(path, &mut report.errors) else {
        return report;
    };
    let Some((contract_path, contract_text)) = rr3_contract_read(path, &mut report.errors) else {
        return report;
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

/// 读 manifest 文本；读不到即点名（路径原样上报）。
fn rr3_manifest_text(path: &Path, errs: &mut Vec<String>) -> Option<String> {
    let read = std::fs::read_to_string(path);
    if let Err(e) = &read {
        errs.push(format!("无法读取 manifest {}: {e}", path.display()));
    }
    read.ok()
}

/// 读主题契约文本（相对 manifest 定位）；读不到即点名。
fn rr3_contract_read(path: &Path, errs: &mut Vec<String>) -> Option<(PathBuf, String)> {
    let contract_path = rr3_contract_path(path);
    let read = std::fs::read_to_string(&contract_path);
    if let Err(e) = &read {
        errs.push(format!(
            "无法读取主题契约 {}（manifest 交叉校验需要它）: {e}",
            contract_path.display()
        ));
    }
    read.ok().map(|t| (contract_path, t))
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

    let Some(root) = rr3_toml_root(text, &mut report.errors) else {
        return report.errors;
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
    rr3_hb_lt_wd(&root, &mut report.errors);

    // 3) subscribes 必须已注册（订阅方不要求前缀归属）
    rr3_chk_subs(&root, topics, &mut report.errors);

    report.errors
}

/// manifest 文本的 TOML 根：解析失败即点名并入 `errs`（返回 `None`）。
fn rr3_toml_root(text: &str, errs: &mut Vec<String>) -> Option<Value> {
    let parsed = toml::from_str(text);
    if let Err(e) = &parsed {
        errs.push(format!("manifest TOML 解析失败: {e}"));
    }
    parsed.ok()
}

/// 跨字段：heartbeat_ms 必须小于 watchdog_timeout_ms（两键都在才判）。
fn rr3_hb_lt_wd(root: &Value, errs: &mut Vec<String>) {
    let hb = rr3_get_by_path(root, "health.heartbeat_ms").and_then(Value::as_integer);
    let wd = rr3_get_by_path(root, "health.watchdog_timeout_ms").and_then(Value::as_integer);
    let (Some(hb), Some(wd)) = (hb, wd) else {
        return;
    };
    if hb >= wd {
        errs.push(format!(
            "heartbeat_ms({hb}) 必须小于 watchdog_timeout_ms({wd})"
        ));
    }
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
    let kind = spec.kind.as_str();
    if kind == "string" {
        return rr3_kind_str(spec, value, errs);
    }
    if kind == "bool" {
        return value.as_bool().is_some();
    }
    if kind == "int" {
        return value.as_integer().is_some();
    }
    if kind == "list" {
        return rr3_chk_list(spec, value, self_name, topics, errs);
    }
    panic!("schema 未知 kind {kind}（契约与代码不同步）");
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
    if !rr3_own_prefix_ok(self_name, s, &want) {
        errs.push(format!("越权主题 {s}（前缀必须是 {want}）"));
    }
}

/// 前缀归属：manifest.name 为空不判，否则主题须以 `<名>/` 开头。
fn rr3_own_prefix_ok(self_name: &str, s: &str, want: &str) -> bool {
    self_name.is_empty() || s.starts_with(want)
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
    if !rr3_decl_gate(spec) {
        return;
    }
    if !topics.rr3_topic_contains(s) {
        errs.push(format!("未注册主题 {s}"));
        return;
    }
    rr3_chk_retired(s, topics, errs);
}

/// 申报面键：只有 publishes / rpc.exposes 进注册与退役两道门。
fn rr3_decl_gate(spec: &Rr3FieldSpec) -> bool {
    spec.path == "events.publishes" || spec.path == "rpc.exposes"
}

/// C14 门改判（0.0.5 批J）：retired 主题 = 号位保留、义务解除，申报发布/暴露即拒。
fn rr3_chk_retired(s: &str, topics: &crate::topics::Rr3TopicSet, errs: &mut Vec<String>) {
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
