//! PCM 环形缓冲与 s16le 解码。

use std::collections::VecDeque;

use super::core::{RR3_ESP32_AUDIO_CHANNELS, RR3_ESP32_AUDIO_RATE};

/// 环形缓冲容量（采样点）：恰 ~1s @16k 双声道。溢出丢最旧**整帧**（保声道
/// 交织结构）+ 计数；~2% 板-宿主钟滑移下溢丢是稳态损耗（见模块文档）。
pub(super) const RR3_ESP32_RING_CAP_SAMPLES: usize =
    RR3_ESP32_AUDIO_RATE as usize * RR3_ESP32_AUDIO_CHANNELS as usize;

/// s16le 字节流 → i16 采样（LE 对；奇数尾字节弃——8192B 片恒偶，防御不改变
/// 现实形状）。
pub(super) fn rr3_s16le_decode(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]))
        .collect()
}

/// 环形采样缓冲：Mutex 内持有的纯数据件。溢出丢最旧**整声道帧**（保交织
/// 结构），丢弃计数对账（稳态滑移损耗可见）。
pub(super) struct Rr3AudioRing {
    deque: VecDeque<i16>,
    cap: usize,
    dropped: u64,
}

impl Rr3AudioRing {
    pub(super) fn rr3_ring_new(cap: usize) -> Self {
        Self {
            deque: VecDeque::with_capacity(cap),
            cap,
            dropped: 0,
        }
    }

    /// 推入一段采样（板面片解码产物）。防御性整帧对齐：输入按 ch 取整
    /// （8192B 片恒 4096 样本，现实输入恒对齐）；溢出丢最旧整帧 + 计数。
    pub(super) fn rr3_ring_push(&mut self, samples: &[i16], ch: usize) {
        let ch = ch.max(1);
        let aligned = samples.len() - samples.len() % ch;
        let samples = &samples[..aligned];
        // 溢出量按整帧对齐后从最旧端丢弃
        let overflow = (self.deque.len() + samples.len()).saturating_sub(self.cap);
        let drop_n = overflow - overflow % ch;
        self.rr3_ring_drop(drop_n);
        self.deque.extend(samples.iter().copied());
        // 入样本身超容量（理论形状，片 4096 << cap 32000 不可达）：再削最旧整帧
        let over = self.deque.len().saturating_sub(self.cap);
        let over_n = over - over % ch;
        self.rr3_ring_drop(over_n);
    }

    /// 从最旧端削 `n` 个采样并计数（溢出丢帧的唯一落点）。
    fn rr3_ring_drop(&mut self, n: usize) {
        for _ in 0..n {
            self.deque.pop_front();
        }
        self.dropped += n as u64;
    }

    /// 弹出整声道帧填充 `out`：`floor(avail/ch)` 与 `floor(out.len()/ch)` 取小
    /// ×ch，返回实填采样点数；欠载（含空环）回 0。非阻塞。
    pub(super) fn rr3_ring_take(&mut self, out: &mut [i16], ch: usize) -> usize {
        let ch = ch.max(1);
        let take = (self.deque.len() / ch).min(out.len() / ch) * ch;
        for slot in out.iter_mut().take(take) {
            *slot = self.deque.pop_front().unwrap_or(0);
        }
        take
    }

    pub(super) fn rr3_ring_dropped(&self) -> u64 {
        self.dropped
    }
}
