//! rr3-sense 协议层（0.0.8 W-A 批3 拆分，ADR-0017 五层表协议层）。
//!
//! 载荷组装与参数下钻面（自原 composite.rs 迁入——0.0.5 批I S-7 上移面，代码
//! 与文档注释逐字迁入，线上字节不变）：pcm/vad 事件体、doa 字段体、dump/ops
//! 两点名应答体。capture/vad 两件的 `*_body`/`*_field` 自原子面整体上移至此
//! （件面只留纯机制，serde_json 清零；载荷组装集中协议层一处，用例随迁——
//! 原子测"输入→机制"，本层测"输入→字节"）。
//!
//! doa 字段体由 capture 原子判出（deg + bin 两键）、本层组装并桥接进 onset——
//! 判向结果经 `rr3_doa_fix` 的返回值上行，跨原子零类型依赖，只过 serde_json::Value。

use rr_hal::Rr3PcmStream;
use rr_ipc::rr3_base64_encode;
use serde_json::{json, Value};

use crate::boot::Rr3SenseConfig;
use crate::capture::Rr3DoaFix;
use crate::vad::Rr3VadEdge;

/// `rr3-sense/audio/pcm` 的事件体（stream）。PCM 按 s16le 小端打包后 base64；
/// 320 帧 @16k/mono ≈ 856 字符，20ms 一帧对 UDS 无压力。
/// 负载里**不含 topic**（topic 由 rr-bus 客户端并入，见 client.rs 的 rr3_publish）。
pub(crate) fn rr3_pcm_event_body(seq: u64, pcm: &Rr3PcmStream) -> Value {
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

/// vad_start onset 的 `doa` 字段体（schema：deg + bin 两键，additionalProperties=false）。
/// 判向机制在 capture 原子（原始帧下混前判定），字段组装在此（S-7 上移）。
pub(crate) fn rr3_doa_field(d: &Rr3DoaFix) -> Value {
    json!({ "deg": d.deg, "bin": d.bin.rr3_doa_bin_as_str() })
}

/// `rr3-sense/audio/vad_start` 的事件体（event）。边沿负载只带摘要
/// （edge / rms_peak / speech_ms），不带任何闭环数据（ADR-0007 §决策一）。
/// 负载里**不含 topic**（topic 由 rr-bus 客户端并入，见 client.rs 的 rr3_publish）。
/// `doa` 字段体由 capture 原子判出（deg + bin 两键）、此处组装并桥接进 onset——
/// 批F2 的负载扩展（append 兼容，schema_version 仍 1）：仅 onset 且判出
/// （多通道非静音帧）时携带，offset 不带——与 rms_peak 同一形状纪律。
pub(crate) fn rr3_vad_event_body(seq: u64, edge: Rr3VadEdge, doa: Option<Value>) -> Value {
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
        body["doa"] = d;
    }
    body
}

