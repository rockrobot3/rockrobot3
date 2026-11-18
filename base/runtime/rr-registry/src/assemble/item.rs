//! 条目级解析：handles / ops / demands 三项数组的形状与语义校验。

use super::*;

/// 点路径合法性：按 `.` 切分后每段非空（空串/首尾点/连续点都不认）
fn rr3_dotted_key_ok(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(|seg| !seg.is_empty())
}

/// 内联表条目骨架（单点）：非表即拒（点名期望形状）、未知键逐条点名（只认名单内
/// 的键）。两条拒因都进 `errs`（收集式校验：一次报全，不遇错即返）。
fn rr3_item_tbl<'a>(
    item: &'a Value,
    what: &str,
    i: usize,
    shape: &str,
    keys: &[&str],
    errs: &mut Vec<String>,
) -> Option<&'a Value> {
    let Some(t) = item.as_table() else {
        errs.push(format!("{what}[{i}] 应为内联表 {shape}"));
        return None;
    };
    for k in t.keys() {
        if !keys.contains(&k.as_str()) {
            errs.push(format!(
                "{what}[{i}] 未知键 {k}（只认 {}）",
                keys.join(" / ")
            ));
        }
    }
    Some(item)
}

/// 必填字符串键（单点）：缺键/非串与"串但不合判据"两类拒因分述，判据不满足时把
/// 值原样点名（`bad` 承载判据说明）。返 None 表示该键已拒，条目不入列。
fn rr3_item_str<'a>(
    t: &'a Value,
    what: &str,
    i: usize,
    key: &str,
    bad: &str,
    ok: impl Fn(&str) -> bool,
    errs: &mut Vec<String>,
) -> Option<&'a str> {
    let Some(v) = t.get(key).and_then(Value::as_str) else {
        errs.push(format!("{what}[{i}].{key} 应为字符串"));
        return None;
    };
    if ok(v) {
        return Some(v);
    }
    // 值原样点名（空串不回声，避免 `slot "" 不得为空` 这类噪声）
    rr3_item_str_bad(what, i, key, v, bad, errs);
    None
}

/// 串键值不合判据时的点名（空串不回声值，其余原样点名）。
fn rr3_item_str_bad(what: &str, i: usize, key: &str, v: &str, bad: &str, errs: &mut Vec<String>) {
    if v.is_empty() {
        errs.push(format!("{what}[{i}].{key} {bad}"));
    } else {
        errs.push(format!("{what}[{i}].{key} \"{v}\" {bad}"));
    }
}

/// handles 数组条目级解析与校验（件级与组合件级**同形同规则**，composite.rs 共用；
/// 点名文案两处一致——一份判据一份真值）
pub(crate) fn rr3_handles_parse(items: &[Value]) -> (Vec<Rr3AssembleHandle>, Vec<String>) {
    let mut handles = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        rr3_handle_item(item, i, &mut handles, &mut errs);
    }
    (handles, errs)
}

/// 单条 handle：未知键 + trait/desc_key 双域在册（两域都合法才入列）。
fn rr3_handle_item(
    item: &Value,
    i: usize,
    handles: &mut Vec<Rr3AssembleHandle>,
    errs: &mut Vec<String>,
) {
    let Some(h) = rr3_item_tbl(
        item,
        "handles",
        i,
        "{ trait, desc_key }",
        &["trait", "desc_key"],
        errs,
    ) else {
        return;
    };
    let trait_name = rr3_item_str(
        h,
        "handles",
        i,
        "trait",
        "未在册（能力面表见 assemble.rs，扩面走契约级评审）",
        |s| RR3_ASSEMBLE_HANDLE_TRAITS.contains(&s),
        errs,
    );
    let desc_key = rr3_item_str(
        h,
        "handles",
        i,
        "desc_key",
        "未在册（键表见 assemble.rs / descriptor.rs，扩键走契约级评审）",
        |s| RR3_ASSEMBLE_DESC_KEYS.contains(&s),
        errs,
    );
    if let (Some(tr), Some(dk)) = (trait_name, desc_key) {
        handles.push(Rr3AssembleHandle {
            trait_name: tr.into(),
            desc_key: dk.into(),
        });
    }
}

