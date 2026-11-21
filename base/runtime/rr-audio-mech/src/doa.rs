//! DoA 粗判向与多通道下混（自 `rr3-sense` capture 原子件抽出，W1-2）。
//!
//! 双麦**到达时差（TDOA）**判向：判向窗上做 plain 时域互相关
//! `cc[d] = Σ_t R[t]·L[t+d]`，lag>0 = R 先到 = 声源在右。通道映射定案：
//! 板档 frame[0] = 物理右麦（R）、frame[1] = 物理左麦（L）——板麦接线/I2S
//! 通道序是装配事实，以板上实测锚定。
//!
//! 搜索域钉死物理可达范围 ±lag_max（lag_max = round(sr·基线/声速)，15cm 基线
//! @16k = ±7 样本）。为什么用 plain 时域而不是 FFT/PHAT：离线对拍与限带 FFT
//! 版同号同档，时域版零浮点零滤波器，golden 纪律最短路径。
//!
//! 整型化（golden 纪律，任何机器逐位一致）：
//! ① 互相关 i64 累加（窗 16000 帧 × 32767² ≈ 1.7e13，i64 安全）；
//! ② 亚样本抛物线定标 Q7：sub = 32·(y0−y2)/(2y1−y0−y2)，clamp [−64, 64]；
//! ③ lag_Q7 × K_Q15 >> 7 反演 sinθ，经 256 格 asin 查表出整度，分档口径不变。
//!
//! 能量门：窗内任一采集帧（L/R 任一通道）rms ≥ `window_rms_min` 才判，防全静音
//! 窗零比零。

use alloc::vec;
use alloc::vec::Vec;

// ---------- 帧节拍换算 ----------

/// 一次采集节拍读多少帧：`rate × frame_ms / 1000`（16000×20 = 320 帧）。
pub fn rr3_frames_per_tick(sample_rate_hz: u32, frame_ms: u64) -> usize {
    (sample_rate_hz as u64 * frame_ms / 1000) as usize
}

// ---------- 多通道下混 ----------

/// 多通道 PCM → 单声道：**选能量最高的通道**（不取平均）。2026-10-31 实机取证：
/// 双麦 150mm 间距，原始 L/R 零延迟相关仅 +0.2，平均下混 RMS 比单声道低 2–3dB
/// 且带梳状滤波；单取能量高的通道识别明显更好。DoA 仍吃原始双通道。
/// `channels <= 1` 时原样克隆；尾部不足一帧的余数丢弃。
pub fn rr3_pcm_best_channel(samples: &[i16], channels: u8) -> Vec<i16> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let frames = samples.len() / channels as usize;
    if frames == 0 {
        return Vec::new();
    }
    let mut out = vec![0i16; frames];
    let sel = rr3_chan_pick(samples, channels);
    rr3_chan_take(samples, channels, sel, &mut out);
    out
}

/// 能量最高的通道号（并列取最小通道号）；**零分配**，供流式路径逐块定声道。
/// `channels == 0` 按单声道处理；空输入回 0。
pub fn rr3_chan_pick(samples: &[i16], channels: u8) -> usize {
    let ch = (channels as usize).max(1);
    let frames = samples.len() / ch;
    let mut best = 0usize;
    let mut best_e = 0i64;
    for c in 0..ch {
        let e = rr3_chan_energy(samples, ch, frames, c);
        if e > best_e {
            best = c;
            best_e = e;
        }
    }
    best
}

/// 单通道整数能量（`sum_sq`，免开方；i64 覆盖 8s×16kHz 满幅）。
fn rr3_chan_energy(samples: &[i16], ch: usize, frames: usize, c: usize) -> i64 {
    let mut sum = 0i64;
    for i in 0..frames {
        let v = samples[i * ch + c] as i64;
        sum += v * v;
    }
    sum
}

