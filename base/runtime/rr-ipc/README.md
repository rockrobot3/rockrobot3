# rr-ipc —— A-IPC 协议层 + 传输层

字节级事实源是 [`contracts/envelope.schema.json`](../../../contracts/envelope.schema.json)，
歧义裁定见 [`contracts/README.md`](../../../contracts/README.md)（ADR-0006）。

| 模块 | 内容 |
|---|---|
| `protocol/header.rs` | 128B 定长消息头（Layout C）逐字段编解码：`RR3_IPC_HDR_SIZE` 与偏移表常量、magic 字段值 `MAGIC = 0x4152_4531`（小端落线）、`opcode` / `flags` / `Rr3PayloadType` / `Rr3StatusCode` 枚举、`Rr3Header`（`from_bytes` / `to_bytes`）、`Rr3HdrError`、`MAX_PAYLOAD_LEN`（64 MiB）；与契约逐字段对齐 |
| `protocol/body.rs` | payload 的 2 字节小端类型前缀：`RR3_BODY_TYPE_PREFIX_LEN`、`rr3_body_encode` / `rr3_body_split`、`Rr3BodyError` |
| `transport/frame.rs` | 帧层读写：`Rr3Frame`（头 + 体）、`rr3_frame_read` / `rr3_frame_write` / `rr3_frame_to_bytes`、`Rr3FrameError` 区分"头只读了一半就 EOF"与"头本身非法"，校验失败时不许有半帧落进流里 |
| `transport/mod.rs` | 两个路径常量 `RR3_UDS_PATH_DEFAULT`（`/var/run/rock3-ipc.sock`）与 `RR3_BUS_PATH_ENV`（`RR3_BUS_PATH`），并 `pub mod frame`；**此处没有 `trait Transport` 定义**——零拷贝签名与 uds / shm / tcp 分层按 ADR-0007 §裁定 3 一并定，本 crate 不预先造会被推翻的接口 |
| `lib.rs` | 对外再导出上面这些类型，并定义契约版本常量 `RR3_CONTRACT_SCHEMA_VERSION`（rr-bus 再导出它） |

守护测试：`cargo test -p rr-ipc`——含 envelope 契约对齐用例、golden 向量比对，以及
`tests/envelope_meta.rs` 那组**读 JSON 契约键**的元数据守护（128B 头长、偏移表合计、
payload 上下界、schema 版本，防"Rust 自己对自己"），由 `tools/ci/check.sh` 的 ①b 全量执行。
**条数不在本文件登记**，唯一落点
[`docs/process/conventions.md`](../../../docs/process/conventions.md) §2；
本 crate 做到哪一步见 [`docs/status.md`](../../../docs/status.md)。
