//! sim 音频源（虚拟麦克风）：波形生成与双麦方位注入。

use std::sync::Mutex;

use crate::{Rr3AudioCfg, Rr3AudioSource, Rr3ErrText, Rr3HalError};

use super::parse::Rr3SimWave;

const RR3_SIM_SINE_HZ: f64 = 440.0;
const RR3_SIM_SINE_AMP: i16 = 12000;
pub(super) const RR3_SIM_NOISE_AMP: i16 = 8000;
/// 噪声种子（xorshift64*）：任何机器同一帧序同一波形，确定性是 golden 向量的前提
const RR3_SIM_NOISE_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// 双麦方位模型的基线（米）：sim 阵列的虚拟麦距。**与 sense 判向反演基线同源**
/// （B批 b3b TDOA 起"注入与反演同一模型"是 sim 正门前提——ILD 时代基线不进反演，
/// 0.06 的 USB 阵列量级无人消费；TDOA 按时延反演，sim 基线与 sense 随版缺省
/// `mic_baseline_mm=150` 必须同值，改任一侧必须同批改另一侧，判据面＝
/// `e2e_audio_chain::audio_chain_doa_stereo_env_injection_end_to_end` 的 ±2° 容差）。
const RR3_SIM_MIC_BASELINE_M: f64 = 0.150;
/// 声速（m/s，20 °C 空气）：ITD（到达时间差）换算用
const RR3_SIM_SOUND_SPEED_MPS: f64 = 343.0;

struct Rr3SimOpen {
    cfg: Rr3AudioCfg,
    /// 已读出的**采样点**数（不是帧数）：正弦相位与噪声序号都按它推进
    cursor: u64,
    /// 波形取值器（open 时解析一次；read 热路径不做波形查表）
    wave: fn(f64, u64, usize, f64) -> f64,
    /// 每通道 (增益, 相位偏移)，长度 = channels（方位参数 open 时一次算好）
    chans: Vec<(f64, f64)>,
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
/// - **ILD（幅度差）**：`g_r = 1 + sin θ`、`g_l = 1 - sin θ`——θ=0 两路同幅，
///   θ=±90° 单侧 2 倍另侧归零；sense 侧 DoA 按同一模型反演（可逆、确定性）；
/// - **ITD（相位差）**：双路按基线时差 `Δt = d·sinθ/c` 做音调相位偏移（B批 b3b
///   TDOA 起它是判据不是保真项：sense 互相关按时延反演，ITD 量级由
///   [`RR3_SIM_MIC_BASELINE_M`] 与反演基线同源保证）。
///
/// 通道序对齐板端口径（B批 b3 定案：frame[0] = 物理右、frame[1] = 物理左）——
/// 本源 ch0 输出物理右、ch1 物理左，θ>0（右）时 ch0 近且先到（增益高、相位超前），
/// sense 侧 `lag>0 = 右` 同号直通，注入角即反演角。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rr3SimDoa {
    deg: f64,
}

impl Rr3SimDoa {
    /// 方位角构造：越界（|deg| > 90）不认——方位角域就是 ±90°，越界属调用方 bug。
    pub fn rr3_sim_doa_new(deg: f64) -> Option<Self> {
        (-90.0..=90.0).contains(&deg).then_some(Self { deg })
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
    pub fn rr3_sim_source_doa(wave: Rr3SimWave, doa: Option<Rr3SimDoa>) -> Self {
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

impl Rr3AudioSource for Rr3SimAudioSource {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        if ![
            cfg.sample_rate_hz != 0,
            cfg.channels != 0,
            cfg.bit_depth == 16,
        ]
        .iter()
        .all(|ok| *ok)
        {
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                &format!("sim 源要求 16bit 采样且 rate/channels 非零，收到 {cfg:?}"),
            )));
        }
        *self.open.lock().unwrap_or_else(|p| p.into_inner()) = Some(Rr3SimOpen {
            cfg,
            cursor: 0,
            wave: rr3_sim_wave_fn(self.wave),
            chans: rr3_sim_chans(cfg.channels as usize, self.doa),
        });
        Ok(())
    }

    fn read(&mut self, buf: &mut [i16]) -> Result<usize, Rr3HalError> {
        let mut guard = self.open.lock().unwrap_or_else(|p| p.into_inner());
        let Some(open) = guard.as_mut() else {
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                "sim 源 read 前必须 open",
            )));
        };
        let n = buf.len();
        // 每采样点的值一律由绝对序号（cursor + i）确定性派生：
        // 同一波形任何机器任何时刻逐点相等，golden 向量与 e2e 都钉在这一点上
        let rate = open.cfg.sample_rate_hz.max(1) as f64;
        let cursor = open.cursor;
        rr3_sim_fill(open.wave, &open.chans, buf, cursor, rate);
        open.cursor += n as u64;
        Ok(n)
    }
}