/// 按选中通道抽单声道写入 `out`，返回写入样本数（≤ 帧数，尾部余数丢弃）。
/// **零分配**：`out` 短于帧数时截断，通道号越界回 0——都不 panic。
pub fn rr3_chan_take(samples: &[i16], channels: u8, sel: usize, out: &mut [i16]) -> usize {
    let ch = (channels as usize).max(1);
    if sel >= ch {
        return 0;
    }
    let frames = (samples.len() / ch).min(out.len());
    for (i, o) in out.iter_mut().take(frames).enumerate() {
        *o = samples[i * ch + sel];
    }
    frames
}

// ---------- DoA 配置与结果 ----------

/// DoA 判向窗配置形状：TDOA 机制面所需的全部策略量一次注入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3DoaCfg {
    pub sample_rate_hz: u32,
    pub frame_ms: u64,
    pub mic_baseline_mm: u32,
    pub window_rms_min: i64,
    pub bin_threshold_deg: i32,
}

/// DoA 分档（±90° 值域的三档离散）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3DoaBin {
    /// 声源在左（θ < -threshold_deg）
    Left,
    /// 声源在正前附近（-threshold_deg ≤ θ ≤ threshold_deg）
    Center,
    /// 声源在右（θ > threshold_deg）
    Right,
}