/// ops 数组条目级解析与校验（同上：件级与组合件级共用）
pub(crate) fn rr3_ops_parse(items: &[Value]) -> (Vec<Rr3AssembleOp>, Vec<String>) {
    let mut ops = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        rr3_op_item(item, i, &mut ops, &mut errs);
    }
    (ops, errs)
}

/// 单条 op：未知键 + slot 非空 + src_key 点路径（两域都合法才入列）。
fn rr3_op_item(item: &Value, i: usize, ops: &mut Vec<Rr3AssembleOp>, errs: &mut Vec<String>) {
    let Some(o) = rr3_item_tbl(
        item,
        "ops",
        i,
        "{ slot, src_key }",
        &["slot", "src_key"],
        errs,
    ) else {
        return;
    };
    let slot = rr3_item_str(
        o,
        "ops",
        i,
        "slot",
        "不得为空",
        |s| !s.trim().is_empty(),
        errs,
    );
    let src_key = rr3_item_str(
        o,
        "ops",
        i,
        "src_key",
        "不是合法点路径（按 . 切分后每段须非空）",
        rr3_dotted_key_ok,
        errs,
    );
    if let (Some(s), Some(k)) = (slot, src_key) {
        ops.push(Rr3AssembleOp {
            slot: s.into(),
            src_key: k.into(),
        });
    }
}

/// demands 数组条目级解析与校验（**件级专用**：组合件级 [assemble] 不认 demands——
/// 电流需求是原子件的物理属性，不随组合件申报，组合件清单未知键点名即拦）。
/// 五键全部显式（T-5 / ADR-0021 裁定 2）：`concurrency_group` 缺键即红（同 no_injection
/// 范式——缺省与漏填不可区分），显式空数组 = 不分组 = 全体并发最坏口径，合法。
pub(crate) fn rr3_demands_parse(items: &[Value]) -> (Vec<Rr3AssembleDemand>, Vec<String>) {
    let mut demands = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        rr3_demand_item(item, i, &mut demands, &mut errs);
    }
    (demands, errs)
}

/// 单条 demand：骨架 → 五键 → 交叉判据 → 齐备入列（拒项不入列）。
fn rr3_demand_item(
    item: &Value,
    i: usize,
    demands: &mut Vec<Rr3AssembleDemand>,
    errs: &mut Vec<String>,
) {
    let Some(d) = rr3_item_tbl(
        item,
        "demands",
        i,
        "{ rail, peak_ma, sustained_ma, critical, concurrency_group }",
        &[
            "rail",
            "peak_ma",
            "sustained_ma",
            "critical",
            "concurrency_group",
        ],
        errs,
    ) else {
        return;
    };
    let rail = rr3_demand_rail(d, i, errs);
    let peak = rr3_demand_peak(d, i, errs);
    let sustained = rr3_demand_sust(d, i, errs);
    rr3_demand_cmp(peak, sustained, i, errs);
    let critical = rr3_demand_crit(d, i, errs);
    let groups = rr3_demand_grp(d, i, errs);
    if let (Some(rail), Some(peak_ma), Some(sustained_ma), Some(critical)) =
        (rail, peak, sustained, critical)
    {
        rr3_demand_push(demands, rail, peak_ma, sustained_ma, critical, groups);
    }
}

/// 五键齐备的需求入列（判据已在前置各步判完，此处只成型）。
fn rr3_demand_push(
    demands: &mut Vec<Rr3AssembleDemand>,
    rail: String,
    peak_ma: i64,
    sustained_ma: i64,
    critical: bool,
    concurrency_group: Vec<String>,
) {
    demands.push(Rr3AssembleDemand {
        rail,
        peak_ma,
        sustained_ma,
        critical,
        concurrency_group,
    });
}

/// 未知键点名（五键定形：拼错键不能被静默忽略）。
/// rail 键：非空字符串。
fn rr3_demand_rail(d: &Value, i: usize, errs: &mut Vec<String>) -> Option<String> {
    let Some(r) = d.get("rail").and_then(Value::as_str) else {
        errs.push(format!("demands[{i}].rail 应为字符串"));
        return None;
    };
    if r.trim().is_empty() {
        errs.push(format!("demands[{i}].rail 不得为空"));
        return None;
    }
    Some(r.to_string())
}

