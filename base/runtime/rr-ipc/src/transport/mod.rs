//! 传输层：trait 定型 + uds 唯一实现（ADR-0008）。
//!
//! **门下沉（批J G1，ADR-0018 修订 3 ①）**：本模块在 `target_os = "none"` 下也编译
//! ——窄缝 [`Rr3ByteStream`]（`bytestream.rs`）与帧缓冲（`buf.rs`，alloc 面）是
//! no_std 可达的；std::io 半边（`frame.rs` / `uds.rs` / `uart.rs`、
//! Listener/Endpoint 两 trait、std 单向 adapter）全部留在门内。裸金属目标上
//! "接总线 = 实现两个 usize 方法"由此成立，宿主侧零变化。
//!
//! 三个 trait 构成接缝（ADR-0007 §裁定 3 的"造一次就不再推翻"落在这里）：
//! - [`Rr3TransportConn`]：连接侧，同步阻塞、**所有权移交**语义——`rr3_send_buf` 消费整个
//!   [`Rr3FrameBuf`](crate::transport::buf::Rr3FrameBuf)（幂等 seal 后单次提交），`rr3_recv_buf`
//!   返回新分配的所有权缓冲（原 shm 计划位的"拷贝降为 grant"设想随 L-32 摘面一并撤销）；
//!   tcp（预留，标准端口 6060）同。
//! - [`Rr3TransportListener`]：监听侧，accept 产出连接。一次 accept 的 `Err` 只属于那一次，
//!   监听方决定继续（rrd 的口径：accept 失败记日志不带走守护进程）。
//! - [`Rr3TransportEndpoint`]：端点建立，清理残留 → bind（uds：删残留 socket 文件 →
//!   `UnixListener::bind`）。
//!
//! **不引 async**（ADR-0008 裁定 1）：rrd 的并发形状是"每连接 2 线程 + 阻塞 IO"，
//! 28+ 条线级用例钉在这个形状上；async 运行时只进模块侧（M3 定 executor）。
//!
//! 规划分层（ADR-0006 §5）：
//! - `uds`：M2 唯一实现，路径取 [`RR3_UDS_PATH_DEFAULT`]，可被 [`RR3_BUS_PATH_ENV`] 覆盖
//! - `tcp`：多机协作，预留
//! - `uart`：跨机点对点（跨芯片对端 ↔ 宿主，0.0.7 批C），conn 面经 blanket impl 同源帧语义
//!
//! 反射环（1k–10kHz 电机 FOC/PID）永远不进总线，见 ADR-0007 §裁定 1。
//! rrd 服务侧（conn.rs）在 M2 仍持有 `UnixStream` 具体类型走 socket 专有门（读超时/shutdown），
//! 只把 bind/cleanup/accept 三处换成 trait 工厂；全量泛化评估挂 M5 里程碑（shm 第二实现
//! 已按 L-32 裁定摘面，0.0.8 批N19——本模块自此无 shm 位）。

pub mod buf;
pub mod bytestream;
#[cfg(not(target_os = "none"))]
pub mod frame;
#[cfg(unix)]
pub mod uds;
// UART 第三实现位（0.0.7 批C，ADR-0008）：宿主侧 raw termios；点对点无监听语义，
// 只落 conn 面（Read+Write 经 std 单向 adapter 进窄缝、再由 blanket impl 覆盖），
// endpoint/listener 两 trait 不适用
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod uart;

// no_std 显式化（0.0.7 批B 口径）：std prelude 不随 `#![no_std]` 隐式注入
use alloc::format;

use crate::protocol::wire::Rr3FrameError;
use crate::transport::buf::{rr3_framebuf_read, Rr3FrameBuf};
use crate::transport::bytestream::Rr3ByteStream;

/// 默认 UDS 路径（ADR-0006 §6，自有路径避免与同机真实 Airymax 运行时冲突）
pub const RR3_UDS_PATH_DEFAULT: &str = "/var/run/rock3-ipc.sock";

/// 覆盖 UDS 路径的环境变量键（naming.md：`RR3_` 大写前缀）
pub const RR3_BUS_PATH_ENV: &str = "RR3_BUS_PATH";

/// 连接侧：同步阻塞、所有权移交（ADR-0007 §裁定 3 / ADR-0008）。
///
/// - `rr3_send_buf`：**消费**整个缓冲；未 seal 时先幂等 seal 再发送，整帧单次提交，
///   读侧不可能看到两半。
/// - `rr3_recv_buf`：阻塞至读满一帧；缓冲所有权随返回值移交。
///   错误语义与帧层一致（`Closed` / `TornHeader` / `TornPayload` / `Header` / `Io`）。
pub trait Rr3TransportConn: Send {
    fn rr3_send_buf(&mut self, buf: Rr3FrameBuf) -> Result<(), Rr3FrameError>;
    fn rr3_recv_buf(&mut self) -> Result<Rr3FrameBuf, Rr3FrameError>;
}

