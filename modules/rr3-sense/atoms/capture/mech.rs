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

/// 多通道 PCM 下混单声道：按帧对各声道取算术平均（i32 累加后整除，不引入饱和偏差），
/// 输出采样点数 = 帧数。`channels <= 1` 时原样克隆（单麦链路零改动）；尾部不足一帧的
/// 余数丢弃（采集侧 samples = 帧×声道，恒整除）。
/// 0.0.5 批I S-6：无件外读者（stream 级下混在组合件承载层，经纯切片本函数完成），
/// pub(crate) 不进契约面。
pub(crate) fn rr3_pcm_downmix_mono(samples: &[i16], channels: u8) -> Vec<i16> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / ch as i32) as i16
        })
        .collect()
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
    if channels < 2 || cfg.mic_baseline_mm == 0 {
        return None;
    }
    let ch = channels as usize;
    let frames = samples.len() / ch;
    if frames == 0 {
        return None;
    }
    // 能量门：任一采集帧（L/R 任一通道）rms ≥ 阈值（阈值²×帧长定标比较，免 isqrt）
    let frame_len = ((cfg.sample_rate_hz as u64 * cfg.frame_ms) / 1000).max(1) as usize;
    let min = cfg.window_rms_min.max(0) as u64;
    if min > 0 {
        let gate_sq = min * min * frame_len as u64;
        let mut loud = false;
        for fr in samples.chunks_exact(ch * frame_len) {
            let (mut sq_l, mut sq_r) = (0u64, 0u64);
            let mut i = 0;
            while i + 1 < fr.len() {
                let v = (fr[i] as i64).unsigned_abs();
                sq_l += v * v;
                let w = (fr[i + 1] as i64).unsigned_abs();
                sq_r += w * w;
                i += ch;
            }
            if sq_l >= gate_sq || sq_r >= gate_sq {
                loud = true;
                break;
            }
        }
        if !loud {
            return None;
        }
    }
    // 物理 lag 上限：round(sr·基线/声速)——15cm @16k = round(6.997) = 7
    let sr_mm = cfg.sample_rate_hz as i64 * cfg.mic_baseline_mm as i64;
    let lag_max = ((2 * sr_mm + RR3_SOUND_MM_PER_S) / (2 * RR3_SOUND_MM_PER_S)) as usize;
    // cc[d] = Σ_t R[t]·L[t+d]，d ∈ [−(lag_max+1), lag_max+1]（±1 供抛物线取邻）
    let half = lag_max + 1;
    let mut cc = vec![0i64; 2 * half + 1];
    for (i, slot) in cc.iter_mut().enumerate() {
        let d = i as i64 - half as i64;
        let mut acc: i64 = 0;
        if d >= 0 {
            let dd = d as usize;
            if dd < frames {
                for t in 0..frames - dd {
                    acc += samples[t * ch] as i64 * samples[(t + dd) * ch + 1] as i64;
                }
            }
        } else {
            let off = (-d) as usize;
            for t in off..frames {
                acc += samples[t * ch] as i64 * samples[(t - off) * ch + 1] as i64;
            }
        }
        *slot = acc;
    }
    // 峰值搜索限物理域 [−lag_max, lag_max]（±31/32 次峰在域外，按构造剔除）
    let mut best = 0i64;
    let mut bv = -1i64;
    for d in -(lag_max as i64)..=(lag_max as i64) {
        let v = cc[(d + half as i64) as usize].abs();
        if v > bv {
            bv = v;
            best = d;
        }
    }
    // 整数抛物线亚样本（Q7）：真峰处 2y1 > y0+y2，den ≤ 0（平台/边界）不插补
    let y0 = cc[(best - 1 + half as i64) as usize].abs();
    let y2 = cc[(best + 1 + half as i64) as usize].abs();
    let den = 2 * bv - y0 - y2;
    let sub = if den > 0 {
        (((y0 - y2) * 32) / den).clamp(-64, 64)
    } else {
        0
    };
    // sinθ = lag·声速/(sr·基线)：K_Q15 定标（Q7 lag → Q15 sinθ），查表出整度
    let k_q15 = RR3_SOUND_MM_PER_S * 32768 / sr_mm;
    let lag_q7 = best * 128 + sub;
    let sin_q15 = ((lag_q7 * k_q15) >> 7).clamp(-32768, 32767) as i32;
    let idx = (((sin_q15 + 32768) * 256 + 32768) >> 16) as usize;
    let deg = RR3_ASIN_DEG_LUT[idx];
    let bin = rr3_doa_bin_of(deg, cfg.bin_threshold_deg);
    Some(Rr3DoaFix { deg, bin })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 多通道下混（R14）：逐帧平均、单声道原样、多声道点数收缩。
    #[test]
    fn downmix_mono_averages_channels_per_frame() {
        // 单声道原样（含负值），零改动
        assert_eq!(rr3_pcm_downmix_mono(&[1, -2, 3], 1), vec![1, -2, 3]);
        assert_eq!(rr3_pcm_downmix_mono(&[1, -2, 3], 0), vec![1, -2, 3]);
        // 双声道：3 帧 × 2 → 3 点；[32767,-32768] 求和 -1、整除得 0（i32 累加不溢出）
        assert_eq!(
            rr3_pcm_downmix_mono(&[100, 200, -300, -100, 32767, -32768], 2),
            vec![150, -200, 0]
        );
        // 四声道：2 帧 × 4 → 2 点
        assert_eq!(
            rr3_pcm_downmix_mono(&[0, 0, 0, 4, 8, 8, 8, 8], 4),
            vec![1, 8]
        );
    }

    // ---------- TDOA 判向用例（B批 b3b，golden 镜像 /tmp/rr3-b3-smoke/golden_doa.py） ----------

    /// 确定性宽带源：xorshift32(13,17,5) 高 16 位带符号截取 × amp 定标——
    /// golden 向量任何机器重跑同值（S-8 整型化纪律的测试侧落点）。
    struct Xorshift(u32);
    impl Xorshift {
        fn next_i16_scaled(&mut self, amp: i32) -> i16 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            let v = (x as i32) >> 16; // 高 16 位带符号 → [−32768, 32767]
            ((v * amp) >> 15) as i16
        }
    }

    /// 延迟立体声窗：τ>0 声源在右（frame[0]=物理右麦先到，R 无延迟、L 延迟 |τ|），
    /// τ<0 声源在左（frame[1] 先到）——与机制取样语义 1:1（b3 回归双侧交叉定案：
    /// esp32 档 frame[0]=物理右、frame[1]=物理左，lag>0 = frame[0] 先到 = 右）。
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

    /// golden 向量（生成器 golden_doa.py 与本件 1:1 镜像）：延迟反演逐位钉死——
    /// 符号约定、物理 lag 上限、Q7 抛物线、K_Q15 定标、asin 查表任何一步漂移
    /// 当场红。0.2s 窗（onset 量级）+ 全幅度 3000（rms ~1732 > 门 800）。
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

    /// 单声道/空窗/全静音/欠能量窗恒 None（无方向信息不硬判）；能量门按帧评，
    /// 窗内只要有一帧过阈即判（onset 窗"头部静音尾部语音"形状照样判出）。
    #[test]
    fn tdoa_returns_none_for_mono_silence_and_quiet() {
        let cfg = doa_cfg();
        assert_eq!(rr3_doa_fix(&[10, -10, 20, -20], 1, &cfg), None, "mono 无方向");
        assert_eq!(rr3_doa_fix(&[], 2, &cfg), None, "空窗");
        assert_eq!(rr3_doa_fix(&[0; 640], 2, &cfg), None, "全静音");
        // amp 375 峰值 → 帧 rms ~216 < 800：整窗欠能量（golden：fmax=224 → None）
        let quiet = delayed_stereo(4, 3200, 375);
        assert_eq!(rr3_doa_fix(&quiet, 2, &cfg), None, "欠能量窗不判");
        // 0.2s 窗里只有尾 0.02s 有声：单帧过阈即过门，照样判出右向
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

    /// 分档阈值注入面（S-5 延续）：同一窗换 `bin_threshold_deg` 即换档，
    /// 反演角与阈值无关。
    #[test]
    fn injected_threshold_moves_the_bins() {
        let samples = delayed_stereo(2, 3200, 3000); // 17°
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
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|f| [f[0], f[1], -777])
            .collect();
        let cfg = doa_cfg();
        assert_eq!(
            rr3_doa_fix(&stereo, 2, &cfg),
            rr3_doa_fix(&three, 3, &cfg),
            "第三路不得影响 L/R 对的判向"
        );
    }

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
                .flat_map(|s| rr3_pcm_downmix_mono(s, 2))
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
        // 零样本下混/判向安全返回（空进空出，不 panic 不产帧）
        assert!(rr3_pcm_downmix_mono(&[], 2).is_empty());
        assert_eq!(rr3_doa_fix(&[], 2, &doa_cfg()), None, "空窗无方向可判");
    }
}
