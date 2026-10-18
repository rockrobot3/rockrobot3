//! 拆帧器（批D④ 判据② 收流线程化的机制面上提）：大块裸字节 → 整帧，纯内存。
//!
//! 上提事实源 `tools/rr3-audio-rec/src/main.rs`（原收流端本地件，2026-10-18 A 批
//! 随 rr-hal esp32 音频源后端共用而单点化）：机制面只做"对齐 magic + 头层解析
//! 定长 + 按 plen 切整帧"，头校验全数交 [`Rr3Header::from_bytes`]（零第二份帧
//! 语义）；失步恢复 = 从坏帧起点弃 4B magic 续扫（原分相读接缝 resync 的内存版
//! ——字节已在缓冲，弃扫不再产生任何新 I/O，滑移由此不放大为风暴）。
//!
//! 纯 core+alloc（no_std 同律）：不触时钟、不触 I/O，喂进拉出全凭调用方节奏。

use alloc::string::String;
use alloc::vec::Vec;

use crate::protocol::header::{Rr3Header, MAGIC, RR3_IPC_HDR_SIZE};

/// 帧头 magic 小端线上字节（字段值 0x41524531 小端存储 = 31 45 52 41 = "1ERA"，
/// 自 [`MAGIC`] 派生单点）。b64 载荷里也会出现该子串，失步后撞上即"合法幻影
/// 帧头"，重同步扫描以它为锚。
pub const RR3_MAGIC_LE: [u8; 4] = MAGIC.to_le_bytes();

/// 拆帧 plen 护栏缺省值（收流端策略面缺省）：板面音频片 plen 实测 ~11KB，
/// rr-ipc 头层上限 64MiB——幻影 magic（b64 载荷撞出的合法 "1ERA"）后的垃圾
/// plen 若照单全收，等待/缓冲都天文。超护栏按坏帧弃 4B magic 续扫。语义上是
/// 策略值，[`FrameAssembler::with_plen_cap`] 可按消费者改档。
pub const RR3_FRAMING_PLEN_CAP_DEFAULT: usize = 1024 * 1024;

/// 拆帧出片：整帧 = 头（已解析）+ 载荷字节。
pub type Assembled = (Rr3Header, Vec<u8>);

/// 拆帧器：`feed` 推裸字节，`pull` 出整帧；坏帧 Err 起点已弃，续扫由下次
/// pull 自然接上。
pub struct FrameAssembler {
    buf: Vec<u8>,
    /// 距上一整帧的弃扫字节（诊断：重同步日志行）。
    discarded: u64,
    /// plen 护栏（构造期注入，缺省 [`RR3_FRAMING_PLEN_CAP_DEFAULT`]）。
    plen_cap: usize,
}

impl FrameAssembler {
    /// 缺省护栏（板面音频片 ~11KB 的千倍余量）。
    pub fn new() -> Self {
        Self::with_plen_cap(RR3_FRAMING_PLEN_CAP_DEFAULT)
    }

    /// 指定 plen 护栏（消费者策略档）。
    pub fn with_plen_cap(plen_cap: usize) -> Self {
        Self {
            buf: Vec::new(),
            discarded: 0,
            plen_cap,
        }
    }

    /// 读线程推来的裸字节进缓冲。
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// 距上一整帧的弃扫字节。
    pub fn discarded(&self) -> u64 {
        self.discarded
    }

    /// 尝试出下一整帧：Ok(Some) = 出帧（消费即移除）；Ok(None) = 不够一帧，
    /// 等更多字节；Err((弃字节数, 诊断)) = 坏帧（magic 在而头非法 / plen 越栏），
    /// 起点已弃，续扫由下次 pull 自然接上。
    pub fn pull(&mut self) -> Result<Option<Assembled>, (u64, String)> {
        // 对齐 magic：无全 magic 则只留尾 3B（可能跨块的前缀），其余静默弃
        // （起始垃圾/断帧残骸——帧间垃圾不计数，丢片由 seq 对账如实记账）。
        if let Some(pos) = find(&self.buf, &RR3_MAGIC_LE) {
            if pos > 0 {
                self.discarded += pos as u64;
                self.buf.drain(..pos);
            }
        } else {
            let keep_from = self.buf.len().saturating_sub(RR3_MAGIC_LE.len() - 1);
            self.discarded += keep_from as u64;
            self.buf.drain(..keep_from);
            return Ok(None);
        }
        if self.buf.len() < RR3_IPC_HDR_SIZE {
            return Ok(None);
        }
        let header = match Rr3Header::from_bytes(&self.buf[..RR3_IPC_HDR_SIZE]) {
            Ok(h) => h,
            Err(e) => return Err(self.drop_magic(4, alloc::format!("头层拒收: {e:?}"))),
        };
        let plen = header.payload_len as usize;
        if plen > self.plen_cap {
            return Err(self.drop_magic(
                4,
                alloc::format!("payload_len {plen} 越拆帧护栏 {}（幻影头）", self.plen_cap),
            ));
        }
        let want = RR3_IPC_HDR_SIZE + plen;
        if self.buf.len() < want {
            return Ok(None);
        }
        let payload = self.buf[RR3_IPC_HDR_SIZE..want].to_vec();
        self.buf.drain(..want);
        self.discarded = 0;
        Ok(Some((header, payload)))
    }

