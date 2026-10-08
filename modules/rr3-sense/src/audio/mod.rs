//! 采集域：麦克风阵列 PCM 抓取 + 本地 VAD 边沿检测。
//!
//! 本文件承载两件与传输形状有关的事（0.0.5 批C①：base64 编码已塌缩进 rr-ipc
//! 线格式层，方案 §12.3——PCM 二进制进 JSON 的通道归 `rr_ipc::rr3_base64_encode`）：
//! 1. 事件体构造器（[`rr3_pcm_event_body`] / [`rr3_vad_event_body`]）——负载里
//!    **不含 topic**（topic 由 rr-bus 客户端并入，见 client.rs 的 rr3_publish）。
//! 2. 配置加载（[`rr3_sense_config_load`]）——`RR3_SENSE_CONFIG` 指路径，
//!    缺省用登记值；缺键回落默认，类型错显式报错。

pub mod capture;
pub mod doa;
pub mod vad;

use rr_hal::{Rr3AudioCfg, Rr3PcmStream};
use rr_ipc::rr3_base64_encode;
use serde_json::{json, Value};

pub use doa::Rr3DoaFix;
pub use vad::{Rr3VadConfig, Rr3VadEdge, Rr3VadState};

pub fn surface() -> impl Iterator<Item = &'static str> {
    capture::SURFACE
        .iter()
        .chain(doa::SURFACE.iter())
        .chain(vad::SURFACE.iter())
        .copied()
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

// ---------- 事件体构造 ----------

/// `rr3-sense/audio/pcm` 的事件体（stream）。PCM 按 s16le 小端打包后 base64；
/// 320 帧 @16k/mono ≈ 856 字符，20ms 一帧对 UDS 无压力（shm 零拷贝随 M5）。
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

/// `rr3-sense/audio/vad_start` 的事件体（event）。边沿负载只带摘要
/// （edge / rms_peak / speech_ms），不带任何闭环数据（ADR-0007 §决策一）。
/// `doa` 是批F2 的负载扩展（append 兼容，schema_version 仍 1）：判向结果由
/// sense 侧音频格位承载（零新契约），仅 onset 且判出（多通道非静音帧）时携带，
/// offset 不带——与 rms_peak 同一形状纪律。
pub fn rr3_vad_event_body(seq: u64, edge: Rr3VadEdge, doa: Option<Rr3DoaFix>) -> Value {
    let mut body = match edge {
        Rr3VadEdge::Onset {
            rms_peak,
            speech_ms,
        } => json!({
            "seq": seq, "edge": "onset", "rms_peak": rms_peak, "speech_ms": speech_ms,
        }),
        Rr3VadEdge::Offset { speech_ms } => json!({
            "seq": seq, "edge": "offset", "speech_ms": speech_ms,
        }),
    };
    if let (Rr3VadEdge::Onset { .. }, Some(d)) = (&edge, doa) {
        body["doa"] = json!({ "deg": d.deg, "bin": d.bin.rr3_doa_bin_as_str() });
    }
    body
}

/// `rr3-sense/capture/dump`（rpc.exposes 的事务面）的应答体：
/// 当前采集状态快照，供联调对表（后端名、生效配置、已发帧序号）。
pub fn rr3_dump_result(seq: u64, backend: &str, cfg: &Rr3SenseConfig) -> Value {
    json!({
        "seq": seq,
        "backend": backend,
        "sample_rate": cfg.audio.sample_rate,
        "channels": cfg.audio.channels,
        "frame_ms": cfg.audio.frame_ms,
        "vad": {
            "rms_start": cfg.vad.rms_start,
            "silence_tail_ms": cfg.vad.silence_tail_ms,
            "min_speech_ms": cfg.vad.min_speech_ms,
        },
    })
}

// ---------- 配置加载 ----------

/// 配置文件路径的环境变量键；未设 = 用登记默认值（config.toml 的值即默认值的登记处）
pub const RR3_SENSE_CONFIG_ENV: &str = "RR3_SENSE_CONFIG";

/// 采集参数（config.toml `[rr3-sense.audio]`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3AudioConfig {
    pub sample_rate: u32,
    pub channels: u8,
    pub frame_ms: u64,
}

/// 模块全量运行期配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SenseConfig {
    pub audio: Rr3AudioConfig,
    pub vad: Rr3VadConfig,
}

impl Default for Rr3SenseConfig {
    fn default() -> Self {
        Self {
            audio: Rr3AudioConfig {
                sample_rate: 16000,
                channels: 1,
                frame_ms: 20,
            },
            vad: Rr3VadConfig::default(),
        }
    }
}

/// 读配置：`RR3_SENSE_CONFIG` 指向 TOML 文件；未设/读不到路径变量时用默认值。
/// 文件存在但非法 = 显式报错退出（半读到的配置比没有更危险）。
pub fn rr3_sense_config_load() -> Result<Rr3SenseConfig, String> {
    match std::env::var(RR3_SENSE_CONFIG_ENV) {
        Err(_) => Ok(Rr3SenseConfig::default()),
        Ok(path) => {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("读配置 {path} 失败: {e}"))?;
            rr3_sense_config_parse(&text)
        }
    }
}

