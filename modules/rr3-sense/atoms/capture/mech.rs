//! capture 原子件机制主体（五件套③，自原 src/audio/{capture.rs, mod.rs, doa.rs} 迁入；
//! DoA 判向段 B批 b3b 由 ILD 重写为 TDOA 时域互相关）。
//!
//! 两段：帧节拍换算 + 多通道下混（R14）｜DoA 粗判向（批F2 立、B批 b3b 换 TDOA；
//! 0.0.5 批I S-8 整型化纪律延续）。
//! base64 编码归 rr-ipc 线格式层（0.0.5 批C① 塌缩，方案 §12.3），本原子只消费；
//! pcm 事件体构造随 0.0.5 批I S-7 上移组合件（ADR-0019 修订 4：件面只留纯机制，
//! 零 serde_json），用例随迁。流级下混随 0.0.5 批I 深挖收口同上移组合件——
//! `Rr3PcmStream` 是承载层数据类型（C7 白名单外），件面只留纯切片机制。

use super::Rr3DoaCfg;

// ---------- 帧节拍换算 ----------

/// 一次采集节拍读多少帧：`rate × frame_ms / 1000`（16000×20 = 320 帧）。
/// HAL 的 `read(buf)`（F1 定形）收采样点 buf（帧×声道交错）、返回实填采样
/// 点数——帧数→采样点数的换算只在组合件组装 buf 这一处。
pub fn rr3_frames_per_tick(sample_rate_hz: u32, frame_ms: u64) -> usize {
    (sample_rate_hz as u64 * frame_ms / 1000) as usize
}

// ---------- 多通道下混（R14） ----------

/// 多通道 PCM → 单声道：**选能量最高的通道**（不取平均）。2026-10-31 实机取证：
/// 双麦 150mm 间距，原始 L/R 零延迟相关仅 +0.2（按最佳时差对齐后仍只 +0.43），
/// 平均下混 RMS 比单声道低 2–3dB 且带梳状滤波——同一句"你好洛克"，下混送 ASR
/// 得"你好，外天。"，单取能量高的通道得"你好，洛哥。"（可被一字容错唤醒匹配）。
/// DoA 仍吃原始双通道（TDOA 不变），本函数只决定总线 mono 送哪一路。
/// `channels <= 1` 时原样克隆（单麦链路零改动）；尾部不足一帧的余数丢弃
/// （采集侧 samples = 帧×声道，恒整除）。
/// 0.0.5 批I S-6：无件外读者（stream 级选道在组合件承载层，经纯切片本函数完成），
/// pub(crate) 不进契约面。
pub(crate) fn rr3_pcm_best_channel(samples: &[i16], channels: u8) -> Vec<i16> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    let frames = samples.len() / ch;
    if frames == 0 {
        return Vec::new();
    }
    // 整数能量比较（sum_sq 足够，免开方）；并列取最小通道号（确定性）
    let sums = rr3_chan_energy(samples, ch, frames);
    let best = rr3_chan_pick(&sums);
    rr3_pcm_take(samples, ch, best, frames)
}

/// 按选中通道抽单声道（尾部余数已由 `frames` 截断）。
fn rr3_pcm_take(samples: &[i16], ch: usize, best: usize, frames: usize) -> Vec<i16> {
    (0..frames).map(|i| samples[i * ch + best]).collect()
}

/// 各通道整数能量（`sum_sq`，免开方）。
fn rr3_chan_energy(samples: &[i16], ch: usize, frames: usize) -> Vec<i64> {
    let mut sums = vec![0i64; ch];
    for i in 0..frames {
        rr3_chan_row(samples, ch, i, &mut sums);
    }
    sums
}

/// 单帧逐通道能量累加（第 `i` 帧）。
fn rr3_chan_row(samples: &[i16], ch: usize, i: usize, sums: &mut [i64]) {
    for (c, sum) in sums.iter_mut().enumerate() {
        let v = samples[i * ch + c] as i64;
        *sum += v * v;
    }
}

/// 能量最高的通道号（并列取最小通道号）。
fn rr3_chan_pick(sums: &[i64]) -> usize {
    let mut best = 0usize;
    for c in 1..sums.len() {
        if sums[c] > sums[best] {
            best = c;
        }
    }
    best
}

