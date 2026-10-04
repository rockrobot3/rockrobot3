//! sim 音频后端：主机联调与 CI 的虚拟麦克风 / 虚拟扬声器（architecture.md §5）。
//!
//! 兑现 `tools/sim` devices.rs "波形注入（正弦/噪声）" 的承诺：四模式
//! [`Rr3SimWave`]——`Sine`（440Hz、幅度 12000，必过 VAD 起音阈值）、`Noise`
//! （确定性 xorshift，幅度 ±8000）、`Silence`（全零）、`Burst`（1s 正弦 + 1s
//! 静音循环，VAD 周期性发 onset/offset，供多轮对话闭环联调）。两个后端都是
//! 纯计算、首拍即完成，是 [`crate::rr3_block_on`] noop-waker 裁定的直接受益者。
//!
//! 确定性是硬要求：同一 wave 同一帧序，任何机器任何时刻读出的 PCM 逐点相等——
//! golden 向量测试与 e2e 都钉在这一点上。

use std::sync::Mutex;

use async_trait::async_trait;

use crate::{Rr3AudioCfg, Rr3AudioSink, Rr3AudioSource, Rr3HalError, Rr3PcmStream};

/// sim 波形模式（环境变量 `RR3_HAL_SIM_WAVE` 的取值集，见 `backend.rs`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3SimWave {
    /// 440Hz 正弦、幅度 12000：能量远超 config.toml 的 `rms_start = 1500`，
    /// 专供 VAD 起音链路联调
    Sine,
    /// 确定性噪声（xorshift64* 固定种子）、幅度 ±8000
    Noise,
    /// 全零静音
    Silence,
    /// 1s 正弦 + 1s 静音循环：VAD 周期性发 onset/offset（≈2s 一轮），
    /// 多轮对话闭环（tools/sim --brain）的驱动源。门控按**绝对采样序号**算
    /// （`idx/rate` 的偶秒有声），与 Sine 同一确定性纪律
    Burst,
}

impl Rr3SimWave {
    /// 环境变量取值 → 波形（`sine` / `noise` / `silence` / `burst`，大小写不敏感）
    pub fn rr3_sim_wave_parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "sine" => Some(Self::Sine),
            "noise" => Some(Self::Noise),
            "silence" => Some(Self::Silence),
            "burst" => Some(Self::Burst),
            _ => None,
        }
    }

    pub fn rr3_sim_wave_as_str(&self) -> &'static str {
        match self {
            Self::Sine => "sine",
            Self::Noise => "noise",
            Self::Silence => "silence",
            Self::Burst => "burst",
        }
    }
}

const RR3_SIM_SINE_HZ: f64 = 440.0;
const RR3_SIM_SINE_AMP: i16 = 12000;
const RR3_SIM_NOISE_AMP: i16 = 8000;
/// 噪声种子（xorshift64*）：任何机器同一帧序同一波形，确定性是 golden 向量的前提
const RR3_SIM_NOISE_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

struct Rr3SimOpen {
    cfg: Rr3AudioCfg,
    /// 已读出的**采样点**数（不是帧数）：正弦相位与噪声序号都按它推进
    cursor: u64,
}

/// sim 音频源（虚拟麦克风）
pub struct Rr3SimAudioSource {
    wave: Rr3SimWave,
    open: Mutex<Option<Rr3SimOpen>>,
}

impl Rr3SimAudioSource {
    pub fn rr3_sim_source_new(wave: Rr3SimWave) -> Self {
        Self {
            wave,
            open: Mutex::new(None),
        }
    }

    pub fn rr3_sim_source_wave(&self) -> Rr3SimWave {
        self.wave
    }
}

#[async_trait]
impl Rr3AudioSource for Rr3SimAudioSource {
    async fn open(&self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        if cfg.sample_rate_hz == 0 || cfg.channels == 0 || cfg.bit_depth != 16 {
            return Err(Rr3HalError::InvalidParam(format!(
                "sim 源要求 16bit 采样且 rate/channels 非零，收到 {cfg:?}"
            )));
        }
        *self.open.lock().unwrap_or_else(|p| p.into_inner()) = Some(Rr3SimOpen { cfg, cursor: 0 });
        Ok(())
    }

