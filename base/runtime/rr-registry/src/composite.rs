//! 组合件装配清单（`modules/<名>/composite.toml`，三层 manifest 的 Composite 层，
//! 0.0.5 批C② 原子重铺落位）的形状契约——组合件申报面定形（增补轮六 N-11：
//! §8 门 j 的前件之一，§5 N-11 行）。
//!
//! **schema 位裁定（版本域）**：组合件清单顶层 `schema_version` 归
//! [`RR3_COMPOSITE_SCHEMA_VERSION`]（= 1）管辖；件级 `atoms/<名>/assemble.toml`
//! 归 [`crate::assemble::RR3_ASSEMBLE_SCHEMA_VERSION`]（= 2）管辖。两者同名
//! 不同域：`schema_version` 指向的不是"一个全局版本"，而是各自文件形状各自的
//! 版本，**不互认、不迁移、不互相 bump**（施工图全局约束："裸写 schema 2→3 会
//! 指错常量"的同一条纪律）。
//!
//! **本轮增三键属只增不 bump**：`[assemble]` 增 `handles` / `ops` /
//! `no_injection`（N-11）。理由：本文件此前无代码读它（六份 composite.toml
//! 零消费者，schema_version = 1 是"无校验器时代"的落位形状），本轮校验器与
//! 数据同批落盘、六份文件同批迁移，不存在旧数据带病运行的兼容窗口——首次
//! 定形不是破坏性变更；将来对已激活形状做语义变更才升版本走契约级评审。
//!
//! 定形内容（与件级 [`crate::assemble`] **同形同规则**，条目级解析直接共用其
//! helper——一份判据一份真值，不许组合件另说一套）：
//!
//! ```toml
//! schema_version = 1
//!
//! [composite]
//! name = "rr3-act"
//! unit = "process"
//!
//! [assemble]
//! atoms = ["servo", "led", "relay"]
//! handles = []        # 组合件级句柄申报（结构同件级 { trait, desc_key }）
//! ops = []            # 组合件级参数槽申报（结构同件级 { slot, src_key }）
//! no_injection = true # L-5：两数组均空必须明写
//! ```
//!
//! - `atoms`：⊗ 链（装配了哪些原子件），非空、每条非空串、不重复——空组合件
//!   无意义，重复申报无意义。
//! - `handles` / `ops` / `no_injection`：L-5 空面规则**整条适用**（0.0.5 批M
//!   MC-10 同一条规则），组合件也必须把"漏填"与"本组合件确无注入面"分开。
//!
//! fail-closed：任何键缺、任何取值出域逐条点名回 `Err`（与件级
//! [`crate::assemble::rr3_assemble_parse`] 同一口径）。

use crate::assemble::{
    rr3_handles_parse, rr3_no_injection_err, rr3_ops_parse, Rr3AssembleHandle, Rr3AssembleOp,
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
    let asm = t.get("assemble").and_then(Value::as_table);
    let Some(asm) = asm else {
        errs.push(
            "组合件清单缺 [assemble] 表（atoms 必填，handles / ops / no_injection 同件级规则）"
                .into(),
        );
        return Err(errs);
    };
    rr3_comp_asm(asm, &mut facts, &mut errs);

    if errs.is_empty() {
        Ok(facts)
    } else {
        Err(errs)
    }
}

/// 顶层 TOML 解析：语法错与"顶层非表"各自成条 Err（与旧实现逐字同文案）。
fn rr3_comp_root(text: &str) -> Result<toml::Table, Vec<String>> {
    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("组合件清单 TOML 解析失败: {e}")]),
    };
    match root.as_table() {
        Some(t) => Ok(t.clone()),
        None => Err(vec![format!("组合件清单顶层必须是 TOML 表，收到 {root:?}")]),
    }
}

/// `[assemble]` 段整体：键域 + atoms 链 + 注入面。
fn rr3_comp_asm(asm: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    for k in asm.keys() {
        if !matches!(k.as_str(), "atoms" | "handles" | "ops" | "no_injection") {
            errs.push(format!(
                "[assemble] 未知键 {k}（只认 atoms / handles / ops / no_injection）"
            ));
        }
    }

    rr3_comp_atoms(asm, facts, errs);

    rr3_comp_surface(asm, facts, errs);
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
        if !matches!(k.as_str(), "schema_version" | "composite" | "assemble") {
            errs.push(format!(
                "组合件清单顶层未知键 {k}（只认 schema_version / composite / assemble）"
            ));
        }
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
        if k != "name" && k != "unit" {
            errs.push(format!("[composite] 未知键 {k}（只认 name / unit）"));
        }
    }
}

