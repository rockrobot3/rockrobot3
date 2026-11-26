// SPDX-License-Identifier: Apache-2.0
//! 组合件装配清单（`modules/<名>/composite.toml`，三层 manifest 的 Composite 层）：
//! 申报组合哪些件、暴露哪些注入面。
//!
//! 版本域：顶层 `schema_version` 归 [`RR3_COMPOSITE_SCHEMA_VERSION`]（=1），件级
//! `assemble.toml` 归 [`crate::assemble::RR3_ASSEMBLE_SCHEMA_VERSION`]（=2）——同名
//! 不同域，不互认、不迁移、不互相 bump。
//!
//! 形状与件级 [`crate::assemble`] 同形同规则（解析共用其 helper）：`atoms` 非空、
//! 每条非空串、不重复；`handles`/`ops` 双空时必须明写 `no_injection = true`，把
//! "漏填"与"确无注入面"分开。fail-closed：键缺/取值出域逐条点名回 `Err`；对已
//! 激活形状做语义变更须升版本并走契约级评审，增键不 bump 只对无消费者形状成立。

use crate::assemble::{
    rr3_handles_key, rr3_no_injection_err, rr3_ops_key, Rr3AssembleHandle, Rr3AssembleOp,
};
use toml::Value;

/// 组合件清单 schema 版本（版本域裁定见模块头注：与件级 RR3_ASSEMBLE_SCHEMA_VERSION
/// = 2 同名不同域）
pub const RR3_COMPOSITE_SCHEMA_VERSION: u32 = 1;

/// `unit` 词域（执行单元形态；扩域走契约级评审）
pub const RR3_COMPOSITE_UNITS: [&str; 1] = ["process"];

/// 一份组合件清单的解析产物
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rr3CompositeFacts {
    /// `[composite].name`：组合件名（与模块名同源）
    pub name: String,
    /// `[composite].unit`：执行单元形态（词域见 [`RR3_COMPOSITE_UNITS`]）
    pub unit: String,
    /// `[assemble].atoms`：⊗ 链（原子件名，非空不重复）
    pub atoms: Vec<String>,
    /// 组合件级句柄申报（结构同件级）
    pub handles: Vec<Rr3AssembleHandle>,
    /// 组合件级参数槽申报（结构同件级）
    pub ops: Vec<Rr3AssembleOp>,
    /// L-5 显式声明：本组合件不需注入面
    pub no_injection: bool,
}

/// 解析 + 全量形状校验一份组合件清单文本。fail-closed：一条不静默放行，
/// 全部错误逐条点名（与件级装配清单同一口径）。
pub fn rr3_composite_parse(text: &str) -> Result<Rr3CompositeFacts, Vec<String>> {
    let t = rr3_comp_root(text)?;

    let mut errs: Vec<String> = Vec::new();
    let mut facts = Rr3CompositeFacts::default();
    rr3_comp_head(&t, &mut facts, &mut errs);

    // [assemble]：atoms 必填非空不重复；handles/ops/no_injection 与件级同规则。
    // 段缺同样只点名，让 schema/[composite] 的错误同批亮出。
    let Some(asm) = rr3_comp_asm_tbl(&t, &mut errs) else {
        return Err(errs);
    };
    rr3_comp_asm(asm, &mut facts, &mut errs);

    rr3_comp_finish(facts, errs)
}

/// 终局：错误集空交表，否则整册拒（与旧内联分支逐字同形）。
fn rr3_comp_finish(
    facts: Rr3CompositeFacts,
    errs: Vec<String>,
) -> Result<Rr3CompositeFacts, Vec<String>> {
    if errs.is_empty() {
        Ok(facts)
    } else {
        Err(errs)
    }
}

/// 取 `[assemble]` 段（缺段点名回 None，调用方按 errs 收兵）。
fn rr3_comp_asm_tbl<'a>(t: &'a toml::Table, errs: &mut Vec<String>) -> Option<&'a toml::Table> {
    let Some(asm) = t.get("assemble").and_then(Value::as_table) else {
        errs.push(
            "组合件清单缺 [assemble] 表（atoms 必填，handles / ops / no_injection 同件级规则）"
                .into(),
        );
        return None;
    };
    Some(asm)
}

/// 顶层 TOML 解析：语法错与"顶层非表"各自成条 Err（与旧实现逐字同文案）。
fn rr3_comp_root(text: &str) -> Result<toml::Table, Vec<String>> {
    let root: Value =
        toml::from_str(text).map_err(|e| vec![format!("组合件清单 TOML 解析失败: {e}")])?;
    root.as_table()
        .cloned()
        .ok_or_else(|| vec![format!("组合件清单顶层必须是 TOML 表，收到 {root:?}")])
}

