# rr-bus —— 总线客户端骨架

主题匹配订阅、QoS、回压的落点（[`architecture.md`](../../../docs/architecture/architecture.md) §6）；
依赖 `rr-ipc`（Cargo path 依赖，并再导出它的契约版本）。

**故意不放半成品的 `BusClient`**，避免总线接口被后续批次推翻自己：本 crate 现在是一套常量。

| 常量 | 含义 |
|---|---|
| `RR3_BROADCAST_DST_TASK` | 广播目标（A-IPC 标准 dst_task=0），只用于 rrd 的下行广播与发布转发 |
| `RR3_TASK_RRD` | rrd 自己的 task 号（`1`）：上行帧的 `dst_task` 必须是它 |
| `RR3_TASK_FIRST_MODULE` | 模块 task 分配池的起点（`2`）——0 与 1 保留，三段号互不重叠 |
| `RR3_TOPIC_SEP` | 主题分隔符 `/` |
| `RR3_PRIORITY_MIN/MAX` | EVENT 优先级窗口，**占位值**（落在常量注释自己声明的 0..=139 带内）：没有任何契约/ADR 定义过该区间，语义随调度 ADR 定稿后替换 |
| `RR3_BUS_CONTRACT_VERSION` | 契约 schema 版本再导出（透传 rr-ipc 的 `RR3_CONTRACT_SCHEMA_VERSION`） |

总线运行时（UDS 客户端、订阅分发、与 rrd 的握手回显）到哪一步属实况，唯一落点
[`docs/status.md`](../../../docs/status.md)；测试条数只登记在
[`docs/process/conventions.md`](../../../docs/process/conventions.md) §2，本文件不抄数字。
