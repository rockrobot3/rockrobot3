// SPDX-License-Identifier: Apache-2.0
//! Kahn 拓扑序（自 `depends.rs` 抽出）：入度表/反邻接表、就绪集合与逐出释放。
//!
//! 判据不变：层内 `start_priority` 升序、同值按名字典序，结果与装载顺序无关；
//! 名字唯一性由调用方（`rr3_boot_order_excl`）先验，就绪集合可安全携带 idx。

use std::collections::{BTreeMap, BTreeSet};

use super::Rr3DependsFacts;

/// Kahn 拓扑：就绪集合按 (start_priority, name) 取最小——同层内 start_priority
/// 稳定升序、同值名字典序，结果与装载顺序无关。
pub(super) fn rr3_kahn(
    facts: &[Rr3DependsFacts],
    excluded: &BTreeSet<String>,
    edges: &BTreeSet<(usize, usize)>,
) -> Vec<String> {
    let (mut indegree, dependents) = rr3_kahn_tables(facts.len(), edges);
    let mut ready = rr3_kahn_ready(facts, excluded, &indegree);
    let mut order = Vec::with_capacity(facts.len());
    while let Some(&(prio, name, idx)) = ready.iter().next() {
        ready.remove(&(prio, name, idx));
        order.push(name.to_string());
        rr3_kahn_release(facts, &dependents, &mut indegree, &mut ready, idx);
    }
    order
}

/// 入度表与反邻接表（边去重后一次建成）。入度按模块下标定长直取——边由
/// `facts` 的枚举下标构造，表长即模块数，查不到键这一态在结构上不存在。
fn rr3_kahn_tables(
    n: usize,
    edges: &BTreeSet<(usize, usize)>,
) -> (Vec<usize>, BTreeMap<usize, Vec<usize>>) {
    let mut indegree = vec![0usize; n];
    let mut dependents: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (ci, pi) in edges {
        indegree[*ci] += 1;
        dependents.entry(*pi).or_default().push(*ci);
    }
    (indegree, dependents)
}

/// 初始就绪集合：非剔除且入度为 0。
fn rr3_kahn_ready<'a>(
    facts: &'a [Rr3DependsFacts],
    excluded: &BTreeSet<String>,
    indegree: &[usize],
) -> BTreeSet<(i64, &'a str, usize)> {
    let mut ready: BTreeSet<(i64, &str, usize)> = BTreeSet::new();
    for (idx, f) in facts.iter().enumerate() {
        if rr3_ready_at(f, idx, excluded, indegree) {
            ready.insert((f.start_priority, f.name.as_str(), idx));
        }
    }
    ready
}

/// 就绪判据：未被剔除且入度为 0（两条判据同处，次序不变）。
fn rr3_ready_at(
    f: &Rr3DependsFacts,
    idx: usize,
    excluded: &BTreeSet<String>,
    indegree: &[usize],
) -> bool {
    !excluded.contains(&f.name) && indegree[idx] == 0
}

/// 出一个模块后释放其下游：入度减一，归零者入就绪集合。
fn rr3_kahn_release<'a>(
    facts: &'a [Rr3DependsFacts],
    dependents: &BTreeMap<usize, Vec<usize>>,
    indegree: &mut [usize],
    ready: &mut BTreeSet<(i64, &'a str, usize)>,
    idx: usize,
) {
    let Some(deps) = dependents.get(&idx) else {
        return;
    };
    for d in deps {
        rr3_release_one(facts, indegree, ready, *d);
    }
}

/// 释放单条下游边：入度减一，归零者入就绪集合。
fn rr3_release_one<'a>(
    facts: &'a [Rr3DependsFacts],
    indegree: &mut [usize],
    ready: &mut BTreeSet<(i64, &'a str, usize)>,
    d: usize,
) {
    indegree[d] -= 1;
    if indegree[d] == 0 {
        let f = &facts[d];
        ready.insert((f.start_priority, f.name.as_str(), d));
    }
}