/// 逐通道 (增益, 相位偏移)：方位参数在此一次解好，read 热路径不再按采样点算。
fn rr3_sim_chans(ch_n: usize, doa: Option<Rr3SimDoa>) -> Vec<(f64, f64)> {
    let (k, itd) = rr3_sim_doa_args(doa, ch_n);
    (0..ch_n).map(|ch| rr3_sim_gain(ch, k, itd)).collect()
}

/// 方位模型参数（无注入或单声道时全 None——mono 链路没有方向语义）。
fn rr3_sim_doa_args(doa: Option<Rr3SimDoa>, ch_n: usize) -> (Option<f64>, Option<f64>) {
    match doa.filter(|_| ch_n >= 2) {
        Some(d) => (Some(d.rr3_sim_doa_sin()), Some(d.rr3_sim_doa_itd_s())),
        None => (None, None),
    }
}

/// 按绝对序号（cursor + i）逐点确定性填充输出缓冲。
///
/// 波形器与逐通道 (增益, 相位) 都由 `open` 预解传入——采集是持续热路径，逐采样
/// 点不做波形查表/参数分派；取值路径与旧实现逐点等价（golden 向量守护）。
fn rr3_sim_fill(
    wave: fn(f64, u64, usize, f64) -> f64,
    chans: &[(f64, f64)],
    buf: &mut [i16],
    cursor: u64,
    rate: f64,
) {
    let ch_n = chans.len();
    for (i, out) in buf.iter_mut().enumerate() {
        let idx = cursor + i as u64;
        // 波形时间轴按**帧序**（idx/ch_n）：每通道按自己的采样时刻取值——B批
        // b3b 修复：旧写法 t = idx/rate 是交错样点轴，双通道下每通道波形时间被
        // 拉伸 2 倍（16k 轴上 440Hz 实际出 880Hz），ITD 注入的互相关出"伪峰与
        // 物理峰并列"双峰。mono 路径 idx/1 = idx 逐点等价，golden 向量不动。
        let t = (idx / ch_n as u64) as f64 / rate;
        let (gain, shift) = chans[i % ch_n];
        *out = (wave(t + shift, idx, ch_n, rate) * gain) as i16;
    }
}

/// 正弦样本（单点振幅/频率参数，形波与 burst 共用）。
fn rr3_sim_sine(t_s: f64) -> f64 {
    RR3_SIM_SINE_AMP as f64 * (2.0 * std::f64::consts::PI * RR3_SIM_SINE_HZ * t_s).sin()
}

/// 波形 → 取值函数（open 时解析一次；热路径只调函数指针，不查表）。
fn rr3_sim_wave_fn(wave: Rr3SimWave) -> fn(f64, u64, usize, f64) -> f64 {
    match wave {
        Rr3SimWave::Sine => |t, _, _, _| rr3_sim_sine(t),
        Rr3SimWave::Burst => rr3_sim_burst,
        Rr3SimWave::Noise => |_, idx, _, _| rr3_sim_noise(idx),
        Rr3SimWave::Silence => |_, _, _, _| 0.0,
    }
}

/// burst 门控：偶秒有声、奇秒静音，按绝对**帧**序号（idx/声道数）判。
fn rr3_sim_burst(t: f64, idx: u64, ch_n: usize, rate: f64) -> f64 {
    // 偶秒有声、奇秒静音：440Hz 的 2s 周期恰好整周期（880 圈），
    // 相位跨门控边界连续，burst 的第 3 秒与第 1 秒逐点相等；
    // 门控按绝对**帧**序号（idx/声道数，0.0.6 批F4 真板实测：按
    // 交错样点序号算会在多通道下把窗口折半成 500ms，短于 VAD
    // 判停尾静音 700ms，offset 永不触发）；有声路内才带 ITD 偏移
    let frame_idx = idx / ch_n as u64;
    if frame_idx / rate as u64 % 2 == 0 {
        rr3_sim_sine(t)
    } else {
        0.0
    }
}

