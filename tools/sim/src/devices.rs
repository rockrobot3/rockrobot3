//! 模拟设备源（`rr-hal` 的 sim 后端消费方）。
//!
//! 只使用 `rr-hal` 的真实数据类型，保证 sim 与 linux 后端将来交出同一种流；
//! 波形注入（正弦/噪声）随 M2，v0.0.1 只出静音帧以钉住尺寸与时长换算。

use rr_hal::{Rr3PcmStream, RR3_AUDIO_CFG_16K_MONO16};

/// 生成 `frames` 帧静音。单声道下 1 帧 = 1 采样点。
pub fn rr3_sim_silence(frames: usize) -> Rr3PcmStream {
    let channels = RR3_AUDIO_CFG_16K_MONO16.channels as usize;
    Rr3PcmStream {
        cfg: RR3_AUDIO_CFG_16K_MONO16,
        samples: vec![0i16; frames * channels],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1600 帧 @16kHz = 100ms：换算走 rr-hal 的实现，sim 不自己算一遍
    #[test]
    fn silence_stream_reports_expected_duration() {
        assert_eq!(rr3_sim_silence(1600).rr3_pcm_duration_ms(), 100);
        assert_eq!(rr3_sim_silence(1600).samples.len(), 1600);
    }

    #[test]
    fn zero_frames_are_empty_not_phantom() {
        let s = rr3_sim_silence(0);
        assert!(s.samples.is_empty());
        assert_eq!(s.rr3_pcm_duration_ms(), 0);
    }
}
