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
//! [`crate::assemble::rr3_registry_assemble_from_text`] 同一口径）。

use crate::assemble::{
    rr3_empty_surface_errors, rr3_handles_from_array, rr3_ops_from_array, Rr3AssembleHandle,
    Rr3AssembleOp,
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
pub fn rr3_registry_composite_from_text(text: &str) -> Result<Rr3CompositeFacts, Vec<String>> {
    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("组合件清单 TOML 解析失败: {e}")]),
    };
    let Some(t) = root.as_table() else {
        return Err(vec![format!("组合件清单顶层必须是 TOML 表，收到 {root:?}")]);
    };

    let mut errs: Vec<String> = Vec::new();
    // 顶层未知键点名（只认 schema_version / composite / assemble）
    for k in t.keys() {
        if k != "schema_version" && k != "composite" && k != "assemble" {
            errs.push(format!(
                "组合件清单顶层未知键 {k}（只认 schema_version / composite / assemble）"
            ));
        }
    }
    // schema_version：组合件面自己的版本域（当前 1）。与件级 assemble.toml 的
    // 2 是两个域——点名时把域差说清，防止"顺手改成 2 对齐件级"的误修。
    match t.get("schema_version") {
        Some(Value::Integer(v)) if *v == RR3_COMPOSITE_SCHEMA_VERSION as i64 => {}
        Some(Value::Integer(v)) => errs.push(format!(
            "组合件清单 schema_version 必须为 {RR3_COMPOSITE_SCHEMA_VERSION}，收到 {v}（组合件面与件级装配清单是两个版本域：件级 RR3_ASSEMBLE_SCHEMA_VERSION = 2，同名不同域，不互认不迁移）"
        )),
        Some(other) => errs.push(format!("组合件清单 schema_version 应为整数，收到 {other:?}")),
        None => errs.push(format!(
            "组合件清单缺 schema_version（必须为 {RR3_COMPOSITE_SCHEMA_VERSION}）"
        )),
    }

    let mut facts = Rr3CompositeFacts::default();

    // [composite]：name 非空 + unit 在册。段缺只点名不提前返回——[assemble] 的
    // 独立错误要一次点全（fail-closed：一次修完，不让用户挤牙膏）。
    match t.get("composite").and_then(Value::as_table) {
        Some(comp) => {
            for k in comp.keys() {
                if k != "name" && k != "unit" {
                    errs.push(format!("[composite] 未知键 {k}（只认 name / unit）"));
                }
            }
            match comp.get("name").and_then(Value::as_str) {
                Some(n) if !n.trim().is_empty() => facts.name = n.to_string(),
                Some(_) => errs.push("[composite].name 不得为空串".into()),
                None => errs.push("[composite].name 应为字符串".into()),
            }
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
        None => errs.push("组合件清单缺 [composite] 表（name / unit 必填）".into()),
    }

    // [assemble]：atoms 必填非空不重复；handles/ops/no_injection 与件级同规则。
    // 段缺同样只点名，让 schema/[composite] 的错误同批亮出。
    let asm = t.get("assemble").and_then(Value::as_table);
    let Some(asm) = asm else {
        errs.push(
            "组合件清单缺 [assemble] 表（atoms 必填，handles / ops / no_injection 同件级规则）"
                .into(),
        );
        return if errs.is_empty() {
            Ok(facts)
        } else {
            Err(errs)
        };
    };
    for k in asm.keys() {
        if k != "atoms" && k != "handles" && k != "ops" && k != "no_injection" {
            errs.push(format!(
                "[assemble] 未知键 {k}（只认 atoms / handles / ops / no_injection）"
            ));
        }
    }

    // atoms：⊗ 链。非空数组（空组合件无意义）、每条非空串、不重复
    match asm.get("atoms") {
        Some(Value::Array(items)) => {
            if items.is_empty() {
                errs.push("[assemble].atoms 不得为空（组合件至少装配一个原子件）".into());
            }
            for (i, item) in items.iter().enumerate() {
                match item.as_str() {
                    Some(a) if !a.trim().is_empty() => {
                        if facts.atoms.iter().any(|x| x == a) {
                            errs.push(format!("atoms[{i}] \"{a}\" 重复（⊗ 链一名一条）"));
                        } else {
                            facts.atoms.push(a.to_string());
                        }
                    }
                    Some(_) => errs.push(format!("atoms[{i}] 不得为空串")),
                    None => errs.push(format!("atoms[{i}] 应为字符串，收到 {item:?}")),
                }
            }
        }
        Some(other) => errs.push(format!("[assemble].atoms 应为数组，收到 {other:?}")),
        None => errs.push("[assemble] 缺 atoms 键（组合件必须申报装配了哪些原子件）".into()),
    }

    // 两数组"已在文本中出现"（L-5 判"漏填 vs 明写"的口径与件级一致）
    let mut handles_present = false;
    let mut handles_empty = false;
    let mut ops_present = false;
    let mut ops_empty = false;

    // handles：条目级解析直接共用件级 helper（一份判据一份真值）
    match asm.get("handles") {
        Some(Value::Array(items)) => {
            handles_present = true;
            handles_empty = items.is_empty();
            let (parsed, mut item_errs) = rr3_handles_from_array(items);
            facts.handles = parsed;
            errs.append(&mut item_errs);
        }
        Some(other) => errs.push(format!("[assemble].handles 应为数组，收到 {other:?}")),
        None => errs.push("[assemble] 缺 handles 键（空数组合法，键本身必填，N-11）".into()),
    }

    // ops：同上共用
    match asm.get("ops") {
        Some(Value::Array(items)) => {
            ops_present = true;
            ops_empty = items.is_empty();
            let (parsed, mut item_errs) = rr3_ops_from_array(items);
            facts.ops = parsed;
            errs.append(&mut item_errs);
        }
        Some(other) => errs.push(format!("[assemble].ops 应为数组，收到 {other:?}")),
        None => errs.push("[assemble] 缺 ops 键（空数组合法，键本身必填，N-11）".into()),
    }

    // no_injection = true 是"本组合件不需注入面"的显式声明（L-5）：只认布尔 true。
    match asm.get("no_injection") {
        Some(Value::Boolean(true)) => facts.no_injection = true,
        Some(Value::Boolean(false)) => {
            errs.push("no_injection 只认 true（本组合件不需注入面时明写；非空面不得写此键）".into())
        }
        Some(other) => errs.push(format!("no_injection 应为布尔 true，收到 {other:?}")),
        None => {}
    }

    // L-5 空面规则（共用件级同一条判据）
    errs.extend(rr3_empty_surface_errors(
        handles_present,
        handles_empty,
        ops_present,
        ops_empty,
        facts.no_injection,
    ));

    if errs.is_empty() {
        Ok(facts)
    } else {
        Err(errs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正例：满配组合件（atoms + 空面明写）与带注入面组合件都过
    #[test]
    fn full_and_empty_surface_parse() {
        let f = rr3_registry_composite_from_text(
            "schema_version = 1\n\n[composite]\nname = \"rr3-x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\", \"b\"]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .expect("空面明写必须过");
        assert_eq!(f.name, "rr3-x");
        assert_eq!(f.unit, "process");
        assert_eq!(f.atoms, vec!["a", "b"]);
        assert!(f.handles.is_empty() && f.ops.is_empty() && f.no_injection);

        let f = rr3_registry_composite_from_text(
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
        let errs = rr3_registry_composite_from_text(
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

        let errs = rr3_registry_composite_from_text(
            &base.replace("unit = \"process\"", "unit = \"thread\""),
        )
        .expect_err("unit 出域须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("unit") && e.contains("thread")),
            "{errs:?}"
        );

        let errs = rr3_registry_composite_from_text(&base.replace("atoms = [\"a\"]", "atoms = []"))
            .expect_err("atoms 空须红");
        assert!(
            errs.iter().any(|e| e.contains("atoms 不得为空")),
            "{errs:?}"
        );

        let errs = rr3_registry_composite_from_text(
            &base.replace("atoms = [\"a\"]", "atoms = [\"a\", \"a\"]"),
        )
        .expect_err("atoms 重复须红");
        assert!(errs.iter().any(|e| e.contains("重复")), "{errs:?}");

        let errs =
            rr3_registry_composite_from_text(&base.replace("atoms = [\"a\"]", "atoms = [\"\"]"))
                .expect_err("atoms 空串须红");
        assert!(
            errs.iter().any(|e| e.contains("atoms[0] 不得为空串")),
            "{errs:?}"
        );

        let errs =
            rr3_registry_composite_from_text(&base.replace("atoms = [\"a\"]", "atoms = [1]"))
                .expect_err("atoms 非串须红");
        assert!(
            errs.iter().any(|e| e.contains("atoms[0] 应为字符串")),
            "{errs:?}"
        );

        let errs = rr3_registry_composite_from_text(
            &base.replace("no_injection = true", "no_injection = true\nextra = 1"),
        )
        .expect_err("[assemble] 未知键须红");
        assert!(
            errs.iter().any(|e| e.contains("[assemble] 未知键 extra")),
            "{errs:?}"
        );

        let errs = rr3_registry_composite_from_text("mystery = 1\n").expect_err("顶层未知键须红");
        assert!(
            errs.iter().any(|e| e.contains("顶层未知键 mystery")),
            "{errs:?}"
        );

        let errs =
            rr3_registry_composite_from_text("schema_version = 1\n").expect_err("缺两段须红");
        assert!(
            errs.iter().any(|e| e.contains("缺 [composite] 表")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("缺 [assemble] 表")),
            "{errs:?}"
        );

        // L-5 漏写（与件级同一判据）
        let errs = rr3_registry_composite_from_text(
            "schema_version = 1\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\n",
        )
        .expect_err("空面漏写声明须红");
        assert!(
            errs.iter().any(|e| e.contains("未明写 no_injection")),
            "{errs:?}"
        );

        // L-5 并存红
        let errs = rr3_registry_composite_from_text(&base.replace(
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
}
