//! # rr-registry
//!
//! [SC] 契约层的消费方：manifest 字段契约（`module.schema.toml`）与
//! 主题所有权（`contracts/topics.toml`）的校验实现。
//! rrd 在模块注册时调用本 crate 决定是否放行（architecture.md §7）。

pub mod schema;
pub mod validate;

pub use schema::{
    Rr3FieldSpec, RR3_MANIFEST_SCHEMA_VERSION, rr3_registry_field_specs,
    rr3_registry_get_by_path, rr3_registry_rule_list,
};
pub use validate::{
    Rr3ValidationReport, rr3_registry_check, rr3_registry_topic_names,
    rr3_registry_topic_owners, rr3_registry_validate_manifest,
    rr3_registry_validate_topics,
};
