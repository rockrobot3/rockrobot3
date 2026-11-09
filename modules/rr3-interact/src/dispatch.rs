//! rr3-interact 机制层注册面（0.0.8 W-A 批4 拆分，ADR-0017 五层表机制层）。
//!
//! [`Rr3InteractDispatcher`] 收拢原 composite.rs 主循环态（sink/face/display/
//! cfg/tts_pcm/link/act_rx/face_tick_at），三入口语义与原主循环逐字同款：
//! [`Dispatcher::on_event`] 两臂（net/link 快照缓存 + tts_pcm 收流计数，坏帧
//! 只弃该帧）、[`Dispatcher::on_rpc`] 四臂（tts/face 点名执行走 [`crate::rpc`]、
//! ops/list 白名单快照、兜底文案点名）、[`Dispatcher::on_tick`] 两面（面板动作
//! 排空 beep/face 主循环串行代执行 → 表情帧节拍出帧）。tts_pcm 收流缓冲与
//! beep 测试音两机制切片随字段同文件（用例随迁）。字段均主线程独占（面板线程
//! 只经 action 通道请求代执行），跨线程物只有 `BusHandle`（Clone）与 link Arc。

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rr3mod::{BusHandle, Dispatcher};
use rr_hal_api::{Rr3AudioSink, Rr3Display};
use serde_json::{json, Value};

use crate::boot::Rr3InteractCfg;
use crate::face;
use crate::panel::server::Rr3PanelAction;
use crate::player;
use crate::rpc;

/// 面板状态页的链路卡片数据源（0.0.4 批C 只做订阅缓存，发布方 rr3-connect 随批D 上线）
const RR3_NET_LINK_TOPIC: &str = "rr3-connect/net/link";

/// tts_pcm 流收缓冲（MC-1）：brain 语音下行**先流后点名**——流是载荷通路，
/// `rr3-interact/tts/text` 点名是"该不该播"的唯一判据（0.0.5 方案 §20 第 1 条
/// (a)）。Event 帧逐帧解码进缓冲，点名对账后排空播放；对账不过整缓冲作废
/// （残流不跨轮滞留）。
#[derive(Default)]
pub(crate) struct Rr3TtsPcmBuf {
    bytes: Vec<u8>,
}

/// 全量收流上界（32MiB）：正常一轮 TTS 是秒级音频（≤数 MB），上界防"只流不
/// 点名"或故障模块把 interact 内存顶穿。超界帧丢弃，点名对账必然失败→整轮拒播。
const RR3_TTS_BUF_CAP: usize = 32 * 1024 * 1024;

/// 收下的帧计数键（运维面，随心跳归并；与 `tts_pcm_bad` 互补对账）
const RR3_TTS_PCM_FRAMES_KEY: &str = "tts_pcm_frames";

/// 弃帧计数键（运维面，随心跳归并；`tts_pcm_frames` 只记收下的帧——两者相加
/// 才是上游实际发出的帧数，"只流不点名"看这里就知道）。
const RR3_TTS_PCM_BAD_KEY: &str = "tts_pcm_bad";

/// 弃帧留痕抽样（首帧必留，其后每 N 帧一条）：坏源可能成百上千帧连续弃，
/// 逐帧打会把日志文件撑爆；抽样保证"有没有在坏、坏因是什么"始终可查。
const RR3_TTS_BAD_LOG_EVERY: u32 = 64;

impl Rr3TtsPcmBuf {
    /// 收一帧：`pcm_b64` 解码追加（STREAM 语义有序到达，帧序即字节序）。
    /// 解码失败/超上界上抛，由调用方弃帧——缺口由点名的对账兜住，不静默。
    pub(crate) fn rr3_tts_buf_push(&mut self, pcm_b64: &str) -> Result<(), String> {
        self.rr3_tts_push_cap(pcm_b64, RR3_TTS_BUF_CAP)
    }

