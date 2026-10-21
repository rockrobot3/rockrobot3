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
//! 3. `rr3_poll` 排空 inbox：应答 `rr3-sense/capture/dump` 点名（rr3.call 回路）；
//!    订阅的 `rr3-brain/audio/tts_pcm` 流帧喂播放期门控（B批 b2，裁定②零新契约）；
//! 4. 反射环（B批 b2，裁定①快环落 sense）：非门开期的 onset 带判向 → 换算 pan
//!    目标存 pending → **判停后**才经命令旁路（rr-hal 音频源内建 CmdProducer）
//!    fire-and-forget 下发舵机——持音频 conn 者执行，flock 单消费者纪律下的板外
//!    设命令唯一下行通路。动作时机 2026-10-21 自 onset 挪判停（EMI 避让）：舵机
//!    激磁电流尖峰经共地打穿宿主 FTDI RX 造成断流重连，onset 即动 = 尖峰正落
//!    采集窗斩 utterance；判停 = 语音已采集完毕，断流落在 ASR/LLM/TTS 窗自愈。
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
use rr_hal::backend::rr3_hal_audio_source_with_cmd;
// 反射旁路是宿主桥档符号（Rr3PeriphCmd，rr-hal backend 宿主档门，批N8 件三）
#[cfg(any(target_os = "linux", target_os = "macos"))]
use rr_hal::backend::Rr3PeriphCmd;
use rr_hal::Rr3PcmStream;
use rr_hal_api::Rr3AudioCfg;
use rr_ipc::{opcode, rr3_base64_encode, Rr3PayloadType};
use serde_json::{json, Value};

use capture::{Rr3AudioConfig, Rr3DoaCfg, Rr3DoaFix};
use vad::{Rr3VadConfig, Rr3VadEdge, Rr3VadState};

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// 本模块 exposes 的唯一事务面主题（module.toml [rpc].exposes 同源）
const RR3_DUMP_TOPIC: &str = "rr3-sense/capture/dump";

/// 本模块 exposes 的 ops 槽快照读口（批N4 件九二，module.toml [rpc].exposes 同源）：
/// 白名单组体应答，槽名与 assemble.toml 槽表申报同源（门 d covers 申报的对账面）
const RR3_OPS_LIST_TOPIC: &str = "rr3-sense/ops/list";

/// 采集新鲜度运维计数键（批M M-β S-3）：`pcm_frames` 每实际上总线一帧 +1；
/// `pcm_empty_frames` 读回欠载空帧（read 实填 0 采样——"活着但不动"的饿死
/// 信号，MC-11 判据：卡死与静默停摆看得见）另账。经心跳自报 rrd 归并，
/// `rr3.ctl.counters` 读面键 `rr3-sense/pcm_frames` / `rr3-sense/pcm_empty_frames`。
const RR3_PCM_FRAMES_KEY: &str = "pcm_frames";
const RR3_PCM_EMPTY_FRAMES_KEY: &str = "pcm_empty_frames";

/// 播放期门控的订阅主题（B批 b2，contracts/topics.toml 在册）：brain 的 TTS
/// 回放流，帧体带 `ms`（块内样点时长）——门控外推的数据源，零新契约（裁定②）。
const RR3_TTS_PCM_TOPIC: &str = "rr3-brain/audio/tts_pcm";

/// 运维计数键（B批 b2 冒烟观测面）：门开期被弃的 onset 数 / 反射环实际下发的
/// pan 指令数。经心跳自报 rrd 归并（与 pcm_frames 同一通路）。
const RR3_VAD_ONSET_GATED_KEY: &str = "vad_onset_gated";
// 反射计数键与旁路下发点同径（宿主桥档专用，批N8 件三）
#[cfg(any(target_os = "linux", target_os = "macos"))]
const RR3_REFLEX_PAN_CMDS_KEY: &str = "reflex_pan_cmds";

