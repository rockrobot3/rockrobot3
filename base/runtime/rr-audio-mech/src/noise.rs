//! 底噪统计与 VAD 阈值整定：帧 RMS 环形样本 → 中位/分位；噪声统计 + 注入
//! 剖面 → VAD 阈值组。机制件零策略载荷：倍率与上下界全在 [`Rr3VadTune`]，
//! 由装配按现场注入。
//!
//! 标定窗 128 帧（20 ms 帧 ≈ 2.56 s）：中位数对短时说话/敲击稳健（说话占窗
//! 不足一半不抬中位），整数排序零浮点，满足 golden 纪律。

use crate::Rr3VadConfig;

/// 标定窗帧数（128 × 20 ms = 2.56 s）。
pub const RR3_NOISE_FRAMES: usize = 128;

/// 底噪样本环：满窗后覆盖最旧，任意时刻保留最近 [`RR3_NOISE_FRAMES`] 帧。
#[derive(Debug)]
pub struct Rr3NoiseEst {
    buf: [i64; RR3_NOISE_FRAMES],
    len: usize,
    pos: usize,
}

impl Rr3NoiseEst {
    pub const fn rr3_new() -> Self {
        Self {
            buf: [0; RR3_NOISE_FRAMES],
            len: 0,
            pos: 0,
        }
    }

    /// 喂一帧 RMS（帧节拍由调用方保证；本件不看时间）。
    pub fn rr3_feed(&mut self, rms: i64) {
        self.buf[self.pos] = rms;
        self.pos = (self.pos + 1) % RR3_NOISE_FRAMES;
        if self.len < RR3_NOISE_FRAMES {
            self.len += 1;
        }
    }

    /// 满窗才可标定：短窗中位数易被起音瞬态拉高。
    pub fn rr3_ready(&self) -> bool {
        self.len >= RR3_NOISE_FRAMES
    }

    /// 分位值（0..=100，向下取下标）；空窗返回 `None`。
    pub fn rr3_pct(&self, pct: u32) -> Option<i64> {
        self.rr3_sorted().map(|s| {
            let idx = ((self.len - 1) * pct.min(100) as usize) / 100;
            s[idx]
        })
    }

    pub fn rr3_median(&self) -> Option<i64> {
        self.rr3_pct(50)
    }

    /// 最近 `len` 帧的升序快照（栈上 1 KB，仅标定期调用一次）。
    fn rr3_sorted(&self) -> Option<[i64; RR3_NOISE_FRAMES]> {
        if self.len == 0 {
            return None;
        }
        let mut a = [0i64; RR3_NOISE_FRAMES];
        a[..self.len].copy_from_slice(&self.buf[..self.len]);
        rr3_ins_sort(&mut a[..self.len]);
        Some(a)
    }
}

/// 插入排序（≤128 项，O(n²) ≈ 8k 比较，启动面一次性开销可忽略）。
fn rr3_ins_sort(a: &mut [i64]) {
    for i in 1..a.len() {
        let v = a[i];
        let mut j = i;
        while j > 0 && a[j - 1] > v {
            a[j] = a[j - 1];
            j -= 1;
        }
        a[j] = v;
    }
}

/// 阈值剖面：起音/收尾倍率（Q8）与上下界，全部是策略量（装配注入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3VadTune {
    /// 起音阈值 = 噪声中位 × `start_q8` / 256
    pub start_q8: i64,
    /// 收尾阈值 = 噪声中位 × `tail_q8` / 256
    pub tail_q8: i64,
    pub start_min: i64,
    pub start_max: i64,
    pub tail_min: i64,
}

/// 噪声中位 → VAD 阈值组：倍率整定 + 上下界夹取，并保证收尾阈值不超过起音
/// 阈值（反向滞回会让语音态永远等不到静音尾）。节拍参数沿用 `base`。
pub fn rr3_vad_tune(noise_rms: i64, tune: &Rr3VadTune, base: Rr3VadConfig) -> Rr3VadConfig {
    let start = (noise_rms * tune.start_q8 >> 8).clamp(tune.start_min, tune.start_max);
    let tail_lo = tune.tail_min.min(start);
    let tail = (noise_rms * tune.tail_q8 >> 8).clamp(tail_lo, start);
    Rr3VadConfig {
        rms_start: start,
        rms_tail: tail,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Rr3VadConfig {
        Rr3VadConfig {
            rms_start: 450,
            rms_tail: 380,
            silence_tail_ms: 400,
            min_speech_ms: 200,
        }
    }

    fn tune() -> Rr3VadTune {
        Rr3VadTune {
            start_q8: 384,
            tail_q8: 320,
            start_min: 450,
            start_max: 3000,
            tail_min: 380,
        }
    }

    #[test]
    fn percentiles_are_exact_on_a_known_ramp() {
        let mut n = Rr3NoiseEst::rr3_new();
        assert_eq!(n.rr3_median(), None, "空窗无统计");
        assert!(!n.rr3_ready());
        for v in 1..=RR3_NOISE_FRAMES as i64 {
            n.rr3_feed(v);
        }
        assert!(n.rr3_ready());
        assert_eq!(n.rr3_pct(0), Some(1));
        assert_eq!(n.rr3_median(), Some(64), "127×50/100 = 63 → 第 64 项");
        assert_eq!(n.rr3_pct(90), Some(115), "127×90/100 = 114 → 第 115 项");
        assert_eq!(n.rr3_pct(100), Some(128));
    }

    #[test]
    fn ring_keeps_only_the_latest_window() {
        let mut n = Rr3NoiseEst::rr3_new();
        for v in 0..200 {
            n.rr3_feed(v);
        }
        // 最近 128 帧 = 72..=199，中位下标 63 → 135
        assert_eq!(n.rr3_median(), Some(135));
    }

    #[test]
    fn tune_scales_clamps_and_keeps_tail_below_start() {
        let t = tune();
        let quiet = rr3_vad_tune(600, &t, base());
        assert_eq!((quiet.rms_start, quiet.rms_tail), (900, 750));
        let floor = rr3_vad_tune(100, &t, base());
        assert_eq!((floor.rms_start, floor.rms_tail), (450, 380), "下界夹取");
        let loud = rr3_vad_tune(4000, &t, base());
        assert_eq!(
            (loud.rms_start, loud.rms_tail),
            (3000, 3000),
            "上界压平后收尾不越起音"
        );
        assert_eq!(
            loud.silence_tail_ms,
            base().silence_tail_ms,
            "节拍参数原样保留"
        );
        let neg = rr3_vad_tune(-50, &t, base());
        assert_eq!((neg.rms_start, neg.rms_tail), (450, 380), "负噪声按 0 处理");
    }
}