    /// 带帽收帧（帽参数显式化，单测用小帽钉边界）：先按 base64 上界估算，
    /// 超帽时不解码、不分配。
    fn rr3_tts_push_cap(&mut self, pcm_b64: &str, cap: usize) -> Result<(), String> {
        let projected = self.bytes.len().saturating_add(pcm_b64.len() / 4 * 3);
        if projected > cap {
            return Err(format!(
                "tts_pcm 缓冲超界（>{}B）：丢弃本帧，后续点名对账将拒播",
                cap
            ));
        }
        let chunk = rr_ipc::rr3_base64_decode(pcm_b64)
            .map_err(|e| format!("tts_pcm 帧 base64 解码失败: {e:?}"))?;
        if self.bytes.len().saturating_add(chunk.len()) > cap {
            return Err(format!(
                "tts_pcm 缓冲超界（>{}B）：丢弃本帧，后续点名对账将拒播",
                cap
            ));
        }
        self.bytes.extend_from_slice(&chunk);
        Ok(())
    }

    /// 缓冲实收字节数（对账的一侧）
    pub(crate) fn rr3_tts_buf_len(&self) -> usize {
        self.bytes.len()
    }

    /// 排空：取走全部字节，缓冲归零（残流不跨轮滞留）。
    pub(crate) fn rr3_tts_buf_take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

// ---------- beep 测试音（批N5 件五笔2，§17 缺口①：speaker 腿波形源） ----------

/// 测试音波形参数：440Hz / 200ms 短正弦，峰值 12000（约 37% 满幅）——同径
/// 增益叠加后仍远低于饱和，不削顶。440 × 0.2s = 88 个整周期，波形首尾同相。
const RR3_BEEP_FREQ_HZ: f64 = 440.0;
const RR3_BEEP_MS: u64 = 200;
const RR3_BEEP_AMP: i16 = 12_000;

/// 合成内置测试音：16k mono s16le 与播放面同一张采样规格
/// （`RR3_TTS_SAMPLE_RATE`），进程内自持不依赖 tts_pcm 流。
fn rr3_beep_samples() -> Vec<i16> {
    let n = RR3_BEEP_MS as usize * player::RR3_TTS_SAMPLE_RATE as usize / 1000;
    (0..n)
        .map(|i| {
            (RR3_BEEP_AMP as f64
                * (2.0 * std::f64::consts::PI * RR3_BEEP_FREQ_HZ * i as f64
                    / player::RR3_TTS_SAMPLE_RATE as f64)
                    .sin()) as i16
        })
        .collect()
}

/// beep 动作执行（主循环代执行）：合成 → 增益（与 tts 同径 `volume_pct`，
/// MC-2 同一播放参数面）→ 直喂播放面，回 `{"played_ms"}`。波形长度编译期
/// 固定 3200 样点 = 200ms ≥ 1ms 窗口，`played_ms == 0` 恒不触（I-21① 同口径
/// 由构造保证）；写播放面失败原样上抛折 error 应答（同 tts 口径）。
fn rr3_beep_play(sink: &mut dyn Rr3AudioSink, volume_pct: u32) -> Result<Value, String> {
    let mut samples = rr3_beep_samples();
    player::rr3_tts_apply_gain(&mut samples, volume_pct);
    player::rr3_tts_play(sink, &samples)
        .map_err(|e| rpc::rr3_tts_err_text(&e))
        .map(|played_ms| json!({ "played_ms": played_ms }))
}

/// 机制层注册面：主循环态收拢为字段（face/tts_pcm/face_tick_at 构造即初始化，
/// sink/display/link/act_rx 由装配段注入，cfg 策略量贯穿三入口）。
pub(crate) struct Rr3InteractDispatcher {
    sink: Box<dyn Rr3AudioSink>,
    face: face::Rr3Face,
    display: Box<dyn Rr3Display>,
    cfg: Rr3InteractCfg,
    /// tts_pcm 收流缓冲（MC-1）：on_event 喂入、tts 点名对账排空
    tts_pcm: Rr3TtsPcmBuf,
    /// net/link 订阅缓存：on_event 只写、面板线程只读（状态页卡片数据源）
    link: Arc<Mutex<Option<Value>>>,
    /// 面板动作通道收端：on_tick 排空（beep/face 主循环串行代执行，与 TTS
    /// 点名天然互斥，无锁）
    act_rx: mpsc::Receiver<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>,
    /// 表情帧节拍锚：on_tick 到点重置（serve 传入 now 实参，同拍等价）
    face_tick_at: Instant,
    /// 弃帧累计（既有计数上报又有抽样留痕闸门：同一自增量，不引第二份状态）
    tts_pcm_bad: u32,
}

impl Rr3InteractDispatcher {
    /// 构造序照装配段实参（sink, display, cfg, link, act_rx）；face 自 config
    /// 兜底表情注入（批N4 件五 I-17/M-3：mech 各回落点取 default_expression，
    /// parse 已对账白名单），节拍锚构造即起算。
    pub(crate) fn new(
        sink: Box<dyn Rr3AudioSink>,
        display: Box<dyn Rr3Display>,
        cfg: Rr3InteractCfg,
        link: Arc<Mutex<Option<Value>>>,
        act_rx: mpsc::Receiver<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>,
    ) -> Self {
        let face = face::rr3_face_new(cfg.face.default_expr);
        Self {
            sink,
            face,
            display,
            tts_pcm: Rr3TtsPcmBuf::default(),
            cfg,
            link,
            act_rx,
            face_tick_at: Instant::now(),
            tts_pcm_bad: 0,
        }
    }

