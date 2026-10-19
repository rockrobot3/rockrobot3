//! rr3-sense 组合件实现（组合件五件套③，0.0.5 批C① 原子重铺）。
//!
//! 组合件 = capture ⊗ vad ⊗ env ⊗ imu（composite.toml 的 ⊗ 链；⊗=装配期、
//! 执行单元内函数调用，ADR-0019 裁定 2）。本文件承载装配与运行机制：config
//! 解析（阈值等策略经 config 数据化注入，机制件零策略载荷）、事务面
//! `rr3-sense/capture/dump` 的点名应答、主循环。载荷组装与参数下钻面同在此处
//! （0.0.5 批I S-7，ADR-0019 修订 4：capture/vad 两件的 `*_body`/`*_field`
//! 上移，件面 serde_json 清零，用例随迁）。
//!
//! 启动序列（自原入口 rr3_run 逐字迁入，语义零改动）：读 `RR3_MANIFEST`
//! （rrd spawn 注入的 manifest 绝对路径，supervisor.rs）→ [`Rr3BusClient::rr3_connect`]
//! （连接/注册/起心跳）→ 按 `RR3_HAL_BACKEND` 选音频源 → 采集循环。循环三件事：
//! 1. **自 pacing** 到 frame_ms 节拍——sim 源的 read 即时返回（无时钟），不睡会全速
//!    刷屏总线；实时源（ALSA）read 本身就耗一个节拍，sleep 余量为零，两种后端同一形状；
//! 2. 发布 `rr3-sense/audio/pcm`（stream）与 VAD 边沿 `rr3-sense/audio/vad_start`
//!    （event；onset 的 doa 字段体由 capture 原子判出、组合件桥接进 vad 事件体）；
//! 3. `rr3_poll` 排空 inbox，应答 `rr3-sense/capture/dump` 点名（rr3.call 回路）。
//!
//! 发布/读帧失败一律 `eprintln` + `exit(1)`：模块不装"活着"，死交给 supervisor 按
//! restart_policy=always 重启（rrd 的进程语义）。

#[path = "../atoms/capture/capture.rs"]
mod capture;
#[path = "../atoms/env/env.rs"]
mod env;
#[path = "../atoms/imu/imu.rs"]
mod imu;
#[path = "../atoms/vad/vad.rs"]
mod vad;

use std::time::{Duration, Instant};

use rr_bus::Rr3BusClient;
use rr_hal::backend::rr3_hal_audio_source;
use rr_hal::Rr3PcmStream;
use rr_hal_api::Rr3AudioCfg;
use rr_ipc::{opcode, rr3_base64_encode, Rr3PayloadType};
use serde_json::{json, Value};

use capture::{Rr3AudioConfig, Rr3DoaFix};
use vad::{Rr3VadConfig, Rr3VadEdge, Rr3VadState};

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// 本模块 exposes 的唯一事务面主题（module.toml [rpc].exposes 同源）
const RR3_DUMP_TOPIC: &str = "rr3-sense/capture/dump";

/// 采集新鲜度运维计数键（批M M-β S-3）：`pcm_frames` 每实际上总线一帧 +1；
/// `pcm_empty_frames` 读回欠载空帧（read 实填 0 采样——"活着但不动"的饿死
/// 信号，MC-11 判据：卡死与静默停摆看得见）另账。经心跳自报 rrd 归并，
/// `rr3.ctl.counters` 读面键 `rr3-sense/pcm_frames` / `rr3-sense/pcm_empty_frames`。
const RR3_PCM_FRAMES_KEY: &str = "pcm_frames";
const RR3_PCM_EMPTY_FRAMES_KEY: &str = "pcm_empty_frames";

