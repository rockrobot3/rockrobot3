//! 跨 manifest 依赖仲裁（ADR-0009）。单件校验（`validate.rs`）看不到全集，故
//! "可满足性 + 环检测"只在这份全集视图上做——CI 全集检查与 rrd boot 共用本实现，
//! 不允许第二处真值。
//!
//! 裁定速记（全文见 ADR-0009）：
//! - 依赖对象永远是**能力名**（ADR-0002 §2）；满足者 = `base_capabilities` ∪ 全集
//!   `provides.capabilities`。base 能力不建边（基座不在 boot 序里），其余能力向其
//!   **全部**提供者建边（多提供者"任一存活即满足"是运行期语义，启动期全排前、保守且确定）。
//! - 启动序 = Kahn 拓扑序，层内按 `start_priority` 升序、同值按名字典序（确定性裁定）。
//! - 环（含自环：依赖只有自己 provides 的能力）→ 拒整个 boot；能力不可满足 → 只拒该模块。
//!   两条分流由 [`rr3_registry_plan_boot`] 一次裁清（rrd boot 的唯一入口）；纯拓扑序
//!   [`rr3_registry_resolve_boot_order`] 保留给 CI 全集检查与契约用例。
//! - `depends.optional` 不参与：不进启动序、不触发降级（v0.0.1 文档性声明）。

use std::collections::{BTreeMap, BTreeSet};

use toml::Value;

/// 环拒绝错误行的固定开头（本文件内唯一拼装点，`plan_boot` 靠它把"拒整个 boot"与
/// "只拒点名模块"分流——rrd 不解析错误文案，第二处前缀字符串就是第二本真值）。
const RR3_CYCLE_ERROR_HEAD: &str = "依赖环涉及";

use crate::schema::{rr3_registry_get_by_path, rr3_registry_rule_list};

/// 单个模块参与仲裁所需的事实（从 manifest 文本提取；不做规则校验，
/// 语法归 `validate.rs` 的单件层）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3DependsFacts {
    pub name: String,
    pub provides: Vec<String>,
    pub depends_required: Vec<String>,
    pub start_priority: i64,
}

fn string_list(root: &Value, path: &str, errs: &mut Vec<String>) -> Vec<String> {
    match rr3_registry_get_by_path(root, path).and_then(Value::as_array) {
        None => vec![],
        Some(arr) => {
            let mut out = Vec::new();
            for (idx, item) in arr.iter().enumerate() {
                match item.as_str() {
                    // 非字符串项必须报告，不许被 filter_map 静默吞掉（fail-open）
                    Some(s) => out.push(s.to_string()),
                    None => errs.push(format!(
                        "{path} 第 {} 项应为字符串，实际为 {item:?}",
                        idx + 1
                    )),
                }
            }
            out
        }
    }
}

/// 不可满足错误行（唯一拼装点）：`resolve` 生成、`plan_boot` 按前缀归还给点名模块。
/// 名字后跟空格再跟"依赖"，所以 "rr3-a" 不会误配 "rr3-ab" 的错误行。
fn unsatisfiable_error(name: &str, cap: &str) -> String {
    format!("模块 {name} 依赖了未知能力 {cap}（不在 base_capabilities，也无任何模块 provides）")
}

fn unsatisfiable_prefix(name: &str) -> String {
    format!("模块 {name} 依赖了未知能力")
}

/// 从 manifest 文本提取仲裁事实。`start_priority` 缺省 100（与 rrd 侧
/// `manifest.rs` 同一缺省）。名字缺失是硬错误——仲裁按名字建边与报错。
pub fn rr3_registry_depends_facts_from_text(text: &str) -> Result<Rr3DependsFacts, Vec<String>> {
    let root: Value =
        toml::from_str(text).map_err(|e| vec![format!("manifest TOML 解析失败: {e}")])?;
    let mut errs = Vec::new();
    let name = match rr3_registry_get_by_path(&root, "manifest.name").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        Some(_) => {
            errs.push("manifest.name 不得为空字符串（依赖仲裁按名字建边）".to_string());
            String::new()
        }
        None => {
            errs.push("缺少 manifest.name（依赖仲裁按名字建边）".to_string());
            String::new()
        }
    };
    let provides = string_list(&root, "provides.capabilities", &mut errs);
    let depends_required = string_list(&root, "depends.required", &mut errs);
    let start_priority = rr3_registry_get_by_path(&root, "resources.start_priority")
        .and_then(Value::as_integer)
        .unwrap_or(100);
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Rr3DependsFacts {
        name,
        provides,
        depends_required,
        start_priority,
    })
}

