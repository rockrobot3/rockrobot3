//! 帧收发：128B 定长头 + `payload_len` 字节 payload（事实源 `contracts/envelope.schema.json`）。
//!
//! 只做"读满一帧 / 整帧一次提交"，不含任何 socket 语义，因此平台无关：
//! 单元测试用 `std::io::Cursor` 就能在本机（含 Windows）跑满。

use std::io::{Read, Write};

use crate::protocol::header::{Rr3Header, Rr3HdrError, RR3_IPC_HDR_SIZE};

/// 一帧 = 头 + payload
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3Frame {
    pub header: Rr3Header,
    pub payload: Vec<u8>,
}

/// 帧层错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3FrameError {
    /// 读到任何字节之前对端干净关闭
    Closed,
    /// 头只读了一半就 EOF（半帧不得当新帧起始）
    TornHeader { got: usize },
    /// payload 比头声明的短
    TornPayload { want: usize, got: usize },
    /// 头本身非法（magic / reserved / 超长）
    Header(Rr3HdrError),
    /// 写侧：头声明的 payload_len 与实际 payload 长度不符
    LengthMismatch { declares: u32, actual: usize },
    /// 其它 IO 错误（文案只带 os 原因，不带对端数据）
    Io(String),
}

impl From<Rr3HdrError> for Rr3FrameError {
    fn from(e: Rr3HdrError) -> Self {
        Self::Header(e)
    }
}

/// 读满 `buf`。返回 `Ok(Some(已读字节数))`；`Ok(None)` = 第一个字节都没读到（对端关闭）。
/// 已读字节数必须由计数给出：payload 本身可以含任意 0 字节。
fn read_fill(r: &mut impl Read, buf: &mut [u8]) -> Result<Option<usize>, Rr3FrameError> {
    let mut done = 0usize;
    while done < buf.len() {
        match r.read(&mut buf[done..]) {
            Ok(0) => return Ok(if done == 0 { None } else { Some(done) }),
            Ok(n) => done += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Rr3FrameError::Io(e.to_string())),
        }
    }
    Ok(Some(done))
}

/// 读一帧（阻塞至读满，或 EOF/坏帧报错）
pub fn rr3_frame_read(mut r: impl Read) -> Result<Rr3Frame, Rr3FrameError> {
    let mut head = [0u8; RR3_IPC_HDR_SIZE];
    match read_fill(&mut r, &mut head)? {
        None => return Err(Rr3FrameError::Closed),
        Some(got) if got < head.len() => return Err(Rr3FrameError::TornHeader { got }),
        _ => {}
    }
    let header = Rr3Header::from_bytes(&head).map_err(Rr3FrameError::Header)?;
    let want = header.payload_len as usize;
    if want == 0 {
        return Ok(Rr3Frame { header, payload: Vec::new() });
    }
    let mut payload = vec![0u8; want];
    match read_fill(&mut r, &mut payload)? {
        Some(got) if got == want => Ok(Rr3Frame { header, payload }),
        Some(got) => Err(Rr3FrameError::TornPayload { want, got }),
        None => Err(Rr3FrameError::TornPayload { want, got: 0 }),
    }
}

/// 头 + payload 拼一整块：单次 write 提交，读侧不可能看到两半
pub fn rr3_frame_to_bytes(f: &Rr3Frame) -> Result<Vec<u8>, Rr3FrameError> {
    if f.header.payload_len as usize != f.payload.len() {
        return Err(Rr3FrameError::LengthMismatch {
            declares: f.header.payload_len,
            actual: f.payload.len(),
        });
    }
    let mut out = Vec::with_capacity(RR3_IPC_HDR_SIZE + f.payload.len());
    out.extend_from_slice(&f.header.to_bytes());
    out.extend_from_slice(&f.payload);
    Ok(out)
}

