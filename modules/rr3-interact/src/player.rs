//! 播放：TTS 音频出话 + 拾音状态机的前端呈现。
//!
//! 入话来自 `rr3-interact/tts/text`（request）。预缓冲/抗抖动参数继承
//! legacy 的语音链路调优（提交 0cf029e），实现随 M3。

pub const SURFACE: &[&str] = &["player: rpc rr3-interact/tts/text (request)"];
