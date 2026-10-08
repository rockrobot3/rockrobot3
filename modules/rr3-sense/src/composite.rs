//! rr3-sense 组合件实现（组合件五件套③，0.0.5 批C① 原子重铺）。
//!
//! 组合件 = capture ⊗ vad ⊗ env ⊗ imu（composite.toml 的 ⊗ 链；⊗=装配期、
//! 执行单元内函数调用，ADR-0019 裁定 2）。本文件承载装配与运行机制：config
//! 解析（阈值等策略经 config 数据化注入，机制件零策略载荷）、事务面
//! `rr3-sense/capture/dump` 的点名应答、主循环。
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
use rr_hal::{backend::rr3_hal_audio_source, rr3_block_on, Rr3AudioCfg};
use rr_ipc::{opcode, Rr3PayloadType};
use serde_json::{json, Value};

use capture::Rr3AudioConfig;
use vad::{Rr3VadConfig, Rr3VadState};

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// 本模块 exposes 的唯一事务面主题（module.toml [rpc].exposes 同源）
const RR3_DUMP_TOPIC: &str = "rr3-sense/capture/dump";

/// 组合件入口：装配原子件并跑主循环（原 main.rs `rr3_run` 逐字迁入，打印序列不变）。
pub fn rr3_sense_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    println!("rr3-sense 0.0.1 (M3 实装)");
    for line in capture::SURFACE
        .iter()
        .copied()
        .chain(vad::SURFACE.iter().copied())
        .chain(env::SURFACE.iter().copied())
        .chain(imu::SURFACE.iter().copied())
    {
        println!("  {line}");
    }

    // 总线接入：manifest 路径是 rrd spawn 的契约环境变量，直跑（开发调试）须自行设置
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let bus = Rr3BusClient::rr3_connect(&manifest_text)?;
    println!("总线就绪: name={} task={}", bus.rr3_name(), bus.rr3_task());

    let cfg = rr3_sense_config_load()?;
    let (source, backend) = rr3_hal_audio_source();
    // 采集配置按 config 真值开（R14：channels>1 即双麦期；默认 1 声道与旧硬编码逐点等价）
    let open_cfg = Rr3AudioCfg {
        sample_rate_hz: cfg.audio.sample_rate,
        channels: cfg.audio.channels,
        bit_depth: 16,
    };
    rr3_block_on(source.open(open_cfg)).map_err(|e| format!("打开音频源({backend})失败: {e}"))?;
    println!(
        "音频源就绪: backend={backend} rate={} ch={} frame_ms={}",
        cfg.audio.sample_rate, cfg.audio.channels, cfg.audio.frame_ms
    );

    let frames_per_tick = capture::rr3_frames_per_tick(cfg.audio.sample_rate, cfg.audio.frame_ms);
    let mut vad_state = Rr3VadState::rr3_vad_new(cfg.vad, cfg.audio.frame_ms);
    let tick = Duration::from_millis(cfg.audio.frame_ms);
    let mut seq: u64 = 0;

    loop {
        let start = Instant::now();

        let pcm = rr3_block_on(source.read(frames_per_tick))
            .map_err(|e| format!("读帧失败({backend}): {e}"))?;
        // DoA 粗判向（批F2）：多通道帧在下混**前**判向（双麦原始通道只在此一用），
        // 最新一档随下一次 onset 边沿上路；单声道/静音帧恒 None，事件体不带 doa
        let doa = capture::rr3_doa_fix(&pcm.samples, pcm.cfg.channels);
        // 多通道下混 mono 入总线（R14）：双麦原始多通道只供 sense 内部（DoA 随批F2），
        // 总线承载恒 mono——brain 消费面零改动；单麦链路 channels=1 时零拷贝等价
        let pcm = capture::rr3_pcm_downmix_stream(pcm);
        seq += 1;
        bus.rr3_publish(
            capture::RR3_PCM_TOPIC,
            capture::rr3_pcm_event_body(seq, &pcm),
        )?;

        if let Some(edge) = vad_state.rr3_vad_feed(&pcm.samples) {
            bus.rr3_publish(
                vad::RR3_VAD_START_TOPIC,
                vad::rr3_vad_event_body(seq, edge, doa.map(|d| capture::rr3_doa_field(&d))),
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

/// 配置文件路径的环境变量键；未设 = 用登记默认值（config.toml 的值即默认值的登记处）
pub const RR3_SENSE_CONFIG_ENV: &str = "RR3_SENSE_CONFIG";

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
fn rr3_sense_config_load() -> Result<Rr3SenseConfig, String> {
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
}