impl Rr3DoaBin {
    pub fn rr3_doa_bin_as_str(&self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

/// 一次判向结果：连续角（度，-90..=90，负=左）+ 三档离散档位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3DoaFix {
    pub deg: i32,
    pub bin: Rr3DoaBin,
}

/// 声速（mm/s，20°C 空气 ~343 m/s）：机制面物理常量，换环境校准随换装批。
const RR3_SOUND_MM_PER_S: i64 = 343_000;

/// 256 格 asin 查表（度）：`RR3_ASIN_DEG_LUT[i] = round(asin((i − 128) / 128))`，
/// i ∈ [0, 256] 覆盖 sinθ ∈ [−1, 1] 全值域。生成式一次性钉死，运行期零浮点。
const RR3_ASIN_DEG_LUT: [i32; 257] = [
    -90, -83, -80, -78, -76, -74, -72, -71, -70, -68, -67, -66, -65, -64, -63, -62, -61, -60, -59,
    -58, -58, -57, -56, -55, -54, -54, -53, -52, -51, -51, -50, -49, -49, -48, -47, -47, -46, -45,
    -45, -44, -43, -43, -42, -42, -41, -40, -40, -39, -39, -38, -38, -37, -36, -36, -35, -35, -34,
    -34, -33, -33, -32, -32, -31, -31, -30, -29, -29, -28, -28, -27, -27, -26, -26, -25, -25, -24,
    -24, -23, -23, -23, -22, -22, -21, -21, -20, -20, -19, -19, -18, -18, -17, -17, -16, -16, -15,
    -15, -14, -14, -14, -13, -13, -12, -12, -11, -11, -10, -10, -9, -9, -9, -8, -8, -7, -7, -6, -6,
    -5, -5, -4, -4, -4, -3, -3, -2, -2, -1, -1, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7,
    7, 8, 8, 9, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 14, 15, 15, 16, 16, 17, 17, 18, 18,
    19, 19, 20, 20, 21, 21, 22, 22, 23, 23, 23, 24, 24, 25, 25, 26, 26, 27, 27, 28, 28, 29, 29, 30,
    31, 31, 32, 32, 33, 33, 34, 34, 35, 35, 36, 36, 37, 38, 38, 39, 39, 40, 40, 41, 42, 42, 43, 43,
    44, 45, 45, 46, 47, 47, 48, 49, 49, 50, 51, 51, 52, 53, 54, 54, 55, 56, 57, 58, 58, 59, 60, 61,
    62, 63, 64, 65, 66, 67, 68, 70, 71, 72, 74, 76, 78, 80, 83, 90,
];

/// DoA 分档：`|θ| ≤ threshold_deg` 落中档，否则左/右档。
fn rr3_doa_bin_of(deg: i32, threshold_deg: i32) -> Rr3DoaBin {
    if deg < -threshold_deg {
        Rr3DoaBin::Left
    } else if deg > threshold_deg {
        Rr3DoaBin::Right
    } else {
        Rr3DoaBin::Center
    }
}

/// 判向窗 TDOA 判向。返回 `None` 的情形：单声道（channels < 2）/ 空窗 /
/// 基线为 0（无几何可换算）/ 能量门不过（全静音或欠能量窗）。
/// `samples` = 判向窗快照（帧交错：帧内 ch 通道，R=0/L=1）。
pub fn rr3_doa_fix(samples: &[i16], channels: u8, cfg: &Rr3DoaCfg) -> Option<Rr3DoaFix> {
    let (ch, frames) = rr3_doa_gate(samples, channels, cfg)?;
    // 物理 lag 上限：round(sr·基线/声速)——15cm @16k = round(6.997) = 7
    let sr_mm = cfg.sample_rate_hz as i64 * cfg.mic_baseline_mm as i64;
    let lag_max = ((2 * sr_mm + RR3_SOUND_MM_PER_S) / (2 * RR3_SOUND_MM_PER_S)) as usize;
    let half = lag_max + 1;
    let cc = rr3_doa_cc(samples, ch, frames, half);
    let lag_q7 = rr3_doa_peak(&cc, lag_max, half);
    // sinθ = lag·声速/(sr·基线)：K_Q15 定标（Q7 lag → Q15 sinθ），查表出整度
    let k_q15 = RR3_SOUND_MM_PER_S * 32768 / sr_mm;
    let sin_q15 = ((lag_q7 * k_q15) >> 7).clamp(-32768, 32767) as i32;
    let idx = (((sin_q15 + 32768) * 256 + 32768) >> 16) as usize;
    let deg = RR3_ASIN_DEG_LUT[idx];
    let bin = rr3_doa_bin_of(deg, cfg.bin_threshold_deg);
    Some(Rr3DoaFix { deg, bin })
}

/// 形状门 + 能量门：通道/基线/帧数合法且至少一采集帧 rms ≥ 阈值才继续。
fn rr3_doa_gate(samples: &[i16], channels: u8, cfg: &Rr3DoaCfg) -> Option<(usize, usize)> {
    let ch = rr3_doa_shape(channels, cfg)?;
    let frames = samples.len() / ch;
    if frames == 0 {
        return None;
    }
    if !rr3_doa_loud(samples, ch, cfg) {
        return None;
    }
    Some((ch, frames))
}

/// 形状门：通道数与基线合法才继续（单声道/零基线无方向可判）。
fn rr3_doa_shape(channels: u8, cfg: &Rr3DoaCfg) -> Option<usize> {
    if channels < 2 || cfg.mic_baseline_mm == 0 {
        return None;
    }
    Some(channels as usize)
}

/// 能量门（门内任一采集帧 rms 过阈即过；阈值²×帧长定标比较，免 isqrt）。
fn rr3_doa_loud(samples: &[i16], ch: usize, cfg: &Rr3DoaCfg) -> bool {
    let frame_len = ((cfg.sample_rate_hz as u64 * cfg.frame_ms) / 1000).max(1) as usize;
    let min = cfg.window_rms_min.max(0) as u64;
    if min == 0 {
        return true;
    }
    let gate_sq = min * min * frame_len as u64;
    rr3_any_loud(samples, ch, frame_len, gate_sq)
}

/// 门内任一采集帧 rms 过阈即过。
fn rr3_any_loud(samples: &[i16], ch: usize, frame_len: usize, gate_sq: u64) -> bool {
    samples
        .chunks_exact(ch * frame_len)
        .any(|fr| rr3_doa_frame_loud(fr, ch, gate_sq))
}

/// 单采集帧的 L/R 能量门（i16 取绝对值后平方累加）。
fn rr3_doa_frame_loud(fr: &[i16], ch: usize, gate_sq: u64) -> bool {
    let (sq_l, sq_r) = rr3_sq_pair(fr, ch);
    sq_l >= gate_sq || sq_r >= gate_sq
}

/// 单采集帧的 L/R 平方和（`(左, 右)`）。
fn rr3_sq_pair(fr: &[i16], ch: usize) -> (u64, u64) {
    let (mut sq_l, mut sq_r) = (0u64, 0u64);
    let mut i = 0;
    while i + 1 < fr.len() {
        let v = (fr[i] as i64).unsigned_abs();
        sq_l += v * v;
        let w = (fr[i + 1] as i64).unsigned_abs();
        sq_r += w * w;
        i += ch;
    }
    (sq_l, sq_r)
}

/// 互相关积分：cc[d] = Σ_t R[t]·L[t+d]，d ∈ [−(lag_max+1), lag_max+1]。
fn rr3_doa_cc(samples: &[i16], ch: usize, frames: usize, half: usize) -> Vec<i64> {
    let mut cc = vec![0i64; 2 * half + 1];
    for (i, slot) in cc.iter_mut().enumerate() {
        *slot = rr3_doa_lag(samples, ch, frames, i as i64 - half as i64);
    }
    cc
}

/// 单个 lag 的互相关：`Σ_t R[t]·L[t+d]`（d 正负两向；d 越出帧数即 0）。
fn rr3_doa_lag(samples: &[i16], ch: usize, frames: usize, d: i64) -> i64 {
    if d >= 0 {
        return rr3_doa_lag_fwd(samples, ch, frames, d as usize);
    }
    rr3_doa_lag_back(samples, ch, frames, (-d) as usize)
}

/// 反向 lag 的互相关（`L` 领先 `off` 帧）。
fn rr3_doa_lag_back(samples: &[i16], ch: usize, frames: usize, off: usize) -> i64 {
    let mut acc: i64 = 0;
    for t in off..frames {
        acc += samples[t * ch] as i64 * samples[(t - off) * ch + 1] as i64;
    }
    acc
}

/// 正向 lag 的互相关（`d` 越出帧数即 0）。
fn rr3_doa_lag_fwd(samples: &[i16], ch: usize, frames: usize, dd: usize) -> i64 {
    let mut acc: i64 = 0;
    if dd < frames {
        for t in 0..frames - dd {
            acc += samples[t * ch] as i64 * samples[(t + dd) * ch + 1] as i64;
        }
    }
    acc
}

/// 峰值 + 整数抛物线亚样本（Q7）：峰值搜索限物理域 [−lag_max, lag_max]。
fn rr3_doa_peak(cc: &[i64], lag_max: usize, half: usize) -> i64 {
    let (best, bv) = rr3_doa_argmax(cc, lag_max, half);
    let y0 = cc[(best - 1 + half as i64) as usize].abs();
    let y2 = cc[(best + 1 + half as i64) as usize].abs();
    let den = 2 * bv - y0 - y2;
    let sub = if den > 0 {
        (((y0 - y2) * 32) / den).clamp(-64, 64)
    } else {
        0
    };
    best * 128 + sub
}

/// 物理域内峰值搜索（域外次峰按构造剔除）。
fn rr3_doa_argmax(cc: &[i64], lag_max: usize, half: usize) -> (i64, i64) {
    let mut best = 0i64;
    let mut bv = -1i64;
    for d in -(lag_max as i64)..=(lag_max as i64) {
        let v = cc[(d + half as i64) as usize].abs();
        if v > bv {
            bv = v;
            best = d;
        }
    }
    (best, bv)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确定性宽带源：xorshift32(13,17,5) 高 16 位带符号截取 × amp 定标。
    struct Xorshift(u32);

    impl Xorshift {
        fn next_i16_scaled(&mut self, amp: i32) -> i16 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            let v = (x as i32) >> 16;
            ((v * amp) >> 15) as i16
        }
    }