// ---------- DoA 粗判向（B批 b3b：TDOA 时域互相关；批F2 ILD 版被 b3a 实证替换） ----------
//
// 双麦**到达时差（TDOA）**判向：判向窗（onset 时 ring 快照，默认 1s 立体声）上做
// plain 时域互相关 `cc[d] = Σ_t R[t]·L[t+d]`，lag>0 = R 先到 = 声源在右。
// 通道映射定案（B批 b3 回归双侧交叉实测）：esp32 档 frame[0] = 物理右麦（R）、
// frame[1] = 物理左麦（L）——板档直接按此取样；板麦接线/I2S 通道序是装配事实，
// 以板上实测锚定，不依赖口头方位确认（b3a「甲=右」口答即被此实测推翻）。
// 搜索域钉死物理可达范围 ±lag_max（lag_max = round(sr·基线/声速)，15cm 基线 @16k
// = ±7 样本）——b3a 离线时间线反复出现的 ±31/32 样本次峰离群在物理域外，搜索域
// 收口后按构造剔除，比峰质门更硬的一道。
//
// 为什么替换 ILD：裸 INMP441 双麦 15cm 间距对语音的幅度差近乎零（b3a 近场实测
// rms 比恒 1.0，方向信息只在相位差里），ILD 无判向力；TDOA 悬空实验 lag 随方向
// 稳定摆动合几何。为什么 plain 时域而不是 FFT 限带/PHAT：离线对拍（b3a 三份
// 录音，原型 proto_tdoa_int.py）整数时域 plain 相关与限带 FFT 版同号同档——反射
// 环 pan 目标本就 clamp(±30°)，档位一致即行为一致；时域版零浮点零滤波器，
// golden 纪律最短路径。
//
// S-8 整型化（golden 纪律，任何机器逐位一致）：
// ① 互相关 i64 累加（窗 16000 帧 × 32767² ≈ 1.7e13，i64 安全）；
// ② 亚样本抛物线定标 Q7：sub = 32·(y0−y2)/(2y1−y0−y2)，clamp [−64, 64]
//   （i64 `/` 截断向零；真峰处 2y1 > y0+y2，den ≤ 0 的平台/边界不插补）；
// ③ lag_Q7 × K_Q15 >> 7 反演 sinθ（K_Q15 = 声速·32768/(sr·基线)，调用处定标），
//    经 256 格 asin 查表（[`RR3_ASIN_DEG_LUT`]，与 ILD 版同表）出整度，分档
//    口径 [`rr3_doa_bin_of`] 不变。
//
// 能量门：窗内任一采集帧（L/R 任一通道）rms ≥ `window_rms_min`（阈值²×帧长定标
// 比较，免 isqrt）。VAD onset 本身已证 ≥min_speech_ms 连续有声帧，此门对 onset
// 路径恒过——防的是非 onset 直调与全静音窗（零比零无方向可判）。
//
// 落档口径（零新契约）不变：结果只随 vad_start **onset** 边沿上路（`doa` 字段）
// ——offset 静音帧无方向可判，不带 doa（与 rms_peak 同一形状纪律）。

/// 声速（mm/s，20°C 空气 ~343 m/s）：机制面物理常量，换环境校准随换装批。
const RR3_SOUND_MM_PER_S: i64 = 343_000;

/// 256 格 asin 查表（度）：`RR3_ASIN_DEG_LUT[i] = round(asin((i − 128) / 128))`，
/// i ∈ [0, 256] 覆盖 sinθ ∈ [−1, 1] 全值域。生成式一次性钉死（python3 生成后
/// 原文抄录），运行期零浮点；两端点（−90/90）与零点在单测钉死。
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