    /// 收流一帧（`on_event` 的 tts_pcm 臂）：成帧计数；坏帧**弃该帧但必须留痕**——
    /// 上抛会按 Dispatcher 契约把模块判死，逐帧打又会撑爆日志文件，故按首帧＋
    /// 每 [`RR3_TTS_BAD_LOG_EVERY`] 帧抽样点名，并另账计数由心跳归并上报。
    fn rr3_tts_frame(&mut self, body: &Value, bus: &BusHandle) {
        let pushed = body
            .get("pcm_b64")
            .and_then(Value::as_str)
            .ok_or_else(|| "帧缺 pcm_b64".to_string())
            .and_then(|b64| self.tts_pcm.rr3_tts_buf_push(b64));
        match pushed {
            Ok(()) => bus.counter_bump(RR3_TTS_PCM_FRAMES_KEY, 1),
            Err(e) => {
                self.tts_pcm_bad = self.tts_pcm_bad.saturating_add(1);
                bus.counter_bump(RR3_TTS_PCM_BAD_KEY, 1);
                if rr3_bad_log_due(self.tts_pcm_bad) {
                    eprintln!("tts_pcm 弃帧（累计 {} 帧）: {e}", self.tts_pcm_bad);
                }
            }
        }
    }
}

/// 弃帧留痕闸门（纯函数，单测钉域）：第 1 帧必留（"什么时候开始坏的"），其后
/// 每 N 帧一条（"还在坏、坏因未变"），中间帧只进计数。
fn rr3_bad_log_due(n: u32) -> bool {
    n == 1 || n % RR3_TTS_BAD_LOG_EVERY == 0
}

impl Dispatcher for Rr3InteractDispatcher {
    /// 点名面四臂（照现状序 tts → face → ops/list → 兜底）：业务性失败返
    /// `Err`（serve 折 `{"error": …}` 应答，与原 unwrap_or_else 同形状）。
    fn on_rpc(&mut self, topic: &str, args: &Value, bus: &BusHandle) -> Result<Value, String> {
        match topic {
            player::RR3_TTS_TOPIC => rpc::rr3_handle_tts(
                bus,
                self.sink.as_mut(),
                &mut self.face,
                &mut self.tts_pcm,
                args,
                &self.cfg.player,
            ),
            face::RR3_FACE_TOPIC => {
                rpc::rr3_handle_face(&mut self.face, args, &self.cfg.face.non_pointable)
            }
            rpc::RR3_OPS_LIST_TOPIC => Ok(json!({ "ops": rpc::rr3_interact_ops(&self.cfg) })),
            other => Err(format!(
                "rr3-interact 只 exposes {}/{}/{}，收到 {other}",
                player::RR3_TTS_TOPIC,
                face::RR3_FACE_TOPIC,
                rpc::RR3_OPS_LIST_TOPIC
            )),
        }
    }

