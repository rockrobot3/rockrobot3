//! rr-ipc —— A-IPC（Airy IPC）协议层与传输层抽象。
//!
//! 字节级事实源是 `contracts/envelope.schema.json`（歧义裁定见 `contracts/README.md`）。
//! v0.0.1 交付：128B 消息头编解码 + 与契约逐字段对齐的守护测试。
//! M2 交付：`trait Transport` 三件（连接 / 监听 / 端点）+ 所有权移交帧缓冲 + uds 实现
//! （ADR-0007 §裁定 3 / ADR-0008）。

pub mod protocol;
pub mod transport;

pub use protocol::body::{rr3_body_encode, rr3_body_split, Rr3BodyError, RR3_BODY_TYPE_PREFIX_LEN};
pub use protocol::header::{
    flags, opcode, Rr3Header, Rr3HdrError, Rr3PayloadType, Rr3StatusCode, MAGIC, MAX_PAYLOAD_LEN,
    RR3_IPC_HDR_SIZE,
};
pub use transport::buf::{rr3_framebuf_read, Rr3FrameBuf};
pub use transport::frame::{
    rr3_frame_read, rr3_frame_to_bytes, rr3_frame_write, Rr3Frame, Rr3FrameError,
};
pub use transport::shm::{Rr3ShmReaderConn, Rr3ShmWriterConn, RR3_SHM_NAME_DEFAULT, RR3_SHM_NAME_ENV};
pub use transport::{
    Rr3TransportConn, Rr3TransportEndpoint, Rr3TransportListener, RR3_BUS_PATH_ENV,
    RR3_UDS_PATH_DEFAULT,
};
#[cfg(unix)]
pub use transport::uds::Rr3UdsListener;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use transport::shm::Rr3ShmListener;

/// 契约 schema 版本（对应 `contracts/envelope.schema.json` 的冻结轮次）
pub const RR3_CONTRACT_SCHEMA_VERSION: u32 = 1;
