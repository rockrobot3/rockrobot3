//! 传输层：trait 定型 + uds 唯一实现（ADR-0008）。
//!
//! 三个 trait 构成接缝（ADR-0007 §裁定 3 的"造一次就不再推翻"落在这里）：
//! - [`Rr3TransportConn`]：连接侧，同步阻塞、**所有权移交**语义——`rr3_send_buf` 消费整个
//!   [`Rr3FrameBuf`]（幂等 seal 后单次 write 提交），`rr3_recv_buf` 返回新分配的所有权缓冲。
//!   M5 的 shm 在同一签名下把"拷贝"降为环形缓冲 grant，不改接口；tcp（预留，标准端口 6060）同。
//! - [`Rr3TransportListener`]：监听侧，accept 产出连接。一次 accept 的 `Err` 只属于那一次，
//!   监听方决定继续（rrd 的口径：accept 失败记日志不带走守护进程）。
//! - [`Rr3TransportEndpoint`]：端点建立，清理残留 → bind（uds：删残留 socket 文件 →
//!   `UnixListener::bind`；shm：解链对象 → 创建共享内存区）。
//!
//! **不引 async**（ADR-0008 裁定 1）：rrd 的并发形状是"每连接 2 线程 + 阻塞 IO"，
//! 28+ 条线级用例钉在这个形状上；async 运行时只进模块侧（M3 定 executor）。
//!
//! 规划分层（ADR-0006 §5）：
//! - `uds`：M2 唯一实现，路径取 [`RR3_UDS_PATH_DEFAULT`]，可被 [`RR3_BUS_PATH_ENV`] 覆盖
//! - `shm`：实时控制面（100Hz–1kHz 关节状态/力矩/PCM 流），初版已落地（ADR-0011：
//!   单写者多读者环形缓冲、丢最旧保最新、POSIX 附着）
//! - `tcp`：多机协作，预留
//! - `uart`：跨机点对点（ESP32 ↔ 宿主，0.0.7 批C），conn 面经 blanket impl 同源帧语义
//!
//! 反射环（1k–10kHz 电机 FOC/PID）永远不进总线，见 ADR-0007 §裁定 1。
//! rrd 服务侧（conn.rs）在 M2 仍持有 `UnixStream` 具体类型走 socket 专有门（读超时/shutdown），
//! 只把 bind/cleanup/accept 三处换成 trait 工厂；全量泛化推迟到 M5 shm 落地时评估（ADR-0008）。

pub mod buf;
pub mod frame;
pub mod shm;
#[cfg(unix)]
pub mod uds;
// UART 第三实现位（0.0.7 批C，ADR-0008）：宿主侧 raw termios；点对点无监听语义，
// 只落 conn 面（经 Read+Write blanket impl），endpoint/listener 两 trait 不适用
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod uart;

use std::io::{Read, Write};
// no_std 显式化（0.0.7 批B）：std prelude 不再随 `#![no_std]` 隐式注入
use std::string::ToString;

use crate::transport::buf::{rr3_framebuf_read, Rr3FrameBuf};
use crate::transport::frame::Rr3FrameError;

/// 默认 UDS 路径（ADR-0006 §6，自有路径避免与同机真实 Airymax 运行时冲突）
pub const RR3_UDS_PATH_DEFAULT: &str = "/var/run/rock3-ipc.sock";

/// 覆盖 UDS 路径的环境变量键（naming.md：`RR3_` 大写前缀）
pub const RR3_BUS_PATH_ENV: &str = "RR3_BUS_PATH";

/// 连接侧：同步阻塞、所有权移交（ADR-0007 §裁定 3 / ADR-0008）。
///
/// - `rr3_send_buf`：**消费**整个缓冲；未 seal 时先幂等 seal 再发送，整帧单次 `write_all`
///   提交，读侧不可能看到两半。
/// - `rr3_recv_buf`：阻塞至读满一帧；缓冲所有权随返回值移交。
///   错误语义与帧层一致（`Closed` / `TornHeader` / `TornPayload` / `Header` / `Io`）。
pub trait Rr3TransportConn: Send {
    fn rr3_send_buf(&mut self, buf: Rr3FrameBuf) -> Result<(), Rr3FrameError>;
    fn rr3_recv_buf(&mut self) -> Result<Rr3FrameBuf, Rr3FrameError>;
}

/// 监听侧：accept 产出连接。
pub trait Rr3TransportListener: Send {
    type Conn: Rr3TransportConn;
    /// 阻塞 accept 一条连接。`Err` 只描述这一次 accept（如临时 fd 用尽），不是监听终结。
    fn rr3_accept(&self) -> std::io::Result<Self::Conn>;
}

/// 监听端点建立：清理残留 → bind。
pub trait Rr3TransportEndpoint: Sized {
    /// 清理上一次运行留下的端点残留。uds：socket 文件存在即删；不存在不报错。
    fn rr3_cleanup(path: &std::path::Path) -> std::io::Result<()>;
    /// 建立监听端点。
    fn rr3_bind(path: &std::path::Path) -> std::io::Result<Self>;
}

/// 任意 `Read + Write` 上的通用实现： framed 收发就是帧层自由函数。
/// uds 的 `UnixStream` 由 `uds.rs` 以本实现覆盖（cfg(unix)）；M5 的 shm 写自己的实现。
impl<T: Read + Write + Send> Rr3TransportConn for T {
    fn rr3_send_buf(&mut self, mut buf: Rr3FrameBuf) -> Result<(), Rr3FrameError> {
        buf.rr3_seal(); // 幂等：未 seal 就 seal，已 seal 落同一串字节
        self.write_all(buf.rr3_as_sealed_bytes()?)
            .map_err(|e| Rr3FrameError::Io(e.to_string()))?;
        self.flush().map_err(|e| Rr3FrameError::Io(e.to_string()))
    }

    fn rr3_recv_buf(&mut self) -> Result<Rr3FrameBuf, Rr3FrameError> {
        rr3_framebuf_read(self)
    }
}
