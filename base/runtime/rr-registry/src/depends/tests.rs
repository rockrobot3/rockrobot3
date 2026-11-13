//! depends 的单测（自同名单测模块原样抽出，判据不变）。

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
    let f = rr3_depends_parse(text).expect("应能提取");
    assert_eq!(f.name, "rr3-x");
    assert_eq!(f.provides, vec!["audio.capture".to_string()]);
    assert_eq!(f.depends_required, vec!["bus.publish".to_string()]);
    assert_eq!(f.start_priority, 40);

    let bare = rr3_depends_parse("[manifest]\nname = \"rr3-y\"\n").expect("最小文本也应能提取");
    assert!(bare.provides.is_empty() && bare.depends_required.is_empty());
    assert_eq!(
        bare.start_priority, 100,
        "缺省 100，与 rrd manifest.rs 同一缺省"
    );
}

#[test]
fn facts_missing_name_and_non_string_items_reported() {
    let errs =
        rr3_depends_parse("[provides]\ncapabilities = [\"x\"]\n").expect_err("缺名字必须报错");
    assert!(
        errs.iter().any(|e| e.contains("缺少 manifest.name")),
        "{errs:?}"
    );

    let errs = rr3_depends_parse(
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
    let errs = rr3_boot_order(&[
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
    let order = rr3_boot_order(&[
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
    let errs = rr3_boot_order(&[
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
    let plan = rr3_plan_boot(&[
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
    let plan = rr3_plan_boot(&[
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
    let plan = rr3_plan_boot(&[
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
