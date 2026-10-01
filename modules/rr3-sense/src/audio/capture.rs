//! ALSA 麦克风采集：16 kHz / 单声道 / S16_LE。
//!
//! 原始 PCM 属实时控制面流（`rr3-sense/audio/pcm`，kind=stream）。v0.0.1 先走 UDS；
//! shm 零拷贝传输随 M5（ADR-0007 §裁定 3、topics.toml 同条 note）。

pub const SURFACE: &[&str] = &["audio/capture: 16k/mono/S16_LE -> rr3-sense/audio/pcm (stream)"];

/// 一次采集节拍读多少帧：`rate × frame_ms / 1000`（16000×20 = 320 帧）。
/// 采样点=帧×声道；HAL 的 `read(frames)` 按**帧**计数，换算只在这一处。
pub fn rr3_frames_per_tick(sample_rate_hz: u32, frame_ms: u64) -> usize {
    (sample_rate_hz as u64 * frame_ms / 1000) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_conversion_covers_registered_and_odd_rates() {
        assert_eq!(rr3_frames_per_tick(16000, 20), 320, "登记配置的黄金值");
        assert_eq!(rr3_frames_per_tick(16000, 10), 160);
        assert_eq!(rr3_frames_per_tick(8000, 20), 160);
        assert_eq!(rr3_frames_per_tick(22050, 20), 441, "22050×20/1000 整除");
        assert_eq!(
            rr3_frames_per_tick(16000, 0),
            0,
            "0ms 节拍读 0 帧（不 panic）"
        );
    }
}
