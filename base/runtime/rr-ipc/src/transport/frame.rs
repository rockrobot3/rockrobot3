//! 帧收发：128B 定长头 + `payload_len` 字节 payload（事实源 `contracts/envelope.schema.json`）。
//!
//! 只做"读满一帧 / 整帧一次提交"，不含任何 socket 语义，因此平台无关：
//! 单元测试用 `std::io::Cursor` 就能在本机（含 Windows）跑满。
//!
//! 帧的**数据形状**与整帧字节化（`Rr3Frame` / `Rr3FrameError` / `rr3_frame_to_bytes`）
//! 批D₁ 起落 `protocol::wire`（no_std 可达，机制核切片共用）；本模块 re-export
//! 保持 `crate::transport::frame::…` 与 lib 顶层两条既有路径逐字不变。

use std::io::{Read, Write};
// no_std 显式化（0.0.7 批B）：std prelude 不再随 `#![no_std]` 隐式注入
use std::string::ToString;
use std::vec;
use std::vec::Vec;

use crate::protocol::header::{Rr3HdrError, Rr3Header, MAX_PAYLOAD_LEN, RR3_IPC_HDR_SIZE};
pub use crate::protocol::wire::{rr3_frame_to_bytes, Rr3Frame, Rr3FrameError};

/// 读满 `buf`。返回 `Ok(Some(已读字节数))`；`Ok(None)` = 第一个字节都没读到（对端关闭）。
/// 已读字节数必须由计数给出：payload 本身可以含任意 0 字节。
/// crate 内可见：`buf.rs` 的 `rr3_framebuf_read` 复用同一份计数语义，不另写第二份。
pub(crate) fn read_fill(r: &mut impl Read, buf: &mut [u8]) -> Result<Option<usize>, Rr3FrameError> {
    let mut done = 0usize;
    while done < buf.len() {
        let n = match r.read(&mut buf[done..]) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Rr3FrameError::Io(e.to_string())),
        };
        if n == 0 {
            return Ok(Some(done).filter(|d| *d > 0));
        }
        done += n;
    }
    Ok(Some(done))
}

/// 读一帧（阻塞至读满，或 EOF/坏帧报错）。分配上界＝协议规格 [`MAX_PAYLOAD_LEN`]。
/// 面向**不可信流**的读侧请走 [`rr3_frame_read_cap`] 带策略帽（0.0.8b 审计 B-7）。
pub fn rr3_frame_read(r: impl Read) -> Result<Rr3Frame, Rr3FrameError> {
    rr3_frame_read_cap(r, MAX_PAYLOAD_LEN as usize)
}

/// 带策略帽的读帧：`payload_len > cap` 在**分配之前**拒收（复用
/// `PayloadTooLarge` 错误形——语义同为"plen 超出可接受上界"）。
/// 动机：坏 plen（撕裂帧/模块内存 bug 写错长度）会让读侧按声明值直接
/// `vec![0u8; want]`，协议规格 64MB 上界对守护进程仍是单连接可打穿的
/// 分配面；消费方按自身最大合法帧给策略帽（总线/面板 = 1MB 档）。
pub fn rr3_frame_read_cap(mut r: impl Read, cap: usize) -> Result<Rr3Frame, Rr3FrameError> {
    let header = rr3_head_read(&mut r)?;
    let want = header.payload_len as usize;
    if want > cap {
        return Err(Rr3FrameError::Header(Rr3HdrError::PayloadTooLarge {
            len: header.payload_len,
        }));
    }
    let payload = rr3_payload_read(&mut r, want)?;
    Ok(Rr3Frame { header, payload })
}

/// 读满 128B 定长头（空读 = 对端关闭；半头 = TornHeader）。
fn rr3_head_read(r: &mut impl Read) -> Result<Rr3Header, Rr3FrameError> {
    let mut head = [0u8; RR3_IPC_HDR_SIZE];
    let got = read_fill(r, &mut head)?.ok_or(Rr3FrameError::Closed)?;
    if got < head.len() {
        return Err(Rr3FrameError::TornHeader { got });
    }
    Rr3Header::from_bytes(&head).map_err(Rr3FrameError::Header)
}

