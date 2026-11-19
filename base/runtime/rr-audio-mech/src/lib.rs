//! 本地语音机制核：VAD 边沿检测 + 双麦 TDOA 判向 + 多通道下混，整数运算，
//! 零平台依赖。VAD 自 `rr3-sense` 的 vad 原子件抽出（W1-1），DoA/下混自
//! capture 原子件抽出（W1-2），行为与 golden 向量逐字保留。
//!
//! VAD：每帧（20ms）调用一次 [`Rr3VadState::rr3_vad_feed`]，状态机只在**边沿**
//! 时刻返回 `Some`，其余帧返回 `None`：
//!
//! ```text
//! （未在语音中）rms ≥ rms_start 连续 ≥ min_speech_ms ──→ Onset{rms_peak, speech_ms}
//! （在语音中）rms <  rms_start 连续 ≥ silence_tail_ms ──→ Offset{speech_ms}
//! ```
//!
//! DoA：判向窗上做 plain 时域互相关，lag>0 = 右麦先到 = 声源在右；Q7 亚样本
//! 抛物线 + 256 格 asin 查表，出整度与三档离散。
//!
//! 整数纪律：RMS 走 `isqrt(sum_sq / n)`，DoA 全程整数——golden 向量要求任何
//! 机器逐位一致，浮点在跨平台上没有这个保证。

#![no_std]

extern crate alloc;

mod doa;

pub use doa::{
    rr3_doa_fix, rr3_frames_per_tick, rr3_pcm_best_channel, Rr3DoaBin, Rr3DoaCfg, Rr3DoaFix,
};

/// VAD 参数（策略量由装配注入；机制件零策略载荷）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3VadConfig {
    /// 起音阈值（帧 RMS ≥ 它才算"有声"）
    pub rms_start: i64,
    /// 判停尾静音：在语音中连续静音达到此时长 → Offset
    pub silence_tail_ms: u64,
    /// 最短语音：起音须连续保持此时长才认（防瞬态 blip 误触发）
    pub min_speech_ms: u64,
}

impl Default for Rr3VadConfig {
    fn default() -> Self {
        Self {
            rms_start: 1500,
            silence_tail_ms: 700,
            min_speech_ms: 200,
        }
    }
}

/// 边沿事件（映射到 `rr3-sense/audio/vad_start` 的负载字段）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3VadEdge {
    /// 起音：`rms_peak` 是起音窗内的峰值帧 RMS，`speech_ms` 是达到 min_speech 的时长
    Onset { rms_peak: i64, speech_ms: u64 },
    /// 判停：`speech_ms` 是本次语音段的净有声时长（不含尾静音）
    Offset { speech_ms: u64 },
}

/// VAD 状态机（每模块一份，非线程安全——主循环单线程喂）
pub struct Rr3VadState {
    cfg: Rr3VadConfig,
    frame_ms: u64,
    in_speech: bool,
    /// 起音窗内连续有声时长（below 一帧即清零）
    above_ms: u64,
    /// 判停窗内连续静音时长（above 一帧即清零）
    silence_ms: u64,
    /// 本次语音段的净有声时长
    speech_ms: u64,
    /// 起音窗内峰值帧 RMS
    rms_peak: i64,
}

/// 整数平方根（牛顿二分）：`isqrt(x)` = ⌊√x⌋。sum_sq/n 最大约
/// 320 × 32767² ≈ 3.4e11，u64 绰绰有余；`mid <= x / mid` 的写法避免 mid*mid 溢出。
fn rr3_isqrt(x: u64) -> u64 {
    if x < 2 {
        return x;
    }
    rr3_isqrt_scan(x)
}

