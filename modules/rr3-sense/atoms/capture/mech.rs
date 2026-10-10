//! capture 原子件机制主体（五件套③，自原 src/audio/{capture.rs, mod.rs, doa.rs} 逐字迁入，
//! 逻辑零改动）。
//!
//! 两段：帧节拍换算 + 多通道下混（R14）｜DoA 粗判向（批F2；0.0.5 批I S-8 整型化）。
//! base64 编码归 rr-ipc 线格式层（0.0.5 批C① 塌缩，方案 §12.3），本原子只消费；
//! pcm 事件体构造随 0.0.5 批I S-7 上移组合件（ADR-0019 修订 4：件面只留纯机制，
//! 零 serde_json），用例随迁。流级下混随 0.0.5 批I 深挖收口同上移组合件——
//! `Rr3PcmStream` 是承载层数据类型（C7 白名单外），件面只留纯切片机制。

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

// ---------- DoA 粗判向（批F2，方案 §5.3 判向-转向编排 / R14） ----------
//
// 双麦**幅度差（ILD）**粗判向：逐通道 RMS 的线性比值直接反演 `sin θ`
// （注入模型 `g_l = 1 - sinθ`、`g_r = 1 + sinθ` 与 rr-hal sim 源同一套，
// 反演可逆且确定——[`rr3_doa_fix`] 的单测与 rr-hal 的注入单测互为正反面）。
// 相位差（ITD）在 sim 源里是保真项不是判据：粗判向 ±90° 分档走幅度差，
// 真件换装（批F5a，USB 麦阵列）按实测阵列特性再评估是否升级互相关测向。
//
// S-8 整型化（0.0.5 批I）：与 vad 原子同一条 golden 纪律——任何机器逐位一致，
// 浮点 sqrt/asin 无此保证。三步全整数流水线：
// ① 逐通道 RMS 走 [`rr3_isqrt`]（与 vad/mech.rs 各持一份，原子间不交叉依赖）；
// ② 比值 k = (r_r − r_l)/(r_r + r_l) 定点成 Q15 整数（[-32768, 32767]）；
// ③ k 经 256 格 asin 查表（[`RR3_ASIN_DEG_LUT`]）反演成整度，下标 =
//    round((k_Q15 + 32768) / 256)——舍入除法，±90° 端点干净落位（k=±1 → ±90°；
//    截断除法会把 +90° 落到 86°，故必须舍入）。误差预算 |θ| ≤ 75° 内 ≤ 1°
//    （Q15 量化与查表舍入的叠加上界）；75° 外落在左/右档深处，分档无歧义即达
//    目的。分档边界（±29°/±31°）在单测钉死不糊。
//
// 落档口径（零新契约）：结果只随在册主题 `rr3-sense/audio/vad_start` 的
// **onset** 边沿上路（`doa` 字段）——offset 静音帧无方向可判，不带 doa
// （与 rms_peak 同一形状纪律）。

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

/// 分档阈值的绝对值（度）：|θ| ≤ 30° 落中档，否则左/右档。
/// 粗判向三档（左/中/右）的边界——±90° 值域对半砍出的常用档位，不进 config
/// （判据只钉"±90° 分档"这个口径，阈值细化随真件换装实测再裁）。
/// 0.0.5 批I S-6：mech 内私有常数，无件外读者，降 pub(crate)。
pub(crate) const RR3_DOA_BIN_THRESHOLD_DEG: i32 = 30;

/// DoA 分档（±90° 值域的三档离散）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3DoaBin {
    /// 声源在左（θ < -30°）
    Left,
    /// 声源在正前附近（-30° ≤ θ ≤ 30°）
    Center,
    /// 声源在右（θ > 30°）
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