/// `[composite].name`：非空字符串（空串/缺键/非字符串分别点名）。
fn rr3_comp_name(comp: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    match comp.get("name").and_then(Value::as_str) {
        Some(n) if !n.trim().is_empty() => facts.name = n.to_string(),
        Some(_) => errs.push("[composite].name 不得为空串".into()),
        None => errs.push("[composite].name 应为字符串".into()),
    }
}

/// `[composite].unit`：在册词域内（扩域走契约级评审）。
fn rr3_comp_unit(comp: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    match comp.get("unit").and_then(Value::as_str) {
        Some(u) if RR3_COMPOSITE_UNITS.contains(&u) => facts.unit = u.to_string(),
        Some(u) => errs.push(format!(
            "[composite].unit \"{u}\" 不在册（词域 {RR3_COMPOSITE_UNITS:?}，扩域走契约级评审）"
        )),
        None => errs.push(format!(
            "[composite].unit 应为字符串（词域 {RR3_COMPOSITE_UNITS:?}）"
        )),
    }
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
    match asm.get("atoms") {
        Some(Value::Array(items)) => Some(items),
        Some(other) => {
            errs.push(format!("[assemble].atoms 应为数组，收到 {other:?}"));
            None
        }
        None => {
            errs.push("[assemble] 缺 atoms 键（组合件必须申报装配了哪些原子件）".into());
            None
        }
    }
}

/// 单条原子件名判据：空串/非字符串/重复各点名；合格则入链。
fn rr3_atom_item(item: &Value, i: usize, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    match item.as_str() {
        Some(a) if !a.trim().is_empty() => rr3_atom_push(facts, a, i, errs),
        Some(_) => errs.push(format!("atoms[{i}] 不得为空串")),
        None => errs.push(format!("atoms[{i}] 应为字符串，收到 {item:?}")),
    }
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
    match asm.get("handles") {
        Some(Value::Array(items)) => {
            let (parsed, mut item_errs) = rr3_handles_parse(items);
            facts.handles = parsed;
            errs.append(&mut item_errs);
            (true, items.is_empty())
        }
        Some(other) => {
            errs.push(format!("[assemble].handles 应为数组，收到 {other:?}"));
            (false, false)
        }
        None => {
            errs.push("[assemble] 缺 handles 键（空数组合法，键本身必填，N-11）".into());
            (false, false)
        }
    }
}

/// ops 键：同上共用件级 helper。返回 (键出现, 数组为空)。
fn rr3_comp_ops(
    asm: &toml::Table,
    facts: &mut Rr3CompositeFacts,
    errs: &mut Vec<String>,
) -> (bool, bool) {
    match asm.get("ops") {
        Some(Value::Array(items)) => {
            let (parsed, mut item_errs) = rr3_ops_parse(items);
            facts.ops = parsed;
            errs.append(&mut item_errs);
            (true, items.is_empty())
        }
        Some(other) => {
            errs.push(format!("[assemble].ops 应为数组，收到 {other:?}"));
            (false, false)
        }
        None => {
            errs.push("[assemble] 缺 ops 键（空数组合法，键本身必填，N-11）".into());
            (false, false)
        }
    }
}