    async fn read(&self, frames: usize) -> Result<Rr3PcmStream, Rr3HalError> {
        let mut guard = self.open.lock().unwrap_or_else(|p| p.into_inner());
        let Some(open) = guard.as_mut() else {
            return Err(Rr3HalError::InvalidParam("sim 源 read 前必须 open".into()));
        };
        let n = frames * open.cfg.channels as usize;
        // 每采样点的值一律由绝对序号（cursor + i）确定性派生：
        // 同一波形任何机器任何时刻逐点相等，golden 向量与 e2e 都钉在这一点上
        let rate = open.cfg.sample_rate_hz.max(1) as f64;
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                let idx = open.cursor + i as u64;
                let sine = |idx: u64| {
                    (RR3_SIM_SINE_AMP as f64
                        * (2.0 * std::f64::consts::PI * RR3_SIM_SINE_HZ * idx as f64 / rate).sin())
                        as i16
                };
                match self.wave {
                    Rr3SimWave::Sine => sine(idx),
                    Rr3SimWave::Burst => {
                        // 偶秒有声、奇秒静音：440Hz 的 2s 周期恰好整周期（880 圈），
                        // 相位跨门控边界连续，burst 的第 3 秒与第 1 秒逐点相等
                        if idx / rate as u64 % 2 == 0 {
                            sine(idx)
                        } else {
                            0
                        }
                    }
                    Rr3SimWave::Noise => {
                        let mut x = RR3_SIM_NOISE_SEED ^ idx.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                        x ^= x >> 12;
                        x ^= x << 25;
                        x ^= x >> 27;
                        let v = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as i64;
                        (v % (RR3_SIM_NOISE_AMP as i64)) as i16
                    }
                    Rr3SimWave::Silence => 0,
                }
            })
            .collect();
        open.cursor += n as u64;
        Ok(Rr3PcmStream {
            cfg: open.cfg,
            samples,
        })
    }
}

/// sim 音频播放面（虚拟扬声器）：write 进来的 PCM 落进有界环形尾窗，
/// 测试与 e2e 用 [`Rr3SimAudioSink::rr3_sim_sink_played`] 回读。
pub struct Rr3SimAudioSink {
    open: Mutex<Option<Rr3AudioCfg>>,
    played: Mutex<Vec<i16>>,
    /// 尾窗上限（采样点数）：e2e 长跑不吃内存
    cap: usize,
}

impl Rr3SimAudioSink {
    pub fn rr3_sim_sink_new(cap_samples: usize) -> Self {
        Self {
            open: Mutex::new(None),
            played: Mutex::new(Vec::new()),
            cap: cap_samples,
        }
    }