    /// 订阅面两臂（其余 Event 忽略——线上全量扇出 + 本地过滤语义）：
    /// net/link 整 body 快照进 link 缓存（面板状态页卡片数据源）；tts_pcm 取
    /// `pcm_b64` 逐帧解码追加，push 成功才计数，坏帧只弃该帧（缺口由点名的
    /// 对账兜住折拒因，不静默）。
    fn on_event(&mut self, topic: &str, body: &Value, bus: &BusHandle) -> Result<(), String> {
        match topic {
            RR3_NET_LINK_TOPIC => {
                *self.link.lock().unwrap_or_else(|p| p.into_inner()) = Some(body.clone());
            }
            player::RR3_TTS_PCM_TOPIC => self.rr3_tts_frame(body, bus),
            _ => {}
        }
        Ok(())
    }

    /// 节拍两面（照现状序）：面板动作排空（while-let try_recv，face/beep 主
    /// 循环串行代执行后回执）→ 表情帧动画一拍（节拍自 config `face_tick_ms`
    /// 数据化，legacy 250ms，推进帧指针；批F5a③ 出帧交 HAL 渲染——sim 后端
    /// 内存记录，真件档 MAX7219 SPI 换后端零编排改动。显示是妆面件，draw
    /// 失败只 eprintln 不退出，同 led worker 口径）。
    fn on_tick(&mut self, now: Instant, _bus: &BusHandle) -> Result<(), String> {
        while let Ok((action, tx)) = self.act_rx.try_recv() {
            let result = match action {
                Rr3PanelAction::Beep => {
                    rr3_beep_play(self.sink.as_mut(), self.cfg.player.volume_pct)
                }
                Rr3PanelAction::Face { expression } => self
                    .face
                    .rr3_face_set(&expression, &self.cfg.face.non_pointable)
                    .map(|(c, p)| rpc::rr3_face_result(c, p))
                    .map_err(|e| rpc::rr3_face_err_text(&e, &self.cfg.face.non_pointable)),
            };
            let _ = tx.send(result);
        }
        if self.face_tick_at.elapsed() >= Duration::from_millis(self.cfg.face.face_tick_ms) {
            // 节拍锚重置用 serve 传入的 now 实参（同拍时刻，帧节拍不可观测）
            self.face_tick_at = now;
            let (expr, ix) = self.face.rr3_face_tick();
            let frame = face::rr3_face_frames(expr)[ix];
            self.display
                .draw(&frame)
                .unwrap_or_else(|e| eprintln!("rr3-interact-face: 出帧失败: {e}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 弃帧留痕闸门（B17 审计）：首帧必留、其后每 64 帧一条，中间帧只进计数——
    /// 坏源连续弃帧时日志不爆、证据不断。
    #[test]
    fn bad_log_gate_keeps_first_then_samples() {
        assert!(rr3_bad_log_due(1), "第一帧必须留痕");
        assert!(!rr3_bad_log_due(2));
        assert!(!rr3_bad_log_due(RR3_TTS_BAD_LOG_EVERY - 1));
        assert!(rr3_bad_log_due(RR3_TTS_BAD_LOG_EVERY));
        assert!(rr3_bad_log_due(RR3_TTS_BAD_LOG_EVERY * 3));
        assert!(!rr3_bad_log_due(RR3_TTS_BAD_LOG_EVERY * 3 + 1));
    }

    /// 收流缓冲：解码追加 + 排空清零（残流不跨轮滞留），坏帧 fail-closed
    #[test]
    fn tts_pcm_buf_pushes_frames_and_takes_all() {
        let mut buf = Rr3TtsPcmBuf::default();
        buf.rr3_tts_buf_push(&rr_ipc::rr3_base64_encode(&[1, 2, 3, 4]))
            .unwrap();
        buf.rr3_tts_buf_push(&rr_ipc::rr3_base64_encode(&[5, 6]))
            .unwrap();
        assert_eq!(buf.rr3_tts_buf_len(), 6);
        assert_eq!(buf.rr3_tts_buf_take(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(buf.rr3_tts_buf_len(), 0, "排空后缓冲清零");
        assert!(
            buf.rr3_tts_buf_push("not-b64!!").is_err(),
            "坏帧 fail-closed"
        );
        // 超帽（本轮审计）：整帧拒收、不部分写入——点名对账随后拒播整轮
        let b64 = rr_ipc::rr3_base64_encode(&[1, 2, 3, 4, 5]);
        assert!(buf.rr3_tts_push_cap(&b64, 4).is_err(), "超帽必须拒");
        assert_eq!(buf.rr3_tts_buf_len(), 0, "超帽帧不得部分写入");
    }

    /// beep 测试音合成（批N5 件五笔2）：确定性——长度 3200（200ms@16k）、
    /// 首样点 sin(0)=0、峰值幅内不削顶（|s| ≤ 12000）且实达半幅以上（可听）、
    /// 两跑逐样本相等
    #[test]
    fn beep_samples_are_deterministic_sine_without_clipping() {
        let s = rr3_beep_samples();
        assert_eq!(s.len(), 3200, "200ms × 16k 样点");
        assert_eq!(s[0], 0, "首样点 sin(0)=0");
        let peak = s.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(peak <= RR3_BEEP_AMP.unsigned_abs(), "幅内不削顶: {peak}");
        assert!(peak > 6_000, "实达半幅以上（可听）: {peak}");
        assert_eq!(rr3_beep_samples(), s, "两跑逐样本相等（确定性）");
    }

    /// beep 动作执行（批N5 件五笔2）：合成→同径增益→直喂播放面——回
    /// played_ms=200，播出流 = 合成样点过同径增益（100% 恒等档对拍）；0% 档
    /// 播出全零流但时长如实 200ms（时长由样点数导出，与 tts 同一口径）
    #[test]
    fn beep_play_feeds_sink_through_gain_path() {
        use rr_hal_api::{Rr3AudioSink, RR3_AUDIO_CFG_16K_MONO16};
        let mut sink = rr_hal::sim::Rr3SimAudioSink::rr3_sim_sink_new(16_000 * 2);
        sink.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
        let v = rr3_beep_play(&mut sink, 100).unwrap();
        assert_eq!(v["played_ms"], 200);
        assert_eq!(
            v.as_object().unwrap().len(),
            1,
            "beep 应答单字段无加戏: {v}"
        );
        let mut want = rr3_beep_samples();
        player::rr3_tts_apply_gain(&mut want, 100);
        assert_eq!(
            sink.rr3_sim_sink_played(),
            want,
            "播出流 = 合成样点过同径增益"
        );

        let mut sink = rr_hal::sim::Rr3SimAudioSink::rr3_sim_sink_new(16_000 * 2);
        sink.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
        let v = rr3_beep_play(&mut sink, 0).unwrap();
        assert_eq!(v["played_ms"], 200, "0% 增益时长如实（样点数导出）");
        assert!(sink.rr3_sim_sink_played().iter().all(|&s| s == 0));
    }
}
