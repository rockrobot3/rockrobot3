//! 所有权移交的整帧缓冲（ADR-0007 §裁定 3 的零拷贝形状）。
//!
//! 单块连续内存：`[0..RR3_IPC_HDR_SIZE)` 为头预留区（seal 前保持全 0），其余为 payload。
//! 用法：`rr3_new` 建缓冲 → `rr3_payload_mut` 直接往里填 payload（不再另配 payload Vec 再拷贝）
//! → `rr3_seal` 原地编头 → 交 [`Rr3TransportConn::rr3_send_buf`](crate::transport::Rr3TransportConn)
//! 消费整个缓冲、单次 write 提交。recv 侧由 [`rr3_framebuf_read`] 产出同一形状。
//! uds 实现整块读写；M5 的 shm 在**同一签名**下把"拷贝"降为环形缓冲 grant，不改接口。
//!
//! [`Rr3Frame`](crate::protocol::wire::Rr3Frame) 保留为纯值视图（测试与既有代码不动）；
//! 两个方向都提供与它的低成本互转。本文件批J G1 起为 **no_std 可达**（core+alloc）：
//! 计数读走窄缝的 [`rr3_stream_read_fill`](crate::transport::bytestream)，不再碰
//! std::io——裸金属目标上 conn blanket impl 的载体由此编译可得。

// no_std 显式化（0.0.7 批B 口径）：alloc 载体显式引入，std 目标上与 std::vec 同一实现
use alloc::vec;
use alloc::vec::Vec;

use crate::protocol::header::{Rr3Header, RR3_IPC_HDR_SIZE};
use crate::protocol::wire::{Rr3Frame, Rr3FrameError};
use crate::transport::bytestream::{rr3_stream_read_fill, Rr3ByteStream};

/// 整帧缓冲：`bytes` 的长度恒为 `RR3_IPC_HDR_SIZE + payload_len`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3FrameBuf {
    hdr: Rr3Header,
    bytes: Vec<u8>,
    sealed: bool,
}

impl Rr3FrameBuf {
    /// 由头建缓冲：按 `header.payload_len` 一次性分配 `128 + payload_len` 字节并清零。
    /// payload 尚未填写，缓冲未 seal。
    pub fn rr3_new(header: Rr3Header) -> Self {
        let len = RR3_IPC_HDR_SIZE + header.payload_len as usize;
        Self {
            hdr: header,
            bytes: vec![0u8; len],
            sealed: false,
        }
    }

    /// 头字段（seal 时据此原地编头）
    pub fn rr3_header(&self) -> &Rr3Header {
        &self.hdr
    }

    /// payload 区（seal 前后都可见）
    pub fn rr3_payload(&self) -> &[u8] {
        &self.bytes[RR3_IPC_HDR_SIZE..]
    }

    /// payload 区可写视图：构造后、seal 前把体直接填进这块内存
    pub fn rr3_payload_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[RR3_IPC_HDR_SIZE..]
    }

    /// 是否已 seal
    pub fn rr3_sealed(&self) -> bool {
        self.sealed
    }

    /// 原地编头：把 `hdr.to_bytes()` 写进 `[0..128)` 预留区。幂等——重复 seal 落同一串字节。
    pub fn rr3_seal(&mut self) {
        let head = self.hdr.to_bytes();
        self.bytes[..RR3_IPC_HDR_SIZE].copy_from_slice(&head);
        self.sealed = true;
    }

    /// 整帧字节视图。发送路径要求已 seal：未 seal 时头区还是全 0，发出去就是对端眼里的坏 magic。
    pub fn rr3_as_sealed_bytes(&self) -> Result<&[u8], Rr3FrameError> {
        if self.sealed {
            Ok(&self.bytes)
        } else {
            Err(Rr3FrameError::Unsealed)
        }
    }

    /// 拆成值视图（`Rr3Frame`）：不分配，payload 直接搬走
    pub fn rr3_into_frame(mut self) -> Rr3Frame {
        let payload = self.bytes.split_off(RR3_IPC_HDR_SIZE);
        Rr3Frame {
            header: self.hdr,
            payload,
        }
    }
}

impl From<&Rr3Frame> for Rr3FrameBuf {
    /// 值视图 → 缓冲：单块分配，seal 后即可发送
    fn from(f: &Rr3Frame) -> Self {
        let mut buf = Self::rr3_new(f.header);
        buf.rr3_payload_mut().copy_from_slice(&f.payload);
        buf.rr3_seal();
        buf
    }
}

impl TryFrom<Rr3FrameBuf> for Rr3Frame {
    type Error = Rr3FrameError;

    /// 缓冲 → 值视图。未 seal 时头区不可信，拒转。
    fn try_from(mut buf: Rr3FrameBuf) -> Result<Self, Self::Error> {
        if !buf.sealed {
            return Err(Rr3FrameError::Unsealed);
        }
        let header =
            Rr3Header::from_bytes(&buf.bytes[..RR3_IPC_HDR_SIZE]).map_err(Rr3FrameError::Header)?;
        // 头以 buf.hdr 为准编出，此处解析回读是防御：两者不一致说明 seal 后缓冲头区被外力改写。
        debug_assert_eq!(header, buf.hdr);
        buf.bytes.drain(..RR3_IPC_HDR_SIZE);
        Ok(Rr3Frame {
            header,
            payload: buf.bytes,
        })
    }
}

