//! rr-hal —— 硬件抽象层 trait 定义（architecture.md §5）。
//!
//! trait 与数据类型 + sim / linux 两个音频后端与后端选择入口，见 `backend.rs`。
//! 慢速事件级接口优先：1kHz 以上闭环属模块进程内的反射环，永不进 HAL trait
//! （ADR-0007 裁定 1）。

use std::future::Future;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use async_trait::async_trait;

pub mod backend;
#[cfg(all(target_os = "linux", feature = "alsa"))]
pub mod linux;
pub mod sim;

/// 无操作 waker 的 vtable（`Waker::noop` 要 1.85，workspace 锁 1.82，故手写同语义版本）。
/// 四个函数全部 no-op：本 crate 的 future 全部首拍即完成，永不真正挂起等唤醒。
static RR3_NOOP_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |d| RawWaker::new(d, &RR3_NOOP_VTABLE), // clone
    |_| {},                                 // wake
    |_| {},                                 // wake_by_ref
    |_| {},                                 // drop
);

/// 模块侧的执行入口：同步代码驱动 async trait 方法。
///
/// 裁定（随 M3 接口定型）：v0.0.1 **不引 async 运行时**——本 crate 的全部 HAL 实现
/// （sim 的纯计算、linux 的阻塞 ALSA 读写）都在**首次 poll 内完成**，不依赖 waker
/// 唤醒，因此 noop-waker 的 `poll` 循环就是正确实现。将来若真出现会返回 `Pending`
/// 的 future，本函数会退化成自旋让步（能跑但烧 CPU）——那是接口违约，届时应换
/// 真运行时而不是修补这里。
pub fn rr3_block_on<F: Future>(fut: F) -> F::Output {
    let raw = RawWaker::new(std::ptr::null(), &RR3_NOOP_VTABLE);
    // SAFETY：vtable 的四个函数都只处理 null 数据指针，clone 返回同一 vtable，
    // 不解引用任何内存（同 `Waker::noop` 的语义，见上）。
    let waker = unsafe { Waker::from_raw(raw) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// HAL 采样配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3AudioCfg {
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub bit_depth: u8,
}

/// 与 legacy ESP32 链路一致的默认拾音格式（16k/单声道/16bit）
pub const RR3_AUDIO_CFG_16K_MONO16: Rr3AudioCfg = Rr3AudioCfg {
    sample_rate_hz: 16000,
    channels: 1,
    bit_depth: 16,
};

/// PCM 帧缓冲。所有权随返回值移交（M2 的 shm 零拷贝沿用同一语义）
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

/// 电池状态摘要（关断决策留在 rr3-power 本地，不经总线）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3BatteryState {
    pub voltage_mv: u16,
    pub percent: u8,
    pub charging: bool,
}

/// 链路状态摘要
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3LinkState {
    pub up: bool,
    pub ssid: Option<String>,
    pub rssi_dbm: Option<i8>,
}

/// HAL 错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3HalError {
    /// 后端未实现该能力（v0.0.1 全部走这条）
    Unsupported(&'static str),
    /// 设备忙 / 已被其他模块持有
    Busy,
    /// 参数越界
    InvalidParam(String),
}

impl std::fmt::Display for Rr3HalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(b) => write!(f, "HAL 后端不支持: {b}"),
            Self::Busy => write!(f, "HAL 设备忙"),
            Self::InvalidParam(s) => write!(f, "HAL 参数非法: {s}"),
        }
    }
}

/// 数字输出（继电器、LED）
#[async_trait]
pub trait Rr3GpioOut: Send + Sync {
    async fn set(&self, level: bool) -> Result<(), Rr3HalError>;
    async fn level(&self) -> Result<bool, Rr3HalError>;
}

/// PWM 舵机（角度级指令，闭环在 rr3-act 反射环内）
#[async_trait]
pub trait Rr3PwmServo: Send + Sync {
    async fn set_angle(&self, deg: u16) -> Result<(), Rr3HalError>;
    async fn angle(&self) -> Result<u16, Rr3HalError>;
}