/// DoA 分档：`|θ| ≤ threshold_deg` 落中档，否则左/右档（`< -t` 左、`> t` 右）。
/// `threshold_deg` = 本形态的分档边界——0.0.5 批M MC-10 S-5 从件内常量迁为
/// config `[rr3-sense.doa] bin_threshold_deg` 注入（ops 槽 `doa_bin_threshold_deg`）。
/// 阈值仍是策略面（±90° 值域常用的对半档），"某形态换档口径"= 改数据不改码。
fn rr3_doa_bin_of(deg: i32, threshold_deg: i32) -> Rr3DoaBin {
    if deg < -threshold_deg {
        Rr3DoaBin::Left
    } else if deg > threshold_deg {
        Rr3DoaBin::Right
    } else {
        Rr3DoaBin::Center
    }
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

/// 判向窗 TDOA 判向。返回 `None` 的情形：单声道（channels < 2）/ 空窗 /
/// 基线为 0（无几何可换算）/ 能量门不过（全静音或欠能量窗）。
/// `samples` = 判向窗快照（帧交错：帧内 ch 通道，R=0/L=1——组合件在 onset 时从
/// ring 线性化快照，窗长 = config `window_ms`；单声道链路不判向恒 `None`）。
/// 通道映射定案（B批 b3 回归双侧交叉实测 2026-10-21）：esp32 档 frame[0] =
/// 物理右麦、frame[1] = 物理左麦（站左判 +90、站右判 −72 的系统性反号钉出来的，
/// b3a「甲=右」口头确认不可靠——悬空实验只有相对稳定性、无绝对方位锚点）。
/// 符号约定：lag>0 = R（frame[0]，物理右）先到 = 声源在右，deg>0 同号直通反射环。
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

/// 形状门 + 能量门（B9 自 doa_fix 抽出）：通道/基线/帧数合法且至少一采集帧
/// rms ≥ 阈值才继续。阈值²×帧长定标比较，免 isqrt。
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

/// 门内任一采集帧 rms 过阈即过（阈值²×帧长定标比较，免 isqrt）。
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

/// 互相关积分（B9 自 doa_fix 抽出）：cc[d] = Σ_t R[t]·L[t+d]，
/// d ∈ [−(lag_max+1), lag_max+1]（±1 供抛物线取邻）。
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

/// 峰值 + 整数抛物线亚样本（B9 自 doa_fix 抽出，Q7）：峰值搜索限物理域
/// [−lag_max, lag_max]（域外次峰按构造剔除）；真峰处 2y1 > y0+y2，den ≤ 0
/// （平台/边界）不插补。返回 Q7 lag。
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

// 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②）：机器锚必须留在
// 机制源文件内（⑧-a 门按内容执法）；其余单测在 `tests.rs`。
#[cfg(test)]
mod parity {
    use super::tests::{delayed_stereo, doa_cfg};
    use super::*;

    // ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②） ----------

    #[test]
    fn parity_leg1_capture_output_train_is_deterministic() {
        // 腿一·确定性：同一延迟立体声序列两跑，(下混流, 判向序列) 逐帧相等
        fn run() -> (Vec<i16>, Vec<Option<Rr3DoaFix>>) {
            let cfg = doa_cfg();
            let seq: Vec<Vec<i16>> = [(-4i32, 800usize), (0, 800), (5, 800)]
                .iter()
                .map(|&(tau, n)| delayed_stereo(tau, n, 3000))
                .collect();
            let mixed: Vec<i16> = seq
                .iter()
                .flat_map(|s| rr3_pcm_best_channel(s, 2))
                .collect();
            let fixes: Vec<Option<Rr3DoaFix>> =
                seq.iter().map(|s| rr3_doa_fix(s, 2, &cfg)).collect();
            (mixed, fixes)
        }
        assert_eq!(run(), run(), "同输入序列两跑逐帧相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_capture_matches_ruling_semantics() {
        // 腿二·正确性：分档等号边界独立重写（|deg| ≤ 30° 含等号落中档）；
        // 零值输入不产帧循环（0 帧换算、空样本安全返回）
        for (tau, want_bin) in [(-4, Rr3DoaBin::Left), (4, Rr3DoaBin::Right)] {
            let fix = rr3_doa_fix(&delayed_stereo(tau, 3200, 3000), 2, &doa_cfg()).unwrap();
            assert_eq!(fix.bin, want_bin, "tau={tau} 档位");
        }
        // 零值：0ms 节拍与 0Hz 采样都换算出 0 帧（调用方不得拿 0 帧当 buf 死循环）
        assert_eq!(rr3_frames_per_tick(16000, 0), 0, "(16000, 0) → 0 帧");
        assert_eq!(rr3_frames_per_tick(0, 20), 0, "(0, 20) → 0 帧");
        // 零样本单声道化/判向安全返回（空进空出，不 panic 不产帧）
        assert!(rr3_pcm_best_channel(&[], 2).is_empty());
        assert_eq!(rr3_doa_fix(&[], 2, &doa_cfg()), None, "空窗无方向可判");
    }
}

#[cfg(test)]
mod tests;
