//! rr3-sense 机制层注册面（0.0.8 W-A 批3 拆分，ADR-0017 五层表机制层）。
//!
//! [`Rr3SenseDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）三入口：
//! `on_tick`＝采集循环体函数化（自原 composite.rs 主循环逐字迁；自 pacing 归一，
//! 0.0.8 方案 §2.6——read→判向窗积累→下混→publish→VAD 边沿→余量睡眠全在 tick
//! 内，poll_wait=ZERO 时序行为不变）；`on_event`＝播放期门控喂帧（B批 b2 订阅面，
//! tts_pcm 流帧；其余订阅帧与本件无关径直忽略）；`on_rpc`＝dump/ops/list/兜底
//! 三臂点名应答（兜底 Err 点名，serve 折 `{"error": …}` wire 等价）。
//!
//! 门控/反射环/判向窗三个纯机制切片随本层（自原 composite.rs 逐字迁）：门控吃
//! tts_pcm 帧流外推静音窗（裁定②：订阅既有主题零新契约），反射环吃 onset 判向
//! 换算 pan 目标（裁定①：快环落 sense，唯一持音频 conn 者），判向窗 ring 是双麦
//! 链路的 TDOA 积累器（b3b：onset 时整窗回看交 capture::rr3_doa_fix）。策略量
//! 全在 config（Rr3ReflexConfig 等），此处只有判定与算术。
//!
//! 发布/读帧失败 Err 上抛致命：模块不装"活着"，死交给 supervisor 按
//! restart_policy=always 重启（rrd 的进程语义）。

use std::time::{Duration, Instant};

use rr3mod::{BusHandle, Dispatcher};
// 反射旁路是宿主桥档符号（Rr3PeriphCmd，rr-hal backend 宿主档门，批N8 件三）
#[cfg(any(target_os = "linux", target_os = "macos"))]
use rr_hal::backend::Rr3PeriphCmd;
use rr_hal::Rr3PcmStream;
use rr_hal_api::Rr3AudioCfg;
use rr_hal_api::Rr3AudioSource;
use serde_json::{json, Value};

use crate::boot::Rr3SenseConfig;
use crate::capture;
use crate::rpc;
use crate::vad::{self, Rr3VadEdge, Rr3VadState};

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
/// 点头示意下发的 tilt 指令数（0.0.8 批"转完点头"，与 pan 计数分开记账：
/// 手势动是否真发生一眼可见）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
const RR3_REFLEX_TILT_CMDS_KEY: &str = "reflex_tilt_cmds";

/// 反射环行程常量（机制面，策略量在 config）：pan 中位 = boot 姿态基准，
/// 单侧摆幅上限是板侧行程 fail-closed 之外的第二道软件限幅。
const RR3_REFLEX_PAN_CENTER_DEG: i32 = 90;
const RR3_REFLEX_PAN_SWING_DEG: i32 = 30;
/// tilt 中位（板侧行程 [80,100] 的中位 90）：点头手势的基准位——先回中位
/// 蓄势，再上仰 `nod_deg`（绝对角 = 中位 + nod_deg，板侧 fail-closed 裁决）。
const RR3_REFLEX_TILT_CENTER_DEG: i32 = 90;

