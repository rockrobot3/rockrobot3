# base —— 基座层

承载全部横切关注点（安全兜底、日志、时间同步、OTA），这些能力**不进模块**——
基座存活则安全兜底存活（architecture.md §2 原则 1）。

| 目录 | 层 | 内容 | 状态 |
|---|---|---|---|
| `os/` | OS 层 | Buildroot + Linux 6.6（RK3566），A/B 分区与 maskrom 自烧录 | 仅 README，M1 落地 |
| `runtime/` | 基座运行时 | Rust workspace：6 个 crate（rrd + rr-ipc/rr-bus/rr-registry/rr-hal/rr-ota） | 已建立，契约层实装、余为骨架（M2 补运行时） |
| `ota/` | 升级通道 | 分区布局文档 + `rr3-ota-ctl` CLI | **规划（M6），目录尚未创建** |

各 crate 职责与 v0.0.1 状态速查见 [`runtime/README.md`](runtime/README.md)。
