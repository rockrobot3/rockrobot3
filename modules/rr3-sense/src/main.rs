//! rr3-sense 真入口（M3）：总线注册/心跳 + 采集循环 + VAD 边沿 + PCM 上行。
//!
//! 启动序列：读 `RR3_MANIFEST`（rrd spawn 时注入的 manifest 绝对路径，supervisor.rs）
//! → [`Rr3BusClient::rr3_connect`]（连接/注册/起心跳）→ 按 `RR3_HAL_BACKEND` 选音频源
//! → 采集循环。循环三件事：
//! 1. **自 pacing** 到 frame_ms 节拍——sim 源的 read 即时返回（无时钟），不睡会全速
//!    刷屏总线；实时源（ALSA）read 本身就耗一个节拍，sleep 余量为零，两种后端同一形状；
//! 2. 发布 `rr3-sense/audio/pcm`（stream）与 VAD 边沿 `rr3-sense/audio/vad_start`（event）；
//! 3. `rr3_poll` 排空 inbox，应答 `rr3-sense/capture/dump` 点名（rr3.call 回路）。
//!
//! 发布/读帧失败一律 `eprintln` + `exit(1)`：模块不装"活着"，死交给 supervisor 按
//! restart_policy=always 重启（rrd 的进程语义）。

mod audio;
mod env;
mod imu;

use std::time::{Duration, Instant};

use rr_bus::Rr3BusClient;
use rr_hal::{backend::rr3_hal_audio_source, rr3_block_on, Rr3AudioCfg};
use rr_ipc::{opcode, Rr3PayloadType};
use serde_json::{json, Value};

use audio::Rr3VadState;

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// 本模块 exposes 的唯一事务面主题（module.toml [rpc].exposes 同源）
const RR3_DUMP_TOPIC: &str = "rr3-sense/capture/dump";

fn main() {
    if let Err(e) = rr3_run() {
        eprintln!("rr3-sense 退出: {e}");
        std::process::exit(1);
    }
}

fn rr3_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    println!("rr3-sense 0.0.1 (M3 实装)");
    for line in audio::surface()
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

    let cfg = audio::rr3_sense_config_load()?;
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

    let frames_per_tick =
        audio::capture::rr3_frames_per_tick(cfg.audio.sample_rate, cfg.audio.frame_ms);
    let mut vad = Rr3VadState::rr3_vad_new(cfg.vad, cfg.audio.frame_ms);
    let tick = Duration::from_millis(cfg.audio.frame_ms);
    let mut seq: u64 = 0;

    loop {
        let start = Instant::now();

        let pcm = rr3_block_on(source.read(frames_per_tick))
            .map_err(|e| format!("读帧失败({backend}): {e}"))?;
        // DoA 粗判向（批F2）：多通道帧在下混**前**判向（双麦原始通道只在此一用），
        // 最新一档随下一次 onset 边沿上路；单声道/静音帧恒 None，事件体不带 doa
        let doa = audio::doa::rr3_doa_fix(&pcm.samples, pcm.cfg.channels);
        // 多通道下混 mono 入总线（R14）：双麦原始多通道只供 sense 内部（DoA 随批F2），
        // 总线承载恒 mono——brain 消费面零改动；单麦链路 channels=1 时零拷贝等价
        let pcm = audio::rr3_pcm_downmix_stream(pcm);
        seq += 1;
        bus.rr3_publish("rr3-sense/audio/pcm", audio::rr3_pcm_event_body(seq, &pcm))?;

        if let Some(edge) = vad.rr3_vad_feed(&pcm.samples) {
            bus.rr3_publish(
                "rr3-sense/audio/vad_start",
                audio::rr3_vad_event_body(seq, edge, doa),
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
                audio::rr3_dump_result(seq, backend, &cfg)
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