/// 依赖仲裁本体（ADR-0009 裁定 1/2）：返回 boot 启动序（模块名），或逐条错误。
/// 可满足性错误全收集完才判环——运维一次改对，不挤牙膏。
pub fn rr3_registry_resolve_boot_order(
    facts: &[Rr3DependsFacts],
) -> Result<Vec<String>, Vec<String>> {
    rr3_registry_resolve_boot_order_excluding(facts, &BTreeSet::new())
}

/// 仲裁引擎：`excluded` 里的名字**不参与启动序**（`plan_boot` 剔除的不可满足者），
/// 但**留在可满足性宇宙里**——它们的 provides 照样算数（manifest 合法，只是它自己
/// 起不来），也不作为边目标（不排在它后面等一个永远不会上线的位置）。
fn rr3_registry_resolve_boot_order_excluding(
    facts: &[Rr3DependsFacts],
    excluded: &BTreeSet<String>,
) -> Result<Vec<String>, Vec<String>> {
    let mut errs = Vec::new();

    // 名字唯一是建边前提：同名两份 manifest 的"谁依赖谁"无从谈起
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for f in facts {
        if !seen.insert(f.name.as_str()) {
            errs.push(format!(
                "manifest 名重复: {}（依赖仲裁要求模块名唯一）",
                f.name
            ));
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    let base: BTreeSet<String> = rr3_registry_rule_list("base_capabilities")
        .unwrap_or_default()
        .into_iter()
        .collect();

    // 裁定 1：可满足性 = base ∪ 全集 provides。错误逐条点名，全收集。
    // 被剔除的模块不再报：它反正不起，重仲裁时把同一错误再报一遍会把剔除
    // 变成 fatal（`plan_boot` 看到剔除集合没增长就走防御分支）。
    for f in facts {
        if excluded.contains(&f.name) {
            continue;
        }
        for cap in &f.depends_required {
            if base.contains(cap) {
                continue;
            }
            let provided = facts.iter().any(|g| g.provides.iter().any(|p| p == cap));
            if !provided {
                errs.push(unsatisfiable_error(&f.name, cap));
            }
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    // 裁定 2：消费者 → 全部提供者的边（base 能力无边；提供者含自己 → 自环，落进环拒绝）。
    // 边去重：一个消费者依赖同一提供者的两个能力是两条需求、一条边。
    // 被剔除的模块两侧都不建边：它不进启动序（没有"排在它后面"），它依赖谁也无关
    // （它反正不起）。
    let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new(); // (consumer, provider)
    for (ci, f) in facts.iter().enumerate() {
        if excluded.contains(&f.name) {
            continue;
        }
        for cap in &f.depends_required {
            if base.contains(cap) {
                continue;
            }
            for (pi, g) in facts.iter().enumerate() {
                if !excluded.contains(&g.name) && g.provides.iter().any(|p| p == cap) {
                    edges.insert((ci, pi));
                }
            }
        }
    }

    // Kahn：就绪集合按 (start_priority, name) 取最小——同层内 start_priority
    // 稳定升序、同值名字典序，结果与装载顺序无关
    let mut indegree: BTreeMap<usize, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (ci, pi) in &edges {
        *indegree.entry(*ci).or_insert(0) += 1;
        dependents.entry(*pi).or_default().push(*ci);
    }
    let mut ready: BTreeSet<(i64, &str)> = BTreeSet::new();
    for (idx, f) in facts.iter().enumerate() {
        if !excluded.contains(&f.name) && indegree.get(&idx).copied().unwrap_or(0) == 0 {
            ready.insert((f.start_priority, f.name.as_str()));
        }
    }
    let mut order = Vec::with_capacity(facts.len());
    while let Some(&(prio, name)) = ready.iter().next() {
        ready.remove(&(prio, name));
        let idx = facts
            .iter()
            .position(|f| f.name == name)
            .expect("名字已在 facts 里");
        order.push(name.to_string());
        if let Some(deps) = dependents.get(&idx) {
            for d in deps {
                let e = indegree.get_mut(d).expect("边表里出现过的消费者必有入度");
                *e -= 1;
                if *e == 0 {
                    let f = &facts[*d];
                    ready.insert((f.start_priority, f.name.as_str()));
                }
            }
        }
    }

    // 环（含自环）与其下游永远进不了就绪集合——整批拒（裁定 2）。被剔除的模块
    // 不算"卡住"：它们本来就不在启动序里。
    let boot_count = facts.iter().filter(|f| !excluded.contains(&f.name)).count();
    if order.len() != boot_count {
        let stuck: Vec<&str> = facts
            .iter()
            .map(|f| f.name.as_str())
            .filter(|n| !excluded.contains(*n) && !order.iter().any(|o| o == n))
            .collect();
        errs.push(format!(
            "{RR3_CYCLE_ERROR_HEAD}（含环下游，拒整个 boot）: {}",
            stuck.join(", ")
        ));
        return Err(errs);
    }
    Ok(order)
}

/// boot 仲裁的**完整裁定**（ADR-0009 裁定 2，rrd 只执行、不 reimplement）：
/// - `fatal` 非空 = 环 / 名重复 / 未知的错误形状 → **拒整个 boot**（fail-closed），
///   此时 `order` 为空、`rejected` 无意义；
/// - 否则 `order` = 可 boot 模块的启动序，`rejected` = 不可满足被拒的模块名——
///   它们的 provides **仍计入**可满足性宇宙（依赖它们能力的其余模块照常 boot，
///   运行期按裁定 3 降级），重仲裁剔除它们不会把下游连坐。
///   终止性：每个非 fatal 轮次至少剔除一个名字，`excluded` 有 `facts.len()` 上界。
pub fn rr3_registry_plan_boot(facts: &[Rr3DependsFacts]) -> Rr3BootPlan {
    let mut excluded: BTreeSet<String> = BTreeSet::new();
    loop {
        match rr3_registry_resolve_boot_order_excluding(facts, &excluded) {
            Ok(order) => {
                return Rr3BootPlan {
                    order,
                    rejected: excluded.into_iter().collect(),
                    fatal: Vec::new(),
                };
            }
            Err(errs) => {
                if errs.iter().any(|e| e.starts_with(RR3_CYCLE_ERROR_HEAD)) {
                    return Rr3BootPlan {
                        order: Vec::new(),
                        rejected: excluded.into_iter().collect(),
                        fatal: errs,
                    };
                }
                // 不可满足：把错误行点名的消费者剔除后重仲裁。错误行的前缀由本文件
                // 的 `unsatisfiable_error` 唯一拼装，这里的匹配与它同源。
                let before = excluded.len();
                for f in facts {
                    if excluded.contains(&f.name) {
                        continue;
                    }
                    if errs
                        .iter()
                        .any(|e| e.starts_with(&unsatisfiable_prefix(&f.name)))
                    {
                        excluded.insert(f.name.clone());
                    }
                }
                if excluded.len() == before {
                    // 防御：不是环、也不点名任何消费者 —— 未知错误形状不当"照常"处理
                    return Rr3BootPlan {
                        order: Vec::new(),
                        rejected: excluded.into_iter().collect(),
                        fatal: errs,
                    };
                }
            }
        }
    }
}

/// [`rr3_registry_plan_boot`] 的回报。字段语义见该函数文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3BootPlan {
    pub order: Vec<String>,
    pub rejected: Vec<String>,
    pub fatal: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(name: &str, provides: &[&str], deps: &[&str], prio: i64) -> Rr3DependsFacts {
        Rr3DependsFacts {
            name: name.to_string(),
            provides: provides.iter().map(|s| s.to_string()).collect(),
            depends_required: deps.iter().map(|s| s.to_string()).collect(),
            start_priority: prio,
        }
    }

    #[test]
    fn facts_extraction_with_default_priority() {
        let text = r#"
[manifest]
name = "rr3-x"
[provides]
capabilities = ["audio.capture"]
[depends]
required = ["bus.publish"]
[resources]
start_priority = 40
"#;
        let f = rr3_registry_depends_facts_from_text(text).expect("应能提取");
        assert_eq!(f.name, "rr3-x");
        assert_eq!(f.provides, vec!["audio.capture".to_string()]);
        assert_eq!(f.depends_required, vec!["bus.publish".to_string()]);
        assert_eq!(f.start_priority, 40);

        let bare = rr3_registry_depends_facts_from_text("[manifest]\nname = \"rr3-y\"\n")
            .expect("最小文本也应能提取");
        assert!(bare.provides.is_empty() && bare.depends_required.is_empty());
        assert_eq!(
            bare.start_priority, 100,
            "缺省 100，与 rrd manifest.rs 同一缺省"
        );
    }

    #[test]
    fn facts_missing_name_and_non_string_items_reported() {
        let errs = rr3_registry_depends_facts_from_text("[provides]\ncapabilities = [\"x\"]\n")
            .expect_err("缺名字必须报错");
        assert!(
            errs.iter().any(|e| e.contains("缺少 manifest.name")),
            "{errs:?}"
        );

        let errs = rr3_registry_depends_facts_from_text(
            "[manifest]\nname = \"rr3-x\"\n[provides]\ncapabilities = [1]\n[depends]\nrequired = [true]\n",
        )
        .expect_err("非字符串项必须报错");
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(
            errs.iter()
                .any(|e| e.contains("provides.capabilities 第 1 项")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("depends.required 第 1 项")),
            "{errs:?}"
        );
    }

    #[test]
    fn duplicate_names_rejected_before_edges() {
        let errs = rr3_registry_resolve_boot_order(&[
            facts("rr3-a", &["cap.x"], &["bus.publish"], 100),
            facts("rr3-a", &[], &[], 100),
        ])
        .expect_err("同名必须报错");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("manifest 名重复: rr3-a"), "{errs:?}");
    }

    #[test]
    fn tie_break_is_priority_then_name() {
        // 同层三模块：priority 决定先后，同 priority 按名字典序（b 在 c 前，与入参顺序无关）
        let order = rr3_registry_resolve_boot_order(&[
            facts("rr3-c", &[], &[], 50),
            facts("rr3-b", &[], &[], 50),
            facts("rr3-a", &[], &[], 10),
        ])
        .expect("无依赖必须成序");
        assert_eq!(order, vec!["rr3-a", "rr3-b", "rr3-c"]);
    }

    #[test]
    fn self_dependency_is_a_cycle() {
        // 自己 provides、base 也没有的能力被自己依赖 = 自环 → 整批拒
        let errs = rr3_registry_resolve_boot_order(&[
            facts("rr3-a", &["cap.x"], &["cap.x"], 100),
            facts("rr3-b", &[], &[], 100),
        ])
        .expect_err("自环必须拒整批");
        assert!(errs[0].contains("依赖环涉及"), "{errs:?}");
        assert!(errs[0].contains("rr3-a"), "{errs:?}");
        assert!(!errs[0].contains("rr3-b"), "无辜模块不得被点名: {errs:?}");
    }

    /// plan_boot：不可满足只拒点名模块，其余照常按拓扑序成序
    #[test]
    fn plan_boot_rejects_only_the_unsatisfiable_and_orders_the_rest() {
        let plan = rr3_registry_plan_boot(&[
            facts("rr3-bad", &[], &["cap.ghost"], 10),
            facts("rr3-c", &[], &["bus.publish"], 50),
            facts("rr3-b", &[], &[], 50),
        ]);
        assert!(plan.fatal.is_empty(), "{plan:?}");
        assert_eq!(plan.rejected, vec!["rr3-bad".to_string()]);
        // rr3-bad 被剔除后，剩余两个同 priority=50，按名字典序
        assert_eq!(plan.order, vec!["rr3-b".to_string(), "rr3-c".to_string()]);
    }

    /// plan_boot：被拒模块的 provides 仍计入可满足性——依赖它能力的下游不连坐，
    /// 也不排在"永远不会上线的位置"后面（剔除者不作为边目标）
    #[test]
    fn plan_boot_keeps_rejected_module_provides_for_dependents() {
        let plan = rr3_registry_plan_boot(&[
            facts("rr3-ghost", &[], &["cap.ghost"], 10),
            // rr3-ghost 自己不可满足，但它 provides cap.y；rr3-user 依赖 cap.y
            facts("rr3-ghost2", &["cap.y"], &["cap.ghost"], 20),
            facts("rr3-user", &[], &["cap.y"], 90),
        ]);
        assert!(plan.fatal.is_empty(), "{plan:?}");
        assert_eq!(
            plan.rejected,
            vec!["rr3-ghost".to_string(), "rr3-ghost2".to_string()]
        );
        assert_eq!(plan.order, vec!["rr3-user".to_string()]);
    }

    /// plan_boot：环照旧拒整个 boot（fatal），不产出半死集合
    #[test]
    fn plan_boot_fatals_on_cycle() {
        let plan = rr3_registry_plan_boot(&[
            facts("rr3-a", &["cap.a"], &["cap.b"], 100),
            facts("rr3-b", &["cap.b"], &["cap.a"], 100),
            facts("rr3-c", &[], &[], 100),
        ]);
        assert!(
            plan.fatal.iter().any(|e| e.starts_with("依赖环涉及")),
            "{plan:?}"
        );
        assert!(plan.order.is_empty(), "{plan:?}");
    }
}
