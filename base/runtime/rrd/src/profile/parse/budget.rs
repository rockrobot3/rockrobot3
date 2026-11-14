//! [supplies.budget] 段解读：不适用位、三数键与分档自洽判据。

use std::collections::BTreeMap;

use super::super::model::*;

pub(super) fn rr3_budget_into(
    t: &toml::Table,
    supplies: &mut Rr3Supplies,
    tier: &str,
    errs: &mut Vec<String>,
) {
    match t.get("budget").and_then(toml::Value::as_table) {
        None => errs.push(
            "缺少 [supplies.budget]（内存预算声明，校验⑩ 分母；schema_version=4 起必填——host 档显式写 not_applicable = true，不许留空）"
                .to_string(),
        ),
        Some(bt) => supplies.budget = rr3_budget_parse(bt, tier, errs),
    }
}

fn rr3_budget_parse(bt: &toml::Table, tier: &str, errs: &mut Vec<String>) -> Rr3Budget {
    let mut budget = Rr3Budget::default();
    rr3_budget_keys(bt, errs);
    match bt.get("not_applicable").and_then(toml::Value::as_bool) {
        Some(b) => budget.not_applicable = b,
        None => errs.push(
            "supplies.budget.not_applicable 缺失或不是布尔（校验⑩ 不适用必须显式声明，不许留空——F-6）"
                .to_string(),
        ),
    }
    let nums = rr3_budget_nums(bt, &mut budget, errs);
    rr3_budget_tier(tier, &budget, &nums, errs);
    budget
}

/// 分档自洽（校验⑩）：host 走不适用位，其余档走数字预算对账。
fn rr3_budget_tier(
    tier: &str,
    budget: &Rr3Budget,
    nums: &BTreeMap<&'static str, i64>,
    errs: &mut Vec<String>,
) {
    if !tier.is_empty() {
        if tier == "host" {
            rr3_budget_host(budget, nums, errs);
        } else {
            rr3_budget_rtos(budget, nums, errs);
        }
    }
}

/// 未知键点名：预算段只认 [`RR3_BUDGET_KEYS`] 四键（三数 + 不适用位）。
fn rr3_budget_keys(bt: &toml::Table, errs: &mut Vec<String>) {
    for k in bt.keys() {
        if !RR3_BUDGET_KEYS.contains(&k.as_str()) {
            errs.push(format!(
                "[supplies.budget] 未知键 {k}（只认 {}）",
                RR3_BUDGET_KEYS.join("/")
            ));
        }
    }
}

/// 三数键收集（类型错点名）：返回"本次出现的键→值"，分档自洽判据共用。
fn rr3_budget_nums(
    bt: &toml::Table,
    budget: &mut Rr3Budget,
    errs: &mut Vec<String>,
) -> BTreeMap<&'static str, i64> {
    let mut nums: BTreeMap<&'static str, i64> = BTreeMap::new();
    for key in [
        "task_stack_budget_bytes",
        "heap_total_bytes",
        "heap_reserved_bytes",
    ] {
        match bt.get(key).and_then(toml::Value::as_integer) {
            Some(v) => {
                nums.insert(key, v);
                rr3_budget_slot(budget, key, v);
            }
            None => {
                if bt.get(key).is_some() {
                    errs.push(format!("supplies.budget.{key} 应为整数"));
                }
            }
        }
    }
    nums
}

/// 三数键落位：按固定键名写入预算字段。
fn rr3_budget_slot(budget: &mut Rr3Budget, key: &'static str, v: i64) {
    match key {
        "task_stack_budget_bytes" => budget.task_stack_budget_bytes = v,
        "heap_total_bytes" => budget.heap_total_bytes = v,
        _ => budget.heap_reserved_bytes = v,
    }
}

/// host 档自洽：不适用必须显式为 true，且三数键不得并存（F-6 / ADR-0021 修订 3）。
fn rr3_budget_host(budget: &Rr3Budget, nums: &BTreeMap<&'static str, i64>, errs: &mut Vec<String>) {
    if !budget.not_applicable {
        errs.push(
            "分档自洽校验失败（校验⑩）：tier = host 但 supplies.budget.not_applicable = false——校验⑩ 对 host 档不适用，合法不适用必须显式写 true，不许留空（F-6 / ADR-0021 修订 3）"
                .to_string(),
        );
    }
    if !nums.is_empty() {
        errs.push(format!(
            "分档自洽校验失败（校验⑩）：tier = host 但 [supplies.budget] 并存数字预算键 {}（与 not_applicable = true 自相矛盾——栈/堆预算只属于 rtos/bare 档，ADR-0021 修订 3）",
            nums.keys().copied().collect::<Vec<_>>().join("/")
        ));
    }
}

/// rtos/bare 档自洽：三数必填、正负域与"可用堆 > 0"物理判据。
fn rr3_budget_rtos(budget: &Rr3Budget, nums: &BTreeMap<&'static str, i64>, errs: &mut Vec<String>) {
    if budget.not_applicable {
        errs.push(
            "分档自洽校验失败（校验⑩）：tier = rtos/bare 但 supplies.budget.not_applicable = true（rtos/bare 档有真栈/堆预算，写不适用是自相矛盾——ADR-0021 修订 3）"
                .to_string(),
        );
    }
    rr3_budget_need(nums, errs);
    rr3_budget_limits(nums, errs);
}

/// 三数必填位（rtos/bare 档）：逐键缺失点名。
fn rr3_budget_need(nums: &BTreeMap<&'static str, i64>, errs: &mut Vec<String>) {
    for key in [
        "task_stack_budget_bytes",
        "heap_total_bytes",
        "heap_reserved_bytes",
    ] {
        if !nums.contains_key(key) {
            errs.push(format!(
                "supplies.budget.{key} 缺失（rtos/bare 档必填——校验⑩ 分母，合法不适用才走 not_applicable，不许留空——F-6）"
            ));
        }
    }
}

/// 三数正负域与"可用堆 > 0"物理判据（三数全在才判）。
fn rr3_budget_limits(nums: &BTreeMap<&'static str, i64>, errs: &mut Vec<String>) {
    if let (Some(&st), Some(&tot), Some(&res)) = (
        nums.get("task_stack_budget_bytes"),
        nums.get("heap_total_bytes"),
        nums.get("heap_reserved_bytes"),
    ) {
        rr3_budget_signs(st, tot, res, errs);
        if tot > 0 && res >= 0 && tot <= res {
            errs.push(format!(
                "supplies.budget.heap_total_bytes = {tot} ≤ heap_reserved_bytes = {res}（可用堆 = total − reserved ≤ 0，⑩-b 无分母——物理矛盾）"
            ));
        }
    }
}

/// 三数正负域判据：栈/堆基数须正、预留非负。
fn rr3_budget_signs(st: i64, tot: i64, res: i64, errs: &mut Vec<String>) {
    if st <= 0 {
        errs.push(format!(
            "supplies.budget.task_stack_budget_bytes = {st} 须为正整数（⑩-a 分母，0 预算 = 栈账无分母）"
        ));
    }
    if tot <= 0 {
        errs.push(format!(
            "supplies.budget.heap_total_bytes = {tot} 须为正整数（⑩-b 分母基数）"
        ));
    }
    if res < 0 {
        errs.push(format!(
            "supplies.budget.heap_reserved_bytes = {res} 须为非负整数（平台常驻占用，负数是物理矛盾）"
        ));
    }
}
