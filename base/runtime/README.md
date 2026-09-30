# base/runtime —— 基座 Rust workspace

在此目录执行 `cargo check --workspace` / `cargo test --workspace`。

| crate | 职责 |
|---|---|
| `rr-ipc` | A-IPC 字节层：128B 定长消息头编解码、payload 类型前缀、帧读写与传输路径常量 |
| `rr-bus` | 总线客户端的落点（主题匹配订阅、QoS、回压）；现为总线常量层（广播/任务号/优先级窗口） |
| `rr-registry` | manifest 字段字典与校验器、主题注册表 `Rr3TopicSet`、前缀归属规则——rrd 与 rr3-mkmod 共用 |
| `rr-hal` | 硬件能力 trait + 数据类型 + 占位后端；具名后端有哪几路、各路由什么承载，以 `src/backend.rs` 的后端表为准，本表不复述 |
| `rr-ota` | 升级"双轨"的类型形状：整镜像 A/B 与模块级 staging |
| `rrd` | 主守护进程装配层（supervisor / health / config），事件总线的宿主：CLI 参数、主题装载、manifest 运行期事实、在册表、上行 RPC、UDS 监听与连接处理 |

每个 crate 内部的文件级事实见各自 `README.md`；**做到哪一步、还缺哪一手不写在 README**，
唯一落点 [`docs/status.md`](../../docs/status.md)。测试条数只登记在
[`docs/process/conventions.md`](../../docs/process/conventions.md) §2。

依赖预算：只用 `[workspace.dependencies]` 里已声明的 `toml`、`async-trait`、`serde_json`，
不引入 tokio/regex/nix/mio。当前边与其真实 `use` 处：`toml` → rr-registry（`schema.rs` /
`topics.rs`）、rrd（`manifest.rs` 取运行期事实）；`async_trait` → rr-hal 的五个 trait；
`serde_json` → rr-ipc（dev-dep，契约元数据守护测试读 JSON）与 rrd（`rpc.rs` / `conn.rs` /
`daemon.rs` 的应答与帧体）；路径依赖 `rr-bus` → rrd（`registry.rs` / `conn.rs` / `daemon.rs`
取 `RR3_TASK_RRD`、`RR3_TASK_FIRST_MODULE`、`RR3_BROADCAST_DST_TASK`）。
