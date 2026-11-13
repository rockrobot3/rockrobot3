//! 依赖仲裁（ADR-0009）：提供者先启、优先级、环拒绝、全集可满足性。

use super::*;

#[test]
fn depends_chain_boots_providers_first_with_priority_tie_break() {
    // 链式：c 依赖 b 提供的能力、b 依赖 a 提供的能力 → a、b 必须排在 c 前；
    // 就绪集合里 priority 小者先行（d=40 先于 a=50），同层排序与装载顺序无关
    let order = rr_registry::rr3_boot_order(&facts_of(&[
        depends_manifest("rr3-cc", &[], &["cap.mid"], 10),
        depends_manifest("rr3-bb", &["cap.mid"], &["cap.low"], 60),
        depends_manifest("rr3-aa", &["cap.low"], &[], 50),
        depends_manifest("rr3-dd", &[], &[], 40),
    ]))
    .expect("链式必须成序");
    assert_eq!(order, vec!["rr3-dd", "rr3-aa", "rr3-bb", "rr3-cc"]);
}

#[test]
fn depends_cycle_rejects_whole_batch() {
    // 环（loop1↔loop2）+ 环下游（down）→ 整批拒且逐个点名；无辜者不得被点名
    let errs = rr_registry::rr3_boot_order(&facts_of(&[
        depends_manifest("rr3-loop1", &["cap.p"], &["cap.q"], 100),
        depends_manifest("rr3-loop2", &["cap.q"], &["cap.p"], 100),
        depends_manifest("rr3-down", &[], &["cap.p"], 100),
        depends_manifest("rr3-ok", &[], &[], 100),
    ]))
    .expect_err("环必须拒整批");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("依赖环涉及"), "{errs:?}");
    for name in ["rr3-loop1", "rr3-loop2", "rr3-down"] {
        assert!(errs[0].contains(name), "缺 {name}: {errs:?}");
    }
    assert!(!errs[0].contains("rr3-ok"), "无辜模块被点名: {errs:?}");
}

#[test]
fn depends_base_capability_needs_no_module_provider() {
    // base_capabilities 永远在位（基座不在 boot 序里），不建边、不需要提供者
    let order = rr_registry::rr3_boot_order(&facts_of(&[depends_manifest(
        "rr3-solo",
        &[],
        &["bus.publish", "net.online"],
        100,
    )]))
    .expect("只依赖 base 能力必须成序");
    assert_eq!(order, vec!["rr3-solo"]);
}

#[test]
fn depends_multiple_providers_all_boot_before_consumer() {
    // 两个提供者并存：启动期全部排前（裁定 2 的保守全序），层内按 priority
    let order = rr_registry::rr3_boot_order(&facts_of(&[
        depends_manifest("rr3-use", &[], &["cap.x"], 10),
        depends_manifest("rr3-p2", &["cap.x"], &[], 60),
        depends_manifest("rr3-p1", &["cap.x"], &[], 50),
    ]))
    .expect("多提供者必须成序");
    assert_eq!(order, vec!["rr3-p1", "rr3-p2", "rr3-use"]);
}

#[test]
fn every_module_dependency_set_is_satisfiable_and_acyclic() {
    // CI 全集补位：单件校验不再判"未知能力"（ADR-0009 裁定 1），modules/ 六模块
    // 的整体依赖面必须在仲裁层过——否则要等 rrd boot 才暴露
    let modules = repo_root().join("modules");
    let mut facts = Vec::new();
    for entry in std::fs::read_dir(&modules)
        .expect("读不到 modules/ 目录")
        .filter_map(|e| e.ok())
    {
        let m = entry.path().join("module.toml");
        if m.is_file() {
            facts.push(
                rr_registry::rr3_depends_parse(&std::fs::read_to_string(&m).unwrap())
                    .expect("模块 manifest 事实提取不该失败"),
            );
        }
    }
    assert_eq!(facts.len(), 6, "modules/ 模块集合漂移");
    let order = rr_registry::rr3_boot_order(&facts).expect("六模块依赖面必须可满足且无环");
    assert_eq!(order.len(), 6, "{order:?}");
}