/// 反射环行程常量（机制面，策略量在 config）：pan 中位 = boot 姿态基准，
/// 单侧摆幅上限是板侧行程 fail-closed 之外的第二道软件限幅。
const RR3_REFLEX_PAN_CENTER_DEG: i32 = 90;
const RR3_REFLEX_PAN_SWING_DEG: i32 = 30;

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
    let (mut source, cmd_tx, backend) = rr3_hal_audio_source_with_cmd();
    if cfg.reflex.enable && cmd_tx.is_none() {
        eprintln!(
            "rr3-sense: 反射环已启用但后端 {backend} 无命令旁路，转头指令无处可去（仅 esp32 档承载）"
        );
    }
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
    let mut tts_gate = Rr3TtsGate::default();
    // 反射环 pending（EMI 避让）：onset 判向换算的目标在此挂起，判停后才实际
    // 下发——激磁尖峰不落采集窗（见模块文档第 4 条）。连发 onset 覆盖旧值
    // （最新意图优先，板侧 audio_q 同款背压口径）。
    let mut pending_pan: Option<u16> = None;

    // 判向策略量一次收进 Rr3DoaCfg（B批 b3b：能量门/几何/档界整体注入机制件）；
    // 判向窗 ring 双麦链路才分配（单声道恒无方向可判，不分配零开销）
    let doa_cfg = Rr3DoaCfg {
        sample_rate_hz: cfg.audio.sample_rate,
        frame_ms: cfg.audio.frame_ms,
        mic_baseline_mm: cfg.doa.mic_baseline_mm,
        window_rms_min: cfg.doa.window_rms_min,
        bin_threshold_deg: cfg.doa.bin_threshold_deg,
    };
    let mut doa_ring = if cfg.audio.channels >= 2 {
        Some(Rr3DoaRing::rr3_ring_new(
            (cfg.audio.sample_rate as u64 * cfg.doa.window_ms / 1000) as usize,
            cfg.audio.channels as usize,
        ))
    } else {
        None
    };

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
        // 判向窗积累（B批 b3b）：原始双通道帧在下混**前**入 ring（双麦原始通道
        // 只在此一用）；判向本身挪到 onset 边沿整窗回看（下方），单声道无 ring 直跳
        if let Some(ring) = doa_ring.as_mut() {
            ring.rr3_ring_feed(&raw.samples);
        }
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
            let gate_open = tts_gate.rr3_tts_gate_open(Instant::now());
            match edge {
                // 播放期门控（B批 b2）：门开只弃 onset 发布（扬声器自声不再触发
                // 听声转头），VAD 状态机照喂不中断；offset 与门无关照发
                Rr3VadEdge::Onset { .. } if gate_open => {
                    bus.rr3_counter_bump(RR3_VAD_ONSET_GATED_KEY, 1);
                    eprintln!("播放期门控: onset 弃发布（TTS 回声中）");
                }
                edge => {
                    // DoA 判向（B批 b3b）：仅 onset 判（offset 静音无方向可判），
                    // 判向窗 = ring 线性化快照（最近 window_ms 的原始立体声）交
                    // 机制纯函数；单声道链路 ring 不分配 → 恒 None，事件体不带 doa
                    // （与旧版等价）。onset 时 VAD 已证 ≥min_speech_ms 连续有声帧，
                    // 能量门对 onset 路径恒过——门防的是非 onset 直调与全静音窗
                    let doa = match (&edge, doa_ring.as_ref()) {
                        (Rr3VadEdge::Onset { .. }, Some(ring)) => {
                            let snap = ring.rr3_ring_snapshot();
                            capture::rr3_doa_fix(&snap, cfg.audio.channels, &doa_cfg)
                        }
                        _ => None,
                    };
                    let doa_deg = doa.map(|d| d.deg);
                    bus.rr3_publish(
                        vad::RR3_VAD_START_TOPIC,
                        rr3_vad_event_body(seq, edge, doa.map(|d| rr3_doa_field(&d))),
                    )?;
                    // 反射环（B批 b2 起 onset 判向；2026-10-21 EMI 避让改判停后
                    // 动作）：onset 实际发布后才换算存 pending（被门弃掉的 onset
                    // 不进 pending），判停（语音采集完毕）才实际下发——激磁尖峰
                    // 不落采集窗。判向缺档（单声道/静音帧）自然无目标，机制面零
                    // 特判；判据全量入参（gate_open 此臂恒 false），纯函数独立可单测
                    match &edge {
                        Rr3VadEdge::Onset { .. } => {
                            if let Some(deg) = doa_deg {
                                pending_pan = rr3_reflex_pan_target(
                                    cfg.reflex.enable,
                                    cfg.reflex.deadband_deg,
                                    deg,
                                    gate_open,
                                );
                            }
                        }
                        Rr3VadEdge::Offset { .. } => {
                            // 旁路下发是宿主桥档行为（Rr3PeriphCmd，rr-hal backend
                            // 宿主档门）：espidf 档 pending 自然落空，take 即弃，
                            // 不进旁路（批N8 件三）。
                            #[cfg(any(target_os = "linux", target_os = "macos"))]
                            if let Some(target) = pending_pan.take() {
                                if let Some(tx) = cmd_tx.as_ref() {
                                    tx.rr3_cmd_push(Rr3PeriphCmd::ServoPan(target));
                                    bus.rr3_counter_bump(RR3_REFLEX_PAN_CMDS_KEY, 1);
                                    eprintln!("反射环: 判停后下发 pan={target}°");
                                }
                            }
                            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                            {
                                let _ = pending_pan.take();
                            }
                        }
                    }
                }
            }
        }

        // 排空 inbox（B批 b2 起有订阅）：tts_pcm 流帧喂播放期门控，其余 Event
        // 与本件无关径直忽略；点名 Request——rr3.call 转发帧的 trace_id=调用方
        // task（rrd 路由语义），顺原路应答
        for frame in bus.rr3_poll() {
            if frame.header.opcode != opcode::SEND {
                continue;
            }
            let Ok((ty, body)) = Rr3BusClient::rr3_frame_body(&frame) else {
                continue;
            };
            match ty {
                Rr3PayloadType::Event => {
                    if body.get("topic").and_then(Value::as_str) == Some(RR3_TTS_PCM_TOPIC) {
                        if let Some(ms) = body.get("ms").and_then(Value::as_u64) {
                            tts_gate.rr3_tts_gate_feed(
                                Instant::now(),
                                ms,
                                cfg.reflex.gate_gap_ms,
                                cfg.reflex.gate_tail_ms,
                            );
                        }
                    }
                }
                Rr3PayloadType::Request => {
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
                    } else if topic == RR3_OPS_LIST_TOPIC {
                        json!({ "ops": rr3_sense_ops_snapshot(&cfg) })
                    } else {
                        json!({ "error": format!("rr3-sense 只 exposes {RR3_DUMP_TOPIC}/{RR3_OPS_LIST_TOPIC}，收到 {topic}") })
                    };
                    bus.rr3_reply_call(frame.header.trace_id, id, result)?;
                }
                _ => {}
            }
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

