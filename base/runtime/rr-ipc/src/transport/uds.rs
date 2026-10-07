//! uds 传输：M2 唯一落地实现（ADR-0006 §5 / ADR-0008）。
//!
//! `Rr3UdsListener` 实现 [`Rr3TransportEndpoint`] + [`Rr3TransportListener`]，
//! `type Conn = UnixStream`——连接侧由 `transport/mod.rs` 对一切 `Read + Write + Send`
//! 的 blanket [`Rr3TransportConn`] 实现覆盖，不另写包装类型。rrd 服务侧因此可以拿到
//! `UnixStream` 具体类型继续走 socket 专有门（读超时 / shutdown），conn.rs 一行不动。
//!
//! cfg(unix)：uds 依赖 `std::os::unix::net`。Windows 侧本模块不参与编译——
//! 与 rrd 的 `conn` / `daemon` 同一条 cfg 边界（见 `docs/status.md` 能力表口径）。

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use crate::transport::{Rr3TransportEndpoint, Rr3TransportListener};

/// uds 监听端点。
pub struct Rr3UdsListener {
    inner: UnixListener,
}

impl Rr3TransportEndpoint for Rr3UdsListener {
    /// 删上一次运行残留的 socket 文件。不存在 = 已干净，`Ok(())`。
    fn rr3_cleanup(path: &Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// `UnixListener::bind`。调用前先 `rr3_cleanup`：uds 不允许 bind 到已存在的路径。
    fn rr3_bind(path: &Path) -> io::Result<Self> {
        Ok(Self {
            inner: UnixListener::bind(path)?,
        })
    }
}

impl Rr3TransportListener for Rr3UdsListener {
    type Conn = UnixStream;

    fn rr3_accept(&self) -> io::Result<UnixStream> {
        self.inner.accept().map(|(s, _)| s)
    }
}

impl Rr3UdsListener {
    /// 客户端连接入口（M3 起的模块、`tools/sim` 的假设备都走这里）。
    pub fn rr3_connect(path: &Path) -> io::Result<UnixStream> {
        UnixStream::connect(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::header::{opcode, Rr3Header};
    use crate::transport::buf::Rr3FrameBuf;
    use crate::transport::frame::Rr3Frame;
    use crate::transport::{Rr3TransportConn, Rr3TransportEndpoint, Rr3TransportListener};

    /// 唯一临时路径：进程号 + 自增计数，免引外部依赖
    fn tmp_sock(tag: &str) -> std::path::PathBuf {
        use std::format;
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "rr3-ipc-uds-{}-{}-{}.sock",
            tag,
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn hdr(payload_len: u32) -> Rr3Header {
        Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload_len)
    }

    /// cleanup 对不存在的路径不报错；对存在的 socket 文件删干净
    #[test]
    fn uds_cleanup_is_idempotent() {
        let p = tmp_sock("cleanup");
        Rr3UdsListener::rr3_cleanup(&p).expect("不存在也须 Ok");
        std::fs::write(&p, b"stale").expect("造残留文件");
        Rr3UdsListener::rr3_cleanup(&p).expect("残留须被清掉");
        assert!(!p.exists());
    }

    /// bind → accept → 客户端 send_buf / 服务端 recv_buf：整帧往返，所有权随值移交
    #[test]
    fn uds_round_trip_over_traits() {
        let p = tmp_sock("roundtrip");
        Rr3UdsListener::rr3_cleanup(&p).unwrap();
        let listener = Rr3UdsListener::rr3_bind(&p).expect("bind 应成功");
        let server = std::thread::spawn(move || {
            let mut conn = listener.rr3_accept().expect("accept 应成功");
            conn.rr3_recv_buf().expect("recv 应成功")
        });
        let mut client = Rr3UdsListener::rr3_connect(&p).expect("connect 应成功");
        let mut buf = Rr3FrameBuf::rr3_new(hdr(5));
        buf.rr3_payload_mut().copy_from_slice(b"hello");
        // 故意不 seal：send_buf 必须幂等补 seal，发出去的不是全 0 头
        client.rr3_send_buf(buf).expect("send 应成功");
        let got = server.join().expect("服务线程不该 panic");
        assert_eq!(got.rr3_payload(), b"hello");
        assert_eq!(*got.rr3_header(), hdr(5));
        let _ = std::fs::remove_file(&p);
    }

    /// 值视图（Rr3Frame）→ 缓冲 → uds → 读回：与既有帧层互操作
    #[test]
    fn uds_carries_value_frames_unchanged() {
        let p = tmp_sock("valueframe");
        Rr3UdsListener::rr3_cleanup(&p).unwrap();
        let listener = Rr3UdsListener::rr3_bind(&p).unwrap();
        let server = std::thread::spawn(move || {
            let mut conn = listener.rr3_accept().unwrap();
            Rr3Frame::try_from(conn.rr3_recv_buf().unwrap()).unwrap()
        });
        let mut client = Rr3UdsListener::rr3_connect(&p).unwrap();
        let f = Rr3Frame {
            header: hdr(3),
            payload: b"abc"[..].to_vec(),
        };
        client.rr3_send_buf(Rr3FrameBuf::from(&f)).unwrap();
        let back = server.join().unwrap();
        assert_eq!(back, f);
        let _ = std::fs::remove_file(&p);
    }

    /// 对端先写半帧头再关闭：recv 报 TornHeader，不得当新帧起始
    #[test]
    fn uds_recv_rejects_torn_header() {
        let p = tmp_sock("torn");
        Rr3UdsListener::rr3_cleanup(&p).unwrap();
        let listener = Rr3UdsListener::rr3_bind(&p).unwrap();
        let server = std::thread::spawn(move || {
            let mut conn = listener.rr3_accept().unwrap();
            conn.rr3_recv_buf()
        });
        let mut client = Rr3UdsListener::rr3_connect(&p).unwrap();
        use std::io::Write;
        let mut torn = hdr(3).to_bytes().to_vec();
        torn.truncate(64);
        client.write_all(&torn).unwrap();
        drop(client);
        let err = server.join().unwrap().unwrap_err();
        assert!(matches!(
            err,
            crate::transport::frame::Rr3FrameError::TornHeader { got: 64 }
        ));
        let _ = std::fs::remove_file(&p);
    }

    /// 对端干净关闭：recv 报 Closed
    #[test]
    fn uds_recv_reports_closed() {
        let p = tmp_sock("closed");
        Rr3UdsListener::rr3_cleanup(&p).unwrap();
        let listener = Rr3UdsListener::rr3_bind(&p).unwrap();
        let server = std::thread::spawn(move || {
            let mut conn = listener.rr3_accept().unwrap();
            conn.rr3_recv_buf()
        });
        let client = Rr3UdsListener::rr3_connect(&p).unwrap();
        drop(client);
        let err = server.join().unwrap().unwrap_err();
        assert_eq!(err, crate::transport::frame::Rr3FrameError::Closed);
        let _ = std::fs::remove_file(&p);
    }
}
