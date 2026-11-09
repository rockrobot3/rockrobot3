//! # rr-registry
//!
//! [SC] 契约层的消费方：manifest 字段契约（`module.schema.toml`）与
//! 主题所有权（`contracts/topics.toml`）的校验实现。
//! v0.0.1 的调用方是 `tools/mkmodule`（生成自检）与本 crate 的契约测试（`tests/contract.rs`，
//! 由 `tools/ci/check.sh` ②/③ 点名）；"rrd 在模块注册时调用本 crate 决定放行"属
//! M1/M2 待落地的运行时环节（architecture.md §6.3、§7）。

pub mod assemble;
pub mod atom;
pub mod composite;
pub mod depends;
pub mod devices;
pub mod platform;
pub mod roles;
pub mod schema;
pub mod slots;
pub mod topics;
pub mod validate;

pub use assemble::{
    rr3_assemble_parse, rr3_injection_gaps, Rr3AssembleDemand, Rr3AssembleFacts, Rr3AssembleHandle,
    Rr3AssembleOp, RR3_ASSEMBLE_DESC_KEYS, RR3_ASSEMBLE_HANDLE_TRAITS, RR3_ASSEMBLE_SCHEMA_VERSION,
};
pub use atom::{
    rr3_atom_chk, rr3_atom_field_specs, rr3_atom_gate, rr3_board_free_chk, RR3_ATOM_SCHEMA_VERSION,
};
pub use composite::{rr3_composite_parse, Rr3CompositeFacts, RR3_COMPOSITE_SCHEMA_VERSION};
pub use depends::{rr3_boot_order, rr3_depends_parse, rr3_plan_boot, Rr3BootPlan, Rr3DependsFacts};
pub use devices::{rr3_dev_parse, Rr3DeviceTable};
pub use platform::{
    rr3_plat_parse, Rr3ConflictEntry, Rr3PlatformTable, RR3_CONTRACT_PLATFORM_PATH,
    RR3_PLATFORM_KINDS, RR3_PLATFORM_OUTPUT_RULES,
};
pub use roles::{rr3_role_parse, Rr3RoleTable};
pub use schema::{
    rr3_field_specs, rr3_get_by_path, rr3_rule_list, Rr3FieldSpec, RR3_MANIFEST_SCHEMA_VERSION,
};
pub use slots::{rr3_slot_parse, Rr3SlotTable};
pub use topics::{
    rr3_topic_parse, Rr3TopicSet, RR3_CONTRACT_REQUEST_COUNT, RR3_CONTRACT_TOPIC_COUNT,
};
pub use validate::{
    rr3_manifest_chk, rr3_manifest_gate, rr3_registry_check, rr3_topics_chk, rr3_topics_names,
    rr3_topics_owners, Rr3ValidationReport,
};