/// `rr3-sense/ops/list` 应答体（批N4 件九二，面板 /api/sense/ops 读口）：ops 槽
/// 快照，**白名单组体**——槽名与 assemble.toml 槽表申报同源（audio/doa/vad 三组，
/// 门 d covers 申报的对账面），config 其余键一律不进应答。槽名 doa_* 与 config 键
/// 的映射是装配清单 src_key 的事（槽表申报同源；b3b 起 doa 组扩到四槽）。
fn rr3_sense_ops_snapshot(cfg: &Rr3SenseConfig) -> Value {
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

// ---------- 播放期门控与反射环（B批 b2） ----------
//
// 两个纯机制面：门控吃 tts_pcm 帧流外推静音窗（裁定②：订阅既有主题零新契约），
// 反射环吃 onset 判向换算 pan 目标（裁定①：快环落 sense，唯一持音频 conn 者）。
// 策略量全在 config（Rr3ReflexConfig），此处只有判定与算术。

/// TTS 回放的门控状态机：帧流（与上一帧间隔 > gap 判新一轮）累计 `ms`，
/// 外推线 = 本轮首帧时刻 + 累计时长 + 尾窗。状态只进不外泄，
/// [`Rr3TtsGate::rr3_tts_gate_open`] 是唯一读口。
#[derive(Debug, Default)]
struct Rr3TtsGate {
    /// 本轮首帧时刻（外推线的锚点）
    round_start: Option<Instant>,
    /// 上一帧到达时刻（轮分隔判据）
    last_frame_at: Option<Instant>,
    /// 本轮累计时长（帧体 ms 直加——brain 侧 ms 由样点数导出，含尾块实算）
    cum_ms: u64,
    /// 门外推线：now ≤ 外推线即门开
    deadline: Option<Instant>,
}

impl Rr3TtsGate {
    /// 喂一帧 tts_pcm：轮分隔（与上一帧间隔 > `gap_ms` 重置累计）→ 累计 → 外推。
    fn rr3_tts_gate_feed(&mut self, now: Instant, ms: u64, gap_ms: u64, tail_ms: u64) {
        let new_round = match self.last_frame_at {
            None => true,
            Some(t) => now.duration_since(t) > Duration::from_millis(gap_ms),
        };
        if new_round {
            self.round_start = Some(now);
            self.cum_ms = 0;
        }
        self.last_frame_at = Some(now);
        self.cum_ms = self.cum_ms.saturating_add(ms);
        if let Some(start) = self.round_start {
            // checked_add：Instant 端点溢出按"门长开"处理，不 panic（裸板口径）
            self.deadline = start.checked_add(Duration::from_millis(self.cum_ms + tail_ms));
        }
    }

    /// 门开着吗（now ≤ 外推线）。从未收到帧 = 关。
    fn rr3_tts_gate_open(&self, now: Instant) -> bool {
        match self.deadline {
            Some(d) => now <= d,
            None => false,
        }
    }
}

/// 反射环换算（纯函数）：onset 判向角 → pan 目标角。全量判据过不了 = `None`
/// （不转头）：未启用 / 门开（播放回声）/ 死区内（正前方说话免抖动）。
/// 目标 = 中位 + clamp(deg, ±摆幅)——正负号直通（右声源右转头），板侧行程
/// fail-closed 之外这里是第二道软件限幅。
fn rr3_reflex_pan_target(
    enable: bool,
    deadband_deg: i32,
    doa_deg: i32,
    gate_open: bool,
) -> Option<u16> {
    if !enable || gate_open || doa_deg.abs() < deadband_deg {
        return None;
    }
    let swing = doa_deg.clamp(-RR3_REFLEX_PAN_SWING_DEG, RR3_REFLEX_PAN_SWING_DEG);
    Some((RR3_REFLEX_PAN_CENTER_DEG + swing) as u16)
}

// ---------- 判向窗 ring（B批 b3b TDOA） ----------
//
// 双麦链路的判向窗积累器：每采集拍把原始立体声帧推入环形缓冲（容量 =
// window_ms 的帧数），onset 边沿那一刻线性化快照交 [`capture::rr3_doa_fix`]
// 整窗判向——判向从"逐帧即时"改为"onset 时回看"（b3a 实证：单帧瞬时判向
// 无稳定方向，1s 窗才有；VAD onset 已证 ≥min_speech_ms 连续有声，窗内
// 新鲜语音主导，无需中位数聚合）。单声道链路不分配（无方向可判）。

/// 判向窗环形缓冲（帧交错存储，与采集 samples 同形；主循环单线程读写）
struct Rr3DoaRing {
    buf: Vec<i16>,
    ch: usize,
    cap_frames: usize,
    /// 下一写入帧位（模 cap_frames）
    pos: usize,
    /// 已积累帧数（≤ cap_frames）
    have: usize,
}

impl Rr3DoaRing {
    fn rr3_ring_new(cap_frames: usize, ch: usize) -> Self {
        let ch = ch.max(1);
        let cap_frames = cap_frames.max(1);
        Self {
            buf: vec![0i16; cap_frames * ch],
            ch,
            cap_frames,
            pos: 0,
            have: 0,
        }
    }

    /// 推入一拍原始采样（帧交错）；不足一帧的余数丢弃（采集整帧纪律）
    fn rr3_ring_feed(&mut self, samples: &[i16]) {
        for frame in samples.chunks_exact(self.ch) {
            let off = self.pos * self.ch;
            self.buf[off..off + self.ch].copy_from_slice(frame);
            self.pos = (self.pos + 1) % self.cap_frames;
            self.have = (self.have + 1).min(self.cap_frames);
        }
    }

    /// 线性化快照：最旧帧在前、最新帧在后（onset 时交判向机制）
    fn rr3_ring_snapshot(&self) -> Vec<i16> {
        if self.have < self.cap_frames {
            return self.buf[..self.have * self.ch].to_vec();
        }
        let off = self.pos * self.ch;
        let mut snap = Vec::with_capacity(self.buf.len());
        snap.extend_from_slice(&self.buf[off..]);
        snap.extend_from_slice(&self.buf[..off]);
        snap
    }
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
    pub reflex: Rr3ReflexConfig,
}

/// DoA 判向配置（B批 b3b 起 TDOA 判向窗口径）：
/// - `bin_threshold_deg`：三档分档边界（S-5，原件内常量迁此）；
/// - `window_ms`：判向窗长（onset 时 ring 回看窗，0 收敛 1000 留痕）；
/// - `window_rms_min`：窗能量门（任一采集帧 rms ≥ 此值才算有声窗，0 = 门全开）；
/// - `mic_baseline_mm`：双麦间距（物理 lag 上限与 sinθ 反演的几何量，0 报错）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3DoaConfig {
    pub bin_threshold_deg: i32,
    pub window_ms: u64,
    pub window_rms_min: i64,
    pub mic_baseline_mm: u32,
}