/// 组合件入口：装配原子件并跑主循环（原 main.rs `rr3_run` 逐字迁入，打印序列不变）。
pub fn rr3_sense_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    eprintln!("rr3-sense 0.0.1 (M3 实装)");
    for line in capture::SURFACE
        .iter()
        .copied()
        .chain(vad::SURFACE.iter().copied())
        .chain(env::SURFACE.iter().copied())
        .chain(imu::SURFACE.iter().copied())
    {
        eprintln!("  {line}");
    }

    // 总线接入：manifest 路径是 rrd spawn 的契约环境变量，直跑（开发调试）须自行设置
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let bus = Rr3BusClient::rr3_connect(&manifest_text)?;
    eprintln!("总线就绪: name={} task={}", bus.rr3_name(), bus.rr3_task());

    let cfg = rr3_sense_config_load(&manifest_path)?;
    let (mut source, backend) = rr3_hal_audio_source();
    // 采集配置按 config 真值开（R14：channels>1 即双麦期；默认 1 声道与旧硬编码逐点等价）
    let open_cfg = Rr3AudioCfg {
        sample_rate_hz: cfg.audio.sample_rate,
        channels: cfg.audio.channels,
        bit_depth: 16,
    };
    source
        .open(open_cfg)
        .map_err(|e| format!("打开音频源({backend})失败: {e}"))?;
    eprintln!(
        "音频源就绪: backend={backend} rate={} ch={} frame_ms={}",
        cfg.audio.sample_rate, cfg.audio.channels, cfg.audio.frame_ms
    );

    let frames_per_tick = capture::rr3_frames_per_tick(cfg.audio.sample_rate, cfg.audio.frame_ms);
    let mut vad_state = Rr3VadState::rr3_vad_new(cfg.vad, cfg.audio.frame_ms);
    let tick = Duration::from_millis(cfg.audio.frame_ms);
    let mut seq: u64 = 0;

    loop {
        let start = Instant::now();

        // 读一拍：buf 按"帧×声道"分配采样点数（F1 定形 read 收采样点 buf，
        // 返回实填采样点数，欠载按实数截断）
        let mut buf = vec![0i16; frames_per_tick * cfg.audio.channels as usize];
        let done = source
            .read(&mut buf)
            .map_err(|e| format!("读帧失败({backend}): {e}"))?;
        buf.truncate(done);
        let raw = Rr3PcmStream {
            cfg: open_cfg,
            samples: buf,
        };
        // DoA 粗判向（批F2）：多通道帧在下混**前**判向（双麦原始通道只在此一用），
        // 最新一档随下一次 onset 边沿上路；单声道/静音帧恒 None，事件体不带 doa
        let doa = capture::rr3_doa_fix(&raw.samples, raw.cfg.channels, cfg.doa.bin_threshold_deg);
        // 多通道下混 mono 入总线（R14）：双麦原始多通道只供 sense 内部（DoA 随批F2），
        // 总线承载恒 mono——brain 消费面零改动；单麦链路 channels=1 时零拷贝等价
        let pcm = rr3_pcm_downmix_stream(raw);
        // 采集新鲜度记账（批M M-β S-3）：空帧另账（欠载饿死信号），帧账在发布
        // 成功后 +1——记账 = 真上了总线的帧数（发布失败即 exit，临死一帧不入账）
        if done == 0 {
            bus.rr3_counter_bump(RR3_PCM_EMPTY_FRAMES_KEY, 1);
        }
        seq += 1;
        bus.rr3_publish(capture::RR3_PCM_TOPIC, rr3_pcm_event_body(seq, &pcm))?;
        bus.rr3_counter_bump(RR3_PCM_FRAMES_KEY, 1);

        if let Some(edge) = vad_state.rr3_vad_feed(&pcm.samples) {
            bus.rr3_publish(
                vad::RR3_VAD_START_TOPIC,
                rr3_vad_event_body(seq, edge, doa.map(|d| rr3_doa_field(&d))),
            )?;
        }

        // 排空 inbox：v0.0.1 不订阅任何主题，inbox 只会有点名 Request——
        // rr3.call 转发帧的 trace_id=调用方 task（rrd 路由语义），顺原路应答
        for frame in bus.rr3_poll() {
            if frame.header.opcode != opcode::SEND {
                continue;
            }
            let Ok((Rr3PayloadType::Request, body)) = Rr3BusClient::rr3_frame_body(&frame) else {
                continue;
            };
            if body.get("method").and_then(Value::as_str) != Some(rr_bus::RR3_METHOD_CALL) {
                continue;
            }
            let id = body.get("id").and_then(Value::as_u64).unwrap_or(0);
            let topic = body
                .get("params")
                .and_then(|p| p.get("topic"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let result = if topic == RR3_DUMP_TOPIC {
                rr3_dump_result(seq, backend, &cfg)
            } else {
                json!({ "error": format!("rr3-sense 只 exposes {RR3_DUMP_TOPIC}，收到 {topic}") })
            };
            bus.rr3_reply_call(frame.header.trace_id, id, result)?;
        }

        // 自 pacing：补足本节拍剩余时间（sim read 即时返回的关键；
        // 抖动可接受——v0.0.1 的 pcm 流只供联调，时序对齐随 M5 shm）
        let spent = start.elapsed();
        if spent < tick {
            std::thread::sleep(tick - spent);
        }
    }
}

// ---------- 载荷组装与参数下钻面（0.0.5 批I S-7，ADR-0019 修订 4） ----------
//
// capture/vad 两件的 `*_body`/`*_field` 自原子面整体上移至此（代码与文档注释
// 逐字迁入，线上字节不变）：件面只留纯机制（结构体+判定函数），serde_json 清零；
// 载荷组装集中组合件一处，用例随迁（原子测"输入→机制"，本层测"输入→字节"）。

/// 流级下混（R14）：`cfg.channels` 归 1、rate/bit_depth 不动、samples 收缩；
/// `channels <= 1` 时原样返回（零改动，默认单麦链路逐点等价）。自 capture/mech.rs
/// 迁入（0.0.5 批I 深挖收口：`Rr3PcmStream` 是承载层数据类型，件面只留纯切片
/// 机制——C7 白名单外件面不得引用 rr_hal 具体类型）；纯切片下混
/// [`capture::rr3_pcm_downmix_mono`] 仍在件内。
fn rr3_pcm_downmix_stream(pcm: Rr3PcmStream) -> Rr3PcmStream {
    if pcm.cfg.channels <= 1 {
        return pcm;
    }
    Rr3PcmStream {
        cfg: Rr3AudioCfg {
            channels: 1,
            ..pcm.cfg
        },
        samples: capture::rr3_pcm_downmix_mono(&pcm.samples, pcm.cfg.channels),
    }
}

/// `rr3-sense/audio/pcm` 的事件体（stream）。PCM 按 s16le 小端打包后 base64；
/// 320 帧 @16k/mono ≈ 856 字符，20ms 一帧对 UDS 无压力（shm 零拷贝随 M5）。
/// 负载里**不含 topic**（topic 由 rr-bus 客户端并入，见 client.rs 的 rr3_publish）。
fn rr3_pcm_event_body(seq: u64, pcm: &Rr3PcmStream) -> Value {
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
/// 判向机制在 capture 原子（原始帧下混前判定），字段组装在此（S-7 上移）——
/// 判向结果经 [`rr3_doa_fix`] 的返回值上行，跨原子零类型依赖，只过 serde_json::Value。
fn rr3_doa_field(d: &Rr3DoaFix) -> Value {
    json!({ "deg": d.deg, "bin": d.bin.rr3_doa_bin_as_str() })
}

/// `rr3-sense/audio/vad_start` 的事件体（event）。边沿负载只带摘要
/// （edge / rms_peak / speech_ms），不带任何闭环数据（ADR-0007 §决策一）。
/// 负载里**不含 topic**（topic 由 rr-bus 客户端并入，见 client.rs 的 rr3_publish）。
/// `doa` 字段体由 capture 原子判出（deg + bin 两键）、此处组装并桥接进 onset——
/// 批F2 的负载扩展（append 兼容，schema_version 仍 1）：仅 onset 且判出
/// （多通道非静音帧）时携带，offset 不带——与 rms_peak 同一形状纪律。
fn rr3_vad_event_body(seq: u64, edge: Rr3VadEdge, doa: Option<Value>) -> Value {
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

// ---------- 事务面应答体 ----------

/// `rr3-sense/capture/dump`（rpc.exposes 的事务面）的应答体：
/// 当前采集状态快照，供联调对表（后端名、生效配置、已发帧序号）。
fn rr3_dump_result(seq: u64, backend: &str, cfg: &Rr3SenseConfig) -> Value {
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

// ---------- 配置加载（装配期注入，原 audio/mod.rs 配置段逐字迁入） ----------

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml）
const RR3_SENSE_CONFIG_ENV: &str = "RR3_SENSE_CONFIG";

/// 模块全量运行期配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SenseConfig {
    pub audio: Rr3AudioConfig,
    pub vad: Rr3VadConfig,
    pub doa: Rr3DoaConfig,
}

/// DoA 判向配置：分档边界（S-5，原件内常量 `RR3_DOA_BIN_THRESHOLD_DEG` 迁此）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3DoaConfig {
    pub bin_threshold_deg: i32,
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
            doa: Rr3DoaConfig {
                bin_threshold_deg: 30,
            },
        }
    }
}

