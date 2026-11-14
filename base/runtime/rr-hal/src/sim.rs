//! sim 音频后端：主机联调与 CI 的虚拟麦克风 / 虚拟扬声器（architecture.md §5）。
//!
//! 兑现 `tools/sim` devices.rs "波形注入（正弦/噪声）" 的承诺：四模式
//! [`Rr3SimWave`]——`Sine`（440Hz、幅度 12000，必过 VAD 起音阈值）、`Noise`
//! （确定性 xorshift，幅度 ±8000）、`Silence`（全零）、`Burst`（1s 正弦 + 1s
//! 静音循环，VAD 周期性发 onset/offset，供多轮对话闭环联调）。两个后端都是
//! 纯计算、同步签名（契约层 rr-hal-api，批I F1 起无 future/noop-waker）。
//!
//! 确定性是硬要求：同一 wave 同一帧序，任何机器任何时刻读出的 PCM 逐点相等——
//! golden 向量测试与 e2e 都钉在这一点上。
//!
//! 模块按件分：`parse` 波形取值、`source` 音频源与波形生成、`sink` 音频接收与
//! 尾窗裁剪、`periph` servo/gpio/display/电池/链路模拟件。

mod parse;
mod periph;
mod sink;
mod source;

#[cfg(test)]
mod tests;

pub use parse::Rr3SimWave;
pub use periph::{
    rr3_sim_scan_list, Rr3SimBattery, Rr3SimDisplay, Rr3SimGpioOut, Rr3SimLink, Rr3SimServo,
};
pub use sink::Rr3SimAudioSink;
pub use source::{rr3_sim_tts_block, Rr3SimAudioSource, Rr3SimDoa};
