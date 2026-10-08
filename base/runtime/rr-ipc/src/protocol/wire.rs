//! 帧数据形状与整帧字节化（ADR-0018 机制核切片：批D₁ 下沉）。
//!
//! 从 `transport::frame` 平移而来的**纯数据与纯逻辑**部分——`Rr3Frame` / `Rr3FrameError`
//! 与 `rr3_frame_to_bytes` 不含任何 socket / io 语义，`no_std`（core+alloc）可达，
//! 供机制核切片（`rr-core` 总线语义层）在裸金属目标上组装帧使用。
//! "读满一帧 / 整帧一次提交"的 io 半边仍在 [`crate::transport::frame`]。
//!
//! 字节级事实源是 `contracts/envelope.schema.json`（歧义裁定见 `contracts/README.md`）。

// no_std 显式化（0.0.7 批B 同款）：std prelude 不随 `#![no_std]` 隐式注入
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::protocol::header::{Rr3HdrError, Rr3Header, RR3_IPC_HDR_SIZE};

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
    /// 缓冲未 seal 就要取整帧字节（头区还是全 0，发出去是对端眼里的坏 magic）
    Unsealed,
    /// 其它 IO 错误（文案只带 os 原因，不带对端数据）
    Io(String),
}

impl From<Rr3HdrError> for Rr3FrameError {
    fn from(e: Rr3HdrError) -> Self {
        Self::Header(e)
    }
}

/// 头 + payload 拼一整块：单次 write 提交，读侧不可能看到两半。
/// 纯函数：输入输出都是内存里的字节，不碰 io、不读时钟。
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::header::opcode;
    use alloc::vec;
    use alloc::vec::Vec;

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
        let s = std::fs::read_to_string(path).expect("读不到 golden hex");
        let s = s.trim();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("非法 hex"))
            .collect()
    }

    /// 帧头部必须与 golden 向量逐字节一致（跨架构对比发射端用的同一个不变量；
    /// 往返与 torn-io 语义仍由 `transport::frame` 的 io 半边用例覆盖）
    #[test]
    fn frame_header_is_byte_identical_to_golden() {
        let g = golden();
        let bytes = rr3_frame_to_bytes(&Rr3Frame {
            header: Rr3Header::from_bytes(&g).unwrap(),
            payload: vec![0xAB; 96],
        })
        .unwrap();
        assert_eq!(
            &bytes[..RR3_IPC_HDR_SIZE],
            &g[..],
            "帧头部与 golden 向量漂移"
        );
        assert_eq!(bytes.len(), 128 + 96);
    }

    /// 写侧自校验：长度不符就一个字都不许拼出去
    #[test]
    fn frame_to_bytes_rejects_length_mismatch() {
        let f = Rr3Frame {
            header: hdr(9),
            payload: b"123"[..].to_vec(),
        };
        assert_eq!(
            rr3_frame_to_bytes(&f),
            Err(Rr3FrameError::LengthMismatch {
                declares: 9,
                actual: 3
            })
        );
    }

    /// 字节化自洽：to_bytes 的产物按头字段解析回同一帧（无 io 参与）
    #[test]
    fn frame_to_bytes_round_trips_through_header_parse() {
        let f = Rr3Frame {
            header: hdr(5),
            payload: b"hello"[..].to_vec(),
        };
        let bytes = rr3_frame_to_bytes(&f).unwrap();
        assert_eq!(bytes.len(), RR3_IPC_HDR_SIZE + 5);
        let slice: &[u8] = &bytes[..RR3_IPC_HDR_SIZE];
        let head: [u8; RR3_IPC_HDR_SIZE] = slice.try_into().unwrap();
        let header = Rr3Header::from_bytes(&head).unwrap();
        assert_eq!(header, f.header);
        assert_eq!(&bytes[RR3_IPC_HDR_SIZE..], &f.payload[..]);
    }
}