/// peak_ma：正整数（每实例峰值电流 mA）。
fn rr3_demand_peak(d: &Value, i: usize, errs: &mut Vec<String>) -> Option<i64> {
    let Some(v) = d.get("peak_ma").and_then(Value::as_integer) else {
        errs.push(format!("demands[{i}].peak_ma 应为整数"));
        return None;
    };
    if v > 0 {
        return Some(v);
    }
    errs.push(format!(
        "demands[{i}].peak_ma = {v} 须为正整数（mA，每实例峰值电流需求）"
    ));
    None
}

/// sustained_ma：正整数，须 ≤ peak_ma（ADR-0021 裁定 2 五键定形）。
fn rr3_demand_sust(d: &Value, i: usize, errs: &mut Vec<String>) -> Option<i64> {
    let Some(v) = d.get("sustained_ma").and_then(Value::as_integer) else {
        errs.push(format!(
            "demands[{i}].sustained_ma 应为整数（连续分量，须 ≤ peak_ma；缺键即红——不给缺省）"
        ));
        return None;
    };
    if v > 0 {
        return Some(v);
    }
    errs.push(format!(
        "demands[{i}].sustained_ma = {v} 须为正整数（mA，每实例连续电流；ADR-0021 裁定 2 五键定形）"
    ));
    None
}

/// peak/sustained 交叉判据：连续分量超过峰值是物理矛盾（两值都可解析时才判）。
fn rr3_demand_cmp(peak: Option<i64>, sust: Option<i64>, i: usize, errs: &mut Vec<String>) {
    let (Some(p), Some(s)) = (peak, sust) else {
        return;
    };
    if p > 0 && s > p {
        errs.push(format!(
            "demands[{i}].sustained_ma = {s} > peak_ma = {p}（连续分量超过峰值是物理矛盾，fail-closed）"
        ));
    }
}

/// critical：必须显式布尔（true 进校验⑨ 预算加总）。
fn rr3_demand_crit(d: &Value, i: usize, errs: &mut Vec<String>) -> Option<bool> {
    let critical = d.get("critical").and_then(Value::as_bool);
    if critical.is_none() {
        errs.push(format!(
            "demands[{i}].critical 应为布尔（显式写不给缺省：true 进校验⑨ 预算加总）"
        ));
    }
    critical
}

/// concurrency_group（T-5）：必须显式写；空数组 = 不分组 = 全体并发（最坏口径）
/// 合法；非空元素须非空字符串（组名是判据 label 的一部分）。
fn rr3_demand_grp(d: &Value, i: usize, errs: &mut Vec<String>) -> Vec<String> {
    let Some(v) = d.get("concurrency_group") else {
        errs.push(format!(
            "demands[{i}].concurrency_group 缺失（T-5：必须显式写，[] = 不分组 = 全体并发最坏口径；同 no_injection 范式）"
        ));
        return Vec::new();
    };
    let Some(gs) = v.as_array() else {
        errs.push(format!(
            "demands[{i}].concurrency_group 应为字符串数组（[] 显式不分组合法——T-5 必须显式写）"
        ));
        return Vec::new();
    };
    let mut out = Vec::new();
    for (j, g) in gs.iter().enumerate() {
        let Some(s) = rr3_grp_name(g, i, j, errs) else {
            continue;
        };
        out.push(s);
    }
    out
}

/// 单个组名：非空字符串入列，坏元素逐条点名（不静默丢弃）。
fn rr3_grp_name(g: &Value, i: usize, j: usize, errs: &mut Vec<String>) -> Option<String> {
    let Some(s) = g.as_str() else {
        errs.push(format!(
            "demands[{i}].concurrency_group[{j}] 应为字符串（组名）"
        ));
        return None;
    };
    if !s.trim().is_empty() {
        return Some(s.to_string());
    }
    errs.push(format!(
        "demands[{i}].concurrency_group[{j}] 不得为空（组名是互斥判据的 label）"
    ));
    None
}
