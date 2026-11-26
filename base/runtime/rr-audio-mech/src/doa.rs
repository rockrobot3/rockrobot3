// SPDX-License-Identifier: Apache-2.0
//! DoA 粗判向（自 capture 原子件抽出；多通道下混已拆到 [`crate::chan`]，本件只吃
//! 原始 L/R 对做时差判向）。
//!
//! 双麦**到达时差（TDOA）**判向：判向窗上做 plain 时域互相关
//! `cc[d] = Σ_t R[t]·L[t+d]`，lag>0 = R 先到 = 声源在右。板档 frame[0] = 物理右麦、
//! frame[1] = 物理左麦——板麦接线/I2S 通道序是装配事实，以板上实测锚定。
//!
//! 搜索域钉死物理可达范围 ±lag_max（lag_max = round(sr·基线/声速)，15cm 基线
//! @16k = ±7 样本）。plain 时域而非 FFT/PHAT：离线对拍与限带 FFT 版同号同档，
//! 时域版零浮点零滤波器，golden 纪律最短路径。
//!
//! 整型化（golden 纪律，任何机器逐位一致）：① 互相关 i64 累加（窗 16000 帧 ×
//! 32767² ≈ 1.7e13）；② 亚样本抛物线定标 Q7：sub = 32·(y0−y2)/(2y1−y0−y2)，
//! clamp [−64, 64]；③ lag_Q7 × K_Q15 >> 7 反演 sinθ，经 256 格 asin 查表出整度。
//!
//! 能量门：窗内任一采集帧 rms ≥ `window_rms_min` 才判。置信度 `conf_q15 =
//! |cc_peak| / √(ΣL²·ΣR²)`——相干声源接近满值；低置信判向由策略面按门槛丢弃。

use alloc::vec;
use alloc::vec::Vec;

use crate::rr3_isqrt;

// ---------- 帧节拍换算 ----------

/// 一次采集节拍读多少帧：`rate × frame_ms / 1000`（16000×20 = 320 帧）。
pub fn rr3_frames_per_tick(sample_rate_hz: u32, frame_ms: u64) -> usize {
    (sample_rate_hz as u64 * frame_ms / 1000) as usize
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
    /// 物理通道序校准：+1 = 帧内 ch0 在物理左；-1 = 通道序反向（换装/接线后按
    /// 真板标定注入）。只翻判向符号，不改 TDOA/基线/阈值。
    pub sign: i32,
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

/// 一次判向结果：连续角（度，-90..=90，负=左）+ 三档离散档位 + 归一化互相关
/// 峰值置信度（Q15，0..=32767）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3DoaFix {
    pub deg: i32,
    pub bin: Rr3DoaBin,
    pub conf_q15: i32,
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
    let (lag_q7, peak) = rr3_doa_peak(&cc, lag_max, half);
    // sinθ = lag·声速/(sr·基线)：K_Q15 定标（Q7 lag → Q15 sinθ），查表出整度
    let k_q15 = RR3_SOUND_MM_PER_S * 32768 / sr_mm;
    let sin_q15 = ((lag_q7 * k_q15) >> 7).clamp(-32768, 32767) as i32;
    let idx = (((sin_q15 + 32768) * 256 + 32768) >> 16) as usize;
    let deg = if cfg.sign < 0 {
        -RR3_ASIN_DEG_LUT[idx]
    } else {
        RR3_ASIN_DEG_LUT[idx]
    };
    let bin = rr3_doa_bin_of(deg, cfg.bin_threshold_deg);
    let conf_q15 = rr3_doa_conf(samples, ch, peak);
    Some(Rr3DoaFix { deg, bin, conf_q15 })
}

/// 归一化互相关峰值置信度（Q15）：`|peak| / √(ΣL²·ΣR²)`。分母按两通道能量
/// 开方后相乘（避免平方和相乘溢出），相干源接近 32767，独立噪声低值。
fn rr3_doa_conf(samples: &[i16], ch: usize, peak: i64) -> i32 {
    let (sq_l, sq_r) = rr3_sq_pair(samples, ch);
    let den = (rr3_isqrt(sq_l) as i64) * (rr3_isqrt(sq_r) as i64);
    if den <= 0 {
        return 0;
    }
    ((peak * 32768) / den).clamp(0, 32767) as i32
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

/// 峰值 + 整数抛物线亚样本（Q7）：峰值搜索限物理域 [−lag_max, lag_max]；
/// 返回 `(lag_q7, 峰值绝对值)` 供置信度复用同一峰值。
fn rr3_doa_peak(cc: &[i64], lag_max: usize, half: usize) -> (i64, i64) {
    let (best, bv) = rr3_doa_argmax(cc, lag_max, half);
    let y0 = cc[(best - 1 + half as i64) as usize].abs();
    let y2 = cc[(best + 1 + half as i64) as usize].abs();
    let den = 2 * bv - y0 - y2;
    let sub = if den > 0 {
        (((y0 - y2) * 32) / den).clamp(-64, 64)
    } else {
        0
    };
    (best * 128 + sub, bv)
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

    /// 独立双通道噪声（两麦不相关）：置信度低值判据的输入。
    fn uncorrelated_stereo(frames: usize, amp: i32) -> Vec<i16> {
        let mut a = Xorshift(0x1234_5678);
        let mut b = Xorshift(0x9e37_79b9);
        (0..frames)
            .flat_map(|_| [a.next_i16_scaled(amp), b.next_i16_scaled(amp)])
            .collect()
    }

    fn doa_cfg() -> Rr3DoaCfg {
        Rr3DoaCfg {
            sample_rate_hz: 16000,
            frame_ms: 20,
            mic_baseline_mm: 150,
            window_rms_min: 800,
            bin_threshold_deg: 30,
            sign: 1,
        }
    }

    #[test]
    fn frame_reg_odd_rates() {
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

    /// golden 向量：延迟反演逐位钉死——符号、lag 上限、Q7、K_Q15、asin 查表。
    #[test]
    fn tdoa_delay_bins_pm90() {
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

    /// 符号校准注入面：同一 TDOA 窗换 `sign` 即翻左右，角度取反。
    #[test]
    fn sign_left_right() {
        let samples = delayed_stereo(4, 3200, 3000);
        let normal = rr3_doa_fix(&samples, 2, &doa_cfg()).unwrap();
        let mut flipped = doa_cfg();
        flipped.sign = -1;
        let inv = rr3_doa_fix(&samples, 2, &flipped).unwrap();
        assert_eq!(normal.bin, Rr3DoaBin::Right);
        assert_eq!(inv.bin, Rr3DoaBin::Left);
        assert_eq!(inv.deg, -normal.deg);
    }

    /// 置信度：相干声源接近满值，独立双麦噪声低值（漫反射抑制的判据）。
    #[test]
    fn confidence_low_noise() {
        let cfg = doa_cfg();
        let coherent = delayed_stereo(4, 3200, 3000);
        let fx = rr3_doa_fix(&coherent, 2, &cfg).expect("相干源应判出");
        assert!(fx.conf_q15 > 20_000, "相干源置信度应高: {}", fx.conf_q15);
        let noise = uncorrelated_stereo(3200, 3000);
        let nf = rr3_doa_fix(&noise, 2, &cfg).expect("独立噪声仍过能量门");
        assert!(nf.conf_q15 < 4_000, "独立噪声置信度应低: {}", nf.conf_q15);
    }

    /// 单声道/空窗/全静音/欠能量窗恒 None；尾帧过门照样判。
    #[test]
    fn tdoa_none_mono_quiet() {
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
    fn injected_thr_bins() {
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
    fn tdoa_two_channels() {
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
