//! rr-hal —— 硬件抽象层实现层（architecture.md §5）。
//!
//! 契约层在 [`rr-hal-api`](../rr-hal-api)（no_std，ADR-0018 修订 1 ③，批I F1）：
//! 7 trait 同步签名 + 值类型 + 错型。本 crate 是它的 **Linux/sim 实现层**，并经
//! `pub use` 把契约名与路径原样保住（`rr_hal::Rr3GpioOut` 等，六模块调用点不改路径）。
//! 慢速事件级接口优先：1kHz 以上闭环属模块进程内的反射环，永不进 HAL trait
//! （ADR-0007 裁定 1）。
//!
//! 承载层数据 [`Rr3PcmStream`]（cfg + `Vec<i16>`）留在此处：契约面的读写已改借用
//! 形状（`read` 写调用方缓冲、`write` 借用切片），`Vec` 形状只在 std 承载层（载荷
//! 组装、件间传递）使用，不再出现在契约签名里。

// 错型尺寸是契约决定的，不是缺陷：Rr3HalError（rr-hal-api no_std 契约错误面，
// InvalidParam(Rr3ErrText) 内联 128B 定长文案缓冲）按值传约 136B，超 clippy
// `result_large_err` 的 128B 缺省阈值。装箱（Box）破坏 no_std 契约面、缩容
// 动契约常量（Rr3ErrText 定长 128B）——两个"修法"都比 lint 本身更伤，crate 级豁免。
#![allow(clippy::result_large_err)]

pub use rr_hal_api::{
    Rr3AudioCfg, Rr3AudioSink, Rr3AudioSource, Rr3BatteryState, Rr3Display, Rr3ErrText,
    Rr3FaceFrame, Rr3GpioOut, Rr3HalError, Rr3Link, Rr3LinkState, Rr3Power, Rr3PwmServo,
    Rr3SsidText, Rr3UnavailableSource, RR3_AUDIO_CFG_16K_MONO16,
};

pub mod backend;
/// 物理绑定描述符（ADR-0018 裁定 4、批D④）：引脚/总线/设备的唯一数据事实源，
/// 描述符为底、env 应急覆盖、代码缺省 fail-closed。
pub mod descriptor;
#[cfg(all(target_os = "linux", feature = "alsa"))]
pub mod linux;
/// linux chardev GPIO 数字输出后端（R11②）：libc 仅做 ioctl/close 声明
/// （musl 静态零额外 C 依赖），同为单重平台门，不挂 alsa feature。
#[cfg(target_os = "linux")]
pub mod linux_gpio;
/// linux NetworkManager 链路读数后端（R18，判据 13）：nmcli 子进程承载（std
/// Command 零新依赖），同为单重平台门，不挂 alsa feature。
#[cfg(target_os = "linux")]
pub mod linux_link;
/// linux I2C 电池读数后端（R18，INA219@0x40）：libc 仅做 ioctl/read/write/close
/// 声明壳，同为单重平台门，不挂 alsa feature。
#[cfg(target_os = "linux")]
pub mod linux_power;
/// linux sysfs PWM 舵机后端：纯 std 无外部依赖，只按平台门编译（R11①），
/// 不挂 alsa feature——音频可用性与舵机可用性是两回事，各能力面各判各的。
#[cfg(target_os = "linux")]
pub mod linux_pwm;
pub mod sim;

/// PCM 帧缓冲（承载层数据，批I F1 起不再进契约签名）。所有权随返回值移交
/// （M2 的 shm 零拷贝沿用同一语义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3PcmStream {
    pub cfg: Rr3AudioCfg,
    pub samples: Vec<i16>,
}

impl Rr3PcmStream {
    /// 时长（毫秒，向下取整）
    pub fn rr3_pcm_duration_ms(&self) -> u64 {
        if self.cfg.sample_rate_hz == 0 || self.cfg.channels == 0 {
            return 0;
        }
        let frames = self.samples.len() / self.cfg.channels as usize;
        frames as u64 * 1000 / self.cfg.sample_rate_hz as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_stream_duration_uses_channels_and_rate() {
        let s = Rr3PcmStream {
            cfg: RR3_AUDIO_CFG_16K_MONO16,
            samples: vec![],
        };
        assert_eq!(s.rr3_pcm_duration_ms(), 0);
        // 2400 帧 @16kHz 单声道 = 150ms
        let full = Rr3PcmStream {
            cfg: RR3_AUDIO_CFG_16K_MONO16,
            samples: vec![0i16; 2400],
        };
        assert_eq!(full.rr3_pcm_duration_ms(), 150);
    }
}
