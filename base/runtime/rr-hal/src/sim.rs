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
    /// 多轮对话闭环（tools/sim --brain）的驱动源。门控按**绝对帧序号**
    /// （`idx/声道数/rate` 的偶秒有声——帧才是音频时间，交错样点序号会在
    /// 多通道下把窗口折半，批F4 真板实测教训），与 Sine 同一确定性纪律
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

/// 双麦方位模型的基线（米）：sim 阵列的虚拟麦距，量级取 USB 麦阵列常见值
/// （R14 定案 USB 阵列；真件换装 F5a 按实际阵列几何替换，sim 模型只求可复现）。
const RR3_SIM_MIC_BASELINE_M: f64 = 0.06;
/// 声速（m/s，20 °C 空气）：ITD（到达时间差）换算用
const RR3_SIM_SOUND_SPEED_MPS: f64 = 343.0;

struct Rr3SimOpen {
    cfg: Rr3AudioCfg,
    /// 已读出的**采样点**数（不是帧数）：正弦相位与噪声序号都按它推进
    cursor: u64,
}

/// sim 音频源（虚拟麦克风）。多通道 + [`Rr3SimDoa`] 时按双麦方位模型出各路增益与
/// 音频相位差（ILD/ITD），单声道链路逐点等价于无方位注入（golden 向量不动）。
pub struct Rr3SimAudioSource {
    wave: Rr3SimWave,
    doa: Option<Rr3SimDoa>,
    open: Mutex<Option<Rr3SimOpen>>,
}

/// 双麦方位注入（批F2 sim 先行，方案 §5.3 判向-转向编排 / R14）：声源方位角
/// `-90..=90`（负=左、0=正前、正=右）。物理模型：
/// - **ILD（幅度差）**：`g_l = 1 - sin θ`、`g_r = 1 + sin θ`——θ=0 两路同幅，
///   θ=±90° 单侧 2 倍另侧归零；sense 侧 DoA 按同一模型反演（可逆、确定性）；
/// - **ITD（相位差）**：有声路按基线时差 `Δt = d·sinθ/c` 做音调相位偏移——
///   噪声无分数延迟不可相移，只吃增益（粗判向走 ILD，ITD 是保真项不是判据）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rr3SimDoa {
    deg: f64,
}

impl Rr3SimDoa {
    /// 方位角构造：越界（|deg| > 90）不认——方位角域就是 ±90°，越界属调用方 bug。
    pub fn rr3_sim_doa_new(deg: f64) -> Option<Self> {
        (-90.0..=90.0)
            .contains(&deg)
            .then_some(Self { deg })
    }

    pub fn rr3_sim_doa_deg(&self) -> f64 {
        self.deg
    }

    /// `sin θ`（ILD/ITD 共用的方位系数，`k`）
    fn rr3_sim_doa_sin(&self) -> f64 {
        self.deg.to_radians().sin()
    }

    /// ITD（秒）：基线时差，有声路的音调相位按 ±Δt/2 偏移
    fn rr3_sim_doa_itd_s(&self) -> f64 {
        RR3_SIM_MIC_BASELINE_M * self.rr3_sim_doa_sin() / RR3_SIM_SOUND_SPEED_MPS
    }
}

impl Rr3SimAudioSource {
    pub fn rr3_sim_source_new(wave: Rr3SimWave) -> Self {
        Self {
            wave,
            doa: None,
            open: Mutex::new(None),
        }
    }