/// 写一帧
pub fn rr3_frame_write(w: &mut impl Write, f: &Rr3Frame) -> Result<(), Rr3FrameError> {
    let bytes = rr3_frame_to_bytes(f)?;
    w.write_all(&bytes).map_err(|e| Rr3FrameError::Io(e.to_string()))?;
    w.flush().map_err(|e| Rr3FrameError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::header::{
        opcode, LEN_DST_TASK, LEN_MAGIC, LEN_OPCODE, LEN_PAYLOAD_LEN, LEN_RESERVED, LEN_SRC_TASK,
        LEN_TIMESTAMP_NS, LEN_TRACE_ID, OFF_DST_TASK, OFF_MAGIC, OFF_OPCODE, OFF_PAYLOAD_LEN,
        OFF_RESERVED, OFF_SRC_TASK, OFF_TIMESTAMP_NS, OFF_TRACE_ID,
    };
    use std::io::Cursor;

    fn hdr(payload_len: u32) -> Rr3Header {
        let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload_len);
        h.trace_id = 0x1234_5678;
        h.timestamp_ns = 100;
        h.src_task = 7;
        h
    }

    fn golden() -> Vec<u8> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../contracts/golden/event-header-128.hex");
        let s = std::fs::read_to_string(path).expect("读不到 golden hex").trim().to_string();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("非法 hex"))
            .collect()
    }

    /// 往返：写多少字节，读回来就是同一帧
    #[test]
    fn frame_round_trip() {
        let f = Rr3Frame { header: hdr(5), payload: b"hello"[..].to_vec() };
        let mut buf = Vec::new();
        rr3_frame_write(&mut buf, &f).expect("写帧应成功");
        assert_eq!(buf.len(), RR3_IPC_HDR_SIZE + 5);
        assert_eq!(rr3_frame_read(Cursor::new(&buf)).expect("读帧应成功"), f);
    }

    /// 帧头部必须与 golden 向量逐字节一致（Task 12 的跨架构对比用的就是这个不变量）
    #[test]
    fn frame_header_is_byte_identical_to_golden() {
        let g = golden();
        let mut body = Vec::new();
        rr3_frame_write(
            &mut body,
            &Rr3Frame { header: Rr3Header::from_bytes(&g).unwrap(), payload: vec![0xAB; 96] },
        )
        .unwrap();
        assert_eq!(&body[..RR3_IPC_HDR_SIZE], &g[..], "帧头部与 golden 向量漂移");
        assert_eq!(body.len(), 128 + 96);
    }

    /// 跨架构等值的**发射端**（`tools/ci/xarch-verify.sh` 在 x86-64 与 aarch64 各跑一次比对）。
    /// 为什么不复用上面那条 golden 往返：往返测的是"编解码自洽"，两种架构可以各自自洽地
    /// 编出**不同**的字节；IPC-T-003 要的是"同一份构造输入 → 同一串线上字节"，所以判据必须
    /// 压在构造路径（`rr3_hdr_broadcast` + 显式字段赋值）上，而不是压在 golden 的读回上。
    /// 输出必须**纯确定性**：固定输入、固定字段值、无时间无随机，否则两侧比对没有意义。
    #[test]
    fn rr3_xarch_encode_emits_deterministic_header() {
        let bytes = hdr(5).to_bytes();
        assert_eq!(bytes.len(), RR3_IPC_HDR_SIZE, "发射的必须是完整 128B 头");
        // 偏移断言直接压在契约的 OFF_*/LEN_* 常量上（与 `x-offset-table` 同源，见本文件
        // tests 的 offset_table），所以这里**不是**在重复 header.rs 的实现，而是在重复契约。
        assert_eq!(
            &bytes[OFF_MAGIC..OFF_MAGIC + LEN_MAGIC],
            &[0x31, 0x45, 0x52, 0x41],
            "magic 0x41524531 必须小端落在线上"
        );
        assert_eq!(
            u16::from_le_bytes(bytes[OFF_OPCODE..OFF_OPCODE + LEN_OPCODE].try_into().unwrap()),
            opcode::SEND
        );
        assert_eq!(
            u64::from_le_bytes(bytes[OFF_TRACE_ID..OFF_TRACE_ID + LEN_TRACE_ID].try_into().unwrap()),
            0x1234_5678
        );
        assert_eq!(
            u64::from_le_bytes(
                bytes[OFF_TIMESTAMP_NS..OFF_TIMESTAMP_NS + LEN_TIMESTAMP_NS]
                    .try_into()
                    .unwrap()
            ),
            100
        );
        assert_eq!(
            u64::from_le_bytes(bytes[OFF_SRC_TASK..OFF_SRC_TASK + LEN_SRC_TASK].try_into().unwrap()),
            7
        );
        assert_eq!(
            u64::from_le_bytes(bytes[OFF_DST_TASK..OFF_DST_TASK + LEN_DST_TASK].try_into().unwrap()),
            0,
            "广播：dst_task=0"
        );
        assert_eq!(
            u32::from_le_bytes(
                bytes[OFF_PAYLOAD_LEN..OFF_PAYLOAD_LEN + LEN_PAYLOAD_LEN]
                    .try_into()
                    .unwrap()
            ),
            5
        );
        assert!(
            bytes[OFF_RESERVED..OFF_RESERVED + LEN_RESERVED].iter().all(|&b| b == 0),
            "reserved 必须全 0，否则两侧比的是垃圾位而不是布局"
        );
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex.len(),
            RR3_IPC_HDR_SIZE * 2,
            "发射行必须是 256 位小写 hex（脚本按此长度校验）"
        );
        println!("rr3-xarch-encode={hex}");
    }

    /// payload_len = 0 合法（无 payload 的心跳/应答形状）
    #[test]
    fn frame_empty_payload_is_allowed() {
        let mut buf = Vec::new();
        rr3_frame_write(&mut buf, &Rr3Frame { header: hdr(0), payload: vec![] }).unwrap();
        let back = rr3_frame_read(Cursor::new(&buf)).unwrap();
        assert_eq!(back.payload.len(), 0);
        assert_eq!(back.header.payload_len, 0);
    }

    /// 空输入 = 对端关闭
    #[test]
    fn frame_read_closed_connection() {
        let empty: [u8; 0] = [];
        assert_eq!(rr3_frame_read(Cursor::new(&empty[..])), Err(Rr3FrameError::Closed));
    }

    /// 半帧头（64B 后 EOF）不得被当成新帧起始
    #[test]
    fn frame_rejects_torn_header() {
        let mut buf = hdr(3).to_bytes().to_vec();
        buf.truncate(64);
        assert_eq!(rr3_frame_read(Cursor::new(&buf)), Err(Rr3FrameError::TornHeader { got: 64 }));
    }

    /// 头说 10 字节，payload 只到 3 字节（payload 含 0 也必须如实报 got=3）
    #[test]
    fn frame_rejects_torn_payload_with_zero_bytes_inside() {
        let mut buf = hdr(10).to_bytes().to_vec();
        buf.extend_from_slice(&[b'a', 0, 0]);
        assert_eq!(
            rr3_frame_read(Cursor::new(&buf)),
            Err(Rr3FrameError::TornPayload { want: 10, got: 3 }),
            "已读字节数必须由 read 计数给出，不能靠扫零推断"
        );
    }

    /// 坏 magic 把头层错误原样上抛
    #[test]
    fn frame_propagates_header_error() {
        let mut buf = hdr(0).to_bytes().to_vec();
        buf[0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        assert!(matches!(
            rr3_frame_read(Cursor::new(&buf)),
            Err(Rr3FrameError::Header(Rr3HdrError::BadMagic { .. }))
        ));
    }

    /// 写侧自校验：长度不符就一个字都不许发出去
    #[test]
    fn frame_write_rejects_length_mismatch() {
        let f = Rr3Frame { header: hdr(9), payload: b"123"[..].to_vec() };
        let mut buf = Vec::new();
        assert_eq!(
            rr3_frame_write(&mut buf, &f),
            Err(Rr3FrameError::LengthMismatch { declares: 9, actual: 3 })
        );
        assert!(buf.is_empty(), "校验失败时不得有半帧落进流里");
    }
}
