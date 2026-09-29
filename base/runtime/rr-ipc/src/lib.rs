//! rr-ipc —— A-IPC（Airy IPC）协议层与传输层抽象。
//!
//! 字节级事实源是 `contracts/envelope.schema.json`（歧义裁定见 `contracts/README.md`）。
//! v0.0.1 交付：128B 消息头编解码 + 与契约逐字段对齐的守护测试。
//! `trait Transport` 的零拷贝签名与 uds 实现随 M2 落地（ADR-0007 §裁定 3）。

pub mod protocol;
pub mod transport;

pub use protocol::header::{
    flags, opcode, Rr3Header, Rr3HdrError, Rr3PayloadType, Rr3StatusCode, MAGIC, MAX_PAYLOAD_LEN,
    RR3_IPC_HDR_SIZE,
};
pub use transport::frame::{
    rr3_frame_read, rr3_frame_to_bytes, rr3_frame_write, Rr3Frame, Rr3FrameError,
};
pub use transport::{RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};

/// 契约 schema 版本（对应 `contracts/envelope.schema.json` 的冻结轮次）
pub const RR3_CONTRACT_SCHEMA_VERSION: u32 = 1;
