# rr-ipc —— A-IPC 协议层 + 传输抽象

字节级事实源是 [`contracts/envelope.schema.json`](../../../contracts/envelope.schema.json)，
歧义裁定见 [`contracts/README.md`](../../../contracts/README.md)（ADR-0006）。

| 模块 | 内容 | v0.0.1 状态 |
|---|---|---|
| `protocol/header.rs` | 128B 定长消息头（Layout C）逐字段编解码：偏移表常量、magic `ARE1`、opcode/flags 枚举、`Rr3Header`/`Rr3StatusCode` | **已实装**，与契约逐字段对齐 |
| `transport/mod.rs` | `trait Transport` 抽象 + 路径常量（`RR3_UDS_PATH_DEFAULT=/var/run/rock3-ipc.sock`、`RR3_BUS_PATH_ENV`） | 仅常量；trait 零拷贝签名与 **uds 实现 → M2**（避免造出 ADR-0007 §裁定 3 要推翻的接口）；shm → M5；tcp 预留 |

守护测试：`cargo test -p rr-ipc`——含 envelope 契约对齐用例与
`tests/envelope_meta.rs`（4 条**读 JSON 契约键**的元数据守护：128B 头长、偏移表合计、
payload 上下界、schema 版本，防"Rust 自己对自己"）。由 `tools/ci/check.sh` ①b 全量执行。
