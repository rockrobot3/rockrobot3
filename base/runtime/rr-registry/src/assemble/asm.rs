//! [assemble] 表逐段校验：版本/顶层键/空面/handles/ops/demands/内存账。

use super::*;

/// 顶层形状：schema_version（必须 2）与顶层未知键（只认 schema_version / assemble）。
pub(super) fn rr3_asm_head(t: &toml::Table, errs: &mut Vec<String>) {
    rr3_asm_ver(t, errs);
    rr3_asm_top_keys(t, errs);
}

/// 顶层未知键（只认 schema_version / assemble）。
fn rr3_asm_top_keys(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        rr3_asm_top_key_err(k, errs);
    }
}

/// 单个顶层键裁决（只认 schema_version / assemble）。
fn rr3_asm_top_key_err(k: &str, errs: &mut Vec<String>) {
    if k != "schema_version" && k != "assemble" {
        errs.push(format!(
            "装配清单顶层未知键 {k}（只认 schema_version / assemble）"
        ));
    }
}

/// schema_version 版本对账（本文件定形即 2）。
fn rr3_asm_ver(t: &toml::Table, errs: &mut Vec<String>) {
    // v1 是"无代码读它"时代的死数据形状，直接点名不迁就
    let Some(v) = t.get("schema_version") else {
        errs.push(format!(
            "装配清单缺 schema_version（必须为 {RR3_ASSEMBLE_SCHEMA_VERSION}）"
        ));
        return;
    };
    let Value::Integer(v) = v else {
        errs.push(format!("装配清单 schema_version 应为整数，收到 {v:?}"));
        return;
    };
    if *v != RR3_ASSEMBLE_SCHEMA_VERSION as i64 {
        errs.push(format!(
            "装配清单 schema_version 必须为 {RR3_ASSEMBLE_SCHEMA_VERSION}，收到 {v}"
        ));
    }
}

/// `[assemble]` 未知键点名（内存申报键 task_stack_bytes/heap_peak_bytes 是件级
/// 专用——组合件级 [assemble] 不认，同 demands：内存账是原子件的物理属性）。
pub(super) fn rr3_asm_keys(asm: &toml::Table, errs: &mut Vec<String>) {
    for k in asm.keys() {
        rr3_asm_key_err(k, errs);
    }
}

/// `[assemble]` 单个键裁决（六键外点名）。
fn rr3_asm_key_err(k: &str, errs: &mut Vec<String>) {
    if ![
        "handles",
        "ops",
        "no_injection",
        "demands",
        "task_stack_bytes",
        "heap_peak_bytes",
    ]
    .contains(&k)
    {
        errs.push(format!(
            "[assemble] 未知键 {k}（只认 handles / ops / no_injection / demands / task_stack_bytes / heap_peak_bytes）"
        ));
    }
}

/// no_injection = true 是"本件不需注入面"的显式声明（L-5）：只认布尔 true。
pub(super) fn rr3_asm_noinj(
    asm: &toml::Table,
    facts: &mut Rr3AssembleFacts,
    errs: &mut Vec<String>,
) {
    let Some(v) = asm.get("no_injection") else {
        return;
    };
    let Value::Boolean(b) = v else {
        errs.push(format!("no_injection 应为布尔 true，收到 {v:?}"));
        return;
    };
    if !*b {
        errs.push("no_injection 只认 true（本件不需注入面时明写；非空面不得写此键）".into());
        return;
    }
    facts.no_injection = true;
}

/// handles：数组，每条 = { trait, desc_key }，两键都在册表内（解析与组合件级共用
/// 一份）。返回（键在文本中出现, 数组为空）供 L-5 空面规则使用。
pub(super) fn rr3_asm_handles(
    asm: &toml::Table,
    facts: &mut Rr3AssembleFacts,
    errs: &mut Vec<String>,
) -> (bool, bool) {
    let Some(v) = asm.get("handles") else {
        errs.push("[assemble] 缺 handles 键（空数组合法，键本身必填）".into());
        return (false, false);
    };
    let Some(items) = v.as_array() else {
        errs.push(format!("[assemble].handles 应为数组，收到 {v:?}"));
        return (false, false);
    };
    let (parsed, mut item_errs) = rr3_handles_parse(items);
    facts.handles = parsed;
    errs.append(&mut item_errs);
    (true, items.is_empty())
}

/// ops：数组，每条 = { slot, src_key }，slot 非空、src_key 为合法点路径（共用同上）。
/// 返回（键在文本中出现, 数组为空）供 L-5 空面规则使用。
pub(super) fn rr3_asm_ops(
    asm: &toml::Table,
    facts: &mut Rr3AssembleFacts,
    errs: &mut Vec<String>,
) -> (bool, bool) {
    let Some(v) = asm.get("ops") else {
        errs.push("[assemble] 缺 ops 键（空数组合法，键本身必填）".into());
        return (false, false);
    };
    let Some(items) = v.as_array() else {
        errs.push(format!("[assemble].ops 应为数组，收到 {v:?}"));
        return (false, false);
    };
    let (parsed, mut item_errs) = rr3_ops_parse(items);
    facts.ops = parsed;
    errs.append(&mut item_errs);
    (true, items.is_empty())
}

/// demands：电流需求申报（基-36 首件）。缺键 = 空申报合法（执法面归门 c，
/// 不在形状层强制——L-5 空面规则只管注入面 handles/ops，不涉 demands）。
pub(super) fn rr3_asm_demands(
    asm: &toml::Table,
    facts: &mut Rr3AssembleFacts,
    errs: &mut Vec<String>,
) {
    let Some(v) = asm.get("demands") else {
        return;
    };
    let Some(items) = v.as_array() else {
        errs.push(format!("[assemble].demands 应为数组，收到 {v:?}"));
        return;
    };
    let (parsed, mut item_errs) = rr3_demands_parse(items);
    facts.demands = parsed;
    errs.append(&mut item_errs);
}

/// task_stack_bytes / heap_peak_bytes：内存需求申报（校验⑩ 分子面，批N2 n2c-v10）。
/// 缺键 = 未申报 = 0 不进 Σ（申报义务未触发——板侧任务映射未建模，0.0.7 批G/批D1；
/// 形状层不强制，同 demands 的"缺键 = 空申报合法"）；给键必须是非负整数
/// （负数 = 物理矛盾，fail-closed）。件级语义非每实例——判据消费在
/// rrd `rr3_memory_budget`，直加不扩算。
pub(super) fn rr3_asm_mem(asm: &toml::Table, facts: &mut Rr3AssembleFacts, errs: &mut Vec<String>) {
    for (key, slot) in [
        ("task_stack_bytes", &mut facts.task_stack_bytes),
        ("heap_peak_bytes", &mut facts.heap_peak_bytes),
    ] {
        rr3_asm_mem_one(asm, key, slot, errs);
    }
}

/// 单个内存申报键：非负整数入档，负数/非整数各点名；缺键 = 未申报。
fn rr3_asm_mem_one(asm: &toml::Table, key: &str, slot: &mut i64, errs: &mut Vec<String>) {
    let Some(v) = asm.get(key) else {
        return;
    };
    let Value::Integer(v) = v else {
        errs.push(format!("[assemble].{key} 应为整数，收到 {v:?}"));
        return;
    };
    if *v >= 0 {
        *slot = *v;
        return;
    }
    errs.push(format!(
        "[assemble].{key} = {v} 须为非负整数（本件内存占用字节数，负数是物理矛盾）"
    ));
}