    /// 延迟立体声窗：τ>0 声源在右（R 无延迟、L 延迟 |τ|），τ<0 反之。
    fn delayed_stereo(tau: i32, frames: usize, amp: i32) -> Vec<i16> {
        let mut xs = Xorshift(0x1234_5678);
        let u: Vec<i16> = (0..frames + tau.unsigned_abs() as usize)
            .map(|_| xs.next_i16_scaled(amp))
            .collect();
        let (r, l): (Vec<i16>, Vec<i16>) = if tau >= 0 {
            (u[tau as usize..].to_vec(), u[..frames].to_vec())
        } else {
            (u[..frames].to_vec(), u[(-tau) as usize..].to_vec())
        };
        (0..frames).flat_map(|t| [r[t], l[t]]).collect()
    }

    fn doa_cfg() -> Rr3DoaCfg {
        Rr3DoaCfg {
            sample_rate_hz: 16000,
            frame_ms: 20,
            mic_baseline_mm: 150,
            window_rms_min: 800,
            bin_threshold_deg: 30,
        }
    }

    #[test]
    fn frame_conversion_covers_registered_and_odd_rates() {
        assert_eq!(rr3_frames_per_tick(16000, 20), 320, "登记配置的黄金值");
        assert_eq!(rr3_frames_per_tick(16000, 10), 160);
        assert_eq!(rr3_frames_per_tick(8000, 20), 160);
        assert_eq!(rr3_frames_per_tick(22050, 20), 441, "22050×20/1000 整除");
        assert_eq!(
            rr3_frames_per_tick(16000, 0),
            0,
            "0ms 节拍读 0 帧（不 panic）"
        );
    }

