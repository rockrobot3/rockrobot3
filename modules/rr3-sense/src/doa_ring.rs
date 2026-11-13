//! 判向窗环形缓冲（0.0.8 批 b3b；批D1 自 dispatch.rs 迁出）。
//!
//! 双麦链路的 TDOA 积累器：原始多通道帧在下混**之前**按帧交错写入，onset
//! 边沿整窗线性化交 [`crate::capture::rr3_doa_fix`] 判向。单声道链路恒无方向
//! 可判，调用方不分配（[`rr3_ring_feed`] 对 `None` 直接跳过，零开销）。
//!
//! 本文件只含机制（帧位推进／回绕线性化）：窗长与声道数由构造参数注入，
//! 写入与快照都在采集拍单线程内完成，无锁、无时钟依赖。

/// 判向窗环形缓冲（帧交错存储，与采集 samples 同形；主循环单线程读写）
pub(crate) struct Rr3DoaRing {
    buf: Vec<i16>,
    ch: usize,
    cap_frames: usize,
    /// 下一写入帧位（模 cap_frames）
    pos: usize,
    /// 已积累帧数（≤ cap_frames）
    have: usize,
}

impl Rr3DoaRing {
    pub(crate) fn rr3_ring_new(cap_frames: usize, ch: usize) -> Self {
        let ch = ch.max(1);
        let cap_frames = cap_frames.max(1);
        Self {
            buf: vec![0i16; cap_frames * ch],
            ch,
            cap_frames,
            pos: 0,
            have: 0,
        }
    }

    /// 推入一拍原始采样（帧交错）；不足一帧的余数丢弃（采集整帧纪律）
    pub(crate) fn rr3_ring_feed(&mut self, samples: &[i16]) {
        for frame in samples.chunks_exact(self.ch) {
            let off = self.pos * self.ch;
            self.buf[off..off + self.ch].copy_from_slice(frame);
            self.pos = (self.pos + 1) % self.cap_frames;
            self.have = (self.have + 1).min(self.cap_frames);
        }
    }

    /// 线性化快照：最旧帧在前、最新帧在后（onset 时交判向机制）
    pub(crate) fn rr3_ring_snapshot(&self) -> Vec<i16> {
        if self.have < self.cap_frames {
            return self.buf[..self.have * self.ch].to_vec();
        }
        let off = self.pos * self.ch;
        let mut snap = Vec::with_capacity(self.buf.len());
        snap.extend_from_slice(&self.buf[off..]);
        snap.extend_from_slice(&self.buf[..off]);
        snap
    }
}

/// 判向窗积累（未分配 ring = 单声道链路，跳过）。
pub(crate) fn rr3_ring_feed(ring: Option<&mut Rr3DoaRing>, samples: &[i16]) {
    if let Some(ring) = ring {
        ring.rr3_ring_feed(samples);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture;
    use crate::capture::Rr3DoaBin;

    /// 判向窗 ring：未满窗快照只有已积累帧（原序）；满窗回绕后快照 = 最旧在前
    /// 最新在后的线性化；不足一帧的余数丢弃（帧交错纪律）。
    #[test]
    fn doa_ring_accumulates_and_linearizes_snapshot() {
        let mut ring = Rr3DoaRing::rr3_ring_new(4, 2);
        ring.rr3_ring_feed(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(
            ring.rr3_ring_snapshot(),
            vec![1, 2, 3, 4, 5, 6],
            "未满窗只回已积累帧"
        );
        // 再喂 2 帧共 5 帧：第 1 帧被挤掉，快照 = 帧2..帧5 原序（回绕线性化）
        ring.rr3_ring_feed(&[7, 8, 9, 10]);
        assert_eq!(
            ring.rr3_ring_snapshot(),
            vec![3, 4, 5, 6, 7, 8, 9, 10],
            "回绕线性化"
        );
        let mut ring = Rr3DoaRing::rr3_ring_new(4, 2);
        ring.rr3_ring_feed(&[1, 2, 3, 4, 9]);
        assert_eq!(ring.rr3_ring_snapshot(), vec![1, 2, 3, 4], "余数丢弃");
    }

    /// ring → snapshot → 判向机制的组合件侧集成形状：τ=+4 声源在右、τ=−4 在左
    /// （确定性宽带源与 mech 侧 golden 同构；此处只证管线通，算法逐位由 mech 钉）。
    #[test]
    fn doa_ring_snapshot_feeds_tdoa_pipeline() {
        fn xs_next(x: &mut u32) -> i16 {
            *x ^= *x << 13;
            *x ^= *x >> 17;
            *x ^= *x << 5;
            ((*x as i32) >> 16) as i16
        }
        let cfg = capture::Rr3DoaCfg {
            sample_rate_hz: 16000,
            frame_ms: 20,
            mic_baseline_mm: 150,
            window_rms_min: 800,
            bin_threshold_deg: 30,
        };
        let n = 3400usize;
        let mut x = 0x1234_5678u32;
        let u: Vec<i16> = (0..n + 4).map(|_| xs_next(&mut x)).collect();
        // τ=+4：frame[0]=物理右麦领先（u[t+4] 相对 u[t] 左移 4 = 提前）→ 声源在右
        let right: Vec<i16> = (0..n).flat_map(|t| [u[t + 4], u[t]]).collect();
        let mut ring = Rr3DoaRing::rr3_ring_new(3200, 2);
        ring.rr3_ring_feed(&right);
        let snap = ring.rr3_ring_snapshot();
        assert_eq!(snap.len(), 3200 * 2, "满窗快照 = 容量帧数×声道");
        assert_eq!(
            capture::rr3_doa_fix(&snap, 2, &cfg).unwrap().bin,
            Rr3DoaBin::Right
        );
        // τ=−4：frame[1]=物理左麦领先 → 声源在左
        let mut x = 0x1234_5678u32;
        let u: Vec<i16> = (0..n + 4).map(|_| xs_next(&mut x)).collect();
        let left: Vec<i16> = (0..n).flat_map(|t| [u[t], u[t + 4]]).collect();
        let mut ring = Rr3DoaRing::rr3_ring_new(3200, 2);
        ring.rr3_ring_feed(&left);
        assert_eq!(
            capture::rr3_doa_fix(&ring.rr3_ring_snapshot(), 2, &cfg)
                .unwrap()
                .bin,
            Rr3DoaBin::Left
        );
    }
}
