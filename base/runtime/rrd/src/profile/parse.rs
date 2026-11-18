//! 装配单文本解读（B9 结构收口）：`toml::Value` → 模型，形状错误全收集。
//!
//! 段序即错误追加序（[os] → [platform] → [runtime] → [supplies] → [slots] →
//! [bindings] → [enabled_slots] → [hardware] → [pins]）；一条形状问题一条点名，
//! 不静默放行。查册、缺格、档位一致性等**判据**不在这里（归 `profile_checks.rs`），
//! 本文件只回答"文本说了什么"。

mod bindings;
mod budget;
mod document;
mod domain;
mod hardware;
mod pins;
mod platform;
mod runtime;
mod slots;
mod supplies;

pub use document::{rr3_profile_abs, rr3_profile_parse};
