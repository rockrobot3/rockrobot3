// SPDX-License-Identifier: Apache-2.0
//! 原子件五件套生成（ADR-0019）：atom.toml / 入口 / mech / assemble / private。
//!
//! 入口 `rr3_mkmod_gen_atom` 由 crate root re-export；本模块只依赖父模块的错误
//! 类型与路径面，不反向调用模块生成器（两族产物互不影响）。

use super::*;

/// 原子件名：`^[a-z][a-z_]*$`，2..=24 字符（atom.schema.toml 的 atom_name 规则同款）
fn rr3_mkmod_atom_stem(name: &str) -> Result<String, Rr3MkmodError> {
    if !rr3_atom_name_ok(name) {
        return Err(Rr3MkmodError::InvalidName(name.to_string()));
    }
    Ok(name.to_string())
}

/// 原子件名合法：`^[a-z][a-z_]*$`，2..=24 字符。
fn rr3_atom_name_ok(name: &str) -> bool {
    (2..=24).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

fn atom_manifest_toml(name: &str, slot: &str) -> String {
    format!(
        r#"# {name} 原子件 manifest（三层 manifest 的 Atom 层，ADR-0019）
# 契约：base/runtime/rr-registry/src/atom.schema.toml；查册门查 contracts/slots.toml 同一张表
schema_version = 1

[atom]
name = "{name}"
version = "0.0.1"

[provides]
slots = ["{slot}"]   # 占哪个格——每格一件（ADR-0019 裁定 1：恰一格、不可再分）

[uses]
slots = []           # 装配期依赖的其他格（描述符注入，不见厂商/引脚）

[events]
publishes = []
subscribes = []
"#
    )
}

fn atom_entry_rs(name: &str, slot: &str) -> String {
    format!(
        r#"//! {name} 原子件入口（五件套②）：只声明公开面，机制在 mech.rs。
//! 占格 {slot}（atom.toml）。组合件以 `#[path]` 引本文件为 mod；
//! 跨域只准 use 本文件 pub 项（五件套⑤纪律，内部共享落 private.rs，永不 re-export）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

pub use mech::*;
"#
    )
}

fn atom_mech_rs(name: &str) -> String {
    format!(
        r#"//! {name} 机制主体（五件套③）：零策略载荷（ADR-0019 裁定 5——厂商名/产品名/
//! 业务阈值不进机制件，策略经 ops 函数表 + manifest/config 数据化注入）。

#[cfg(test)]
mod tests {{
    /// 骨架自检：机制件可编译、可测。真实现落位后替换本测试。
    #[test]
    fn skeleton_compiles() {{
        assert_eq!(1 + 1, 2);
    }}
}}
"#
    )
}

fn atom_assemble_toml(name: &str) -> String {
    format!(
        r#"# {name} 装配清单（五件套④）：装配期注入面声明（ADR-0019 修订 1 ③——
# [assemble.handles] 声明件用哪个能力面 trait ← 绑描述符哪个键、[assemble.ops]
# 声明件暴露哪些参数槽 ← 取哪个 config 键；装配段校验缺项即该格 Degraded）。
schema_version = 2

[assemble]
handles = []          # 每条 {{ trait = "能力面", desc_key = "描述符键" }}（在册表见 rr-registry assemble.rs）
ops = []              # 每条 {{ slot = "参数槽", src_key = "config 点路径" }}
# 骨架期无注入面：明写 no_injection（L-5）。件填实后删本行，按上面两数组申报真实句柄/参数槽；
# 两数组均空而缺本行即判红（"漏填"与"确无注入面"不可区分）。
no_injection = true
"#
    )
}

fn atom_private_rs(name: &str) -> String {
    format!(
        r#"//! {name} 唯一私有头（五件套⑤）：原子件内部共享的类型与常量。
//! 私有性纪律：本文件项不进入口 re-export，跨域不可见（architecture.md §4.0.4）。
"#
    )
}

/// 在 `out_root` 下生成原子件 `atoms 形状` 的 `<name>/` 五件套；返回生成的文件列表。
///
/// `out_root` 语义 = 原子根目录（惯例 `modules/<组合件>/atoms`），产物落
/// `<out_root>/<name>/`。生成后当场回灌 `rr3_atom_gate`（字段契约 +
/// slots 查册门 + 板无关断言）：`out_root` 须位于仓内以向上定位 `contracts/slots.toml`，
/// 校验不过即整体丢弃（staging→rename，不留半成品）。
pub fn rr3_mkmod_gen_atom(
    name: &str,
    slot: &str,
    out_root: &Path,
) -> Result<Vec<PathBuf>, Rr3MkmodError> {
    let stem = rr3_mkmod_atom_stem(name)?;
    let target = out_root.join(&stem);
    if target.exists() {
        return Err(Rr3MkmodError::AlreadyExists(target));
    }

    let staging = out_root.join(format!(".staging-{stem}"));
    let files = rr3_mkmod_atom_files(&stem, slot);
    rr3_mkmod_atom_emit(&staging, &files)?;

    std::fs::rename(&staging, &target)?;
    Ok(files.iter().map(|(rel, _)| target.join(rel)).collect())
}

/// staging 落盘 + 回灌门（任一步失败即整体丢弃，不留半成品）。
fn rr3_mkmod_atom_emit(staging: &Path, files: &[(String, String)]) -> Result<(), Rr3MkmodError> {
    rr3_mkmod_atom_write(staging, files)?;
    rr3_mkmod_atom_gate(staging, files)
}

/// 五件套内容（相对路径 + 正文），顺序即落盘顺序。
fn rr3_mkmod_atom_files(stem: &str, slot: &str) -> Vec<(String, String)> {
    vec![
        ("atom.toml".into(), atom_manifest_toml(stem, slot)),
        (format!("{stem}.rs"), atom_entry_rs(stem, slot)),
        ("mech.rs".into(), atom_mech_rs(stem)),
        ("assemble.toml".into(), atom_assemble_toml(stem)),
        ("private.rs".into(), atom_private_rs(stem)),
    ]
}

/// staging 目录就绪并逐件写入（旧 staging 先清空）。
fn rr3_mkmod_atom_write(staging: &Path, files: &[(String, String)]) -> Result<(), Rr3MkmodError> {
    let _ = std::fs::remove_dir_all(staging);
    std::fs::create_dir_all(staging)?;
    for (rel, body) in files {
        std::fs::write(staging.join(rel), body)?;
    }
    Ok(())
}

/// 回灌校验（②b 同款纪律）：manifest + 装配清单生成即自检，不合规即中止。
fn rr3_mkmod_atom_gate(staging: &Path, files: &[(String, String)]) -> Result<(), Rr3MkmodError> {
    // G4：装配清单走 rr-registry 形状级真值（模板空数组必过；填实后坏形当场听见）。
    let mut errs = rr_registry::rr3_atom_gate(&staging.join("atom.toml"));
    let Some(assemble_body) = files
        .iter()
        .find(|(rel, _)| rel == "assemble.toml")
        .map(|(_, body)| body.as_str())
    else {
        return Err(Rr3MkmodError::Manifest(vec![
            "五件套缺 assemble.toml（生成器内部错误）".into(),
        ]));
    };
    let assemble_errs = rr_registry::rr3_assemble_parse(assemble_body).err();
    errs.extend(assemble_errs.unwrap_or_default());
    if errs.is_empty() {
        return Ok(());
    }
    rr3_atom_gate_fail(staging, errs)
}
/// 回灌门失败收口：清临时树后带回全部错误。
fn rr3_atom_gate_fail(staging: &Path, errs: Vec<String>) -> Result<(), Rr3MkmodError> {
    let _ = std::fs::remove_dir_all(staging);
    Err(Rr3MkmodError::Manifest(errs))
}