/// 二分求整数平方根（`x ≥ 2`）。
fn rr3_isqrt_scan(x: u64) -> u64 {
    let (mut lo, mut hi, mut ans) = (1u64, x, 1u64);
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        if mid <= x / mid {
            ans = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    ans
}

/// 帧 RMS（整数）：空帧按 0 处理
pub fn rr3_frame_rms(samples: &[i16]) -> i64 {
    if samples.is_empty() {
        return 0;
    }
    let sum_sq: u64 = samples
        .iter()
        .map(|&s| {
            let v = s as i64;
            (v * v) as u64
        })
        .sum();
    rr3_isqrt(sum_sq / samples.len() as u64) as i64
}

impl Rr3VadState {
    pub fn rr3_vad_new(cfg: Rr3VadConfig, frame_ms: u64) -> Self {
        Self {
            cfg,
            frame_ms,
            in_speech: false,
            above_ms: 0,
            silence_ms: 0,
            speech_ms: 0,
            rms_peak: 0,
        }
    }

    /// 喂一帧采样，返回边沿（无边沿返回 `None`）。
    /// 判定按**帧 RMS**整帧二值化：一帧内不做过渡带。
    pub fn rr3_vad_feed(&mut self, samples: &[i16]) -> Option<Rr3VadEdge> {
        let rms = rr3_frame_rms(samples);
        if !self.in_speech {
            return self.rr3_vad_onset(rms);
        }
        self.rr3_vad_tail(rms)
    }

    /// 非语音态：起音窗积累（`min_speech_ms` 到点发 Onset；below 一帧即清零）。
    fn rr3_vad_onset(&mut self, rms: i64) -> Option<Rr3VadEdge> {
        if rms >= self.cfg.rms_start {
            self.above_ms += self.frame_ms;
            self.rms_peak = self.rms_peak.max(rms);
            if self.above_ms >= self.cfg.min_speech_ms {
                self.in_speech = true;
                self.speech_ms = self.above_ms;
                self.silence_ms = 0;
                return Some(Rr3VadEdge::Onset {
                    rms_peak: self.rms_peak,
                    speech_ms: self.speech_ms,
                });
            }
        } else {
            // below 一帧即清零起音窗：瞬态 blip 永远攒不满 min_speech_ms
            self.above_ms = 0;
            self.rms_peak = 0;
        }
        None
    }

    /// 语音态：净有声时长累计 / 判停窗积累（`silence_tail_ms` 到点发 Offset）。
    fn rr3_vad_tail(&mut self, rms: i64) -> Option<Rr3VadEdge> {
        if rms >= self.cfg.rms_start {
            self.silence_ms = 0;
            self.speech_ms += self.frame_ms;
            return None;
        }
        self.rr3_tail_silence()
    }

    /// 静音尾段：攒够静音尾即报 offset 并清窗。
    fn rr3_tail_silence(&mut self) -> Option<Rr3VadEdge> {
        self.silence_ms += self.frame_ms;
        if self.silence_ms < self.cfg.silence_tail_ms {
            return None;
        }
        self.in_speech = false;
        let speech_ms = self.speech_ms;
        self.above_ms = 0;
        self.silence_ms = 0;
        self.speech_ms = 0;
        self.rms_peak = 0;
        Some(Rr3VadEdge::Offset { speech_ms })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_MS: u64 = 20;
    const CFG: Rr3VadConfig = Rr3VadConfig {
        rms_start: 1500,
        silence_tail_ms: 700,
        min_speech_ms: 200,
    };

    fn state() -> Rr3VadState {
        Rr3VadState::rr3_vad_new(CFG, FRAME_MS)
    }

    fn frame(amp: i16) -> [i16; 320] {
        [amp; 320]
    }

    #[test]
    fn isqrt_matches_floor_sqrt_on_known_and_edge_values() {
        let cases = [
            (0u64, 0u64),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 2),
            (8, 2),
            (9, 3),
            (15, 3),
            (16, 4),
            (24, 4),
            (25, 5),
            (9_000_000, 3000),
            (8_999_999, 2999),
            (320 * 3000i64.pow(2) as u64, 53_665),
        ];
        for (x, want) in cases {
            assert_eq!(rr3_isqrt(x), want, "isqrt({x})");
        }
        assert_eq!(rr3_isqrt(u64::MAX), 4_294_967_295);
    }

    #[test]
    fn frame_rms_is_exact_integer_for_constant_frames() {
        assert_eq!(rr3_frame_rms(&[]), 0);
        assert_eq!(rr3_frame_rms(&[0; 320]), 0);
        assert_eq!(rr3_frame_rms(&frame(3000)), 3000);
        assert_eq!(rr3_frame_rms(&frame(-3000)), 3000);
    }

    /// golden 向量：常幅 3000 连喂 10 帧（200ms）→ 第 10 帧发 Onset。
    #[test]
    fn constant_speech_fires_onset_at_min_speech_boundary() {
        let mut vad = state();
        for i in 1..10u32 {
            assert_eq!(vad.rr3_vad_feed(&frame(3000)), None, "第 {i} 帧不该发边沿");
        }
        assert_eq!(
            vad.rr3_vad_feed(&frame(3000)),
            Some(Rr3VadEdge::Onset {
                rms_peak: 3000,
                speech_ms: 200
            })
        );
    }

    /// golden 向量：起音后 5 帧语音 + 35 帧静音 → speech_ms=300 的 Offset。
    #[test]
    fn speech_then_tail_silence_fires_offset_with_net_speech_ms() {
        let mut vad = state();
        for _ in 0..10 {
            vad.rr3_vad_feed(&frame(3000));
        }
        for _ in 0..5 {
            assert_eq!(vad.rr3_vad_feed(&frame(3000)), None);
        }
        for i in 1..35u32 {
            assert_eq!(vad.rr3_vad_feed(&frame(0)), None, "静音第 {i} 帧不该发边沿");
        }
        assert_eq!(
            vad.rr3_vad_feed(&frame(0)),
            Some(Rr3VadEdge::Offset { speech_ms: 300 })
        );
    }

    /// 短于 min_speech_ms 的 blip 不发任何边沿。
    #[test]
    fn blip_shorter_than_min_speech_emits_nothing() {
        let mut vad = state();
        for _ in 0..5 {
            assert_eq!(vad.rr3_vad_feed(&frame(3000)), None);
        }
        for _ in 0..60 {
            assert_eq!(vad.rr3_vad_feed(&frame(0)), None, "blip 后静音不得补发边沿");
        }
    }

    /// rms_peak 取起音窗内的最大帧 RMS。
    #[test]
    fn onset_carries_peak_rms_across_the_window() {
        let mut vad = state();
        for _ in 0..3 {
            assert_eq!(vad.rr3_vad_feed(&frame(2000)), None);
        }
        for _ in 0..6 {
            assert_eq!(vad.rr3_vad_feed(&frame(4000)), None);
        }
        assert_eq!(
            vad.rr3_vad_feed(&frame(4000)),
            Some(Rr3VadEdge::Onset {
                rms_peak: 4000,
                speech_ms: 200
            })
        );
    }

    #[test]
    fn silence_only_never_emits_any_edge() {
        let mut vad = state();
        for _ in 0..200 {
            assert_eq!(vad.rr3_vad_feed(&frame(0)), None);
        }
    }

    /// 判停中途恢复有声：silence_ms 清零重新数，净 speech_ms 继续累计。
    #[test]
    fn mid_tail_speech_resets_silence_window() {
        let mut vad = state();
        for _ in 0..10 {
            vad.rr3_vad_feed(&frame(3000));
        }
        for _ in 0..30 {
            assert_eq!(vad.rr3_vad_feed(&frame(0)), None);
        }
        for _ in 0..10 {
            assert_eq!(vad.rr3_vad_feed(&frame(3000)), None);
        }
        for i in 1..35u32 {
            assert_eq!(
                vad.rr3_vad_feed(&frame(0)),
                None,
                "恢复后静音第 {i} 帧不该发边沿"
            );
        }
        assert_eq!(
            vad.rr3_vad_feed(&frame(0)),
            Some(Rr3VadEdge::Offset { speech_ms: 400 })
        );
    }

    /// 等号边界：rms == rms_start 算有声。
    #[test]
    fn rms_equal_to_threshold_counts_as_speech() {
        let mut vad = state();
        for _ in 0..9 {
            assert_eq!(vad.rr3_vad_feed(&frame(1500)), None);
        }
        assert_eq!(
            vad.rr3_vad_feed(&frame(1500)),
            Some(Rr3VadEdge::Onset {
                rms_peak: 1500,
                speech_ms: 200
            })
        );
    }
}