/// no_injection = true 是"本组合件不需注入面"的显式声明（L-5）：只认布尔 true。
fn rr3_comp_noinj(asm: &toml::Table, facts: &mut Rr3CompositeFacts, errs: &mut Vec<String>) {
    match asm.get("no_injection") {
        Some(Value::Boolean(true)) => facts.no_injection = true,
        Some(Value::Boolean(false)) => {
            errs.push("no_injection 只认 true（本组合件不需注入面时明写；非空面不得写此键）".into())
        }
        Some(other) => errs.push(format!("no_injection 应为布尔 true，收到 {other:?}")),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正例：满配组合件（atoms + 空面明写）与带注入面组合件都过
    #[test]
    fn full_and_empty_surface_parse() {
        let f = rr3_composite_parse(
            "schema_version = 1\n\n[composite]\nname = \"rr3-x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\", \"b\"]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .expect("空面明写必须过");
        assert_eq!(f.name, "rr3-x");
        assert_eq!(f.unit, "process");
        assert_eq!(f.atoms, vec!["a", "b"]);
        assert!(f.handles.is_empty() && f.ops.is_empty() && f.no_injection);

        let f = rr3_composite_parse(
            "schema_version = 1\n\n[composite]\nname = \"rr3-x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = [ { trait = \"Rr3Power\", desc_key = \"i2c_bus\" } ]\nops = [ { slot = \"s\", src_key = \"k.v\" } ]\n",
        )
        .expect("带注入面必须过");
        assert_eq!(f.handles.len(), 1);
        assert_eq!(f.ops.len(), 1);
        assert!(!f.no_injection);
    }

    /// schema 位裁定：写 2（件级域的值）必须红，且点名"同名不同域"
    #[test]
    fn schema_version_domain_is_named() {
        let errs = rr3_composite_parse(
            "schema_version = 2\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .expect_err("组合件面写件级版本值必须红");
        assert!(errs.iter().any(|e| e.contains("同名不同域")), "{errs:?}");
    }

    /// 形状坏逐条点名：unit 出域 / atoms 空·重复·空串·非串 / 顶层与段级未知键 /
    /// 缺 [composite] / 缺 [assemble] / L-5 漏写与并存
    #[test]
    fn bad_shapes_are_all_named() {
        let base = "schema_version = 1\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\nno_injection = true\n";

        let errs = rr3_composite_parse(&base.replace("unit = \"process\"", "unit = \"thread\""))
            .expect_err("unit 出域须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("unit") && e.contains("thread")),
            "{errs:?}"
        );

        let errs = rr3_composite_parse(&base.replace("atoms = [\"a\"]", "atoms = []"))
            .expect_err("atoms 空须红");
        assert!(
            errs.iter().any(|e| e.contains("atoms 不得为空")),
            "{errs:?}"
        );

        let errs = rr3_composite_parse(&base.replace("atoms = [\"a\"]", "atoms = [\"a\", \"a\"]"))
            .expect_err("atoms 重复须红");
        assert!(errs.iter().any(|e| e.contains("重复")), "{errs:?}");

        let errs = rr3_composite_parse(&base.replace("atoms = [\"a\"]", "atoms = [\"\"]"))
            .expect_err("atoms 空串须红");
        assert!(
            errs.iter().any(|e| e.contains("atoms[0] 不得为空串")),
            "{errs:?}"
        );

        let errs = rr3_composite_parse(&base.replace("atoms = [\"a\"]", "atoms = [1]"))
            .expect_err("atoms 非串须红");
        assert!(
            errs.iter().any(|e| e.contains("atoms[0] 应为字符串")),
            "{errs:?}"
        );

        let errs = rr3_composite_parse(
            &base.replace("no_injection = true", "no_injection = true\nextra = 1"),
        )
        .expect_err("[assemble] 未知键须红");
        assert!(
            errs.iter().any(|e| e.contains("[assemble] 未知键 extra")),
            "{errs:?}"
        );

        let errs = rr3_composite_parse("mystery = 1\n").expect_err("顶层未知键须红");
        assert!(
            errs.iter().any(|e| e.contains("顶层未知键 mystery")),
            "{errs:?}"
        );

        let errs = rr3_composite_parse("schema_version = 1\n").expect_err("缺两段须红");
        assert!(
            errs.iter().any(|e| e.contains("缺 [composite] 表")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("缺 [assemble] 表")),
            "{errs:?}"
        );

        // L-5 漏写（与件级同一判据）
        let errs = rr3_composite_parse(
            "schema_version = 1\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\n",
        )
        .expect_err("空面漏写声明须红");
        assert!(
            errs.iter().any(|e| e.contains("未明写 no_injection")),
            "{errs:?}"
        );

        // L-5 并存红
        let errs = rr3_composite_parse(&base.replace(
            "handles = []",
            "handles = [ { trait = \"Rr3Nope\", desc_key = \"nope_key\" } ]",
        ))
        .expect_err("helper 生效（trait 不在册红）+ 声明并存红");
        assert!(
            errs.iter().any(|e| e.contains("trait \"Rr3Nope\" 未在册")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("只允许与空的 handles/ops 并存")),
            "{errs:?}"
        );
    }

    /// 内存申报键是件级专用（校验⑩ 分子面，批N2 n2c-v10）：组合件级 [assemble]
    /// 点名即拦，同 demands 的件级专用语义——内存账是原子件的物理属性。
    #[test]
    fn composite_assemble_rejects_memory_keys() {
        for key in ["task_stack_bytes", "heap_peak_bytes"] {
            let errs = rr3_composite_parse(&format!(
                "schema_version = 1\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\nno_injection = true\n{key} = 4096\n"
            ))
            .expect_err("组合件级内存申报键必须拦");
            assert!(
                errs.iter()
                    .any(|e| e.contains(&format!("[assemble] 未知键 {key}"))),
                "{errs:?}"
            );
        }
    }
}
