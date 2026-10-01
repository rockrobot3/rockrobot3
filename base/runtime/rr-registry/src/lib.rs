//! # rr-registry
//!
//! [SC] 契约层的消费方：manifest 字段契约（`module.schema.toml`）与
//! 主题所有权（`contracts/topics.toml`）的校验实现。
//! v0.0.1 的调用方是 `tools/mkmodule`（生成自检）与本 crate 的契约测试（`tests/contract.rs`，
//! 由 `tools/ci/check.sh` ②/③ 点名）；"rrd 在模块注册时调用本 crate 决定放行"属
//! M1/M2 待落地的运行时环节（architecture.md §6.3、§7）。

pub mod depends;
pub mod schema;
pub mod slots;
pub mod topics;
pub mod validate;

pub use schema::{
    Rr3FieldSpec, RR3_MANIFEST_SCHEMA_VERSION, rr3_registry_field_specs,
    rr3_registry_get_by_path, rr3_registry_rule_list,
};
pub use topics::{Rr3TopicSet, rr3_registry_topic_set_from_text};
pub use slots::{Rr3SlotTable, rr3_registry_slot_table_from_text};
pub use depends::{
    Rr3BootPlan, Rr3DependsFacts, rr3_registry_depends_facts_from_text, rr3_registry_plan_boot,
    rr3_registry_resolve_boot_order,
};
pub use validate::{
    Rr3ValidationReport, rr3_registry_check, rr3_registry_topic_names,
    rr3_registry_topic_owners, rr3_registry_validate_manifest,
    rr3_registry_validate_manifest_text, rr3_registry_validate_topics,
};
