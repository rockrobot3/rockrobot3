//! rr-ipc —— A-IPC（Airy IPC）协议层与传输层抽象。
//!
//! 字节级事实源是 `contracts/envelope.schema.json`（歧义裁定见 `contracts/README.md`）。
//! v0.0.1 交付：128B 消息头编解码 + 与契约逐字段对齐的守护测试。
//! M2 交付：`trait Transport` 三件（连接 / 监听 / 端点）+ 所有权移交帧缓冲 + uds 实现
//! （ADR-0007 §裁定 3 / ADR-0008）。

// no_std 起手（0.0.7 批B）：协议层纯 core+alloc，裸金属目标（xtensa）可编译；
// std 目标经条件性 `extern crate std` 恢复 transport 层的 std::io 面——std 目标上
// 行为零变化（std 路径是显式的，不依赖 prelude）。
#![no_std]

// alloc 无条件引入（批J G1）：Rr3FrameBuf/Rr3Frame 的 Vec/String 载体在裸金属
// 目标上走 alloc（std 目标上 alloc 与 std::vec 是同一实现，零行为差）。
extern crate alloc;

#[cfg(not(target_os = "none"))]
extern crate std;

pub mod protocol;
// transport 层拆门（批J G1，ADR-0018 修订 3 ①）：窄缝 bytestream（Rr3ByteStream）
// 与帧缓冲 buf（alloc 面）在裸金属目标（target_os="none"，如 0.0.7 层① 的
// xtensa-esp32-none-elf）上可达——"板侧接总线 = 实现两个 usize 方法"由此成立；
// std::io 半边（frame/shm/uds/uart + Listener/Endpoint trait + std 单向 adapter）
// 留在门内。ESP32 侧 std 语境（esp-rs std / espidf 目标，0.0.7 Q2 裁定）走 unix
// 系照常编译，行为与线上字节零改动。
pub mod transport;

pub use protocol::b64::rr3_base64_encode;
pub use protocol::body::{rr3_body_encode, rr3_body_split, Rr3BodyError, RR3_BODY_TYPE_PREFIX_LEN};
pub use protocol::header::{
    flags, opcode, Rr3HdrError, Rr3Header, Rr3PayloadType, Rr3StatusCode, MAGIC, MAX_PAYLOAD_LEN,
    RR3_IPC_HDR_SIZE,
};
// 窄缝与帧缓冲（no_std 可达，无条件导出）；帧数据形状（Rr3Frame / 编码）住
// protocol::wire；io 半边（读写）依赖 std::io，随门控。
pub use protocol::wire::{rr3_frame_to_bytes, Rr3Frame, Rr3FrameError};
pub use transport::buf::{rr3_framebuf_read, Rr3FrameBuf};
pub use transport::bytestream::Rr3ByteStream;
#[cfg(not(target_os = "none"))]
pub use transport::frame::{rr3_frame_read, rr3_frame_write};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use transport::shm::Rr3ShmListener;
// 门与 transport::shm 模块自身同径（批D1 修正）：espidf（unix 但非 linux/macos）
// 上 `not(target_os = "none")` 放行而模块缺席 → E0432，xtensa-espidf 编译面实爆。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use transport::shm::{
    Rr3ShmReaderConn, Rr3ShmWriterConn, RR3_SHM_NAME_DEFAULT, RR3_SHM_NAME_ENV,
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use transport::uart::{rr3_uart_baud_speed, Rr3UartConn, RR3_UART_BAUD_DEFAULT};
#[cfg(unix)]
pub use transport::uds::Rr3UdsListener;
pub use transport::{Rr3TransportConn, RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};
#[cfg(not(target_os = "none"))]
pub use transport::{Rr3TransportEndpoint, Rr3TransportListener};

/// 契约 schema 版本（对应 `contracts/envelope.schema.json` 的冻结轮次）
pub const RR3_CONTRACT_SCHEMA_VERSION: u32 = 1;
