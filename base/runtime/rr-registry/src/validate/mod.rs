// SPDX-License-Identifier: Apache-2.0
//! manifest / topics 校验。规则语义全部来自 `module.schema.toml` 的 `[rules]`。
//!
//! 规则清单（方案 §4 表格逐行落地）：
//! - `module_name`：`^rr3-[a-z]+$`
//! - `semver`：点分数字
//! - `type` / `lang` / `restart_policy`：枚举，取自 `[rules]`
//! - `capability`：`^[a-z]+(\.[a-z_]+)+$`
//! - `topic`：`rr3-<模块>/<域>/<事件>`
//! - `own_topic`：先过 `topic`，再要求前缀等于本 manifest 的 `manifest.name`
//! - `none`：放行
//!
//! 申报面退役门（0.0.5 批J C14 首例，ADR-0019 修订 2 ②）：`contracts/topics.toml`
//! 显式 `status = "retired"` 的主题——号位保留、义务解除——不得被 publishes /
//! rpc.exposes / subscribes 申报引用，引用即拒并点名。

mod manifest;
mod report;
mod rules;
#[cfg(test)]
mod tests;
mod topics;

pub use manifest::{rr3_manifest_chk, rr3_manifest_gate, rr3_no_unknown_keys};
pub use report::Rr3ValidationReport;
pub use rules::rr3_registry_check;
pub use topics::{rr3_topics_chk, rr3_topics_names, rr3_topics_owners};

/// 字段类型不符的统一文案：module/atom 两侧共用一份格式串，避免文案漂移。
pub(crate) fn rr3_type_err(path: &str, kind: &str, v: &toml::Value) -> String {
    format!("字段 {path} 类型应为 {kind}，实际为 {v:?}")
}

/// list 项形状不符的统一文案（序号从 1 起）：module/atom 两侧共用。
pub(crate) fn rr3_item_err(path: &str, idx: usize, v: &toml::Value) -> String {
    format!("字段 {path} 第 {} 项应为字符串，实际为 {v:?}", idx + 1)
}
