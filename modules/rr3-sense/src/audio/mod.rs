//! 采集域：麦克风阵列 PCM 抓取 + 本地 VAD 边沿检测。
//! 实现在 M3（随 rr-hal 的 linux/ALSA 后端）。

pub mod capture;
pub mod vad;

pub fn surface() -> impl Iterator<Item = &'static str> {
    capture::SURFACE.iter().chain(vad::SURFACE.iter()).copied()
}