/// 音频源（上行拾音）
#[async_trait]
pub trait Rr3AudioSource: Send + Sync {
    async fn open(&self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError>;
    async fn read(&self, frames: usize) -> Result<Rr3PcmStream, Rr3HalError>;
}

/// 音频播放面（下行出话）——M3 接口定型（architecture.md §5 裁定 2 的"同一次定型给形状"）。
///
/// 与 [`Rr3AudioSource`] 对称：open 固定采样配置，write 消费整段 [`Rr3PcmStream`]。
/// TTS 文本到 PCM 的合成属模块侧（rr3-interact），不进 HAL；HAL 只搬运已合成的波形。
#[async_trait]
pub trait Rr3AudioSink: Send + Sync {
    async fn open(&self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError>;
    async fn write(&self, pcm: Rr3PcmStream) -> Result<(), Rr3HalError>;
}

/// 电源读数
#[async_trait]
pub trait Rr3Power: Send + Sync {
    async fn battery(&self) -> Result<Rr3BatteryState, Rr3HalError>;
}

/// 网络链路读数
#[async_trait]
pub trait Rr3Link: Send + Sync {
    async fn state(&self) -> Result<Rr3LinkState, Rr3HalError>;
}

/// 占位后端：所有能力返回 `Unsupported`。它承载的是"能力缺失必须显式回错，不许静默退化"
/// 的设计裁定，不是一时代替后端的临时件。音频两路（source/sink）已由 `backend::sim` /
/// `backend::linux` 承担（M3）；gpio/pwm/power/link 仍由本占位回错，直到 M5 的三模块实装。
pub struct Rr3UnavailableSource;

#[async_trait]
impl Rr3GpioOut for Rr3UnavailableSource {
    async fn set(&self, _level: bool) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/gpio"))
    }
    async fn level(&self) -> Result<bool, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/gpio"))
    }
}

#[async_trait]
impl Rr3PwmServo for Rr3UnavailableSource {
    async fn set_angle(&self, _deg: u16) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/pwm"))
    }
    async fn angle(&self) -> Result<u16, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/pwm"))
    }
}

#[async_trait]
impl Rr3AudioSource for Rr3UnavailableSource {
    async fn open(&self, _cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
    async fn read(&self, _frames: usize) -> Result<Rr3PcmStream, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
}

#[async_trait]
impl Rr3AudioSink for Rr3UnavailableSource {
    async fn open(&self, _cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
    async fn write(&self, _pcm: Rr3PcmStream) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
}

#[async_trait]
impl Rr3Power for Rr3UnavailableSource {
    async fn battery(&self) -> Result<Rr3BatteryState, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/power"))
    }
}

#[async_trait]
impl Rr3Link for Rr3UnavailableSource {
    async fn state(&self) -> Result<Rr3LinkState, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/link"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct DummyGpio(AtomicBool);

    #[async_trait]
    impl Rr3GpioOut for DummyGpio {
        async fn set(&self, level: bool) -> Result<(), Rr3HalError> {
            self.0.store(level, Ordering::Relaxed);
            Ok(())
        }
        async fn level(&self) -> Result<bool, Rr3HalError> {
            Ok(self.0.load(Ordering::Relaxed))
        }
    }

    /// dyn 兼容校验：trait 对象必须能装箱/借用（M2 的注册表要持有异构设备集合）。
    /// 不调用 .await —— v0.0.1 不引入 async 运行时（tokio 属 M2 决策）。
    #[test]
    fn hal_traits_are_object_safe() {
        let boxed: Box<dyn Rr3GpioOut + Send + Sync> = Box::new(DummyGpio(AtomicBool::new(false)));
        let as_ref: &dyn Rr3GpioOut = &*boxed;
        let _ = as_ref;
        let _: Box<dyn Rr3AudioSource + Send + Sync> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3AudioSink + Send + Sync> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3Power + Send + Sync> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3Link + Send + Sync> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3PwmServo + Send + Sync> = Box::new(Rr3UnavailableSource);
        let _ = boxed;
    }

    #[test]
    fn audio_default_cfg_matches_legacy_pcm() {
        let cfg = RR3_AUDIO_CFG_16K_MONO16;
        assert_eq!(
            (cfg.sample_rate_hz, cfg.channels, cfg.bit_depth),
            (16000, 1, 16)
        );
    }

    #[test]
    fn hal_error_carries_backend_name() {
        let e = Rr3HalError::Unsupported("linux/iio");
        assert_eq!(e.to_string(), "HAL 后端不支持: linux/iio");
    }

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

    /// rr3_block_on 的契约：首拍即完成的 future 拿到值返回。sim 源的一条 read
    /// 走的就是这条路径（模块侧的真实用法）。
    #[test]
    fn block_on_drives_first_poll_completion() {
        let sink = sim::Rr3SimAudioSink::rr3_sim_sink_new(16000);
        rr3_block_on(sink.open(RR3_AUDIO_CFG_16K_MONO16)).expect("open 应成功");
        let pcm = Rr3PcmStream {
            cfg: RR3_AUDIO_CFG_16K_MONO16,
            samples: vec![3i16; 320],
        };
        rr3_block_on(sink.write(pcm)).expect("write 应成功");
        assert_eq!(sink.rr3_sim_sink_played(), vec![3i16; 320]);
    }
}