/// 读配置：`RR3_SENSE_CONFIG` 覆盖优先，缺省读 manifest 同目录的 config.toml
/// （0.0.5 批I S-1：与 power/connect 同口径——原"未设 env 则整份 config.toml
/// 不生效、静默用代码缺省"的分叉消灭；代码侧 `Default` 仍是文件缺键的回落）。
/// 文件读不到/非法 = 显式报错退出（半读到的配置比没有更危险）。
/// 生效来源随启动日志写明。
fn rr3_sense_config_load(manifest_path: &str) -> Result<Rr3SenseConfig, String> {
    let path = std::env::var(RR3_SENSE_CONFIG_ENV).unwrap_or_else(|_| {
        std::path::Path::new(manifest_path)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("config.toml")
            .to_string_lossy()
            .into_owned()
    });
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读 config {path} 失败: {e}"))?;
    let cfg = rr3_sense_config_parse(&text)?;
    eprintln!(
        "config 就绪: 来源={path} doa_bin_threshold_deg={}",
        cfg.doa.bin_threshold_deg
    );
    Ok(cfg)
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
        // 0.0.5 批I S-1：frame_ms < 1（u64 域即 0）无节拍意义（0 帧 → read 空
        // → 全速刷总线），收敛到登记默认 20ms 并留痕——半有效的配置比报错更隐蔽
        if fms < 1 {
            eprintln!("config [rr3-sense.audio] frame_ms={fms} 无节拍意义，收敛 20ms");
            fms = 20;
        }
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
    // S-5：DoA 分档边界（策略面数据化，缺键回落默认 30）
    if let Some(doa) = section.and_then(|s| s.get("doa")) {
        let mut t = cfg.doa.bin_threshold_deg as i64;
        rr3_cfg_i64(doa, "bin_threshold_deg", &mut t)?;
        cfg.doa.bin_threshold_deg = i32::try_from(t)
            .map_err(|_| format!("[rr3-sense.doa] bin_threshold_deg 越界: {t}"))?;
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
    use capture::Rr3DoaBin;

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
             [rr3-sense.vad]\nrms_start = 2000\nsilence_tail_ms = 500\nmin_speech_ms = 150\n\n\
             [rr3-sense.doa]\nbin_threshold_deg = 20\n",
        )
        .unwrap();
        assert_eq!(full.audio.sample_rate, 48000);
        assert_eq!(full.audio.channels, 2);
        assert_eq!(full.audio.frame_ms, 10);
        assert_eq!(full.vad.rms_start, 2000);
        assert_eq!(full.vad.silence_tail_ms, 500);
        assert_eq!(full.vad.min_speech_ms, 150);
        assert_eq!(full.doa.bin_threshold_deg, 20, "S-5 阈值注入生效");
    }

    #[test]
    fn config_parse_rejects_wrong_types_and_broken_toml() {
        // 类型错显式报错，不静默吞
        assert!(rr3_sense_config_parse("[rr3-sense.vad]\nrms_start = \"loud\"\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.audio]\nframe_ms = -20\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nbin_threshold_deg = \"wide\"\n").is_err());
        assert!(rr3_sense_config_parse("not toml at all [").is_err());
    }

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

    /// 0.0.5 批I S-1：frame_ms=0 无节拍意义，收敛 20 留痕（eprintln 进测试输出无害）；
    /// 负值仍被 rr3_cfg_u64 显式拒收，不参与收敛。
    #[test]
    fn config_parse_clamps_zero_frame_ms_to_registered_20() {
        let cfg = rr3_sense_config_parse("[rr3-sense.audio]\nframe_ms = 0\n").unwrap();
        assert_eq!(cfg.audio.frame_ms, 20);
        assert!(rr3_sense_config_parse("[rr3-sense.audio]\nframe_ms = -5\n").is_err());
    }

    // ---------- 载荷组装用例（随 S-7 自原子面随迁） ----------

    /// 流级下混：channels 归 1、rate 不动、samples 收缩；单麦原样返回（零改动）。
    /// 随下混函数自 capture/mech.rs 迁入（0.0.5 批I 深挖收口）。
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
