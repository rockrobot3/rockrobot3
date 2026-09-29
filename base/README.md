# base —— 基座层

承载全部横切关注点（安全兜底、日志、时间同步、OTA），这些能力**不进模块**——
基座存活则安全兜底存活（[`architecture.md`](../docs/architecture/architecture.md) §2 原则 1）。

| 目录 | 层 | 内容 |
|---|---|---|
| `os/` | OS 层 | Buildroot + Linux 6.6（RK3566）的 defconfig / 分区与板级文件，A/B 槽与 maskrom 自烧录 |
| `runtime/` | 基座运行时 | Rust workspace：6 个 crate（rrd + rr-ipc/rr-bus/rr-registry/rr-hal/rr-ota） |
| `ota/` | 升级通道 | 分区布局文档 `partition-layout.md` + `rr3-ota-ctl` CLI（独立 Rust 工程） |

各 crate 的职责见 [`runtime/README.md`](runtime/README.md)；本目录做到哪一步属实况，
唯一落点 [`docs/status.md`](../docs/status.md)，本文件不记进度。