/// 整数平方根（牛顿二分）：`isqrt(x)` = ⌊√x⌋。通道平方和最大约
/// 320 × 32767² ≈ 3.4e11，u64 绰绰有余；`mid <= x / mid` 的写法避免 mid*mid 溢出。
/// 与 vad 原子的 rr3_isqrt 各持一份（原子间不交叉依赖，0.0.5 批C① 同款纪律）。
fn rr3_isqrt(x: u64) -> u64 {
    if x < 2 {
        return x;
    }
    let (mut lo, mut hi, mut ans) = (1u64, x, 1u64);
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        if mid <= x / mid {
            ans = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    ans
}

/// 一帧多通道 PCM 的粗判向。返回 `None` 的三种情形：
/// - 单声道（channels < 2）——没有方向信息，mono 链路恒 `None`；
/// - 两路全静音——零比零无方向可判（VAD 起音前这类帧不会带 doa 上路）。
///
/// 判据：`sinθ = (r_r - r_l) / (r_r + r_l)`，r 为逐通道 RMS（平方和开根）。
/// 注入模型（rr-hal sim 源）`g_l = 1 - sinθ`、`g_r = 1 + sinθ` 下该式严格可逆；
/// 更多通道（USB 阵列真件）先只取前两路 L/R 对，通道序重排随换装批裁定。
pub fn rr3_doa_fix(samples: &[i16], channels: u8) -> Option<Rr3DoaFix> {
    if channels < 2 {
        return None;
    }
    let ch = channels as usize;
    let mut sq_l: i64 = 0;
    let mut sq_r: i64 = 0;
    for frame in samples.chunks_exact(ch) {
        sq_l += frame[0] as i64 * frame[0] as i64;
        sq_r += frame[1] as i64 * frame[1] as i64;
    }
    if sq_l + sq_r == 0 {
        return None;
    }
    let (r_l, r_r) = (rr3_isqrt(sq_l as u64), rr3_isqrt(sq_r as u64));
    // Q15 定点比值：k ∈ [−1, 1] → 整数 [−32768, 32767]（+1 端点饱和到 32767）
    let k_q15 =
        (((r_r as i64 - r_l as i64) * 32768 / (r_l + r_r) as i64) as i32).clamp(-32768, 32767);
    // 256 格查表下标：round((k_Q15 + 32768) / 256)，值域 [0, 256]
    let idx = (((k_q15 + 32768) * 256 + 32768) >> 16) as usize;
    let deg = RR3_ASIN_DEG_LUT[idx];
    let bin = if deg < -RR3_DOA_BIN_THRESHOLD_DEG {
        Rr3DoaBin::Left
    } else if deg > RR3_DOA_BIN_THRESHOLD_DEG {
        Rr3DoaBin::Right
    } else {
        Rr3DoaBin::Center
    };
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

    /// 用注入模型造一帧双通道样本：`g_l = 1 - sinθ`、`g_r = 1 + sinθ` 的整幅正弦
    /// （与 rr-hal sim 源同一形状，独立小实现——判向正确性不依赖被测源头）。
    fn stereo_frame(deg: f64, frames: usize) -> Vec<i16> {
        let k = deg.to_radians().sin();
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 16000.0).sin();
                [
                    ((1.0 - k) * v * 12000.0) as i16,
                    ((1.0 + k) * v * 12000.0) as i16,
                ]
            })
            .collect()
    }

    /// 单声道恒 None；双通道全零恒 None——无方向信息不硬判。
    #[test]
    fn doa_returns_none_for_mono_and_silence() {
        assert_eq!(rr3_doa_fix(&[10, -10, 20, -20], 1), None, "mono 无方向");
        assert_eq!(rr3_doa_fix(&[0, 0, 0, 0], 2), None, "全静音无方向");
        assert_eq!(rr3_doa_fix(&[], 2), None, "空帧无方向");
    }

    /// 注入角 → 反演角逐点贴合（±1° 容差：S-8 整型化后的预算——Q15 量化与查表
    /// 舍入的叠加），三档边界两侧各验一档；±90° 端点经舍入除法干净落位。
    #[test]
    fn doa_inverts_ild_model_and_bins_pm90() {
        for (deg, want_bin) in [
            (-90.0, Rr3DoaBin::Left),
            (-60.0, Rr3DoaBin::Left),
            (-31.0, Rr3DoaBin::Left),
            (-29.0, Rr3DoaBin::Center),
            (0.0, Rr3DoaBin::Center),
            (29.0, Rr3DoaBin::Center),
            (31.0, Rr3DoaBin::Right),
            (60.0, Rr3DoaBin::Right),
            (90.0, Rr3DoaBin::Right),
        ] {
            let samples = stereo_frame(deg, 1600);
            let fix = rr3_doa_fix(&samples, 2).unwrap_or_else(|| panic!("{deg} 应判出"));
            assert!(
                (fix.deg as f64 - deg).abs() <= 1.0,
                "deg={deg}: 反演角 {} 超容差",
                fix.deg
            );
            assert_eq!(fix.bin, want_bin, "deg={deg} 的档位");
        }
    }

    /// 分档阈值常数钉死 30°：档位语义（左/中/右）与 as_str 字面量一起 pin——
    /// 事件消费方按字面量分派，漂移当场红。查表端点与零点一并 pin。
    #[test]
    fn doa_bin_str_and_threshold_are_pinned() {
        assert_eq!(RR3_DOA_BIN_THRESHOLD_DEG, 30);
        assert_eq!(Rr3DoaBin::Left.rr3_doa_bin_as_str(), "left");
        assert_eq!(Rr3DoaBin::Center.rr3_doa_bin_as_str(), "center");
        assert_eq!(Rr3DoaBin::Right.rr3_doa_bin_as_str(), "right");
        assert_eq!(RR3_ASIN_DEG_LUT[0], -90);
        assert_eq!(RR3_ASIN_DEG_LUT[128], 0);
        assert_eq!(RR3_ASIN_DEG_LUT[256], 90);
    }

    /// 多通道（>2）取前两路 L/R 对：第三路起的内容不影响判向结果。
    #[test]
    fn doa_uses_first_two_channels_only() {
        let stereo = stereo_frame(-60.0, 400);
        // 三通道：前两路同 stereo，第三路塞满任意噪声值
        let three: Vec<i16> = stereo
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|f| [f[0], f[1], -777])
            .collect();
        let a = rr3_doa_fix(&stereo, 2).unwrap();
        let b = rr3_doa_fix(&three, 3).unwrap();
        assert_eq!(a, b, "第三路不得影响 L/R 对的判向");
    }

    // ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②） ----------

    #[test]
    fn parity_leg1_capture_output_train_is_deterministic() {
        // 腿一·确定性：同一多通道样本序列两跑，(下混流, 判向序列) 逐帧相等
        fn run() -> (Vec<i16>, Vec<Option<Rr3DoaFix>>) {
            let seq: Vec<Vec<i16>> = [(-60.0f64, 400), (0.0, 400), (45.0, 400)]
                .iter()
                .map(|&(deg, n)| stereo_frame(deg, n))
                .collect();
            let mixed: Vec<i16> = seq
                .iter()
                .flat_map(|s| rr3_pcm_downmix_mono(s, 2))
                .collect();
            let fixes: Vec<Option<Rr3DoaFix>> = seq.iter().map(|s| rr3_doa_fix(s, 2)).collect();
            (mixed, fixes)
        }
        assert_eq!(run(), run(), "同输入序列两跑逐帧相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_capture_matches_ruling_semantics() {
        // 腿二·正确性：分档等号边界独立重写——|θ| ≤ 30° 落中档（含等号），
        // 严格出界才落左/右；零值输入不产帧循环（0 帧换算、空样本安全返回）
        for (deg, want_bin) in [
            (-30.0, Rr3DoaBin::Center),
            (30.0, Rr3DoaBin::Center),
            (-31.0, Rr3DoaBin::Left),
            (31.0, Rr3DoaBin::Right),
        ] {
            let fix = rr3_doa_fix(&stereo_frame(deg, 1600), 2).unwrap();
            assert_eq!(fix.bin, want_bin, "deg={deg} 等号边界档位");
        }
        // 零值：0ms 节拍与 0Hz 采样都换算出 0 帧（调用方不得拿 0 帧当 buf 死循环）
        assert_eq!(rr3_frames_per_tick(16000, 0), 0, "(16000, 0) → 0 帧");
        assert_eq!(rr3_frames_per_tick(0, 20), 0, "(0, 20) → 0 帧");
        // 零样本下混/判向安全返回（空进空出，不 panic 不产帧）
        assert!(rr3_pcm_downmix_mono(&[], 2).is_empty());
        assert_eq!(rr3_doa_fix(&[], 2), None, "空帧无方向可判");
    }
}
