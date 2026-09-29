# base/runtime —— 基座 Rust workspace

在此目录执行 `cargo check --workspace` / `cargo test --workspace`。

| crate | 职责 | v0.0.1 状态 |
|---|---|---|
| `rr-ipc` | A-IPC 128B 头编解码 + `trait Transport` | 头编解码 + 契约对齐守护测试；transport uds 实现 → M2 |
| `rr-bus` | 主题匹配订阅、QoS、回压 | 广播/优先级/任务号常量；总线运行时 → M2 |
| `rr-registry` | manifest 解析、依赖仲裁、生命周期状态机 | **字段字典 + 校验器（真实实现，rrd 与 rr3-mkmod 共用）**；状态机 → M2 |
| `rr-hal` | traits + linux{alsa,gpiod,nm} / sim 双实现 | traits 与数据类型（architecture.md §5）；实现 → M3/M5 |
| `rr-ota` | slot 状态机、回滚、factory recovery、模块级更新 | 类型与双轨文档 → M6 |
| `rrd` | 主守护进程装配层（supervisor/health/config） | lib+bin 已拆：命令行参数 + 所有权表装载 + 非 Unix 平台门（Windows 本机实测退出码 1）；UDS 监听/注册/心跳 → M2 |

依赖预算：只用 `[workspace.dependencies]` 里已声明的 `toml`、`async-trait`、`serde_json`，
不引入 tokio/regex/nix/mio。当前边：`toml` → rr-registry、rrd；`async-trait` → rr-hal；
`serde_json` → rr-ipc（dev-dep）、rrd（rrd 这两条边为 M2 后续任务的 manifest/RPC 预留，
本阶段代码尚未 `use` 它们，`cargo check` 不会因未使用依赖告警）。