/// `rr3-sense/capture/dump`（rpc.exposes 的事务面）的应答体：
/// 当前采集状态快照，供联调对表（后端名、生效配置、已发帧序号）。
pub(crate) fn rr3_dump_result(seq: u64, backend: &str, cfg: &Rr3SenseConfig) -> Value {
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

/// `rr3-sense/ops/list` 应答体（批N4 件九二，面板 /api/sense/ops 读口）：ops 槽
/// 快照，**白名单组体**——槽名与 assemble.toml 槽表申报同源（audio/doa/vad 三组，
/// 门 d covers 申报的对账面），config 其余键一律不进应答。槽名 doa_* 与 config 键
/// 的映射是装配清单 src_key 的事（槽表申报同源；b3b 起 doa 组扩到四槽）。
pub(crate) fn rr3_sense_ops_snapshot(cfg: &Rr3SenseConfig) -> Value {
    json!({
        "audio": {
            "sample_rate": cfg.audio.sample_rate,
            "channels": cfg.audio.channels,
            "frame_ms": cfg.audio.frame_ms,
        },
        "doa": {
            "doa_bin_threshold_deg": cfg.doa.bin_threshold_deg,
            "doa_window_ms": cfg.doa.window_ms,
            "doa_window_rms_min": cfg.doa.window_rms_min,
            "doa_mic_baseline_mm": cfg.doa.mic_baseline_mm,
        },
        "vad": {
            "rms_start": cfg.vad.rms_start,
            "silence_tail_ms": cfg.vad.silence_tail_ms,
            "min_speech_ms": cfg.vad.min_speech_ms,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::Rr3DoaBin;
    use rr_hal_api::Rr3AudioCfg;

    /// 事务面应答体形状：dump 快照逐字段点名（后端名、生效配置、vad 阈值组）。
    #[test]
    fn dump_result_carries_backend_cfg_and_vad_section() {
        let cfg = Rr3SenseConfig::default();
        let v = rr3_dump_result(9, "sim", &cfg);
        assert_eq!(v["seq"], 9);
        assert_eq!(v["backend"], "sim");
        assert_eq!(v["sample_rate"], 16000);
        assert_eq!(v["channels"], 1);
        assert_eq!(v["frame_ms"], 20);
        assert_eq!(v["vad"]["rms_start"], 1500);
        assert_eq!(v["vad"]["silence_tail_ms"], 700);
        assert_eq!(v["vad"]["min_speech_ms"], 200);
    }

    /// ops 槽快照（批N4 面 + b3b 三新槽）：白名单十槽应答，槽名与 assemble.toml
    /// 槽表申报同源；白名单纪律 = 组内恰好这些槽，config 其余键不进应答。
    #[test]
    fn ops_snapshot_carries_whitelisted_slots_including_doa_window() {
        let v = rr3_sense_ops_snapshot(&Rr3SenseConfig::default());
        assert_eq!(v["audio"]["sample_rate"], 16000);
        assert_eq!(v["audio"]["channels"], 1);
        assert_eq!(v["audio"]["frame_ms"], 20);
        assert_eq!(v["doa"]["doa_bin_threshold_deg"], 30);
        assert_eq!(v["doa"]["doa_window_ms"], 1000);
        assert_eq!(v["doa"]["doa_window_rms_min"], 800);
        assert_eq!(v["doa"]["doa_mic_baseline_mm"], 150);
        assert_eq!(v["vad"]["rms_start"], 1500);
        assert_eq!(v["vad"]["silence_tail_ms"], 700);
        assert_eq!(v["vad"]["min_speech_ms"], 200);
        assert_eq!(v["doa"].as_object().unwrap().len(), 4, "doa 组恰好四槽: {v}");
    }

    // ---------- 载荷组装用例（随 S-7 自原子面随迁） ----------

    /// RFC 4648 标准字母表（测试侧独立解码器用，与实现侧各持一份）
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
        assert_eq!(on.get("topic"), None, "负载不重复放 topic（由客户端并入）");
        assert!(on.get("doa").is_none(), "无判向结果的 onset 不带 doa 字段");
        let off = rr3_vad_event_body(4, Rr3VadEdge::Offset { speech_ms: 900 }, None);
        assert_eq!(off["edge"], "offset");
        assert_eq!(off["speech_ms"], 900);
        assert_eq!(off.get("rms_peak"), None, "offset 不带 rms_peak");
    }

    /// doa 字段（批F2 负载扩展）：onset 携带判向档（deg+bin），offset 即使传了
    /// 也丢弃——静音帧无方向可判，形状纪律与 rms_peak 同一条。
    /// 字段体由 capture 原子产出（deg+bin 两键），此处只钉桥接形状。
    #[test]
    fn vad_event_body_carries_doa_on_onset_only() {
        let on = rr3_vad_event_body(
            5,
            Rr3VadEdge::Onset {
                rms_peak: 4200,
                speech_ms: 200,
            },
            Some(json!({ "deg": -60, "bin": "left" })),
        );
        assert_eq!(on["doa"]["deg"], -60);
        assert_eq!(on["doa"]["bin"], "left");
        let off = rr3_vad_event_body(
            6,
            Rr3VadEdge::Offset { speech_ms: 900 },
            Some(json!({ "deg": 0, "bin": "center" })),
        );
        assert!(off.get("doa").is_none(), "offset 不得携带 doa: {off}");
    }
}
