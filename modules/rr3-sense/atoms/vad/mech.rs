//! vad 原子件机制入口（W1-1 抽取后）：机制主体住在 `rr-audio-mech`，本文件保留
//! 件面 re-export 与两腿对拍锚（⑧-a 按内容执法，机器锚必须留在机制源文件内）。
//!
//! 行为与 golden 向量逐字未变：`rr-audio-mech/src/lib.rs` 的 VAD 状态机即原
//! `mech.rs` 的逐字搬迁，单元用例随迁；本文件只保留对拍用例，防止抽取后
//! 件面接线与机制核脱节。

pub use rr_audio_mech::{Rr3VadEdge, Rr3VadState};

#[cfg(test)]
mod parity {
    use super::*;
    use rr_audio_mech::Rr3VadConfig;

    const FRAME_MS: u64 = 20;

    fn cfg() -> Rr3VadConfig {
        Rr3VadConfig {
            rms_start: 1500,
            silence_tail_ms: 700,
            min_speech_ms: 200,
        }
    }

    fn frame(amp: i16) -> Vec<i16> {
        vec![amp; 320]
    }

    // ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②） ----------

    #[test]
    fn parity_leg1_vad_edge_train_is_deterministic() {
        // 腿一·确定性：同一帧序列（20 帧声 / 20 帧静交替共 120 帧）两跑，
        // 边沿输出逐帧相等
        fn run() -> Vec<Option<Rr3VadEdge>> {
            let mut vad = Rr3VadState::rr3_vad_new(cfg(), FRAME_MS);
            (0..120)
                .map(|i| {
                    let amp = if (i / 20) % 2 == 0 { 3000 } else { 0 };
                    vad.rr3_vad_feed(&frame(amp))
                })
                .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐帧相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_vad_fires_edge_only_once_per_segment() {
        // 腿二·正确性：100 拍连续高电平只发 1 个 Onset（边沿语义非电平语义）；
        // 等号边界：rms == rms_start 算有声（≥ 判据含等号）
        let mut vad = Rr3VadState::rr3_vad_new(cfg(), FRAME_MS);
        let mut onsets = 0;
        for i in 0..100 {
            if let Some(e) = vad.rr3_vad_feed(&frame(3000)) {
                onsets += 1;
                assert!(
                    matches!(
                        e,
                        Rr3VadEdge::Onset {
                            rms_peak: 3000,
                            speech_ms: 200
                        }
                    ),
                    "第 {i} 拍的边沿形状"
                );
            }
        }
        assert_eq!(onsets, 1, "100 拍高电平只发 1 个 Onset，不重复报");
        // 等号：常幅帧 RMS 恰等于 rms_start=1500 也攒得起音窗
        let mut vad = Rr3VadState::rr3_vad_new(cfg(), FRAME_MS);
        for _ in 0..9 {
            assert_eq!(vad.rr3_vad_feed(&frame(1500)), None);
        }
        assert_eq!(
            vad.rr3_vad_feed(&frame(1500)),
            Some(Rr3VadEdge::Onset {
                rms_peak: 1500,
                speech_ms: 200
            }),
            "rms == rms_start 算有声（等号归中）"
        );
    }
}