/// 读一帧进单块缓冲（阻塞至读满，或 EOF/坏帧报错）。计数走窄缝
/// [`Rr3ByteStream`](crate::transport::bytestream::Rr3ByteStream)（std 类型经
/// 单向 adapter 进缝，Cursor 等测试载体照旧）。错误语义与
/// [`rr3_frame_read`](crate::transport::frame::rr3_frame_read) 完全一致：
/// `Closed` / `TornHeader` / `TornPayload` / `Header(..)`。
pub fn rr3_framebuf_read<S: Rr3ByteStream>(s: &mut S) -> Result<Rr3FrameBuf, Rr3FrameError> {
    let mut bytes = vec![0u8; RR3_IPC_HDR_SIZE];
    match rr3_stream_read_fill(s, &mut bytes)? {
        None => return Err(Rr3FrameError::Closed),
        Some(got) if got < RR3_IPC_HDR_SIZE => {
            return Err(Rr3FrameError::TornHeader { got });
        }
        _ => {}
    }
    let hdr = Rr3Header::from_bytes(&bytes).map_err(Rr3FrameError::Header)?;
    let want = hdr.payload_len as usize;
    if want > 0 {
        bytes.resize(RR3_IPC_HDR_SIZE + want, 0);
        match rr3_stream_read_fill(s, &mut bytes[RR3_IPC_HDR_SIZE..])? {
            Some(got) if got == want => {}
            Some(got) => return Err(Rr3FrameError::TornPayload { want, got }),
            None => return Err(Rr3FrameError::TornPayload { want, got: 0 }),
        }
    }
    Ok(Rr3FrameBuf {
        hdr,
        bytes,
        sealed: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn hdr(payload_len: u32) -> Rr3Header {
        let mut h =
            Rr3Header::rr3_hdr_broadcast(crate::protocol::header::opcode::SEND, payload_len);
        h.trace_id = 0x1234_5678;
        h
    }

    /// seal 前头区全 0、payload 可写；seal 后头区与 `to_bytes` 逐字节一致（原地编头）
    #[test]
    fn buf_seal_writes_header_in_place() {
        let mut buf = Rr3FrameBuf::rr3_new(hdr(3));
        assert_eq!(buf.bytes.len(), RR3_IPC_HDR_SIZE + 3);
        assert!(!buf.rr3_sealed());
        assert!(
            buf.rr3_as_sealed_bytes().is_err(),
            "未 seal 不得给出可发字节"
        );
        buf.rr3_payload_mut().copy_from_slice(b"abc");
        buf.rr3_seal();
        assert!(buf.rr3_sealed());
        let bytes = buf.rr3_as_sealed_bytes().unwrap().to_vec();
        assert_eq!(&bytes[..RR3_IPC_HDR_SIZE], &hdr(3).to_bytes()[..]);
        assert_eq!(&bytes[RR3_IPC_HDR_SIZE..], b"abc");
        // seal 幂等
        buf.rr3_seal();
        assert_eq!(buf.rr3_as_sealed_bytes().unwrap(), &bytes[..]);
    }

    /// 值视图 ↔ 缓冲 双向往返不丢字段
    #[test]
    fn buf_frame_round_trip() {
        let f = Rr3Frame {
            header: hdr(5),
            payload: b"hello"[..].to_vec(),
        };
        let buf = Rr3FrameBuf::from(&f);
        assert!(buf.rr3_sealed());
        let back = Rr3Frame::try_from(buf).unwrap();
        assert_eq!(back, f);
    }

    /// 未 seal 的缓冲拒转值视图
    #[test]
    fn buf_unsealed_rejects_frame_conversion() {
        let buf = Rr3FrameBuf::rr3_new(hdr(0));
        assert_eq!(Rr3Frame::try_from(buf), Err(Rr3FrameError::Unsealed));
    }

    /// 读侧：seal 过的缓冲整块发出，读回同一帧（单块提交，读侧不可能看到两半）
    #[test]
    fn framebuf_read_round_trip() {
        let mut buf = Rr3FrameBuf::rr3_new(hdr(4));
        buf.rr3_payload_mut().copy_from_slice(b"wxyz");
        buf.rr3_seal();
        let sent = buf.rr3_as_sealed_bytes().unwrap().to_vec();
        // Cursor<Vec<u8>>（Read+Write 双全，经 adapter 进缝；Cursor<&[u8]> 无 Write）
        let back = rr3_framebuf_read(&mut Cursor::new(sent.clone())).unwrap();
        assert!(back.rr3_sealed());
        assert_eq!(back.rr3_payload(), b"wxyz");
        assert_eq!(*back.rr3_header(), hdr(4));
    }

    /// 读侧错误语义与帧层一致：干净关闭 / 半帧头 / 短 payload
    #[test]
    fn framebuf_read_error_semantics() {
        assert_eq!(
            rr3_framebuf_read(&mut Cursor::new(Vec::new())),
            Err(Rr3FrameError::Closed)
        );

        let mut torn = hdr(3).to_bytes().to_vec();
        torn.truncate(64);
        assert_eq!(
            rr3_framebuf_read(&mut Cursor::new(torn.clone())),
            Err(Rr3FrameError::TornHeader { got: 64 })
        );

        let mut short = hdr(10).to_bytes().to_vec();
        short.extend_from_slice(b"abc");
        assert_eq!(
            rr3_framebuf_read(&mut Cursor::new(short.clone())),
            Err(Rr3FrameError::TornPayload { want: 10, got: 3 })
        );
    }

    /// payload_len = 0 合法（心跳/应答形状），读回空 payload
    #[test]
    fn framebuf_read_empty_payload() {
        let mut buf = Rr3FrameBuf::rr3_new(hdr(0));
        buf.rr3_seal();
        let sent = buf.rr3_as_sealed_bytes().unwrap().to_vec();
        let back = rr3_framebuf_read(&mut Cursor::new(sent.clone())).unwrap();
        assert!(back.rr3_payload().is_empty());
    }
}