/// 主循环状态整体持有（自原 composite.rs 局部状态迁入，状态归属随分发走）：
/// 音频源与采集形状、VAD 状态机、流 seq、门控/反射环/判向窗三切片状态。
/// cmd_tx/tts_gate/pending_pan/doa_cfg/doa_ring/open_cfg 为方案 §4.3 字段级
/// 规格的后补字段（批N28 件一笔5：B批 b2/b3b 后于方案快照——门控/反射环/
/// 判向窗三面与开源形状都有读者，"无用状态不迁移"的反面）。
pub(crate) struct Rr3SenseDispatcher {
    source: Box<dyn Rr3AudioSource>,
    /// "读帧失败({backend})"文案复用（探活编排随 boot.rs，后端名随句柄走）
    backend: &'static str,
    /// 采集拍构造 `Rr3PcmStream` 复用（boot.rs probe 按 config 真值开）
    open_cfg: Rr3AudioCfg,
    cfg: Rr3SenseConfig,
    /// capture::rr3_frames_per_tick 预计算一次（方案 §4.3 字段级规格字面）
    frames_per_tick: usize,
    vad: Rr3VadState,
    /// frame_ms——自 pacing 余量睡眠基准
    tick: Duration,
    /// pcm / vad 共用流序（现况）
    seq: u64,
    /// 反射环命令旁路（None = 后端无旁路；探测与警告随 boot.rs probe）
    cmd_tx: Option<rr_hal::backend::Rr3AudioCmdProducer>,
    /// 播放期门控状态机（B批 b2；on_event 喂帧、on_tick onset 臂读门）
    tts_gate: Rr3TtsGate,
    /// 反射环 pending（EMI 避让）：onset 判向换算的目标在此挂起，判停后才实际
    /// 下发——激磁尖峰不落采集窗（见模块文档第 4 条）。连发 onset 覆盖旧值
    /// （最新意图优先，板侧 audio_q 同款背压口径）。
    pending_pan: Option<u16>,
    /// 点头手势时序（0.0.8 批"转完点头"）：pan 下发后按 nod_delay_ms 先回 tilt
    /// 中位（蓄势位），再按 nod_hold_ms 上仰到中位 + nod_deg（示意"听到了"）。
    /// None = 无手势在途（nod_deg=0 或尚未判停）。
    pending_nod: Option<Rr3NodState>,
    /// 判向策略量一次收进 Rr3DoaCfg（B批 b3b：能量门/几何/档界整体注入机制件）
    doa_cfg: capture::Rr3DoaCfg,
    /// 判向窗 ring（B批 b3b）：双麦链路才分配（单声道恒无方向可判，不分配零开销）
    doa_ring: Option<Rr3DoaRing>,
}

impl Rr3SenseDispatcher {
    /// 装配构造（main.rs 装配段调用）：探活句柄/命令旁路/后端名/开源形状由
    /// boot.rs 编排产出；frames_per_tick 预计算、VAD 状态机、判向窗策略收口、
    /// ring（channels≥2 才分配）构造即初始化，与原主循环语义一致。
    pub(crate) fn new(
        source: Box<dyn Rr3AudioSource>,
        cmd_tx: Option<rr_hal::backend::Rr3AudioCmdProducer>,
        backend: &'static str,
        open_cfg: Rr3AudioCfg,
        cfg: Rr3SenseConfig,
    ) -> Self {
        let frames_per_tick =
            capture::rr3_frames_per_tick(cfg.audio.sample_rate, cfg.audio.frame_ms);
        let vad = Rr3VadState::rr3_vad_new(cfg.vad, cfg.audio.frame_ms);
        let doa_cfg = capture::Rr3DoaCfg {
            sample_rate_hz: cfg.audio.sample_rate,
            frame_ms: cfg.audio.frame_ms,
            mic_baseline_mm: cfg.doa.mic_baseline_mm,
            window_rms_min: cfg.doa.window_rms_min,
            bin_threshold_deg: cfg.doa.bin_threshold_deg,
        };
        let doa_ring = if cfg.audio.channels >= 2 {
            Some(Rr3DoaRing::rr3_ring_new(
                (cfg.audio.sample_rate as u64 * cfg.doa.window_ms / 1000) as usize,
                cfg.audio.channels as usize,
            ))
        } else {
            None
        };
        Self {
            source,
            backend,
            open_cfg,
            cfg,
            frames_per_tick,
            vad,
            tick: Duration::from_millis(cfg.audio.frame_ms),
            seq: 0,
            cmd_tx,
            tts_gate: Rr3TtsGate::default(),
            pending_pan: None,
            pending_nod: None,
            doa_cfg,
            doa_ring,
        }
    }