/// 监听侧：accept 产出连接。
#[cfg(not(target_os = "none"))]
pub trait Rr3TransportListener: Send {
    type Conn: Rr3TransportConn;
    /// 阻塞 accept 一条连接。`Err` 只描述这一次 accept（如临时 fd 用尽），不是监听终结。
    fn rr3_accept(&self) -> std::io::Result<Self::Conn>;
}

/// 监听端点建立：清理残留 → bind。
#[cfg(not(target_os = "none"))]
pub trait Rr3TransportEndpoint: Sized {
    /// 清理上一次运行留下的端点残留。uds：socket 文件存在即删；不存在不报错。
    fn rr3_cleanup(path: &std::path::Path) -> std::io::Result<()>;
    /// 建立监听端点。
    fn rr3_bind(path: &std::path::Path) -> std::io::Result<Self>;
}

/// 窄缝上的通用实现（批J G1 换底，ADR-0018 修订 1 ③）：blanket impl 的下底从
/// `Read + Write` 换到 [`Rr3ByteStream`]——uds 的 `UnixStream`、uart 的 raw fd 包装
/// 经 std 单向 adapter 进缝后仍由本实现覆盖（行为与线上字节零改动）；板侧类型
/// 直接实现两个 `usize` 方法即可获得同一份帧语义（mod.rs tests 的形状用例为证）。
impl<S: Rr3ByteStream + Send> Rr3TransportConn for S {
    fn rr3_send_buf(&mut self, mut buf: Rr3FrameBuf) -> Result<(), Rr3FrameError> {
        buf.rr3_seal(); // 幂等：未 seal 就 seal，已 seal 落同一串字节
        let bytes = buf.rr3_as_sealed_bytes()?;
        let want = bytes.len();
        let got = self.rr3_write_all_bytes(bytes);
        if got != want {
            // 缝上无错误对象：按已写出数如实点名（io 错误折进计数，见 bytestream.rs 注记）
            return Err(Rr3FrameError::Io(format!(
                "写字节流失败：要 {want} 字节实出 {got} 字节"
            )));
        }
        Ok(())
    }

    fn rr3_recv_buf(&mut self) -> Result<Rr3FrameBuf, Rr3FrameError> {
        rr3_framebuf_read(self)
    }
}

