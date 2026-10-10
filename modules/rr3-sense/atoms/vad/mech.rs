//! vad 原子件机制主体（五件套③，自原 src/audio/vad.rs 的状态机逐字迁入，逻辑零改动）。
//! vad 事件体构造随 0.0.5 批I S-7 上移组合件（ADR-0019 修订 4：件面只留纯机制，
//! 零 serde_json），用例随迁。
//!
//! 本地 VAD：静音/起音**边沿**检测。只发边沿 + 置信度摘要，不发闭环数据
//! （ADR-0007 §决策一）。判停阈值与 legacy/main.c 的对话策略一致，参数落
//! config.toml，硬编码视为回归。
//!
//! 全程整数运算：RMS 走 `isqrt(sum_sq / n)`，不用浮点——golden 向量要求任何机器
//! 逐位一致，浮点在跨平台上没有这个保证。每帧（20ms）喂一次 [`Rr3VadState::rr3_vad_feed`]，
//! 状态机只在**边沿**那一刻返回 `Some`，其余帧返回 `None`：
//!
//! ```text
//! （未在语音中）rms ≥ rms_start 连续 ≥ min_speech_ms ──→ Onset{rms_peak, speech_ms}
//! （在语音中）rms <  rms_start 连续 ≥ silence_tail_ms ──→ Offset{speech_ms}
//! ```

use super::Rr3VadConfig;

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
/// 0.0.5 批I S-6：无件外读者（状态机内部判定用），降 pub(crate) 不进契约面。
pub(crate) fn rr3_frame_rms(samples: &[i16]) -> i64 {
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
        } else if rms >= self.cfg.rms_start {
            self.silence_ms = 0;
            self.speech_ms += self.frame_ms;
        } else {
            self.silence_ms += self.frame_ms;
            if self.silence_ms >= self.cfg.silence_tail_ms {
                self.in_speech = false;
                let speech_ms = self.speech_ms;
                self.above_ms = 0;
                self.silence_ms = 0;
                self.speech_ms = 0;
                self.rms_peak = 0;
                return Some(Rr3VadEdge::Offset { speech_ms });
            }
        }
        None
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

    fn frame(amp: i16) -> Vec<i16> {
        vec![amp; 320]
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
        // 常幅帧的 RMS 恰等于幅度（整数，无浮点误差）
        assert_eq!(rr3_frame_rms(&frame(3000)), 3000);
        assert_eq!(rr3_frame_rms(&frame(-3000)), 3000);
    }

    /// golden 向量：常幅 3000 连喂 10 帧（200ms）→ 第 10 帧发 Onset，
    /// rms_peak=3000、speech_ms=200。前 9 帧必须安静。
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

    /// golden 向量：起音后再喂 5 帧语音 + 35 帧静音（700ms 判停）→ 第 35 帧静音
    /// 发 Offset，speech_ms=300（起音 200 + 续语音 100，净有声不含尾静音）。
    /// 第 34 帧必须安静。
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

    /// 短于 min_speech_ms 的 blip（5 帧 = 100ms < 200ms）不发任何边沿——
    /// 起音窗被一帧静音清零，之后再多静音也不该补发 Offset（没进过语音态）。
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

    /// rms_peak 取起音窗内的最大帧 RMS：2000×3 帧后跳到 4000×7 帧，
    /// Onset 应报 4000 而不是首帧的 2000。
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

    /// 纯静音永不发边沿（没进过语音态就没有 Offset；起音窗也攒不起来）。
    #[test]
    fn silence_only_never_emits_any_edge() {
        let mut vad = state();
        for _ in 0..200 {
            assert_eq!(vad.rr3_vad_feed(&frame(0)), None);
        }
    }

    /// 判停中途恢复有声：silence_ms 清零重新数，不发 Offset；
    /// 净 speech_ms 继续累计（尾静音不计入）。
    #[test]
    fn mid_tail_speech_resets_silence_window() {
        let mut vad = state();
        // 起音 10 帧（Onset 在第 10 帧发，取走不再断言）
        for _ in 0..10 {
            vad.rr3_vad_feed(&frame(3000));
        }
        // 30 帧静音（600ms < 700ms 判停线）
        for _ in 0..30 {
            assert_eq!(vad.rr3_vad_feed(&frame(0)), None);
        }
        // 恢复 10 帧有声
        for _ in 0..10 {
            assert_eq!(vad.rr3_vad_feed(&frame(3000)), None);
        }
        // 再 35 帧静音 → 第 35 帧发 Offset，净 speech_ms = 200（起音）+ 200（恢复），
        // 首段尾静音（600ms）与第二段尾静音均不计入
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
}