    /// 点头手势推进（宿主桥档专用）：蓄势段（回中位）到点下发后，按
    /// `nod_hold_ms` 再下发上仰段并清账。两段各自计数（`reflex_tilt_cmds`），
    /// 板侧应答失败不回插——手势是示意动作，丢一次可接受，重试会拉长手势。
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn rr3_nod_step(&mut self, now: Instant, bus: &BusHandle) {
        let Some(st) = self.pending_nod else {
            return;
        };
        let Some(tx) = self.cmd_tx.as_ref() else {
            self.pending_nod = None;
            return;
        };
        if !st.dip_sent && now >= st.dip_at {
            tx.rr3_cmd_push(Rr3PeriphCmd::ServoTilt(st.dip_deg));
            bus.counter_bump(RR3_REFLEX_TILT_CMDS_KEY, 1);
            eprintln!("反射环: 点头蓄势 tilt={}°", st.dip_deg);
            self.pending_nod = Some(Rr3NodState {
                dip_sent: true,
                ..st
            });
        } else if st.dip_sent && now >= st.lift_at {
            tx.rr3_cmd_push(Rr3PeriphCmd::ServoTilt(st.lift_deg));
            bus.counter_bump(RR3_REFLEX_TILT_CMDS_KEY, 1);
            eprintln!("反射环: 点头示意 tilt={}°", st.lift_deg);
            self.pending_nod = None;
        }
    }
}

impl Dispatcher for Rr3SenseDispatcher {
    /// 点名应答三臂（dump／ops/list／兜底）：现行主循环 Request 臂逐字迁——
    /// 成功臂 Ok(json!) 应答，兜底臂 Err 点名（serve 折 `{"error": e}`，wire
    /// 等价；请求-应答语义不当致命错误）。
    fn on_rpc(&mut self, topic: &str, _args: &Value, _bus: &BusHandle) -> Result<Value, String> {
        if topic == RR3_DUMP_TOPIC {
            Ok(rpc::rr3_dump_result(self.seq, self.backend, &self.cfg))
        } else if topic == RR3_OPS_LIST_TOPIC {
            Ok(json!({ "ops": rpc::rr3_sense_ops_snapshot(&self.cfg) }))
        } else {
            Err(format!(
                "rr3-sense 只 exposes {RR3_DUMP_TOPIC}/{RR3_OPS_LIST_TOPIC}，收到 {topic}"
            ))
        }
    }

    /// 播放期门控喂帧（B批 b2 订阅面）：tts_pcm 流帧喂门控，其余订阅帧与本件
    /// 无关径直忽略。门控轮分隔/外推用真实时刻（原主循环 poll 排空逐字语义）。
    fn on_event(&mut self, topic: &str, body: &Value, _bus: &BusHandle) -> Result<(), String> {
        if topic == RR3_TTS_PCM_TOPIC {
            if let Some(ms) = body.get("ms").and_then(Value::as_u64) {
                self.tts_gate.rr3_tts_gate_feed(
                    Instant::now(),
                    ms,
                    self.cfg.reflex.gate_gap_ms,
                    self.cfg.reflex.gate_tail_ms,
                );
            }
        }
        Ok(())
    }

