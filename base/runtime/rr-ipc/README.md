# rr-ipc —— A-IPC 协议层 + 传输层

字节级事实源是 [`contracts/envelope.schema.json`](../../../contracts/envelope.schema.json)，
歧义裁定见 [`contracts/README.md`](../../../contracts/README.md)（ADR-0006）。

| 模块 | 内容 |
|---|---|
| `protocol/header.rs` | 128B 定长消息头（Layout C）逐字段编解码：`RR3_IPC_HDR_SIZE` 与偏移表常量、magic 字段值 `MAGIC = 0x4152_4531`（小端落线）、`opcode` / `flags` / `Rr3PayloadType` / `Rr3StatusCode` 枚举、`Rr3Header`（`from_bytes` / `to_bytes`）、`Rr3HdrError`、`MAX_PAYLOAD_LEN`（64 MiB）；与契约逐字段对齐 |
| `protocol/body.rs` | payload 的 2 字节小端类型前缀：`RR3_BODY_TYPE_PREFIX_LEN`、`rr3_body_encode` / `rr3_body_split`、`Rr3BodyError` |
| `transport/frame.rs` | 帧层读写：`Rr3Frame`（头 + 体）、`rr3_frame_read` / `rr3_frame_write` / `rr3_frame_to_bytes`、`Rr3FrameError` 区分"头只读了一半就 EOF"与"头本身非法"（另有 `Unsealed`：缓冲未 seal 就取整帧字节），校验失败时不许有半帧落进流里 |
| `transport/buf.rs` | 所有权移交的整帧缓冲（ADR-0007 §裁定 3 / ADR-0008）：`Rr3FrameBuf` 单块连续内存（`[0..128)` 头预留区 + payload，`rr3_payload_mut` 直填、`rr3_seal` 原地幂等编头）与 `rr3_framebuf_read`（读侧同一错误语义）；`Rr3Frame` 保留为纯值视图，两形状可低成本互转 |
| `transport/mod.rs` | trait 三件（ADR-0008）：连接侧 `Rr3TransportConn`（`rr3_send_buf` 消费缓冲、幂等 seal 后单次 `write_all`；`rr3_recv_buf` 返回所有权缓冲；对一切 `Read + Write + Send` 有统一 blanket 实现）、监听侧 `Rr3TransportListener`、端点 `Rr3TransportEndpoint`（清理残留 → bind）；两个路径常量 `RR3_UDS_PATH_DEFAULT`（`/var/run/rock3-ipc.sock`）与 `RR3_BUS_PATH_ENV`（`RR3_BUS_PATH`）。**不引 async**（裁定 1：同步线程模型，async 只进模块侧）；shm（M5）/tcp（预留）在同一签名下落地，不写半成品 |
| `transport/uds.rs`（`#[cfg(unix)]`） | M2 唯一实现：`Rr3UdsListener`（Endpoint + Listener，`type Conn = UnixStream`——服务侧继续走 socket 专有门）与客户端入口 `rr3_connect` |
| `lib.rs` | 对外再导出上面这些类型，并定义契约版本常量 `RR3_CONTRACT_SCHEMA_VERSION`（rr-bus 再导出它） |
| `src/bin/ipct003_dump.rs` | IPC-T-003 跨架构对拍夹具：打印 golden 头与五种 payload 类型各一整帧的 hex（全部编译期字面量，无时钟/随机/环境依赖），末行带 `RR3_CONTRACT_SCHEMA_VERSION`；对拍入口 `tools/ci/ipct003.sh`（不进九步门，登记于 conventions.md §2） |

守护测试：`cargo test -p rr-ipc`——含 envelope 契约对齐用例、golden 向量比对，以及
`tests/envelope_meta.rs` 那组**读 JSON 契约键**的元数据守护（128B 头长、偏移表合计、
payload 上下界、schema 版本，防"Rust 自己对自己"），由 `tools/ci/check.sh` 的 ①b 全量执行。
**条数不在本文件登记**，唯一落点
[`docs/process/conventions.md`](../../../docs/process/conventions.md) §2；
本 crate 做到哪一步见 [`docs/status.md`](../../../docs/status.md)。
