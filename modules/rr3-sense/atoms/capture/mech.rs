// SPDX-License-Identifier: Apache-2.0
//! capture 原子件机制入口（W1-2 抽取后）：机制主体住在 `rr-audio-mech`，本文件
//! 保留件面 re-export 与两腿对拍锚（⑧-a 按内容执法，机器锚必须留在机制源文件内）。
//!
//! 行为与 golden 向量逐字未变：TDOA 判向、下混、节拍换算即原 `mech.rs` 的逐字
//! 搬迁，单元用例随迁；本文件只保留对拍用例，防止抽取后件面接线与机制核脱节。

pub use rr_audio_mech::{rr3_doa_fix, rr3_frames_per_tick, rr3_pcm_best_channel, Rr3DoaFix};

/// `Rr3DoaBin` 仅被组合件测试按名构造——测试面专用导出。
#[cfg(test)]
pub use rr_audio_mech::Rr3DoaBin;

#[cfg(test)]
mod parity {
    use super::*;
    use rr_audio_mech::Rr3DoaCfg;

    struct Xorshift(u32);

    impl Xorshift {
        fn next_i16_scaled(&mut self, amp: i32) -> i16 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            let v = (x as i32) >> 16;
            ((v * amp) >> 15) as i16
        }
    }

    fn delayed_stereo(tau: i32, frames: usize, amp: i32) -> Vec<i16> {
        let mut xs = Xorshift(0x1234_5678);
        let u: Vec<i16> = (0..frames + tau.unsigned_abs() as usize)
            .map(|_| xs.next_i16_scaled(amp))
            .collect();
        let (r, l): (Vec<i16>, Vec<i16>) = if tau >= 0 {
            (u[tau as usize..].to_vec(), u[..frames].to_vec())
        } else {
            (u[..frames].to_vec(), u[(-tau) as usize..].to_vec())
        };
        (0..frames).flat_map(|t| [r[t], l[t]]).collect()
    }

    fn doa_cfg() -> Rr3DoaCfg {
        Rr3DoaCfg {
            sample_rate_hz: 16000,
            frame_ms: 20,
            mic_baseline_mm: 150,
            window_rms_min: 800,
            bin_threshold_deg: 30,
            sign: 1,
        }
    }

    // ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②） ----------

    #[test]
    fn parity_leg1_capture_output_train_is_deterministic() {
        // 腿一·确定性：同一延迟立体声序列两跑，(下混流, 判向序列) 逐帧相等
        fn run() -> (Vec<i16>, Vec<Option<Rr3DoaFix>>) {
            let cfg = doa_cfg();
            let seq: Vec<Vec<i16>> = [(-4i32, 800usize), (0, 800), (5, 800)]
                .iter()
                .map(|&(tau, n)| delayed_stereo(tau, n, 3000))
                .collect();
            let mixed: Vec<i16> = seq
                .iter()
                .flat_map(|s| rr3_pcm_best_channel(s, 2))
                .collect();
            let fixes: Vec<Option<Rr3DoaFix>> =
                seq.iter().map(|s| rr3_doa_fix(s, 2, &cfg)).collect();
            (mixed, fixes)
        }
        assert_eq!(run(), run(), "同输入序列两跑逐帧相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_capture_matches_ruling_semantics() {
        // 腿二·正确性：分档等号边界独立重写（|deg| ≤ 30° 含等号落中档）；
        // 零值输入不产帧循环（0 帧换算、空样本安全返回）
        for (tau, want_bin) in [(-4, Rr3DoaBin::Left), (4, Rr3DoaBin::Right)] {
            let fix = rr3_doa_fix(&delayed_stereo(tau, 3200, 3000), 2, &doa_cfg()).unwrap();
            assert_eq!(fix.bin, want_bin, "tau={tau} 档位");
        }
        assert_eq!(rr3_frames_per_tick(16000, 0), 0, "(16000, 0) → 0 帧");
        assert_eq!(rr3_frames_per_tick(0, 20), 0, "(0, 20) → 0 帧");
        assert!(rr3_pcm_best_channel(&[], 2).is_empty());
        assert_eq!(rr3_doa_fix(&[], 2, &doa_cfg()), None, "空窗无方向可判");
    }
}