    /// 带方位注入的构造（`doa` 为 `None` 等价 [`Self::rr3_sim_source_new`]）
    pub fn rr3_sim_source_new_with_doa(wave: Rr3SimWave, doa: Option<Rr3SimDoa>) -> Self {
        Self {
            wave,
            doa,
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
        let ch_n = open.cfg.channels as usize;
        let n = frames * ch_n;
        // 每采样点的值一律由绝对序号（cursor + i）确定性派生：
        // 同一波形任何机器任何时刻逐点相等，golden 向量与 e2e 都钉在这一点上
        let rate = open.cfg.sample_rate_hz.max(1) as f64;
        // 方位模型参数（无注入或单声道时全 None——mono 链路没有方向语义，
        // 逐采样路径与旧实现逐点等价，golden 向量不因 DoA 注入而动）
        let (k, itd) = match self.doa.filter(|_| ch_n >= 2) {
            Some(d) => (Some(d.rr3_sim_doa_sin()), Some(d.rr3_sim_doa_itd_s())),
            None => (None, None),
        };
        let sine_at = |t_s: f64| -> f64 {
            RR3_SIM_SINE_AMP as f64
                * (2.0 * std::f64::consts::PI * RR3_SIM_SINE_HZ * t_s).sin()
        };
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                let idx = open.cursor + i as u64;
                let t = idx as f64 / rate;
                // 通道归属：交错排布，i % 声道数。方位注入只进前两路（L/R 对），
                // 更多路输出静音（真件换装 F5a 按阵列实际通道序重排，sim 不发明通道语义）
                let ch = i % ch_n;
                // 各路形状：增益（ILD）与音调相位偏移（ITD，仅音调波）
                let (gain, t_shift) = match (k, itd) {
                    (Some(k), itd) => {
                        let g = match ch {
                            0 => 1.0 - k,
                            1 => 1.0 + k,
                            _ => 0.0,
                        };
                        let dt = match (ch, itd) {
                            (0, Some(itd)) => -itd / 2.0,
                            (1, Some(itd)) => itd / 2.0,
                            _ => 0.0,
                        };
                        (g, dt)
                    }
                    _ => (1.0, 0.0),
                };
                let v = match self.wave {
                    Rr3SimWave::Sine => sine_at(t + t_shift),
                    Rr3SimWave::Burst => {
                        // 偶秒有声、奇秒静音：440Hz 的 2s 周期恰好整周期（880 圈），
                        // 相位跨门控边界连续，burst 的第 3 秒与第 1 秒逐点相等；
                        // 门控按绝对**帧**序号（idx/声道数，0.0.6 批F4 真板实测：按
                        // 交错样点序号算会在多通道下把窗口折半成 500ms，短于 VAD
                        // 判停尾静音 700ms，offset 永不触发）；有声路内才带 ITD 偏移
                        let frame_idx = idx / ch_n as u64;
                        if frame_idx / rate as u64 % 2 == 0 {
                            sine_at(t + t_shift)
                        } else {
                            0.0
                        }
                    }
                    Rr3SimWave::Noise => {
                        let mut x = RR3_SIM_NOISE_SEED ^ idx.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                        x ^= x >> 12;
                        x ^= x << 25;
                        x ^= x >> 27;
                        let v = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as i64;
                        (v % (RR3_SIM_NOISE_AMP as i64)) as f64
                    }
                    Rr3SimWave::Silence => 0.0,
                };
                (v * gain) as i16
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
        self.level.load(std::sync::atomic::Ordering::Relaxed)
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

/// sim 电池（虚拟电量计）：电量与充电态记录在内存，电压按电量**确定性线性派生**。
///
/// 分工口径与舵机/数字输出同一条：sim 是"设备"不是"策略"——低电阈值裁决属模块侧
/// （low_voltage_mv 来自 rr3-power config），低压关断属 rr3-power 本地决策与 M6，
/// HAL 只如实报数。电压映射钉死为 `3200mV + 10mV/1%`（0% → 3200、100% → 4200），
/// 与 golden 向量同一条确定性纪律：同一电量任何机器读出同一电压。
pub struct Rr3SimBattery {
    percent: std::sync::atomic::AtomicU8,
    charging: std::sync::atomic::AtomicBool,
}

impl Rr3SimBattery {
    pub fn rr3_sim_battery_new(percent: u8, charging: bool) -> Self {
        Self {
            percent: std::sync::atomic::AtomicU8::new(percent),
            charging: std::sync::atomic::AtomicBool::new(charging),
        }
    }

    /// 直接写设备状态（测试与 e2e 驱动用；trait 面只读，驱动必须走构造方持有的引用）
    pub fn rr3_sim_battery_set(&self, percent: u8, charging: bool) {
        self.percent
            .store(percent, std::sync::atomic::Ordering::Relaxed);
        self.charging
            .store(charging, std::sync::atomic::Ordering::Relaxed);
    }

    /// 同步读电量（测试与诊断用；trait 的 `battery` 是 async 面）
    pub fn rr3_sim_battery_percent(&self) -> u8 {
        self.percent.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn rr3_sim_battery_charging(&self) -> bool {
        self.charging.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 电量 → 电压（毫伏）的唯一映射。放关联函数而不是方法：它是"这颗虚拟电芯的
    /// 特性曲线"，不随实例状态变——e2e 断言与实现引用同一处，改曲线这里当场红。
    pub fn rr3_sim_battery_voltage_mv(percent: u8) -> u16 {
        3200 + percent as u16 * 10
    }
}

#[async_trait]
impl crate::Rr3Power for Rr3SimBattery {
    async fn battery(&self) -> Result<crate::Rr3BatteryState, Rr3HalError> {
        let percent = self.rr3_sim_battery_percent();
        Ok(crate::Rr3BatteryState {
            voltage_mv: Self::rr3_sim_battery_voltage_mv(percent),
            percent,
            charging: self.rr3_sim_battery_charging(),
        })
    }
}

/// sim 网络链路（虚拟 WiFi 状态源）：up / ssid / rssi 记录在内存。
///
/// 分工口径与舵机/电池同一条：sim 是"设备"不是"策略"——漫游/重连决策属模块侧
/// （rr3-connect），NetworkManager/D-Bus 属板上件（cfg 门骨架，0.0.2 方案 D7），
/// HAL 只如实报数。断链时 ssid/rssi 强制 `None`：真设备断链后 NM 侧拿不到这些
/// 读数，设备面如实表现，模块侧不许从"up=false 还带着 ssid"里发明语义。
pub struct Rr3SimLink {
    up: std::sync::atomic::AtomicBool,
    ssid: Mutex<Option<String>>,
    rssi_dbm: std::sync::atomic::AtomicI8,
}

impl Rr3SimLink {
    pub fn rr3_sim_link_new(up: bool, ssid: &str, rssi_dbm: i8) -> Self {
        Self {
            up: std::sync::atomic::AtomicBool::new(up),
            ssid: Mutex::new(Some(ssid.to_string())),
            rssi_dbm: std::sync::atomic::AtomicI8::new(rssi_dbm),
        }
    }

    /// 直接写设备状态（测试与 e2e 驱动用；trait 面只读，驱动必须走构造方持有的引用）
    pub fn rr3_sim_link_set(&self, up: bool, ssid: &str, rssi_dbm: i8) {
        self.up.store(up, std::sync::atomic::Ordering::Relaxed);
        *self.ssid.lock().unwrap_or_else(|p| p.into_inner()) = Some(ssid.to_string());
        self.rssi_dbm
            .store(rssi_dbm, std::sync::atomic::Ordering::Relaxed);
    }
}

#[async_trait]
impl crate::Rr3Link for Rr3SimLink {
    async fn state(&self) -> Result<crate::Rr3LinkState, Rr3HalError> {
        let up = self.up.load(std::sync::atomic::Ordering::Relaxed);
        let mut ssid = self.ssid.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut rssi = Some(self.rssi_dbm.load(std::sync::atomic::Ordering::Relaxed));
        if !up {
            ssid = None;
            rssi = None;
        }
        Ok(crate::Rr3LinkState {
            up,
            ssid,
            rssi_dbm: rssi,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rr3_block_on, Rr3GpioOut, Rr3Link, Rr3Power, Rr3PwmServo, RR3_AUDIO_CFG_16K_MONO16,
    };

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
        assert_eq!(
            rr3_block_on(s.angle()).unwrap(),
            200,
            "读回最后一次 set 的值"
        );
    }

    /// 方位角构造的值域门：±90 闭区间收，越界与 NaN 不认。
    #[test]
    fn sim_doa_ctor_accepts_only_pm90() {
        for deg in [-90.0, -60.0, 0.0, 45.0, 90.0] {
            let d = Rr3SimDoa::rr3_sim_doa_new(deg).unwrap_or_else(|| panic!("{deg} 应收"));
            assert_eq!(d.rr3_sim_doa_deg(), deg);
        }
        for bad in [-90.5, 90.1, 180.0, f64::NAN] {
            assert!(
                Rr3SimDoa::rr3_sim_doa_new(bad).is_none(),
                "{bad} 越域必须不认"
            );
        }
    }

    /// 双麦 ILD 模型的可逆性：按注入角读双通道帧，逐通道 RMS 反演出 sinθ，
    /// 必须与注入角自洽（sense 侧 DoA 的正门前提——注入与反演同一模型）。
    #[test]
    fn sim_doa_stereo_ild_matches_injected_direction() {
        for deg in [-90.0, -60.0, -30.0, 0.0, 30.0, 60.0, 90.0] {
            let s = Rr3SimAudioSource::rr3_sim_source_new_with_doa(
                Rr3SimWave::Sine,
                Rr3SimDoa::rr3_sim_doa_new(deg),
            );
            rr3_block_on(s.open(Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 2,
                bit_depth: 16,
            }))
            .unwrap();
            // 读 16000 帧（1 秒）：ITD 相移对 1s 整周期数平均后不扰 RMS 比值
            let pcm = rr3_block_on(s.read(16000)).unwrap();
            assert_eq!(pcm.samples.len(), 32000);
            let (mut e0, mut e1) = (0i64, 0i64);
            for f in pcm.samples.chunks_exact(2) {
                e0 += f[0] as i64 * f[0] as i64;
                e1 += f[1] as i64 * f[1] as i64;
            }
            // sinθ 的 RMS 反演 = (r_r - r_l)/(r_r + r_l)；注入模型 g_l=1-sinθ、g_r=1+sinθ
            let (r0, r1) = ((e0 as f64).sqrt(), (e1 as f64).sqrt());
            let k_est = (r1 - r0) / (r1 + r0);
            let k_want = deg.to_radians().sin();
            assert!(
                (k_est - k_want).abs() < 0.01,
                "deg={deg}: RMS 反演 k={k_est:.4} 必须贴合注入 sinθ={k_want:.4}"
            );
        }
    }

    /// 批F4 真板实测回归：burst 门控按**帧**序号——双通道下仍保证"1s 有声 +
    /// 1s 静音"的音频时间语义。交错样点序号口径会把窗口折半成 500ms，短于
    /// sense VAD 的 silence_tail_ms=700ms，offset 永不触发（--chain 演练
    /// 60s 只见一次 onset 的根因）。
    #[test]
    fn burst_gate_counts_frames_so_stereo_keeps_one_second_windows() {
        let s = Rr3SimAudioSource::rr3_sim_source_new_with_doa(
            Rr3SimWave::Burst,
            Rr3SimDoa::rr3_sim_doa_new(-60.0),
        );
        rr3_block_on(s.open(Rr3AudioCfg {
            sample_rate_hz: 16000,
            channels: 2,
            bit_depth: 16,
        }))
        .unwrap();
        // 第 1 秒（16000 帧）有声
        let loud = rr3_block_on(s.read(16000)).unwrap();
        assert_eq!(loud.samples.len(), 32000);
        assert!(loud.samples.iter().any(|&v| v != 0), "第 1 秒必须有声");
        // 第 2 秒全零（交错序号口径下这里是第 2 个"有声 500ms 窗"，修前必红）
        let silent = rr3_block_on(s.read(16000)).unwrap();
        assert!(silent.samples.iter().all(|&v| v == 0), "第 2 秒必须全静音");
        // 第 3 秒与第 1 秒逐点相等（440Hz 整周期 + 门控按帧，确定性）
        let again = rr3_block_on(s.read(16000)).unwrap();
        assert_eq!(again.samples, loud.samples, "burst 第 3 秒须复刻第 1 秒");
    }

    /// 确定性：同角两源读出逐点相等；单声道链路不受方位注入影响（逐点等价）。
    #[test]
    fn sim_doa_is_deterministic_and_mono_is_invariant() {
        let mk = || {
            Rr3SimAudioSource::rr3_sim_source_new_with_doa(
                Rr3SimWave::Sine,
                Rr3SimDoa::rr3_sim_doa_new(-60.0),
            )
        };
        let (a, b) = (mk(), mk());
        let cfg2 = Rr3AudioCfg {
            sample_rate_hz: 16000,
            channels: 2,
            bit_depth: 16,
        };
        rr3_block_on(a.open(cfg2)).unwrap();
        rr3_block_on(b.open(cfg2)).unwrap();
        assert_eq!(
            rr3_block_on(a.read(160)).unwrap().samples,
            rr3_block_on(b.read(160)).unwrap().samples,
            "同角同帧序逐点相等"
        );
        // 单声道：带注入与不带注入逐点相等（mono 无方向语义）
        let (m1, m2) = (mk(), Rr3SimAudioSource::rr3_sim_source_new(Rr3SimWave::Sine));
        let cfg1 = RR3_AUDIO_CFG_16K_MONO16;
        rr3_block_on(m1.open(cfg1)).unwrap();
        rr3_block_on(m2.open(cfg1)).unwrap();
        assert_eq!(
            rr3_block_on(m1.read(160)).unwrap().samples,
            rr3_block_on(m2.read(160)).unwrap().samples,
            "mono + DoA 注入必须逐点等价无注入"
        );
    }

    /// sim 数字输出：电平写读回环 + 覆写。LED/继电器各持一路，行为同一件。
    #[test]
    fn sim_gpio_level_roundtrip_and_overwrite() {
        let g = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        assert!(!g.rr3_sim_gpio_level(), "初始断电态");
        assert!(!rr3_block_on(g.level()).unwrap());
        rr3_block_on(g.set(true)).unwrap();
        assert!(g.rr3_sim_gpio_level());
        rr3_block_on(g.set(false)).unwrap();
        assert!(!rr3_block_on(g.level()).unwrap(), "覆写生效");
    }

    /// sim 电池：async 面（trait battery）读回构造/覆写后的三元组，电压曲线按
    /// 钉死的线性映射逐点核对——"设备只报数、策略不在这里"的分工同舵机那条。
    #[test]
    fn sim_battery_reads_percent_charging_and_derived_voltage() {
        let b = Rr3SimBattery::rr3_sim_battery_new(80, false);
        let st = rr3_block_on(b.battery()).unwrap();
        assert_eq!(
            (st.percent, st.charging, st.voltage_mv),
            (80, false, 4000),
            "初始 80% 未充电，电压 = 3200 + 80×10"
        );
        b.rr3_sim_battery_set(5, true);
        let st = rr3_block_on(b.battery()).unwrap();
        assert_eq!(
            (st.percent, st.charging, st.voltage_mv),
            (5, true, 3250),
            "覆写后读回同一真值"
        );
        // 特性曲线两端钉死：e2e 的低电断言（5% → 3250 ≤ low_voltage_mv=3300）靠它成立
        assert_eq!(Rr3SimBattery::rr3_sim_battery_voltage_mv(0), 3200);
        assert_eq!(Rr3SimBattery::rr3_sim_battery_voltage_mv(100), 4200);
    }

    /// sim 链路：async 面（trait state）读回构造/覆写后的三元组；断链时
    /// ssid/rssi 强制 None——设备面如实表现，模块侧不许从死链里读出网络信息。
    #[test]
    fn sim_link_reads_up_ssid_rssi_and_blanks_them_when_down() {
        let l = Rr3SimLink::rr3_sim_link_new(true, "rock-sim", -50);
        let st = rr3_block_on(l.state()).unwrap();
        assert_eq!(
            (st.up, st.ssid.as_deref(), st.rssi_dbm),
            (true, Some("rock-sim"), Some(-50)),
            "初始在线，三元组原样读回"
        );
        l.rr3_sim_link_set(false, "rock-sim", -90);
        let st = rr3_block_on(l.state()).unwrap();
        assert_eq!(
            (st.up, st.ssid.as_deref(), st.rssi_dbm),
            (false, None, None),
            "断链后 ssid/rssi 必须被设备面抹掉"
        );
        l.rr3_sim_link_set(true, "other-net", -40);
        let st = rr3_block_on(l.state()).unwrap();
        assert_eq!(
            (st.up, st.ssid.as_deref(), st.rssi_dbm),
            (true, Some("other-net"), Some(-40)),
            "覆写恢复在线后读回新真值"
        );
    }
}