/// `[assemble]` 段整体：键域 + atoms 链 + 注入面。
fn rr3_comp_asm(asm: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    for k in asm.keys() {
        rr3_asm_key_chk(k, errs);
    }

    rr3_comp_atoms(asm, facts, errs);

    rr3_comp_surface(asm, facts, errs);
}

/// `[assemble]` 单个键裁决（四键外点名）。
fn rr3_asm_key_chk(k: &str, errs: &mut Vec<String>) {
    if !matches!(k, "atoms" | "handles" | "ops" | "no_injection") {
        errs.push(format!(
            "[assemble] 未知键 {k}（只认 atoms / handles / ops / no_injection）"
        ));
    }
}

/// 顶层形状段：顶层未知键、schema_version 域、`[composite]`（name / unit）。
/// 错误按原顺序追加；段缺只点名不提前返回（[assemble] 的错误同批亮出）。
fn rr3_comp_head(t: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    rr3_comp_keys(t, errs);
    rr3_comp_meta(t, facts, errs);
}

/// 顶层键域与版本域（只认 schema_version / composite / assemble）。
fn rr3_comp_keys(t: &toml::Table, errs: &mut Vec<String>) {
    rr3_comp_top_keys(t, errs);
    rr3_comp_schema(t, errs);
}

/// 顶层未知键点名（只认 schema_version / composite / assemble）。
fn rr3_comp_top_keys(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        rr3_top_key_chk(k, errs);
    }
}

/// 顶层单个键裁决（只认 schema_version / composite / assemble）。
fn rr3_top_key_chk(k: &str, errs: &mut Vec<String>) {
    if !matches!(k, "schema_version" | "composite" | "assemble") {
        errs.push(format!(
            "组合件清单顶层未知键 {k}（只认 schema_version / composite / assemble）"
        ));
    }
}

/// schema_version：组合件面自己的版本域（当前 1）。与件级 assemble.toml 的
/// 2 是两个域——点名时把域差说清，防止"顺手改成 2 对齐件级"的误修。
fn rr3_comp_schema(t: &toml::Table, errs: &mut Vec<String>) {
    let Some(v) = t.get("schema_version") else {
        errs.push(format!(
            "组合件清单缺 schema_version（必须为 {RR3_COMPOSITE_SCHEMA_VERSION}）"
        ));
        return;
    };
    rr3_comp_schema_val(v, errs);
}

/// schema_version 取值：缺与"非整数"分条，整数则比对版本域。
fn rr3_comp_schema_val(v: &Value, errs: &mut Vec<String>) {
    let Some(n) = v.as_integer() else {
        errs.push(format!("组合件清单 schema_version 应为整数，收到 {v:?}"));
        return;
    };
    if n != RR3_COMPOSITE_SCHEMA_VERSION as i64 {
        errs.push(format!(
            "组合件清单 schema_version 必须为 {RR3_COMPOSITE_SCHEMA_VERSION}，收到 {n}（组合件面与件级装配清单是两个版本域：件级 RR3_ASSEMBLE_SCHEMA_VERSION = 2，同名不同域，不互认不迁移）"
        ));
    }
}

/// `[composite]` 元数据：name 非空 + unit 在册。
fn rr3_comp_meta(t: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    // 段缺只点名不提前返回——[assemble] 的独立错误要一次点全（fail-closed：
    // 一次修完，不让用户挤牙膏）。
    let Some(comp) = t.get("composite").and_then(Value::as_table) else {
        errs.push("组合件清单缺 [composite] 表（name / unit 必填）".into());
        return;
    };
    rr3_comp_meta_keys(comp, errs);
    rr3_comp_name(comp, facts, errs);
    rr3_comp_unit(comp, facts, errs);
}

/// `[composite]` 键词表核：只认 name / unit（其余逐键点名）。
fn rr3_comp_meta_keys(comp: &toml::Table, errs: &mut Vec<String>) {
    for k in comp.keys() {
        rr3_meta_key_chk(k, errs);
    }
}

/// `[composite]` 单个键裁决（只认 name / unit）。
fn rr3_meta_key_chk(k: &str, errs: &mut Vec<String>) {
    if !rr3_meta_key_ok(k) {
        errs.push(format!("[composite] 未知键 {k}（只认 name / unit）"));
    }
}

/// `[composite]` 键在册（只认 name / unit）。
fn rr3_meta_key_ok(k: &str) -> bool {
    k == "name" || k == "unit"
}

/// `[composite].name`：非空字符串（空串/缺键/非字符串分别点名）。
fn rr3_comp_name(comp: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    let Some(n) = comp.get("name").and_then(Value::as_str) else {
        errs.push("[composite].name 应为字符串".into());
        return;
    };
    if n.trim().is_empty() {
        errs.push("[composite].name 不得为空串".into());
        return;
    }
    facts.name = n.to_string();
}

