# rr-registry —— manifest 与主题注册表校验器

[SC] 契约层的消费方：字段契约 `src/module.schema.toml`（编译期内嵌）与
[`contracts/topics.toml`](../../../contracts/topics.toml)（主题所有权）的**校验实现**。

| 文件 | 内容 |
|---|---|
| `schema.rs` | 加载 `module.schema.toml`，导出 `Rr3FieldSpec`、字段查询与规则列表；`RR3_MANIFEST_SCHEMA_VERSION=1`（与信封契约的 `x-schema-version` 是两套键） |
| `validate.rs` | `rr3_registry_validate_manifest`（单 manifest）、`rr3_registry_validate_topics`（前缀归属：主题首段必须等于某 owner 名）、`rr3_registry_check`（规则执行） |

调用方实况：`rr3-mkmod` 生成自检 + 本 crate `tests/contract.rs`（schema 校验 +
注册表形状 pin）；**"rrd 在模块注册时调用本 crate 决定放行"属 M1/M2 待落地**
（architecture.md §6.3）。

测试：`cargo test -p rr-registry`；契约用例由 check.sh ②/③ 点名执行。