/// std 承载侧**单向 adapter**（批J G1）：把 `std::io` 的 Read+Write 世界接进窄缝。
/// 机制核（[`Rr3ByteStream`] 与上面的 blanket impl）不引 std；本 impl 只在 std
/// 目标编译。错误折叠（缝上没有错误通道，代价见 bytestream.rs 模块注记）：
/// 读侧 io 错误 → 0（EOF 形状）；写侧 io 错误 → 返回已写出字节数（短写形状）。
/// Interrupted 重试语义原样保留（与旧 `read_fill` / `write_all` 同源）。
#[cfg(not(target_os = "none"))]
impl<T: std::io::Read + std::io::Write> Rr3ByteStream for T {
    fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
        loop {
            match self.read(buf) {
                Ok(0) => return 0,
                Ok(n) => return n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return 0,
            }
        }
    }

    fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
        let mut done = 0usize;
        while done < buf.len() {
            match self.write(&buf[done..]) {
                Ok(0) => return done, // 无进展（WriteZero 形状）——按已写出数返回
                Ok(n) => done += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return done,
            }
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::header::{opcode, Rr3Header};
    use alloc::vec::Vec;

    fn hdr(payload_len: u32) -> Rr3Header {
        let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload_len);
        h.trace_id = 0x1234_5678;
        h
    }

    /// 板侧 conn 形状用例（批J G2 的宿主面证据）：FakeStream 只实现
    /// [`Rr3ByteStream`] 两个 `usize` 方法——这就是板侧作者要写的全部——不实现
    /// Read/Write，经 blanket impl 即得完整帧语义。缓冲进出都在内存，无 io 无时钟。
    struct FakeStream {
        /// 对端发来的字节（recv 侧读它）
        rx: Vec<u8>,
        /// 发往对端的字节（send 侧写它）
        tx: Vec<u8>,
        rx_pos: usize,
        /// Some(n) = 写超过 n 字节的部分被丢弃（短写注入，验证 Io 点名）
        fail_write_after: Option<usize>,
    }

    impl FakeStream {
        fn new(rx: Vec<u8>) -> Self {
            Self {
                rx,
                tx: Vec::new(),
                rx_pos: 0,
                fail_write_after: None,
            }
        }
    }

    impl Rr3ByteStream for FakeStream {
        fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
            if self.rx_pos >= self.rx.len() {
                return 0; // 对端关闭
            }
            let n = buf.len().min(self.rx.len() - self.rx_pos);
            buf[..n].copy_from_slice(&self.rx[self.rx_pos..self.rx_pos + n]);
            self.rx_pos += n;
            n
        }

        fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
            if let Some(limit) = self.fail_write_after {
                if limit < buf.len() {
                    self.tx.extend_from_slice(&buf[..limit]);
                    return limit;
                }
            }
            self.tx.extend_from_slice(buf);
            buf.len()
        }
    }

    /// send → recv 往返逐字节一致：seal 幂等、128B 头、payload 前缀全走缝面。
    #[test]
    fn fake_stream_conn_round_trip() {
        let f = crate::protocol::wire::Rr3Frame {
            header: hdr(5),
            payload: b"hello"[..].to_vec(),
        };
        let mut conn = FakeStream::new(Vec::new());
        conn.rr3_send_buf(Rr3FrameBuf::from(&f))
            .expect("发送应成功");

        // 线上字节 = 头 + payload 一整块（128B 定长头，与 golden 同一布局事实源）
        let sent = crate::protocol::wire::rr3_frame_to_bytes(&f).unwrap();
        assert_eq!(conn.tx, sent, "整帧单次提交，读侧不可能看到两半");

        // 把发出的字节喂回读侧，recv 应还原同一帧
        let mut back = FakeStream::new(conn.tx.clone());
        let got = back.rr3_recv_buf().expect("读帧应成功");
        // 头里的 CRC 由载荷算出（线上真值），不是发送前的 0
        let mut want = f.clone();
        want.header.payload_crc32 = crate::protocol::header::rr3_crc32(&f.payload);
        assert_ne!(want.header.payload_crc32, 0, "带载荷的头 CRC 必须非零");
        assert_eq!(crate::protocol::wire::Rr3Frame::try_from(got).unwrap(), want);
    }

    /// 未 seal 的缓冲照走"先幂等 seal 再发"路径——rr3_seal 语义不随换底漂移。
    #[test]
    fn fake_stream_seals_unsealed_buf_before_sending() {
        let mut buf = Rr3FrameBuf::rr3_new(hdr(3));
        buf.rr3_payload_mut().copy_from_slice(b"abc");
        assert!(!buf.rr3_sealed());

        let mut conn = FakeStream::new(Vec::new());
        conn.rr3_send_buf(buf).expect("发送应成功");
        let want = {
            let f = crate::protocol::wire::Rr3Frame {
                header: hdr(3),
                payload: b"abc"[..].to_vec(),
            };
            crate::protocol::wire::rr3_frame_to_bytes(&f).unwrap()
        };
        assert_eq!(conn.tx, want, "未 seal 时先幂等 seal 再发送");
    }

    /// 短写（板侧写出失败的最常见形状）→ Io 点名要/实出，且已写出的部分如实落流
    /// （与旧 write_all 部分写语义一致：缝上错误折进计数）。
    #[test]
    fn fake_stream_short_write_is_named() {
        let f = crate::protocol::wire::Rr3Frame {
            header: hdr(8),
            payload: b"12345678"[..].to_vec(),
        };
        let mut conn = FakeStream::new(Vec::new());
        conn.fail_write_after = Some(10); // 128 + 8 = 136 字节只出 10 个
        let err = conn
            .rr3_send_buf(Rr3FrameBuf::from(&f))
            .expect_err("短写必须报错");
        assert!(
            matches!(err, Rr3FrameError::Io(ref m) if m.contains("要 136 字节实出 10 字节")),
            "错误必须点名要/实出: {err:?}"
        );
        assert_eq!(conn.tx.len(), 10, "已写出的部分如实落流");
    }

    /// 对端关闭（读侧恒 0）→ Closed，与帧层错误语义同源。
    #[test]
    fn fake_stream_eof_is_closed() {
        let mut conn = FakeStream::new(Vec::new());
        assert_eq!(conn.rr3_recv_buf(), Err(Rr3FrameError::Closed));
    }

    /// 半帧头 → TornHeader{got}（半帧不得当新帧起始——板侧拒收路径与 uds 同源）。
    #[test]
    fn fake_stream_torn_header_is_rejected() {
        let mut torn = hdr(9).to_bytes().to_vec();
        torn.truncate(64);
        let mut conn = FakeStream::new(torn);
        assert_eq!(
            conn.rr3_recv_buf(),
            Err(Rr3FrameError::TornHeader { got: 64 })
        );
    }
}