    /// 坏帧处置：弃 magic 起点（坏头只弃 4B——真帧起点可能就在其后不远处，
    /// 续扫自会找到），返回 (弃字节数, 诊断)。
    fn drop_magic(&mut self, n: usize, why: String) -> (u64, String) {
        self.buf.drain(..n);
        self.discarded += n as u64;
        (n as u64, why)
    }
}

/// 子串查找（拆帧器专用；core 无字节切片 `find`，不引依赖）。
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::body::rr3_body_encode;
    use crate::protocol::header::{opcode, Rr3PayloadType, MAX_PAYLOAD_LEN};

    /// 构造一条与板面 mic.rs 同形的 Event 帧线上字节（头 + 载荷，无 CRC 尾——
    /// 线格式本就无尾，FrameBuf 的 seal 是所有权语义非校验）。载荷体对拆帧器
    /// 不透明（topic/字段门是消费方面，不属本机制）。
    fn board_like_frame(seq: u32) -> Vec<u8> {
        let body = alloc::format!(
            "{{\"topic\":\"rr3-sense/audio/pcm\",\"seq\":{seq},\"format\":\"s16le\"}}"
        );
        let payload = rr3_body_encode(Rr3PayloadType::Event, body.as_bytes());
        let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
        hdr.trace_id = 0;
        hdr.timestamp_ns = 100;
        hdr.src_task = 2; // 板面外设执行面位形（诊断显示用；拆帧器不读）
        hdr.dst_task = 1; // 宿主位形
        let mut bytes = hdr.to_bytes().to_vec();
        bytes.extend_from_slice(&payload);
        bytes
    }

    /// 拆帧器：分块喂入（头中/载荷中切）→ 整帧逐字节还原。
    #[test]
    fn assembler_reassembles_split_frames() {
        let bytes = board_like_frame(7);
        let mut a = FrameAssembler::new();
        a.feed(&bytes[..64]);
        assert!(a.pull().unwrap().is_none(), "半个头不出帧");
        a.feed(&bytes[64..100]);
        assert!(a.pull().unwrap().is_none(), "载荷中段不出帧");
        a.feed(&bytes[100..]);
        let (hdr, payload) = a.pull().unwrap().expect("拼满必须出帧");
        assert_eq!(hdr.payload_len as usize, payload.len());
        assert_eq!(
            hdr.trace_id, 0,
            "头逐字段还原（trace_id 是构造时显式钉的值）"
        );
        assert!(a.pull().unwrap().is_none(), "缓冲耗尽");
    }

    /// 拆帧器：前导垃圾（含半个 magic 前缀）静默弃扫，真帧照常出。
    #[test]
    fn assembler_skips_garbage_then_yields_frame() {
        let bytes = board_like_frame(9);
        let mut a = FrameAssembler::new();
        let mut noisy = b"junk\x01\x02junk1ER".to_vec(); // 尾部半个 magic 前缀
        noisy.extend_from_slice(&bytes);
        a.feed(&noisy);
        let (hdr, payload) = a.pull().unwrap().expect("垃圾弃扫后必须出真帧");
        assert_eq!(hdr.dst_task, 1);
        assert!(!payload.is_empty());
        assert!(a.pull().unwrap().is_none());
    }

    /// 拆帧器：坏帧（reserved 污染——滑移实测签名）计错弃 4B magic，续扫
    /// 收敛到下一真帧。
    #[test]
    fn assembler_bad_header_drops_magic_and_resumes() {
        let mut bad = board_like_frame(1);
        bad[RR3_IPC_HDR_SIZE - 1] = 0xFF; // 头 reserved 尾字节污染
        let bytes2 = board_like_frame(2);
        let mut a = FrameAssembler::new();
        a.feed(&bad);
        a.feed(&bytes2);
        let (dropped, why) = a.pull().unwrap_err();
        assert_eq!(dropped, 4, "坏帧只弃 magic 起点");
        assert!(why.contains("ReservedNotZero"), "{why}");
        let (hdr, payload) = a.pull().unwrap().expect("坏帧后续扫必须出真帧");
        assert_eq!(hdr.trace_id, 0);
        assert!(!payload.is_empty());
    }

    /// 拆帧器：头层合法的 64MiB plen 越拆帧护栏——按坏帧弃 magic，不缓冲；
    /// 自定义护栏同径生效。
    #[test]
    fn assembler_rejects_plen_over_cap() {
        let hdr_bytes = {
            let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, MAX_PAYLOAD_LEN);
            h.trace_id = 0;
            h.timestamp_ns = 100;
            h.src_task = 2;
            h.dst_task = 1;
            h.to_bytes().to_vec()
        };
        let mut a = FrameAssembler::new();
        a.feed(&hdr_bytes);
        let (dropped, why) = a.pull().unwrap_err();
        assert_eq!(dropped, 4);
        assert!(why.contains("越拆帧护栏"), "{why}");

        // 消费者策略档：护栏抬高到 64MiB 即放行头层（合法 plen 只等载荷，不出帧）
        let mut wide = FrameAssembler::with_plen_cap(MAX_PAYLOAD_LEN as usize);
        wide.feed(&hdr_bytes);
        assert!(wide.pull().unwrap().is_none());
    }
}
