// SPDX-License-Identifier: Apache-2.0
//! composite 单测（文件头：本文件只放测试，生产面在 `composite.rs`）。
//!
//! 拆分动因：`composite.rs` 524 行、内联测试自 383 行起，先按仓内同形搬出。

use super::*;

/// 正例：满配组合件（atoms + 空面明写）与带注入面组合件都过
#[test]
fn full_empty_parse() {
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
fn schema_domain_named() {
    let errs = rr3_composite_parse(
        "schema_version = 2\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\nno_injection = true\n",
    )
    .expect_err("组合件面写件级版本值必须红");
    assert!(errs.iter().any(|e| e.contains("同名不同域")), "{errs:?}");
}

/// 合法底稿：unit=process、单 atom、空 handles/ops、显式 no_injection。
const BAD_BASE: &str = "schema_version = 1\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\nno_injection = true\n";

/// unit 出域。
fn bad_unit_shape() {
    let errs = rr3_composite_parse(&BAD_BASE.replace("unit = \"process\"", "unit = \"thread\""))
        .expect_err("unit 出域须红");
    assert!(
        errs.iter()
            .any(|e| e.contains("unit") && e.contains("thread")),
        "{errs:?}"
    );
}

/// atoms 空/重复/空串/非串四形逐条点名。
fn bad_atoms_shape() {
    let errs = rr3_composite_parse(&BAD_BASE.replace("atoms = [\"a\"]", "atoms = []"))
        .expect_err("atoms 空须红");
    assert!(
        errs.iter().any(|e| e.contains("atoms 不得为空")),
        "{errs:?}"
    );

    let errs = rr3_composite_parse(&BAD_BASE.replace("atoms = [\"a\"]", "atoms = [\"a\", \"a\"]"))
        .expect_err("atoms 重复须红");
    assert!(errs.iter().any(|e| e.contains("重复")), "{errs:?}");

    let errs = rr3_composite_parse(&BAD_BASE.replace("atoms = [\"a\"]", "atoms = [\"\"]"))
        .expect_err("atoms 空串须红");
    assert!(
        errs.iter().any(|e| e.contains("atoms[0] 不得为空串")),
        "{errs:?}"
    );

    let errs = rr3_composite_parse(&BAD_BASE.replace("atoms = [\"a\"]", "atoms = [1]"))
        .expect_err("atoms 非串须红");
    assert!(
        errs.iter().any(|e| e.contains("atoms[0] 应为字符串")),
        "{errs:?}"
    );
}

/// 未知键（assemble 段/顶层）与缺段三形。
fn bad_sections_shape() {
    let errs = rr3_composite_parse(
        &BAD_BASE.replace("no_injection = true", "no_injection = true\nextra = 1"),
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
}

/// L-5 注入声明：空面漏写与 helper 并存两形。
fn bad_injection_shape() {
    let errs = rr3_composite_parse(
        "schema_version = 1\n\n[composite]\nname = \"x\"\nunit = \"process\"\n\n[assemble]\natoms = [\"a\"]\nhandles = []\nops = []\n",
    )
    .expect_err("空面漏写声明须红");
    assert!(
        errs.iter().any(|e| e.contains("未明写 no_injection")),
        "{errs:?}"
    );

    let errs = rr3_composite_parse(&BAD_BASE.replace(
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

/// 形状坏逐条点名：unit 出域 / atoms 空·重复·空串·非串 / 顶层与段级未知键 /
/// 缺 [composite] / 缺 [assemble] / L-5 漏写与并存
#[test]
fn bad_shapes_named() {
    bad_unit_shape();
    bad_atoms_shape();
    bad_sections_shape();
    bad_injection_shape();
}

/// 内存申报键是件级专用（校验⑩ 分子面，批N2 n2c-v10）：组合件级 [assemble]
/// 点名即拦，同 demands 的件级专用语义——内存账是原子件的物理属性。
#[test]
fn composite_rej_keys() {
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
