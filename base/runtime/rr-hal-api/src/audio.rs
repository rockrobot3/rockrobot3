//! 音频契约：上行拾音源与下行播放面。

use crate::cfg::Rr3AudioCfg;
use crate::err::Rr3HalError;

/// 音频源（上行拾音）。`open` 固定采样配置；`read` 写入调用方缓冲、返回有效
/// 样本数（0 = 无新数据）。缓冲形状由调用方定（固定帧长用栈数组，承载层可
/// 用堆缓冲），契约层不再拥有 `Vec`。
pub trait Rr3AudioSource {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError>;
    fn read(&mut self, buf: &mut [i16]) -> Result<usize, Rr3HalError>;
}

/// 音频播放面（下行出话）。与 [`Rr3AudioSource`] 对称：`open` 固定采样配置，
/// `write` 消费借用切片。TTS 文本到 PCM 的合成属模块侧（rr3-interact），不进
/// HAL；HAL 只搬运已合成的波形。
pub trait Rr3AudioSink {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError>;
    fn write(&mut self, samples: &[i16]) -> Result<(), Rr3HalError>;
}
