# rr-registry —— manifest 与主题注册表校验器

[SC] 契约层的消费方：字段契约 `src/module.schema.toml`（编译期内嵌）与
[`contracts/topics.toml`](../../../contracts/topics.toml)（主题所有权）的**校验实现**。

| 文件 | 内容 |
|---|---|
| `schema.rs` | 加载 `module.schema.toml`，导出 `Rr3FieldSpec`、字段查询与规则列表（`rr3_registry_field_specs` / `rr3_registry_rule_list` / `rr3_registry_get_by_path`）；`RR3_MANIFEST_SCHEMA_VERSION=1`（与信封契约的 `x-schema-version` 是两套键） |
| `topics.rs` | `Rr3TopicSet`（由 `rr3_registry_topic_set_from_text` 从 topics.toml 文本构造）与其访问器 `rr3_topic_set_contains` / `rr3_topic_set_owner` / `rr3_topic_set_kind` / `rr3_topic_set_names` / `rr3_topic_set_is_empty` / `rr3_topic_set_merge` / `rr3_topic_set_check`（后者强制"主题首段 == 登记的 owner"） |
| `validate.rs` | `Rr3ValidationReport` 与入口：`rr3_registry_validate_manifest`（按路径）、`rr3_registry_validate_manifest_text`（按文本，注册握手走这条）、`rr3_registry_validate_topics`（前缀归属：主题首段必须等于某 owner 名）、`rr3_registry_topic_owners` / `rr3_registry_topic_names`、`rr3_registry_check`（单条规则执行） |

调用方实况：`rr3-mkmod` 的生成自检、本 crate `tests/contract.rs`（schema 校验 + 注册表形状 pin），
以及 **rrd 的注册握手与发布判定**——`conn.rs` 用 `rr3_registry_validate_manifest_text` 决定要不要
让这条连接进在册表，`registry.rs` 的 `RrdRegistry::rr3_reg_publish_check` 拿 `Rr3TopicSet`
判在册与 owner（职责与顺序见
[`architecture.md`](../../../docs/architecture/architecture.md) §6.3；接到哪一段属实况，见
[`docs/status.md`](../../../docs/status.md)）。
主题与 kind 的清单本身不在这里复述，事实源是
[`contracts/topics.toml`](../../../contracts/topics.toml)。

测试：`cargo test -p rr-registry`；契约用例由 check.sh 的 ②/③ 点名执行，
**条数只登记在 [`docs/process/conventions.md`](../../../docs/process/conventions.md) §2**。
