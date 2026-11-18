//! 装配单装载链（B9 结构收口）：选册 → 查册 → 探测 → 校验 → boot 输入。
//!
//! 全部发生在模块起之前（ADR-0012 裁定 4）。真值集/选册（[`rr3_truth_set`]、
//! [`rr3_truth_locations`]）、boot 入口（[`rr3_boot_with_probe`]）、五册装载
//! 小件、装配扫描与预算判据同址；单条判据体归 `profile_checks.rs`。

mod load;
mod plan;
mod reject;

// ── TOML 键行手术面（批N22 件一；批N44 起机制迁 [`crate::profile_edit`]）──────
// 本文件只留原路径转出（写面调用与文档锚点不变），手术判据与实现单点在
// `profile_edit`——超长文件按"装载 / 校验 / 手术"三分的首刀。
pub use crate::profile_edit::{rr3_domain_apply, rr3_key_line_apply};
pub use load::{rr3_boot_inputs, rr3_boot_with_probe};
pub(crate) use plan::{rr3_axis_files_load, rr3_axis_state_text, rr3_bindings_text};
pub use plan::{
    rr3_truth_locations, rr3_truth_set, Rr3RuntimeTruthSet, Rr3TruthSetLocation, RR3_CONTRACT_BOOKS,
};
pub use reject::rr3_evidence_check;
pub(crate) use reject::{
    rr3_assemble_scan, rr3_demand_budget, rr3_demand_expand, rr3_memory_budget, rr3_power_gaps,
};
