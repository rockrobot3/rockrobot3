# rr-bus —— 总线客户端骨架

主题匹配订阅、QoS、回压的落点（architecture.md §6）；依赖 `rr-ipc`（Cargo path 依赖）。

v0.0.1 **故意不放半成品的 `BusClient`**，只冻结常量层，避免 M2 推翻自己：

| 常量 | 含义 | 状态 |
|---|---|---|
| `RR3_BROADCAST_DST_TASK` | 广播目标（A-IPC 标准 dst_task=0） | 已定稿 |
| `RR3_TOPIC_SEP` | 主题分隔符 `/` | 已定稿 |
| `RR3_PRIORITY_MIN/MAX` | EVENT 优先级窗口 | **占位值**（0..=139 带内），语义随 M2 调度 ADR 定稿后替换 |
| `RR3_BUS_CONTRACT_VERSION` | 契约 schema 版本再导出（透传 rr-ipc） | 已定稿 |

总线运行时（UDS 客户端、订阅分发、rrd 握手回显）→ **M2**。
测试：`cargo test -p rr-bus`（3 条，check.sh ①b 覆盖）。