/// `[composite].unit`：在册词域内（扩域走契约级评审）。
fn rr3_comp_unit(comp: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    let Some(u) = comp.get("unit").and_then(Value::as_str) else {
        errs.push(format!(
            "[composite].unit 应为字符串（词域 {RR3_COMPOSITE_UNITS:?}）"
        ));
        return;
    };
    if !RR3_COMPOSITE_UNITS.contains(&u) {
        errs.push(format!(
            "[composite].unit \"{u}\" 不在册（词域 {RR3_COMPOSITE_UNITS:?}，扩域走契约级评审）"
        ));
        return;
    }
    facts.unit = u.to_string();
}

/// atoms（⊗ 链）：非空数组、每条非空串、不重复。
fn rr3_comp_atoms(asm: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    let Some(items) = rr3_comp_atom_items(asm, errs) else {
        return;
    };
    if items.is_empty() {
        errs.push("[assemble].atoms 不得为空（组合件至少装配一个原子件）".into());
    }
    for (i, item) in items.iter().enumerate() {
        rr3_atom_item(item, i, facts, errs);
    }
}

/// `atoms` 数组取形：缺键/非数组各自点名后回 None。
fn rr3_comp_atom_items<'a>(asm: &'a toml::Table, errs: &mut Vec<String>) -> Option<&'a Vec<Value>> {
    let Some(v) = asm.get("atoms") else {
        errs.push("[assemble] 缺 atoms 键（组合件必须申报装配了哪些原子件）".into());
        return None;
    };
    let Value::Array(items) = v else {
        errs.push(format!("[assemble].atoms 应为数组，收到 {v:?}"));
        return None;
    };
    Some(items)
}

/// 单条原子件名判据：空串/非字符串/重复各点名；合格则入链。
fn rr3_atom_item(item: &Value, i: usize, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    let Some(a) = item.as_str() else {
        errs.push(format!("atoms[{i}] 应为字符串，收到 {item:?}"));
        return;
    };
    if a.trim().is_empty() {
        errs.push(format!("atoms[{i}] 不得为空串"));
        return;
    }
    rr3_atom_push(facts, a, i, errs);
}

/// 原子名去重入列：重复项点名（⊗ 链一名一条）。
fn rr3_atom_push(facts: &mut Rr3CompositeFacts, a: &str, i: usize, errs: &mut Vec<String>) {
    if facts.atoms.iter().any(|x| x == a) {
        errs.push(format!("atoms[{i}] \"{a}\" 重复（⊗ 链一名一条）"));
    } else {
        facts.atoms.push(a.to_string());
    }
}

/// 注入面三键（handles / ops / no_injection）与 L-5 空面规则；
/// 条目级解析共用件级 helper（一份判据一份真值）。
fn rr3_comp_surface(asm: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    // 两数组"已在文本中出现"（L-5 判"漏填 vs 明写"的口径与件级一致）
    let (handles_present, handles_empty) = rr3_comp_handles(asm, facts, errs);
    let (ops_present, ops_empty) = rr3_comp_ops(asm, facts, errs);
    rr3_comp_noinj(asm, facts, errs);
    // L-5 空面规则（共用件级同一条判据）
    errs.extend(rr3_no_injection_err(
        handles_present,
        handles_empty,
        ops_present,
        ops_empty,
        facts.no_injection,
    ));
}

/// handles 键：条目级解析共用件级 helper（一份判据一份真值）。返回
/// (键出现, 数组为空)。
fn rr3_comp_handles(
    asm: &toml::Table,
    facts: &mut Rr3CompositeFacts,
    errs: &mut Vec<String>,
) -> (bool, bool) {
    let (parsed, present, empty) = rr3_handles_key(asm, errs);
    facts.handles = parsed;
    (present, empty)
}

/// ops 键：同上共用件级 helper。返回 (键出现, 数组为空)。
fn rr3_comp_ops(
    asm: &toml::Table,
    facts: &mut Rr3CompositeFacts,
    errs: &mut Vec<String>,
) -> (bool, bool) {
    let (parsed, present, empty) = rr3_ops_key(asm, errs);
    facts.ops = parsed;
    (present, empty)
}

/// no_injection = true 是"本组合件不需注入面"的显式声明（L-5）：只认布尔 true。
fn rr3_comp_noinj(asm: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    let Some(v) = asm.get("no_injection") else {
        return;
    };
    let Value::Boolean(b) = v else {
        errs.push(format!("no_injection 应为布尔 true，收到 {v:?}"));
        return;
    };
    if !*b {
        errs.push("no_injection 只认 true（本组合件不需注入面时明写；非空面不得写此键）".into());
        return;
    }
    facts.no_injection = true;
}

#[cfg(test)]
mod tests;