    /// 多通道单声道化：选能量最高通道、单声道原样、并列取首通道、空输入安全。
    #[test]
    fn best_channel_picks_louder_and_mono_passthrough() {
        assert_eq!(rr3_pcm_best_channel(&[1, -2, 3], 1), vec![1, -2, 3]);
        assert_eq!(rr3_pcm_best_channel(&[1, -2, 3], 0), vec![1, -2, 3]);
        assert_eq!(
            rr3_pcm_best_channel(&[1000, 100, 2000, 200, -500, -50], 2),
            vec![1000, 2000, -500]
        );
        assert_eq!(
            rr3_pcm_best_channel(&[100, 900, 50, 1500, -30, 700], 2),
            vec![900, 1500, 700]
        );
        assert_eq!(rr3_pcm_best_channel(&[0, 0, 0, 0], 2), vec![0, 0]);
        assert_eq!(
            rr3_pcm_best_channel(&[0, 0, 0, 4, 8, 8, 8, 8], 4),
            vec![4, 8]
        );
        assert!(rr3_pcm_best_channel(&[], 2).is_empty());
    }

    /// 零分配定声道：能量高者胜、并列取最小通道、单声道/空输入回 0。
    #[test]
    fn chan_pick_matches_energy_order() {
        assert_eq!(rr3_chan_pick(&[1000, 100, 2000, 200], 2), 0);
        assert_eq!(rr3_chan_pick(&[100, 900, 50, 1500], 2), 1);
        assert_eq!(rr3_chan_pick(&[0, 0, 0, 0], 2), 0, "并列取首通道");
        assert_eq!(rr3_chan_pick(&[7, -7, 7], 1), 0);
        assert_eq!(rr3_chan_pick(&[7, -7, 7], 0), 0);
        assert_eq!(rr3_chan_pick(&[], 2), 0);
    }

    /// 零分配抽通道：写入选中通道、`out` 短则截断、越界不 panic。
    #[test]
    fn chan_take_writes_selected_and_truncates() {
        let s = [1000, 100, 2000, 200, -500, -50];
        let mut full = [0i16; 3];
        assert_eq!(rr3_chan_take(&s, 2, 1, &mut full), 3);
        assert_eq!(full, [100, 200, -50]);
        let mut one = [0i16; 1];
        assert_eq!(rr3_chan_take(&s, 2, 0, &mut one), 1);
        assert_eq!(one, [1000]);
        let mut none = [0i16; 2];
        assert_eq!(rr3_chan_take(&s, 2, 2, &mut none), 0, "通道号越界回 0");
        assert_eq!(rr3_chan_take(&[], 2, 0, &mut none), 0);
        let mut mono = [0i16; 3];
        assert_eq!(rr3_chan_take(&[1, -2, 3, 4], 1, 0, &mut mono), 3);
        assert_eq!(mono, [1, -2, 3]);
    }