    /// 回读已"播放"的采样（尾窗内）
    pub fn rr3_sim_sink_played(&self) -> Vec<i16> {
        self.played
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn rr3_sim_sink_written_count(&self) -> usize {
        self.played.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

#[async_trait]
impl Rr3AudioSink for Rr3SimAudioSink {
    async fn open(&self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        if cfg.sample_rate_hz == 0 || cfg.channels == 0 || cfg.bit_depth != 16 {
            return Err(Rr3HalError::InvalidParam(format!(
                "sim 扬声器要求 16bit 采样且 rate/channels 非零，收到 {cfg:?}"
            )));
        }
        *self.open.lock().unwrap_or_else(|p| p.into_inner()) = Some(cfg);
        Ok(())
    }

    async fn write(&self, pcm: Rr3PcmStream) -> Result<(), Rr3HalError> {
        let open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        let Some(cfg) = open.as_ref() else {
            return Err(Rr3HalError::InvalidParam(
                "sim 扬声器 write 前必须 open".into(),
            ));
        };
        if pcm.cfg != *cfg {
            return Err(Rr3HalError::InvalidParam(format!(
                "sim 扬声器已按 {cfg:?} 打开，write 的配置是 {:?}",
                pcm.cfg
            )));
        }
        let mut played = self.played.lock().unwrap_or_else(|p| p.into_inner());
        played.extend_from_slice(&pcm.samples);
        let over = played.len().saturating_sub(self.cap);
        if over > 0 {
            played.drain(..over);
        }
        Ok(())
    }
}

/// sim 舵机（虚拟 PWM 舵机）：角度记录在内存，读回即最后一次 set 的值。
///
/// 分工口径与音频 sim 同一条：sim 是"设备"不是"策略"——行程越界裁决属模块侧
/// （angle_limit 来自模块 config），HAL 只如实记录。初始角 90°（中位）。
pub struct Rr3SimServo {
    angle: Mutex<u16>,
}

impl Rr3SimServo {
    pub fn rr3_sim_servo_new(initial_deg: u16) -> Self {
        Self {
            angle: Mutex::new(initial_deg),
        }
    }

    /// 同步读角（测试与诊断用；trait 的 `angle` 是 async 面）
    pub fn rr3_sim_servo_angle(&self) -> u16 {
        *self.angle.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[async_trait]
impl crate::Rr3PwmServo for Rr3SimServo {
    async fn set_angle(&self, deg: u16) -> Result<(), Rr3HalError> {
        *self.angle.lock().unwrap_or_else(|p| p.into_inner()) = deg;
        Ok(())
    }

    async fn angle(&self) -> Result<u16, Rr3HalError> {
        Ok(self.rr3_sim_servo_angle())
    }
}

/// sim 数字输出（虚拟 GPIO 通道）：电平记录在内存。LED 与继电器各持一路。
/// 初始电平由构造方给定（执行件一律从断电态起步）。
pub struct Rr3SimGpioOut {
    level: std::sync::atomic::AtomicBool,
}

impl Rr3SimGpioOut {
    pub fn rr3_sim_gpio_new(initial: bool) -> Self {
        Self {
            level: std::sync::atomic::AtomicBool::new(initial),
        }
    }

    /// 同步读电平（测试与诊断用；trait 的 `level` 是 async 面）
    pub fn rr3_sim_gpio_level(&self) -> bool {
        self.level
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[async_trait]
impl crate::Rr3GpioOut for Rr3SimGpioOut {
    async fn set(&self, level: bool) -> Result<(), Rr3HalError> {
        self.level
            .store(level, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    async fn level(&self) -> Result<bool, Rr3HalError> {
        Ok(self.rr3_sim_gpio_level())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rr3_block_on, Rr3GpioOut, Rr3PwmServo, RR3_AUDIO_CFG_16K_MONO16};

    fn source(wave: Rr3SimWave) -> Rr3SimAudioSource {
        Rr3SimAudioSource::rr3_sim_source_new(wave)
    }

    #[test]
    fn wave_parse_covers_three_modes_and_rejects_garbage() {
        for (s, want) in [
            ("sine", Rr3SimWave::Sine),
            ("SINE", Rr3SimWave::Sine),
            ("noise", Rr3SimWave::Noise),
            ("silence", Rr3SimWave::Silence),
            ("burst", Rr3SimWave::Burst),
        ] {
            assert_eq!(Rr3SimWave::rr3_sim_wave_parse(s), Some(want), "{s}");
            assert_eq!(want.rr3_sim_wave_as_str(), s.to_ascii_lowercase());
        }
        for bad in ["", "square", "sine ", "0"] {
            assert_eq!(Rr3SimWave::rr3_sim_wave_parse(bad), None, "{bad}");
        }
    }

    /// golden 向量：正弦前 8 点逐点相等（16k 采样率下 440Hz 的确定性开头）。
    /// 这批字面量同时钉住频率与幅度——改波形参数，这里当场红。
    #[test]
    fn sine_wave_matches_golden_prefix() {
        let s = source(Rr3SimWave::Sine);
        rr3_block_on(s.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let pcm = rr3_block_on(s.read(8)).unwrap();
        // golden 前缀：12000·sin(2π·440·i/16000) 的 i64 截断值（独立双精度实现算出，
        // 不是被测代码自己的输出回抄）——改频率/幅度/取整方式，这里当场红。
        let golden: [i16; 8] = [0, 2063, 4064, 5945, 7649, 9124, 10328, 11225];
        assert_eq!(&pcm.samples[..8], &golden, "正弦开头必须与 golden 逐点一致");
    }

    /// 相位跨 read 连续：两次 read 拼起来等于一次读双份
    #[test]
    fn sine_phase_is_continuous_across_reads() {
        let a = source(Rr3SimWave::Sine);
        rr3_block_on(a.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let half1 = rr3_block_on(a.read(160)).unwrap().samples;
        let half2 = rr3_block_on(a.read(160)).unwrap().samples;

        let b = source(Rr3SimWave::Sine);
        rr3_block_on(b.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let whole = rr3_block_on(b.read(320)).unwrap().samples;
        let mut joined = half1;
        joined.extend(half2);
        assert_eq!(joined, whole);
    }

    /// burst：偶秒正弦、奇秒静音；2s 周期恰为 440Hz 整周期数，第 3 秒与第 1 秒逐点相等
    #[test]
    fn burst_gates_sine_by_even_seconds_and_stays_deterministic() {
        let s = source(Rr3SimWave::Burst);
        rr3_block_on(s.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let pcm = rr3_block_on(s.read(3 * 16000)).unwrap().samples;
        // 第 1 秒有声（远超 VAD 起音阈值），第 2 秒全零，第 3 秒与第 1 秒逐点相等
        assert!(
            pcm[..16000].iter().any(|&v| v.abs() > 1500),
            "第 1 秒必过起音阈值"
        );
        assert!(
            pcm[16000..32000].iter().all(|&v| v == 0),
            "第 2 秒必须全静音"
        );
        assert_eq!(&pcm[32000..], &pcm[..16000], "第 3 秒复现第 1 秒");
        // 门控与正弦同源：第 1 秒头部与 Sine 波形的同序号采样逐点相等
        let sine = source(Rr3SimWave::Sine);
        rr3_block_on(sine.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let head = rr3_block_on(sine.read(16000)).unwrap().samples;
        assert_eq!(&pcm[..16000], &head[..], "burst 有声段 = Sine 同序号采样");
    }

    /// 噪声确定性：同波两源读出逐点相等，且幅度不越 ±8000
    #[test]
    fn noise_is_deterministic_and_bounded() {
        let a = source(Rr3SimWave::Noise);
        rr3_block_on(a.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let pa = rr3_block_on(a.read(512)).unwrap().samples;
        let b = source(Rr3SimWave::Noise);
        rr3_block_on(b.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let pb = rr3_block_on(b.read(512)).unwrap().samples;
        assert_eq!(pa, pb);
        assert!(pa.iter().all(|&v| v.abs() <= RR3_SIM_NOISE_AMP));
        // 不是正弦：存在相邻样本跳变超过 2000（440Hz 正弦 16k 下的最大步进约 1900）
        assert!(pa
            .windows(2)
            .any(|w| (w[0] as i32 - w[1] as i32).abs() > 2000));
    }

    #[test]
    fn silence_is_all_zero_and_read_before_open_is_invalid() {
        let s = source(Rr3SimWave::Silence);
        let e = rr3_block_on(s.read(10)).unwrap_err();
        assert!(matches!(e, Rr3HalError::InvalidParam(_)));
        rr3_block_on(s.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let pcm = rr3_block_on(s.read(64)).unwrap();
        assert!(pcm.samples.iter().all(|&v| v == 0));
        assert_eq!(pcm.rr3_pcm_duration_ms(), 4); // 64 帧 @16k
    }

    #[test]
    fn open_rejects_zero_rate_and_non_16bit() {
        for bad in [
            Rr3AudioCfg {
                sample_rate_hz: 0,
                channels: 1,
                bit_depth: 16,
            },
            Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 0,
                bit_depth: 16,
            },
            Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 1,
                bit_depth: 24,
            },
        ] {
            let e = rr3_block_on(source(Rr3SimWave::Sine).open(bad)).unwrap_err();
            assert!(matches!(e, Rr3HalError::InvalidParam(_)), "{bad:?}");
        }
    }

    #[test]
    fn sink_rejects_mismatched_cfg_and_caps_tail_window() {
        let sink = Rr3SimAudioSink::rr3_sim_sink_new(100);
        let e = rr3_block_on(sink.write(Rr3PcmStream {
            cfg: RR3_AUDIO_CFG_16K_MONO16,
            samples: vec![1],
        }))
        .unwrap_err();
        assert!(
            matches!(e, Rr3HalError::InvalidParam(_)),
            "write 前必须 open"
        );
        rr3_block_on(sink.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let other = Rr3AudioCfg {
            sample_rate_hz: 48000,
            channels: 1,
            bit_depth: 16,
        };
        let e = rr3_block_on(sink.write(Rr3PcmStream {
            cfg: other,
            samples: vec![1],
        }))
        .unwrap_err();
        assert!(matches!(e, Rr3HalError::InvalidParam(_)), "配置不符必须拒");
        for chunk in 0..7u8 {
            rr3_block_on(sink.write(Rr3PcmStream {
                cfg: RR3_AUDIO_CFG_16K_MONO16,
                samples: vec![chunk as i16; 40],
            }))
            .unwrap();
        }
        let played = sink.rr3_sim_sink_played();
        assert_eq!(played.len(), 100, "尾窗封顶");
        // 7×40=280 采样进 cap=100 的尾窗：最旧的 180 个被挤出，
        // 窗内 = chunk4 的后 20 个 + chunk5 整块 + chunk6 整块
        assert_eq!(&played[..20], &[4i16; 20], "最旧部分被挤出");
        assert_eq!(&played[20..60], &[5i16; 40]);
        assert_eq!(&played[60..], &[6i16; 40], "最新一块保留");
        assert_eq!(sink.rr3_sim_sink_written_count(), 100);
    }

    /// sim 舵机：初始角即构造值，set 后读回最后一次的值——设备面只记录不裁决，
    /// 越界裁决属模块侧（angle_limit 在模块 config），这条用例钉住分工边界。
    #[test]
    fn sim_servo_records_last_set_angle_without_judging() {
        let s = Rr3SimServo::rr3_sim_servo_new(90);
        assert_eq!(s.rr3_sim_servo_angle(), 90, "初始角 = 中位 90°");
        assert_eq!(
            rr3_block_on(s.set_angle(135)).unwrap(),
            (),
            "set_angle 如实接受任何 u16——裁决不在这里"
        );
        assert_eq!(s.rr3_sim_servo_angle(), 135);
        rr3_block_on(s.set_angle(0)).unwrap();
        rr3_block_on(s.set_angle(200)).unwrap();
        assert_eq!(rr3_block_on(s.angle()).unwrap(), 200, "读回最后一次 set 的值");
    }

    /// sim 数字输出：电平写读回环 + 覆写。LED/继电器各持一路，行为同一件。
    #[test]
    fn sim_gpio_level_roundtrip_and_overwrite() {
        let g = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        assert!(!g.rr3_sim_gpio_level(), "初始断电态");
        assert_eq!(rr3_block_on(g.level()).unwrap(), false);
        rr3_block_on(g.set(true)).unwrap();
        assert!(g.rr3_sim_gpio_level());
        rr3_block_on(g.set(false)).unwrap();
        assert_eq!(rr3_block_on(g.level()).unwrap(), false, "覆写生效");
    }
}
