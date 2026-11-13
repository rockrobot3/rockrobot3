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
        let Err(e) = std::fs::remove_file(path) else {
            return Ok(());
        };
        if e.kind() == io::ErrorKind::NotFound {
            return Ok(());
        }
        Err(e)
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
    /// 独占 bind（0.0.8d 审计 B-9）：先探测路径是否被**存活**实例占用——connect
    /// 成功 = 有活的守护在听，返回 AddrInUse 拒绝启动（双实例夺活实例端点的裂脑
    /// 就此禁绝：旧实例的 supervisor/在册表照跑，新实例却抢走模块连接面，两套
    /// 状态各说各话）；connect 失败 = 残留死文件，清掉再 bind（原无条件 cleanup
    /// 语义并入）。两个实例恰在"探测-清-bind"窗口内同时启动的理论竞态仍有残余
    /// （UDS 无抽象命名空间），由部署面单实例约束（systemd/装机脚本）兜底。
    pub fn rr3_bind_exclusive(path: &Path) -> io::Result<Self> {
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "探测到存活实例占用该路径——禁止双实例启动",
            ));
        }
        Self::rr3_cleanup(path)?;
        Self::rr3_bind(path)
    }

    /// 客户端连接入口（M3 起的模块、`tools/sim` 的假设备都走这里）。
    pub fn rr3_connect(path: &Path) -> io::Result<UnixStream> {
        UnixStream::connect(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::header::{opcode, rr3_crc32, Rr3Header};
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
        // send_buf 幂等 seal 时把 CRC 算进头：期望头必须带 CRC（不是 0）
        let mut want = hdr(5);
        want.payload_crc32 = rr3_crc32(b"hello");
        assert_ne!(want.payload_crc32, 0, "带载荷的头 CRC 必须非零");
        assert_eq!(*got.rr3_header(), want);
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
        let mut f = Rr3Frame {
            header: hdr(3),
            payload: b"abc"[..].to_vec(),
        };
        f.header.payload_crc32 = rr3_crc32(&f.payload);
        client.rr3_send_buf(Rr3FrameBuf::from(&f)).unwrap();
        let back = server.join().unwrap();
        assert_eq!(back.header.payload_crc32, rr3_crc32(b"abc"));
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

    /// B-9：存活实例在听 = 独占 bind 拒且不得动活的端点文件，活实例照常服务。
    /// 先消费掉 `rr3_bind_exclusive` 探测时排进 accept 队列的那条连接，再连新客户端
    /// ——若把 listener 移进线程只 accept 一次，闭包结束即 drop listener，第 209 行
    /// 的新连接会与"监听端已关闭"竞态（macOS 上实锤 ConnectionRefused 偶发）。
    #[test]
    fn bind_exclusive_refuses_live_listener() {
        let p = tmp_sock("excl-live");
        Rr3UdsListener::rr3_cleanup(&p).unwrap();
        let live = Rr3UdsListener::rr3_bind(&p).expect("先起一个活实例");
        let err = match Rr3UdsListener::rr3_bind_exclusive(&p) {
            Err(e) => e,
            Ok(_) => panic!("存活实例在听时必须拒绝独占 bind"),
        };
        assert_eq!(err.kind(), io::ErrorKind::AddrInUse, "存活实例必须拒");
        assert!(p.exists(), "拒绝不得删活实例的端点文件");
        let probe = live.rr3_accept().expect("活实例 accept 应成功");
        let _c = Rr3UdsListener::rr3_connect(&p).expect("活实例仍可连");
        drop(probe);
        drop(live);
        let _ = std::fs::remove_file(&p);
    }

    /// B-9：残留死 socket（实例已亡、文件还在）= 清掉后正常 bind（接续原 cleanup 语义）
    #[test]
    fn bind_exclusive_recovers_stale_socket() {
        let p = tmp_sock("excl-stale");
        {
            let _s = Rr3UdsListener::rr3_bind(&p).expect("先绑一次");
        }
        assert!(p.exists(), "drop 后 socket 文件残留（模拟崩溃现场）");
        let live = Rr3UdsListener::rr3_bind_exclusive(&p).expect("残留须被清掉并成功 bind");
        drop(live);
        let _ = std::fs::remove_file(&p);
    }
}