/// 反射环配置（B批 b2）：策略量全部数据化。代码缺省 `enable = false`——不显式
/// 开（config-esp32.toml 档开）行为与旧版逐点等价：无门控无转头，纯采集照旧。
/// - `deadband_deg`：判向死区（|deg| < 死区不转头，正前方说话免抖动）；
/// - `gate_tail_ms`：TTS 帧流停后门保持开的尾窗（扬声器回声拖尾）；
/// - `gate_gap_ms`：tts_pcm 帧间隔超过此值判新一轮播放（门控累计重置）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ReflexConfig {
    pub enable: bool,
    pub deadband_deg: i32,
    pub gate_tail_ms: u64,
    pub gate_gap_ms: u64,
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
                window_ms: 1000,
                window_rms_min: 800,
                mic_baseline_mm: 150,
            },
            reflex: Rr3ReflexConfig {
                enable: false,
                deadband_deg: 10,
                gate_tail_ms: 1000,
                gate_gap_ms: 500,
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
        "config 就绪: 来源={path} doa_bin_threshold_deg={} doa_window_ms={} doa_window_rms_min={} doa_mic_baseline_mm={}",
        cfg.doa.bin_threshold_deg, cfg.doa.window_ms, cfg.doa.window_rms_min, cfg.doa.mic_baseline_mm
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
    // S-5 + B批 b3b：DoA 档界与判向窗策略量（缺键回落默认；异常值显式报错，
    // window_ms=0 无窗意义收敛默认留痕——与 audio.frame_ms 同一口径）
    if let Some(doa) = section.and_then(|s| s.get("doa")) {
        let mut t = cfg.doa.bin_threshold_deg as i64;
        rr3_cfg_i64(doa, "bin_threshold_deg", &mut t)?;
        cfg.doa.bin_threshold_deg =
            i32::try_from(t).map_err(|_| format!("[rr3-sense.doa] bin_threshold_deg 越界: {t}"))?;
        rr3_cfg_u64(doa, "window_ms", &mut cfg.doa.window_ms)?;
        if cfg.doa.window_ms < 1 {
            eprintln!(
                "config [rr3-sense.doa] window_ms={} 无窗意义，收敛 1000ms",
                cfg.doa.window_ms
            );
            cfg.doa.window_ms = 1000;
        }
        let mut wrm = cfg.doa.window_rms_min;
        rr3_cfg_i64(doa, "window_rms_min", &mut wrm)?;
        if !(0..=32767).contains(&wrm) {
            return Err(format!(
                "[rr3-sense.doa] window_rms_min 须在 0..=32767，收到 {wrm}"
            ));
        }
        cfg.doa.window_rms_min = wrm;
        let mut mm = cfg.doa.mic_baseline_mm as u64;
        rr3_cfg_u64(doa, "mic_baseline_mm", &mut mm)?;
        if mm == 0 {
            return Err("[rr3-sense.doa] mic_baseline_mm=0 无几何可判向".to_string());
        }
        cfg.doa.mic_baseline_mm =
            u32::try_from(mm).map_err(|_| format!("[rr3-sense.doa] mic_baseline_mm 越界: {mm}"))?;
    }
    // B批 b2：反射环与播放期门控策略量（缺键回落默认，enable 缺省 false）
    if let Some(reflex) = section.and_then(|s| s.get("reflex")) {
        rr3_cfg_bool(reflex, "enable", &mut cfg.reflex.enable)?;
        let mut db = cfg.reflex.deadband_deg as i64;
        rr3_cfg_i64(reflex, "deadband_deg", &mut db)?;
        cfg.reflex.deadband_deg =
            i32::try_from(db).map_err(|_| format!("[rr3-sense.reflex] deadband_deg 越界: {db}"))?;
        rr3_cfg_u64(reflex, "gate_tail_ms", &mut cfg.reflex.gate_tail_ms)?;
        rr3_cfg_u64(reflex, "gate_gap_ms", &mut cfg.reflex.gate_gap_ms)?;
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

fn rr3_cfg_bool(table: &toml::Value, key: &str, slot: &mut bool) -> Result<(), String> {
    match table.get(key) {
        None => Ok(()),
        Some(v) => {
            *slot = v
                .as_bool()
                .ok_or_else(|| format!("配置项 {key} 须为布尔值，收到 {v}"))?;
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

    // ---------- B批 b2：播放期门控与反射环用例 ----------

    /// 门控三拍：帧到开门 → 尾窗外推到期关门 → 帧间隔超 gap 判新一轮重置累计。
    #[test]
    fn tts_gate_opens_expires_and_re_rounds_on_gap() {
        let t0 = Instant::now();
        let mut g = Rr3TtsGate::default();
        assert!(!g.rr3_tts_gate_open(t0), "从未收到帧 = 门关");
        g.rr3_tts_gate_feed(t0, 200, 500, 1000);
        // deadline = t0 + 200 + 1000
        assert!(g.rr3_tts_gate_open(t0));
        assert!(g.rr3_tts_gate_open(t0 + Duration::from_millis(1200)));
        assert!(!g.rr3_tts_gate_open(t0 + Duration::from_millis(1201)));
        // 间隔 600ms > gap 500ms：新一轮，累计重置——新 deadline = t0.6 + 100 + 1000。
        // t0+1.6s 在旧线（t0+1.2s）外本已关门，新线把它重新覆盖，即轮分隔生效的证据
        g.rr3_tts_gate_feed(t0 + Duration::from_millis(600), 100, 500, 1000);
        assert!(
            g.rr3_tts_gate_open(t0 + Duration::from_millis(1600)),
            "新轮外推线生效（旧线早已到期）"
        );
        assert!(g.rr3_tts_gate_open(t0 + Duration::from_millis(1700)));
        assert!(!g.rr3_tts_gate_open(t0 + Duration::from_millis(1701)));
    }

    /// 同轮多帧：ms 逐帧直加（brain 侧尾块按实算），deadline 随累计外推。
    #[test]
    fn tts_gate_accumulates_ms_within_round() {
        let t0 = Instant::now();
        let mut g = Rr3TtsGate::default();
        // 同轮三帧各 200ms、间隔 20ms（< gap 500）：deadline = t0 + 600 + 1000
        g.rr3_tts_gate_feed(t0, 200, 500, 1000);
        g.rr3_tts_gate_feed(t0 + Duration::from_millis(20), 200, 500, 1000);
        g.rr3_tts_gate_feed(t0 + Duration::from_millis(40), 200, 500, 1000);
        assert!(g.rr3_tts_gate_open(t0 + Duration::from_millis(1600)));
        assert!(!g.rr3_tts_gate_open(t0 + Duration::from_millis(1601)));
    }

    /// 反射环全量判据：未启用 / 门开 / 死区内三路不转头；过判据的换算
    /// = 中位 90 + clamp(deg, ±30)，左右号直通、双端限幅。
    #[test]
    fn reflex_pan_target_respects_enable_gate_and_deadband() {
        assert_eq!(rr3_reflex_pan_target(false, 10, -60, false), None, "未启用");
        assert_eq!(rr3_reflex_pan_target(true, 10, -60, true), None, "门开");
        assert_eq!(rr3_reflex_pan_target(true, 10, 5, false), None, "死区内");
        assert_eq!(rr3_reflex_pan_target(true, 10, -9, false), None, "死区负侧");
        // 死区边界恰过：|deg| >= deadband 即动作
        assert_eq!(rr3_reflex_pan_target(true, 10, 10, false), Some(100));
        assert_eq!(rr3_reflex_pan_target(true, 10, -15, false), Some(75));
        assert_eq!(
            rr3_reflex_pan_target(true, 10, 60, false),
            Some(120),
            "右摆限幅"
        );
        assert_eq!(
            rr3_reflex_pan_target(true, 10, -60, false),
            Some(60),
            "左摆限幅"
        );
    }

    /// reflex 段解析：缺段 = enable 缺省 false（不开与旧版逐点等价）；全量注入
    /// 生效；类型错显式拒收（bool 有专属 helper，字符串 "on" 不认）。
    #[test]
    fn config_parse_reflex_section_defaults_disabled() {
        let cfg = rr3_sense_config_parse("").unwrap();
        assert!(!cfg.reflex.enable);
        assert_eq!(cfg.reflex.deadband_deg, 10);
        assert_eq!(cfg.reflex.gate_tail_ms, 1000);
        assert_eq!(cfg.reflex.gate_gap_ms, 500);
        let cfg = rr3_sense_config_parse(
            "[rr3-sense.reflex]\nenable = true\ndeadband_deg = 15\ngate_tail_ms = 800\ngate_gap_ms = 300\n",
        )
        .unwrap();
        assert!(cfg.reflex.enable);
        assert_eq!(cfg.reflex.deadband_deg, 15);
        assert_eq!(cfg.reflex.gate_tail_ms, 800);
        assert_eq!(cfg.reflex.gate_gap_ms, 300);
        assert!(rr3_sense_config_parse("[rr3-sense.reflex]\nenable = \"on\"\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.reflex]\ndeadband_deg = \"wide\"\n").is_err());
    }

    // ---------- B批 b3b：DoA 判向窗配置段 / ops 槽 / ring 用例 ----------

    /// doa 段解析：缺段 = 全默认；全量注入生效；window_ms=0 收敛 1000 留痕
    /// （与 audio.frame_ms 同口径）；异常值显式拒收（window_rms_min 越界 /
    /// mic_baseline_mm=0 无几何），类型错走 rr3_cfg_* 通用报错。
    #[test]
    fn config_parse_doa_section_defaults_full_and_rejects() {
        let cfg = rr3_sense_config_parse("").unwrap();
        assert_eq!(cfg.doa.bin_threshold_deg, 30);
        assert_eq!(cfg.doa.window_ms, 1000);
        assert_eq!(cfg.doa.window_rms_min, 800);
        assert_eq!(cfg.doa.mic_baseline_mm, 150);
        let cfg = rr3_sense_config_parse(
            "[rr3-sense.doa]\nbin_threshold_deg = 20\nwindow_ms = 500\nwindow_rms_min = 1200\nmic_baseline_mm = 180\n",
        )
        .unwrap();
        assert_eq!(cfg.doa.bin_threshold_deg, 20);
        assert_eq!(cfg.doa.window_ms, 500);
        assert_eq!(cfg.doa.window_rms_min, 1200);
        assert_eq!(cfg.doa.mic_baseline_mm, 180);
        let cfg = rr3_sense_config_parse("[rr3-sense.doa]\nwindow_ms = 0\n").unwrap();
        assert_eq!(cfg.doa.window_ms, 1000, "0 窗无意义收敛默认");
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nwindow_rms_min = -1\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nwindow_rms_min = 32768\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nmic_baseline_mm = 0\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nwindow_ms = \"long\"\n").is_err());
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

    /// 判向窗 ring：未满窗快照只有已积累帧（原序）；满窗回绕后快照 = 最旧在前
    /// 最新在后的线性化；不足一帧的余数丢弃（帧交错纪律）。
    #[test]
    fn doa_ring_accumulates_and_linearizes_snapshot() {
        let mut ring = Rr3DoaRing::rr3_ring_new(4, 2);
        ring.rr3_ring_feed(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(ring.rr3_ring_snapshot(), vec![1, 2, 3, 4, 5, 6], "未满窗只回已积累帧");
        // 再喂 2 帧共 5 帧：第 1 帧被挤掉，快照 = 帧2..帧5 原序（回绕线性化）
        ring.rr3_ring_feed(&[7, 8, 9, 10]);
        assert_eq!(ring.rr3_ring_snapshot(), vec![3, 4, 5, 6, 7, 8, 9, 10], "回绕线性化");
        let mut ring = Rr3DoaRing::rr3_ring_new(4, 2);
        ring.rr3_ring_feed(&[1, 2, 3, 4, 9]);
        assert_eq!(ring.rr3_ring_snapshot(), vec![1, 2, 3, 4], "余数丢弃");
    }

    /// ring → snapshot → 判向机制的组合件侧集成形状：τ=+4 声源在右、τ=−4 在左
    /// （确定性宽带源与 mech 侧 golden 同构；此处只证管线通，算法逐位由 mech 钉）。
    #[test]
    fn doa_ring_snapshot_feeds_tdoa_pipeline() {
        fn xs_next(x: &mut u32) -> i16 {
            *x ^= *x << 13;
            *x ^= *x >> 17;
            *x ^= *x << 5;
            ((*x as i32) >> 16) as i16
        }
        let cfg = Rr3DoaCfg {
            sample_rate_hz: 16000,
            frame_ms: 20,
            mic_baseline_mm: 150,
            window_rms_min: 800,
            bin_threshold_deg: 30,
        };
        let n = 3400usize;
        let mut x = 0x1234_5678u32;
        let u: Vec<i16> = (0..n + 4).map(|_| xs_next(&mut x)).collect();
        // τ=+4：frame[0]=物理右麦领先（u[t+4] 相对 u[t] 左移 4 = 提前）→ 声源在右
        let right: Vec<i16> = (0..n).flat_map(|t| [u[t + 4], u[t]]).collect();
        let mut ring = Rr3DoaRing::rr3_ring_new(3200, 2);
        ring.rr3_ring_feed(&right);
        let snap = ring.rr3_ring_snapshot();
        assert_eq!(snap.len(), 3200 * 2, "满窗快照 = 容量帧数×声道");
        assert_eq!(capture::rr3_doa_fix(&snap, 2, &cfg).unwrap().bin, Rr3DoaBin::Right);
        // τ=−4：frame[1]=物理左麦领先 → 声源在左
        let mut x = 0x1234_5678u32;
        let u: Vec<i16> = (0..n + 4).map(|_| xs_next(&mut x)).collect();
        let left: Vec<i16> = (0..n).flat_map(|t| [u[t], u[t + 4]]).collect();
        let mut ring = Rr3DoaRing::rr3_ring_new(3200, 2);
        ring.rr3_ring_feed(&left);
        assert_eq!(
            capture::rr3_doa_fix(&ring.rr3_ring_snapshot(), 2, &cfg).unwrap().bin,
            Rr3DoaBin::Left
        );
    }
}
