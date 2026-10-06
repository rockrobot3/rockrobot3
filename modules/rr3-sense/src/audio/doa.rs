//! DoA 粗判向（批F2 sim 先行，方案 §5.3 判向-转向编排 / R14）。
//!
//! 双麦**幅度差（ILD）**粗判向：逐通道 RMS 的线性比值直接反演 `sin θ`
//! （注入模型 `g_l = 1 - sinθ`、`g_r = 1 + sinθ` 与 rr-hal sim 源同一套，
//! 反演可逆且确定——[`rr3_doa_fix`] 的单测与 rr-hal 的注入单测互为正反面）。
//! 相位差（ITD）在 sim 源里是保真项不是判据：粗判向 ±90° 分档走幅度差，
//! 真件换装（批F5a，USB 麦阵列）按实测阵列特性再评估是否升级互相关测向。
//!
//! 落档口径（零新契约）：结果只随 [`crate::audio::rr3_vad_event_body`] 的
//! **onset** 边沿上路（`doa` 字段），承载面是在册主题 `rr3-sense/audio/vad_start`
//! 的负载扩展——offset 静音帧无方向可判，不带 doa（与 rms_peak 同一形状纪律）。

pub const SURFACE: &[&str] = &[
    "audio/doa: 双麦 ILD 粗判向（±90° 分档）→ vad_start onset 的 doa 字段（sense 内部落档，零新契约）",
];

/// 分档阈值的绝对值（度）：|θ| ≤ 30° 落中档，否则左/右档。
/// 粗判向三档（左/中/右）的边界——±90° 值域对半砍出的常用档位，不进 config
/// （判据只钉"±90° 分档"这个口径，阈值细化随真件换装实测再裁）。
pub const RR3_DOA_BIN_THRESHOLD_DEG: i32 = 30;

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
    let (r_l, r_r) = ((sq_l as f64).sqrt(), (sq_r as f64).sqrt());
    let k = ((r_r - r_l) / (r_r + r_l)).clamp(-1.0, 1.0);
    // ±90° 值域内的连续角：asin 值域恰是 [-90°, +90°]，舍入到整度
    let deg = (k.asin().to_degrees()).round() as i32;
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

    /// 用注入模型造一帧双通道样本：`g_l = 1 - sinθ`、`g_r = 1 + sinθ` 的整幅正弦
    /// （与 rr-hal sim 源同一形状，独立小实现——判向正确性不依赖被测源头）。
    fn stereo_frame(deg: f64, frames: usize) -> Vec<i16> {
        let k = deg.to_radians().sin();
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 16000.0).sin();
                [((1.0 - k) * v * 12000.0) as i16, ((1.0 + k) * v * 12000.0) as i16]
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

    /// 注入角 → 反演角逐点贴合（±2° 容差吸收整型截断），三档边界两侧各验一档。
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
                (fix.deg as f64 - deg).abs() <= 2.0,
                "deg={deg}: 反演角 {} 超容差",
                fix.deg
            );
            assert_eq!(fix.bin, want_bin, "deg={deg} 的档位");
        }
    }

    /// 分档阈值常数钉死 30°：档位语义（左/中/右）与 as_str 字面量一起 pin——
    /// 事件消费方按字面量分派，漂移当场红。
    #[test]
    fn doa_bin_str_and_threshold_are_pinned() {
        assert_eq!(RR3_DOA_BIN_THRESHOLD_DEG, 30);
        assert_eq!(Rr3DoaBin::Left.rr3_doa_bin_as_str(), "left");
        assert_eq!(Rr3DoaBin::Center.rr3_doa_bin_as_str(), "center");
        assert_eq!(Rr3DoaBin::Right.rr3_doa_bin_as_str(), "right");
    }

    /// 多通道（>2）取前两路 L/R 对：第三路起的内容不影响判向结果。
    #[test]
    fn doa_uses_first_two_channels_only() {
        let stereo = stereo_frame(-60.0, 400);
        // 三通道：前两路同 stereo，第三路塞满任意噪声值
        let three: Vec<i16> = stereo
            .chunks_exact(2)
            .flat_map(|f| [f[0], f[1], -777])
            .collect();
        let a = rr3_doa_fix(&stereo, 2).unwrap();
        let b = rr3_doa_fix(&three, 3).unwrap();
        assert_eq!(a, b, "第三路不得影响 L/R 对的判向");
    }
}