/// 确定性噪声（xorshift）：同序号的点在任何机器任何时刻逐点相等。
fn rr3_sim_noise(idx: u64) -> f64 {
    let mut x = RR3_SIM_NOISE_SEED ^ idx.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    let v = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as i64;
    (v % (RR3_SIM_NOISE_AMP as i64)) as f64
}

/// 通道方位模型：增益（ILD）与音调相位偏移（ITD，仅音调波）。方位注入只进
/// 前两路（L/R 对），更多路输出静音（真件换装 F5a 按阵列实际通道序重排，
/// sim 不发明通道语义）。通道序对齐板端口径（ch0=物理右、ch1=物理左）。
fn rr3_sim_gain(ch: usize, k: Option<f64>, itd: Option<f64>) -> (f64, f64) {
    match (k, itd) {
        (Some(k), itd) => rr3_sim_gain_ch(ch, k, itd),
        _ => (1.0, 0.0),
    }
}

/// 单通道增益与相位：只有前两路（L/R 对）承载方位，其余静音。
fn rr3_sim_gain_ch(ch: usize, k: f64, itd: Option<f64>) -> (f64, f64) {
    match (ch, itd) {
        // θ>0（右）时 ch0 近——增益高（1+sinθ）、先到
        (0, Some(itd)) => (1.0 + k, itd / 2.0),
        (1, Some(itd)) => (1.0 - k, -itd / 2.0),
        _ => (0.0, 0.0),
    }
}

// ---------- 占位 TTS 波形（0.0.5 批I I-10 自 rr3-interact player 迁入） ----------
// player 的 AMP/HZ 本是"只借 sim 源参数"的复制品（件内 f64 sin 违反件面
// 数学面纪律）；波形本体归 sim 后端后，参数与下方 RR3_SIM_SINE_HZ /
// RR3_SIM_SINE_AMP（sim 源参数）同库同值一处管理，件面（player）零 std
// 数学、零分配。

/// 占位 TTS 波形查表（32 项 + 首点回绕成 33 项）：`LUT[i] = round(12000·sin(2πi/32))`，
/// 幅度/频率与 sim 源同参数（440Hz / 12000，见 `RR3_SIM_SINE_HZ`/`RR3_SIM_SINE_AMP`）。
const RR3_TTS_LUT: [i16; 33] = [
    0, 2341, 4592, 6667, 8485, 9978, 11087, 11769, 12000, 11769, 11087, 9978, 8485, 6667, 4592,
    2341, 0, -2341, -4592, -6667, -8485, -9978, -11087, -11769, -12000, -11769, -11087, -9978,
    -8485, -6667, -4592, -2341, 0,
];

/// 每采样相位步进（Q20 定点）：440Hz × 32 格 × 2²⁰ / 16000 采样率 = 922747。
/// Q16 的步进量化漂移在 10s 口播尾段可达 ±1800（陡坡段相位误差被斜率放大），
/// Q20 全程与双精度公式偏差 ≤100（幅度 12000 的 0.8%，听感等价预算，单测钉死）。
const RR3_TTS_PHASE_STEP_Q20: u64 = 922_747;

/// 填一段占位 TTS 波形（440Hz 正弦、幅度 12000；32 项查表 + 线性插值整型合成）：
/// `buf` 从波形第 `start` 个采样起连续填充，块间相位连续由调用方传全局游标保证。
/// u64 相位累乘在 10s 口播上界（160_000 样点）内无溢出。
pub fn rr3_sim_tts_block(buf: &mut [i16], start: usize) {
    const PERIOD_Q20: u64 = 32u64 << 20;
    for (k, slot) in buf.iter_mut().enumerate() {
        let acc = ((start + k) as u64 * RR3_TTS_PHASE_STEP_Q20) % PERIOD_Q20;
        let idx = (acc >> 20) as usize; // 0..=31
        let frac = (acc & ((1 << 20) - 1)) as i32;
        let a = RR3_TTS_LUT[idx] as i32;
        let b = RR3_TTS_LUT[idx + 1] as i32;
        // 插值中间量升 i64：(b-a) 最大 ±24000 乘 frac 最大 2²⁰-1 ≈ 2.5e10，
        // i32 装不下（debug 溢出 panic）；i64 中间量后收 i16 无损
        *slot = (a + ((b - a) as i64 * frac as i64 / (1i64 << 20)) as i32) as i16;
    }
}
