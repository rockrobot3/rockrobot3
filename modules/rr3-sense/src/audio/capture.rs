//! ALSA 麦克风采集：16 kHz / 单声道 / S16_LE。
//!
//! 原始 PCM 属实时控制面流（`rr3-sense/audio/pcm`，kind=stream）。v0.0.1 先走 UDS；
//! shm 零拷贝传输随 M5（ADR-0007 §裁定 3、topics.toml 同条 note）。

pub const SURFACE: &[&str] = &["audio/capture: 16k/mono/S16_LE -> rr3-sense/audio/pcm (stream)"];
