# base/runtime —— 基座 Rust workspace

在此目录执行 `cargo check --workspace` / `cargo test --workspace`。

| crate | 职责 | v0.0.1 状态 |
|---|---|---|
| `rr-ipc` | A-IPC 128B 头编解码 + `trait Transport` | 头编解码 + 契约对齐守护测试；transport uds 实现 → M2 |
| `rr-bus` | 主题匹配订阅、QoS、回压 | 广播/优先级常量；总线运行时 → M2 |
| `rr-registry` | manifest 解析、依赖仲裁、生命周期状态机 | **字段字典 + 校验器（真实实现，rrd 与 rr3-mkmod 共用）**；状态机 → M2 |
| `rr-hal` | traits + linux{alsa,gpiod,nm} / sim 双实现 | traits 与数据类型（architecture.md §5）；实现 → M3/M5 |
| `rr-ota` | slot 状态机、回滚、factory recovery、模块级更新 | 类型与双轨文档 → M6 |
| `rrd` | 主守护进程装配层（supervisor/health/config） | 打印契约版本；注册/心跳 → M2 |

依赖预算：`toml`、`async-trait`，`serde_json` 仅在 rr-ipc dev-dep。不引入 tokio/regex。
