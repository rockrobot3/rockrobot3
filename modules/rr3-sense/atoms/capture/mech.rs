//! capture 原子件机制主体（五件套③，自原 src/audio/{capture.rs, mod.rs, doa.rs} 逐字迁入，
//! 逻辑零改动）。
//!
//! 三段：帧节拍换算 + 多通道下混（R14）｜pcm 事件体构造｜DoA 粗判向（批F2）。
//! base64 编码归 rr-ipc 线格式层（0.0.5 批C① 塌缩，方案 §12.3），本原子只消费。

use rr_hal::{Rr3AudioCfg, Rr3PcmStream};
use rr_ipc::rr3_base64_encode;
use serde_json::{json, Value};

// ---------- 帧节拍换算 ----------

/// 一次采集节拍读多少帧：`rate × frame_ms / 1000`（16000×20 = 320 帧）。
/// 采样点=帧×声道；HAL 的 `read(frames)` 按**帧**计数，换算只在这一处。
pub fn rr3_frames_per_tick(sample_rate_hz: u32, frame_ms: u64) -> usize {
    (sample_rate_hz as u64 * frame_ms / 1000) as usize
}

// ---------- 多通道下混（R14） ----------

/// 多通道 PCM 下混单声道：按帧对各声道取算术平均（i32 累加后整除，不引入饱和偏差），
/// 输出采样点数 = 帧数。`channels <= 1` 时原样克隆（单麦链路零改动）；尾部不足一帧的
/// 余数丢弃（采集侧 samples = 帧×声道，恒整除）。
pub fn rr3_pcm_downmix_mono(samples: &[i16], channels: u8) -> Vec<i16> {
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

/// 把一帧多通道 PCM 下混为单声道流（`cfg.channels` 归 1，rate/bit_depth 不动）。
/// 总线承载恒 mono（R14：双麦原始多通道只供 sense 内部 DoA，brain 消费面零改动）；
/// `channels <= 1` 时原样返回（零改动，默认单麦链路逐点等价）。
pub fn rr3_pcm_downmix_stream(pcm: Rr3PcmStream) -> Rr3PcmStream {
    if pcm.cfg.channels <= 1 {
        return pcm;
    }
    let samples = rr3_pcm_downmix_mono(&pcm.samples, pcm.cfg.channels);
    Rr3PcmStream {
        cfg: Rr3AudioCfg {
            channels: 1,
            ..pcm.cfg
        },
        samples,
    }
}

// ---------- pcm 事件体构造 ----------

/// `rr3-sense/audio/pcm` 的事件体（stream）。PCM 按 s16le 小端打包后 base64；
/// 320 帧 @16k/mono ≈ 856 字符，20ms 一帧对 UDS 无压力（shm 零拷贝随 M5）。
/// 负载里**不含 topic**（topic 由 rr-bus 客户端并入，见 client.rs 的 rr3_publish）。
pub fn rr3_pcm_event_body(seq: u64, pcm: &Rr3PcmStream) -> Value {
    let mut bytes = Vec::with_capacity(pcm.samples.len() * 2);
    for s in &pcm.samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    json!({
        "seq": seq,
        "format": "s16le",
        "rate": pcm.cfg.sample_rate_hz,
        "channels": pcm.cfg.channels,
        "ms": pcm.rr3_pcm_duration_ms(),
        "pcm_b64": rr3_base64_encode(&bytes),
    })
}

// ---------- DoA 粗判向（批F2，方案 §5.3 判向-转向编排 / R14） ----------
//
// 双麦**幅度差（ILD）**粗判向：逐通道 RMS 的线性比值直接反演 `sin θ`
// （注入模型 `g_l = 1 - sinθ`、`g_r = 1 + sinθ` 与 rr-hal sim 源同一套，
// 反演可逆且确定——[`rr3_doa_fix`] 的单测与 rr-hal 的注入单测互为正反面）。
// 相位差（ITD）在 sim 源里是保真项不是判据：粗判向 ±90° 分档走幅度差，
// 真件换装（批F5a，USB 麦阵列）按实测阵列特性再评估是否升级互相关测向。
//
// 落档口径（零新契约）：结果只随在册主题 `rr3-sense/audio/vad_start` 的
// **onset** 边沿上路（`doa` 字段）——offset 静音帧无方向可判，不带 doa
// （与 rms_peak 同一形状纪律）。

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

/// vad_start onset 的 `doa` 字段体（schema：deg + bin 两键，additionalProperties=false）。
/// 判向机制在本原子（原始帧下混前判定），字段形状随承载事件落 vad 原子——组合件
/// 桥接两原子时以此值为载荷（跨原子零类型依赖，只过 serde_json::Value）。
pub fn rr3_doa_field(d: &Rr3DoaFix) -> Value {
    json!({ "deg": d.deg, "bin": d.bin.rr3_doa_bin_as_str() })
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

    /// RFC 4648 标准字母表（测试侧独立解码器用，与实现侧各持一份）
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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

    /// 流级下混：channels 归 1、rate 不动、samples 收缩；单麦原样返回（零改动）。
    #[test]
    fn downmix_stream_rewrites_cfg_to_mono() {
        let stereo = Rr3PcmStream {
            cfg: Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 2,
                bit_depth: 16,
            },
            samples: vec![100, 300, -400, -200],
        };
        let mono = rr3_pcm_downmix_stream(stereo);
        assert_eq!(mono.cfg.channels, 1);
        assert_eq!(mono.cfg.sample_rate_hz, 16000);
        assert_eq!(mono.samples, vec![200, -300]);
        let one = Rr3PcmStream {
            cfg: Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 1,
                bit_depth: 16,
            },
            samples: vec![7, 8, 9],
        };
        assert_eq!(rr3_pcm_downmix_stream(one.clone()), one);
    }

    #[test]
    fn pcm_event_body_round_trips_through_base64() {
        let pcm = Rr3PcmStream {
            cfg: Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 1,
                bit_depth: 16,
            },
            samples: vec![1000, -2000, 32767, -32768],
        };
        let body = rr3_pcm_event_body(7, &pcm);
        assert_eq!(body["seq"], 7);
        assert_eq!(body["format"], "s16le");
        assert_eq!(body["rate"], 16000);
        assert_eq!(body["channels"], 1);
        assert_eq!(body["ms"], 0, "4 帧 @16k 不足 1ms");
        assert_eq!(
            body.get("topic"),
            None,
            "负载不重复放 topic（由客户端并入）"
        );
        // 解回字节序，逐点核对（含 i16 两个端点值）。测试侧独立解码器：按 `=` 位
        // 决定该组贡献几个数据字节（与编码器的补位规则对称）。
        let b64 = body["pcm_b64"].as_str().unwrap();
        let alphabet = |c: char| B64.iter().position(|&a| a as char == c).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        for q in b64.as_bytes().chunks(4) {
            let v = |c: u8| alphabet(c as char) as u32;
            let n = (v(q[0]) << 18)
                | (v(q[1]) << 12)
                | (if q[2] == b'=' { 0 } else { v(q[2]) << 6 })
                | (if q[3] == b'=' { 0 } else { v(q[3]) });
            bytes.push((n >> 16) as u8);
            if q[2] != b'=' {
                bytes.push((n >> 8) as u8);
            }
            if q[3] != b'=' {
                bytes.push(n as u8);
            }
        }
        assert_eq!(bytes.len(), 8);
        for (i, s) in pcm.samples.iter().enumerate() {
            assert_eq!(i16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]), *s);
        }
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
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|f| [f[0], f[1], -777])
            .collect();
        let a = rr3_doa_fix(&stereo, 2).unwrap();
        let b = rr3_doa_fix(&three, 3).unwrap();
        assert_eq!(a, b, "第三路不得影响 L/R 对的判向");
    }

    /// doa 字段体形状：deg + bin 两键（schema additionalProperties=false，多带即违约）。
    #[test]
    fn doa_field_carries_deg_and_bin_only() {
        let v = rr3_doa_field(&Rr3DoaFix {
            deg: -60,
            bin: Rr3DoaBin::Left,
        });
        assert_eq!(v["deg"], -60);
        assert_eq!(v["bin"], "left");
        assert_eq!(v.as_object().unwrap().len(), 2, "字段超集即违约: {v}");
    }
}