    /// 流式两函数与分配版逐位同源：pick+take 必须复现 best_channel。
    #[test]
    fn pick_take_reproduce_best_channel() {
        for (samples, ch) in [
            (vec![1000i16, 100, 2000, 200, -500, -50], 2u8),
            (vec![100, 900, 50, 1500, -30, 700], 2),
            (vec![0, 0, 0, 4, 8, 8, 8, 8], 4),
            (vec![1, -2, 3], 1),
        ] {
            let sel = rr3_chan_pick(&samples, ch);
            let mut out = vec![0i16; samples.len() / ch.max(1) as usize];
            let n = rr3_chan_take(&samples, ch, sel, &mut out);
            out.truncate(n);
            assert_eq!(out, rr3_pcm_best_channel(&samples, ch), "ch={ch}");
        }
    }

    /// golden 向量：延迟反演逐位钉死——符号、lag 上限、Q7、K_Q15、asin 查表。
    #[test]
    fn tdoa_inverts_delay_and_bins_pm90() {
        let cfg = doa_cfg();
        for (tau, want_deg, want_bin) in [
            (-7, -90, Rr3DoaBin::Left),
            (-4, -35, Rr3DoaBin::Left),
            (-2, -17, Rr3DoaBin::Center),
            (0, 0, Rr3DoaBin::Center),
            (2, 17, Rr3DoaBin::Center),
            (4, 35, Rr3DoaBin::Right),
            (7, 90, Rr3DoaBin::Right),
        ] {
            let samples = delayed_stereo(tau, 3200, 3000);
            let fix = rr3_doa_fix(&samples, 2, &cfg).unwrap_or_else(|| panic!("tau={tau} 应判出"));
            assert_eq!(fix.deg, want_deg, "tau={tau} 反演角");
            assert_eq!(fix.bin, want_bin, "tau={tau} 档位");
        }
    }

    /// 单声道/空窗/全静音/欠能量窗恒 None；尾帧过门照样判。
    #[test]
    fn tdoa_returns_none_for_mono_silence_and_quiet() {
        let cfg = doa_cfg();
        assert_eq!(
            rr3_doa_fix(&[10, -10, 20, -20], 1, &cfg),
            None,
            "mono 无方向"
        );
        assert_eq!(rr3_doa_fix(&[], 2, &cfg), None, "空窗");
        assert_eq!(rr3_doa_fix(&[0; 640], 2, &cfg), None, "全静音");
        let quiet = delayed_stereo(4, 3200, 375);
        assert_eq!(rr3_doa_fix(&quiet, 2, &cfg), None, "欠能量窗不判");
        let mut burst = vec![0i16; 3200 * 2];
        for (i, v) in delayed_stereo(4, 320, 3000).into_iter().enumerate() {
            burst[(3200 - 320) * 2 + i] = v;
        }
        assert_eq!(
            rr3_doa_fix(&burst, 2, &cfg).map(|f| f.bin),
            Some(Rr3DoaBin::Right),
            "尾部语音帧过门判向"
        );
    }

    /// 分档阈值注入面：同一窗换 `bin_threshold_deg` 即换档，反演角与阈值无关。
    #[test]
    fn injected_threshold_moves_the_bins() {
        let samples = delayed_stereo(2, 3200, 3000);
        let wide = rr3_doa_fix(&samples, 2, &doa_cfg()).unwrap();
        assert_eq!(wide.bin, Rr3DoaBin::Center, "30° 边界：17° 落中档");
        let mut narrow = doa_cfg();
        narrow.bin_threshold_deg = 10;
        let narrowed = rr3_doa_fix(&samples, 2, &narrow).unwrap();
        assert_eq!(narrowed.bin, Rr3DoaBin::Right, "10° 边界越界落右档");
        assert_eq!(wide.deg, narrowed.deg, "反演角与阈值无关");
    }

    /// 多通道（>2）取前两路 L/R 对：第三路起的内容不影响判向结果。
    #[test]
    fn tdoa_uses_first_two_channels_only() {
        let stereo = delayed_stereo(-4, 1600, 3000);
        let three: Vec<i16> = stereo
            .chunks_exact(2)
            .flat_map(|f| [f[0], f[1], -777])
            .collect();
        let cfg = doa_cfg();
        assert_eq!(
            rr3_doa_fix(&stereo, 2, &cfg),
            rr3_doa_fix(&three, 3, &cfg),
            "第三路不得影响 L/R 对的判向"
        );
    }
}