    /// 采集拍（自原 composite.rs 主循环体逐字迁，状态归 self；自 pacing 归一
    /// ——余量睡眠在本拍尾，方案 §2.6）。读帧/发布失败 Err 上抛致命。
    fn on_tick(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String> {
        // 读一拍：buf 按"帧×声道"分配采样点数（F1 定形 read 收采样点 buf，
        // 返回实填采样点数，欠载按实数截断）
        let mut buf = vec![0i16; self.frames_per_tick * self.cfg.audio.channels as usize];
        let done = self
            .source
            .read(&mut buf)
            .map_err(|e| format!("读帧失败({}): {e}", self.backend))?;
        buf.truncate(done);
        let raw = Rr3PcmStream {
            cfg: self.open_cfg,
            samples: buf,
        };
        // 判向窗积累（B批 b3b）：原始双通道帧在下混**前**入 ring（双麦原始通道
        // 只在此一用）；判向本身挪到 onset 边沿整窗回看（下方），单声道无 ring 直跳
        if let Some(ring) = self.doa_ring.as_mut() {
            ring.rr3_ring_feed(&raw.samples);
        }
        // 多通道下混 mono 入总线（R14）：双麦原始多通道只供 sense 内部（DoA 随批F2），
        // 总线承载恒 mono——brain 消费面零改动；单麦链路 channels=1 时零拷贝等价
        let pcm = rr3_pcm_downmix_stream(raw);
        // 采集新鲜度记账（批M M-β S-3）：空帧另账（欠载饿死信号），帧账在发布
        // 成功后 +1——记账 = 真上了总线的帧数（发布失败即 exit，临死一帧不入账）
        if done == 0 {
            bus.counter_bump(RR3_PCM_EMPTY_FRAMES_KEY, 1);
        }
        self.seq += 1;
        bus.publish(
            capture::RR3_PCM_TOPIC,
            rpc::rr3_pcm_event_body(self.seq, &pcm),
        )?;
        bus.counter_bump(RR3_PCM_FRAMES_KEY, 1);

        if let Some(edge) = self.vad.rr3_vad_feed(&pcm.samples) {
            let gate_open = self.tts_gate.rr3_tts_gate_open(Instant::now());
            match edge {
                // 播放期门控（B批 b2）：门开只弃 onset 发布（扬声器自声不再触发
                // 听声转头），VAD 状态机照喂不中断；offset 与门无关照发
                Rr3VadEdge::Onset { .. } if gate_open => {
                    bus.counter_bump(RR3_VAD_ONSET_GATED_KEY, 1);
                    eprintln!("播放期门控: onset 弃发布（TTS 回声中）");
                }
                edge => {
                    // DoA 判向（B批 b3b）：仅 onset 判（offset 静音无方向可判），
                    // 判向窗 = ring 线性化快照（最近 window_ms 的原始立体声）交
                    // 机制纯函数；单声道链路 ring 不分配 → 恒 None，事件体不带 doa
                    // （与旧版等价）。onset 时 VAD 已证 ≥min_speech_ms 连续有声帧，
                    // 能量门对 onset 路径恒过——门防的是非 onset 直调与全静音窗
                    let doa = match (&edge, self.doa_ring.as_ref()) {
                        (Rr3VadEdge::Onset { .. }, Some(ring)) => {
                            let snap = ring.rr3_ring_snapshot();
                            capture::rr3_doa_fix(&snap, self.cfg.audio.channels, &self.doa_cfg)
                        }
                        _ => None,
                    };
                    let doa_deg = doa.map(|d| d.deg);
                    bus.publish(
                        vad::RR3_VAD_START_TOPIC,
                        rpc::rr3_vad_event_body(
                            self.seq,
                            edge,
                            doa.map(|d| rpc::rr3_doa_field(&d)),
                        ),
                    )?;
                    // 反射环（B批 b2 起 onset 判向；2026-10-21 EMI 避让改判停后
                    // 动作）：onset 实际发布后才换算存 pending（被门弃掉的 onset
                    // 不进 pending），判停（语音采集完毕）才实际下发——激磁尖峰
                    // 不落采集窗。判向缺档（单声道/静音帧）自然无目标，机制面零
                    // 特判；判据全量入参（gate_open 此臂恒 false），纯函数独立可单测
                    match &edge {
                        Rr3VadEdge::Onset { .. } => {
                            if let Some(deg) = doa_deg {
                                self.pending_pan = rr3_reflex_pan_target(
                                    self.cfg.reflex.enable,
                                    self.cfg.reflex.deadband_deg,
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
                            if let Some(target) = self.pending_pan.take() {
                                if let Some(tx) = self.cmd_tx.as_ref() {
                                    tx.rr3_cmd_push(Rr3PeriphCmd::ServoPan(target));
                                    bus.counter_bump(RR3_REFLEX_PAN_CMDS_KEY, 1);
                                    eprintln!("反射环: 判停后下发 pan={target}°");
                                    // 点头示意（0.0.8 批）：pan 之后接一手势，
                                    // 时序量全在 config；nod_deg=0 时计划为 None
                                    self.pending_nod = rr3_reflex_nod_plan(
                                        RR3_REFLEX_TILT_CENTER_DEG,
                                        self.cfg.reflex.nod_deg,
                                        self.cfg.reflex.nod_delay_ms,
                                        self.cfg.reflex.nod_hold_ms,
                                        now,
                                    );
                                }
                            }
                            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                            {
                                self.pending_pan.take();
                            }
                        }
                    }
                }
            }
        }

        // 点头手势推进（0.0.8 批）：与 VAD 边沿无关，每拍看时刻——蓄势/上仰
        // 两段按 config 时序到点下发（宿主桥档专用，espidf 档无旁路自然跳过）
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        self.rr3_nod_step(now, bus);

        // 自 pacing：补足本节拍剩余时间（sim read 即时返回的关键；
        // 抖动可接受——v0.0.1 的 pcm 流只供联调，时序对齐挂 M5）
        let spent = now.elapsed();
        if spent < self.tick {
            std::thread::sleep(self.tick - spent);
        }
        Ok(())
    }
}

/// 流级单声道化（R14 + 2026-10-31 实机订正）：`cfg.channels` 归 1、rate/bit_depth
/// 不动、samples 收缩为**能量最高通道**（不平均——双麦平均下混实测梳状滤波，
/// ASR 质量显著劣于单通道）；`channels <= 1` 时原样返回（零改动）。自 capture/
/// mech.rs 迁入（0.0.5 批I 深挖收口：`Rr3PcmStream` 是承载层数据类型，件面只留
/// 纯切片机制——C7 白名单外件面不得引用 rr_hal 具体类型）；纯切片选道
/// [`capture::rr3_pcm_best_channel`] 仍在件内。
fn rr3_pcm_downmix_stream(pcm: Rr3PcmStream) -> Rr3PcmStream {
    if pcm.cfg.channels <= 1 {
        return pcm;
    }
    Rr3PcmStream {
        cfg: Rr3AudioCfg {
            channels: 1,
            ..pcm.cfg
        },
        samples: capture::rr3_pcm_best_channel(&pcm.samples, pcm.cfg.channels),
    }
}

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

/// 点头手势两段计划（纯数据；时序在 [`rr3_reflex_nod_plan`] 一次算定）：
/// 蓄势段 = 回 tilt 中位，上仰段 = 中位 + nod_deg。两段各自到点下发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rr3NodState {
    dip_deg: u16,
    lift_deg: u16,
    dip_at: Instant,
    lift_at: Instant,
    dip_sent: bool,
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

/// 点头手势计划（纯函数）：pan 下发时刻 `now` → 两段 tilt 目标与到点时刻。
/// `nod_deg = 0` = 不做手势（旧版逐点等价）。目标 = tilt 中位（蓄势）→ 中位 +
/// `nod_deg`（上仰）；绝对角越出舵机全域 0..=180 不生成（板侧行程 fail-closed
/// 是最终裁决，这里只防无意义负角）。
fn rr3_reflex_nod_plan(
    center_deg: i32,
    nod_deg: i32,
    delay_ms: u64,
    hold_ms: u64,
    now: Instant,
) -> Option<Rr3NodState> {
    if nod_deg == 0 {
        return None;
    }
    let lift_deg = center_deg + nod_deg;
    if !(0..=180).contains(&lift_deg) {
        return None;
    }
    Some(Rr3NodState {
        dip_deg: center_deg as u16,
        lift_deg: lift_deg as u16,
        dip_at: now + Duration::from_millis(delay_ms),
        lift_at: now + Duration::from_millis(delay_ms + hold_ms),
        dip_sent: false,
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::Rr3DoaBin;

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
        // 2026-10-31：单声道化取能量更高通道（ch0 = 100²+400² > ch1 = 300²+200²）
        assert_eq!(mono.samples, vec![100, -400]);
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

    /// 点头计划：nod_deg=0 不生成（旧版逐点等价）；两段目标 = 中位 → 中位+deg，
    /// 时刻按 delay/hold 排定；越出舵机全域不生成（板侧行程仍 fail-closed 裁决）。
    #[test]
    fn reflex_nod_plan_derives_two_segments_and_skips_disabled() {
        let now = Instant::now();
        assert_eq!(rr3_reflex_nod_plan(90, 0, 500, 200, now), None, "手势关");
        let st = rr3_reflex_nod_plan(90, 8, 500, 200, now).expect("手势开必生成");
        assert_eq!(st.dip_deg, 90, "蓄势段回 tilt 中位");
        assert_eq!(st.lift_deg, 98, "上仰段 = 中位 + nod_deg");
        assert_eq!(st.dip_at, now + Duration::from_millis(500));
        assert_eq!(st.lift_at, now + Duration::from_millis(700));
        assert!(!st.dip_sent);
        assert_eq!(
            rr3_reflex_nod_plan(90, 120, 500, 200, now),
            None,
            "越出 0..=180 不生成"
        );
        // 负值 = 下仰（行程内合法时照常生成，板侧裁决）
        assert_eq!(
            rr3_reflex_nod_plan(90, -8, 500, 200, now).unwrap().lift_deg,
            82
        );
    }

    // ---------- B批 b3b：判向窗 ring 用例 ----------

    /// 判向窗 ring：未满窗快照只有已积累帧（原序）；满窗回绕后快照 = 最旧在前
    /// 最新在后的线性化；不足一帧的余数丢弃（帧交错纪律）。
    #[test]
    fn doa_ring_accumulates_and_linearizes_snapshot() {
        let mut ring = Rr3DoaRing::rr3_ring_new(4, 2);
        ring.rr3_ring_feed(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(
            ring.rr3_ring_snapshot(),
            vec![1, 2, 3, 4, 5, 6],
            "未满窗只回已积累帧"
        );
        // 再喂 2 帧共 5 帧：第 1 帧被挤掉，快照 = 帧2..帧5 原序（回绕线性化）
        ring.rr3_ring_feed(&[7, 8, 9, 10]);
        assert_eq!(
            ring.rr3_ring_snapshot(),
            vec![3, 4, 5, 6, 7, 8, 9, 10],
            "回绕线性化"
        );
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
        let cfg = capture::Rr3DoaCfg {
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
        assert_eq!(
            capture::rr3_doa_fix(&snap, 2, &cfg).unwrap().bin,
            Rr3DoaBin::Right
        );
        // τ=−4：frame[1]=物理左麦领先 → 声源在左
        let mut x = 0x1234_5678u32;
        let u: Vec<i16> = (0..n + 4).map(|_| xs_next(&mut x)).collect();
        let left: Vec<i16> = (0..n).flat_map(|t| [u[t], u[t + 4]]).collect();
        let mut ring = Rr3DoaRing::rr3_ring_new(3200, 2);
        ring.rr3_ring_feed(&left);
        assert_eq!(
            capture::rr3_doa_fix(&ring.rr3_ring_snapshot(), 2, &cfg)
                .unwrap()
                .bin,
            Rr3DoaBin::Left
        );
    }
}