/// 解析配置文本：缺键回落默认，类型错显式报错（不静默吞）。
fn rr3_sense_config_parse(text: &str) -> Result<Rr3SenseConfig, String> {
    let root: toml::Value = text
        .parse()
        .map_err(|e| format!("配置不是合法 TOML: {e}"))?;
    let mut cfg = Rr3SenseConfig::default();
    let section = root.get("rr3-sense");
    if let Some(audio) = section.and_then(|s| s.get("audio")) {
        let mut rate = cfg.audio.sample_rate as u64;
        let mut ch = cfg.audio.channels as u64;
        let mut fms = cfg.audio.frame_ms;
        rr3_cfg_u64(audio, "sample_rate", &mut rate)?;
        rr3_cfg_u64(audio, "channels", &mut ch)?;
        rr3_cfg_u64(audio, "frame_ms", &mut fms)?;
        cfg.audio.sample_rate = u32::try_from(rate)
            .map_err(|_| format!("[rr3-sense.audio] sample_rate 越界: {rate}"))?;
        cfg.audio.channels =
            u8::try_from(ch).map_err(|_| format!("[rr3-sense.audio] channels 越界: {ch}"))?;
        cfg.audio.frame_ms = fms;
    }
    if let Some(vad) = section.and_then(|s| s.get("vad")) {
        rr3_cfg_i64(vad, "rms_start", &mut cfg.vad.rms_start)?;
        rr3_cfg_u64(vad, "silence_tail_ms", &mut cfg.vad.silence_tail_ms)?;
        rr3_cfg_u64(vad, "min_speech_ms", &mut cfg.vad.min_speech_ms)?;
    }
    Ok(cfg)
}

fn rr3_cfg_u64(table: &toml::Value, key: &str, slot: &mut u64) -> Result<(), String> {
    match table.get(key) {
        None => Ok(()),
        Some(v) => {
            *slot = v
                .as_integer()
                .and_then(|i| u64::try_from(i).ok())
                .ok_or_else(|| format!("配置项 {key} 须为非负整数，收到 {v}"))?;
            Ok(())
        }
    }
}

fn rr3_cfg_i64(table: &toml::Value, key: &str, slot: &mut i64) -> Result<(), String> {
    match table.get(key) {
        None => Ok(()),
        Some(v) => {
            *slot = v
                .as_integer()
                .ok_or_else(|| format!("配置项 {key} 须为整数，收到 {v}"))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::Rr3AudioCfg;

    /// RFC 4648 标准字母表（测试侧独立解码器用，与实现侧各持一份）
    const B64: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
        let alphabet = |c: char| {
            B64.iter()
                .position(|&a| a as char == c)
                .unwrap()
        };
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

    #[test]
    fn vad_event_body_carries_edge_shape() {
        let on = rr3_vad_event_body(
            3,
            Rr3VadEdge::Onset {
                rms_peak: 4200,
                speech_ms: 200,
            },
            None,
        );
        assert_eq!(on["edge"], "onset");
        assert_eq!(on["rms_peak"], 4200);
        assert_eq!(on["speech_ms"], 200);
        assert_eq!(on["seq"], 3);
        assert!(
            on.get("doa").is_none(),
            "无判向结果的 onset 不带 doa 字段"
        );
        let off = rr3_vad_event_body(4, Rr3VadEdge::Offset { speech_ms: 900 }, None);
        assert_eq!(off["edge"], "offset");
        assert_eq!(off["speech_ms"], 900);
        assert_eq!(off.get("rms_peak"), None, "offset 不带 rms_peak");
    }

    /// doa 字段（批F2 负载扩展）：onset 携带判向档（deg+bin），offset 即使传了
    /// 也丢弃——静音帧无方向可判，形状纪律与 rms_peak 同一条。
    #[test]
    fn vad_event_body_carries_doa_on_onset_only() {
        let fix = Rr3DoaFix {
            deg: -60,
            bin: doa::Rr3DoaBin::Left,
        };
        let on = rr3_vad_event_body(
            5,
            Rr3VadEdge::Onset {
                rms_peak: 4200,
                speech_ms: 200,
            },
            Some(fix),
        );
        assert_eq!(on["doa"]["deg"], -60);
        assert_eq!(on["doa"]["bin"], "left");
        let off = rr3_vad_event_body(6, Rr3VadEdge::Offset { speech_ms: 900 }, Some(fix));
        assert!(
            off.get("doa").is_none(),
            "offset 不得携带 doa: {off}"
        );
    }

    #[test]
    fn config_parse_defaults_on_empty_and_fills_partial() {
        // 空文本 = 全默认
        assert_eq!(
            rr3_sense_config_parse("").unwrap(),
            Rr3SenseConfig::default()
        );
        // 只给部分键：给的生效，缺的回落默认
        let cfg = rr3_sense_config_parse("[rr3-sense.vad]\nrms_start = 900\n").unwrap();
        assert_eq!(cfg.vad.rms_start, 900);
        assert_eq!(cfg.vad.silence_tail_ms, 700);
        assert_eq!(cfg.audio.sample_rate, 16000);
        // 全量
        let full = rr3_sense_config_parse(
            "[rr3-sense.audio]\nsample_rate = 48000\nchannels = 2\nframe_ms = 10\n\n\
             [rr3-sense.vad]\nrms_start = 2000\nsilence_tail_ms = 500\nmin_speech_ms = 150\n",
        )
        .unwrap();
        assert_eq!(full.audio.sample_rate, 48000);
        assert_eq!(full.audio.channels, 2);
        assert_eq!(full.audio.frame_ms, 10);
        assert_eq!(full.vad.rms_start, 2000);
        assert_eq!(full.vad.silence_tail_ms, 500);
        assert_eq!(full.vad.min_speech_ms, 150);
    }

    #[test]
    fn config_parse_rejects_wrong_types_and_broken_toml() {
        // 类型错显式报错，不静默吞
        assert!(rr3_sense_config_parse("[rr3-sense.vad]\nrms_start = \"loud\"\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.audio]\nframe_ms = -20\n").is_err());
        assert!(rr3_sense_config_parse("not toml at all [").is_err());
    }
}