/// 读满 payload（`want=0` 不分配；半 payload 报 TornPayload，已读计数如实）。
fn rr3_payload_read(r: &mut impl Read, want: usize) -> Result<Vec<u8>, Rr3FrameError> {
    if want == 0 {
        return Ok(Vec::new());
    }
    let mut payload = vec![0u8; want];
    let got = read_fill(r, &mut payload)?.ok_or(Rr3FrameError::TornPayload { want, got: 0 })?;
    if got != want {
        return Err(Rr3FrameError::TornPayload { want, got });
    }
    Ok(payload)
}

/// 写一帧
pub fn rr3_frame_write(w: &mut impl Write, f: &Rr3Frame) -> Result<(), Rr3FrameError> {
    let bytes = rr3_frame_to_bytes(f)?;
    w.write_all(&bytes)
        .map_err(|e| Rr3FrameError::Io(e.to_string()))?;
    w.flush().map_err(|e| Rr3FrameError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    // no_std 显式化（0.0.7 批B）：std prelude 不再随 `#![no_std]` 隐式注入
    use crate::protocol::header::{
        opcode, rr3_crc32, Rr3HdrError, LEN_DST_TASK, LEN_MAGIC, LEN_OPCODE, LEN_PAYLOAD_LEN,
        LEN_RESERVED, LEN_SRC_TASK, LEN_TIMESTAMP_NS, LEN_TRACE_ID, OFF_CRC32, OFF_DST_TASK,
        OFF_MAGIC, OFF_OPCODE, OFF_PAYLOAD_LEN, OFF_RESERVED, OFF_SRC_TASK, OFF_TIMESTAMP_NS,
        OFF_TRACE_ID,
    };
    use crate::protocol::wire::rr3_golden_pure;
    use std::format;
    use std::io::Cursor;
    use std::println;
    use std::string::String;

    /// B-7（0.0.8b 审计）：策略帽在**分配之前**拒超限 plen——合法头、长度过帽，
    /// 错误形复用 PayloadTooLarge；帽内同帧照常通过。
    #[test]
    fn read_cap_rejects_plen_over_cap_before_alloc() {
        let f = Rr3Frame {
            header: hdr(1024),
            payload: vec![7u8; 1024],
        };
        let buf = rr3_frame_to_bytes(&f).unwrap();
        assert!(matches!(
            rr3_frame_read_cap(Cursor::new(&buf), 512),
            Err(Rr3FrameError::Header(Rr3HdrError::PayloadTooLarge {
                len: 1024
            }))
        ));
        assert!(rr3_frame_read_cap(Cursor::new(&buf), 1024).is_ok());
        assert!(rr3_frame_read(Cursor::new(&buf)).is_ok());
    }

    fn hdr(payload_len: u32) -> Rr3Header {
        let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload_len);
        h.trace_id = 0x1234_5678;
        h.timestamp_ns = 100;
        h.src_task = 7;
        h
    }

    fn golden() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../contracts/golden/event-header-128.hex"
        );
        let s = std::fs::read_to_string(path)
            .expect("读不到 golden hex")
            .trim()
            .to_string();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("非法 hex"))
            .collect()
    }

    /// 往返：写多少字节，读回来就是同一帧
    #[test]
    fn frame_round_trip() {
        let f = Rr3Frame {
            header: hdr(5),
            payload: b"hello"[..].to_vec(),
        };
        let mut buf = Vec::new();
        rr3_frame_write(&mut buf, &f).expect("写帧应成功");
        assert_eq!(buf.len(), RR3_IPC_HDR_SIZE + 5);
        // 写侧把 CRC 算进头，读回的头必须带上它（不是发送前的 0）
        let mut want = f.clone();
        want.header.payload_crc32 = rr3_crc32(&f.payload);
        assert_ne!(want.header.payload_crc32, 0, "带载荷的头 CRC 必须非零");
        assert_eq!(rr3_frame_read(Cursor::new(&buf)).expect("读帧应成功"), want);
    }

    /// 帧头部必须与 golden 向量逐字节一致（Task 12 的跨架构对比用的就是这个不变量）
    #[test]
    fn frame_header_is_byte_identical_to_golden() {
        let g = golden();
        let pure = rr3_golden_pure(&g);

        // 带载荷的真帧：头里 CRC 由载荷算出（非零）且接收侧校验必过；
        // 除 CRC 字段外，其余头字节仍与 golden 逐字节一致
        let payload = vec![0xAB; 96];
        let mut body = Vec::new();
        rr3_frame_write(
            &mut body,
            &Rr3Frame {
                header: pure,
                payload: payload.clone(),
            },
        )
        .unwrap();
        assert_eq!(body.len(), 128 + 96);
        let head = Rr3Header::from_bytes(&body[..RR3_IPC_HDR_SIZE]).unwrap();
        assert_ne!(head.payload_crc32, 0, "带载荷的真帧头 CRC 必须非零");
        assert_eq!(head.payload_crc32, rr3_crc32(&payload));
        assert_eq!(
            crate::transport::buf::rr3_verify_crc(&head, &payload),
            Ok(()),
            "带载荷的真帧头必须能通过接收侧 CRC 校验"
        );
        assert_eq!(
            &body[..OFF_CRC32],
            &g[..OFF_CRC32],
            "CRC 之前的头字段与 golden 漂移"
        );
        assert_eq!(
            &body[OFF_RESERVED..RR3_IPC_HDR_SIZE],
            &g[OFF_RESERVED..],
            "reserved 区与 golden 漂移"
        );
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
            u16::from_le_bytes(
                bytes[OFF_OPCODE..OFF_OPCODE + LEN_OPCODE]
                    .try_into()
                    .unwrap()
            ),
            opcode::SEND
        );
        assert_eq!(
            u64::from_le_bytes(
                bytes[OFF_TRACE_ID..OFF_TRACE_ID + LEN_TRACE_ID]
                    .try_into()
                    .unwrap()
            ),
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
            u64::from_le_bytes(
                bytes[OFF_SRC_TASK..OFF_SRC_TASK + LEN_SRC_TASK]
                    .try_into()
                    .unwrap()
            ),
            7
        );
        assert_eq!(
            u64::from_le_bytes(
                bytes[OFF_DST_TASK..OFF_DST_TASK + LEN_DST_TASK]
                    .try_into()
                    .unwrap()
            ),
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
            bytes[OFF_RESERVED..OFF_RESERVED + LEN_RESERVED]
                .iter()
                .all(|&b| b == 0),
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
        rr3_frame_write(
            &mut buf,
            &Rr3Frame {
                header: hdr(0),
                payload: vec![],
            },
        )
        .unwrap();
        let back = rr3_frame_read(Cursor::new(&buf)).unwrap();
        assert_eq!(back.payload.len(), 0);
        assert_eq!(back.header.payload_len, 0);
    }

    /// 空输入 = 对端关闭
    #[test]
    fn frame_read_closed_connection() {
        let empty: [u8; 0] = [];
        assert_eq!(
            rr3_frame_read(Cursor::new(&empty[..])),
            Err(Rr3FrameError::Closed)
        );
    }

    /// 半帧头（64B 后 EOF）不得被当成新帧起始
    #[test]
    fn frame_rejects_torn_header() {
        let mut buf = hdr(3).to_bytes().to_vec();
        buf.truncate(64);
        assert_eq!(
            rr3_frame_read(Cursor::new(&buf)),
            Err(Rr3FrameError::TornHeader { got: 64 })
        );
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
        let f = Rr3Frame {
            header: hdr(9),
            payload: b"123"[..].to_vec(),
        };
        let mut buf = Vec::new();
        assert_eq!(
            rr3_frame_write(&mut buf, &f),
            Err(Rr3FrameError::LengthMismatch {
                declares: 9,
                actual: 3
            })
        );
        assert!(buf.is_empty(), "校验失败时不得有半帧落进流里");
    }
}
