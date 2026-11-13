//! rr3-brain 机制层注册面（批N32 迁移件1 立树；迁移件2 cloud 三客户端；迁移件4
//! 语音主链编排全接线）。
//!
//! [`Rr3BrainDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）。事务面
//! on_rpc 六臂（module.toml [rpc].exposes 六条全接线）：`llm/completion` 已激活
//! （[`cloud::Rr3LlmClient`]，mock 档回显/real 档带历史真调；业务失败 Err 折
//! `{"error": …}` 应答——Python try/except RuntimeError 同 wire）、`ops/list`
//! 快照（[`rpc::rr3_ops_snapshot`] 白名单组体）、`ops/set` 写臂（[`rpc::rr3_ops_set_arm`]
//! 全语义：白名单/类型域校验 → live 内存生效不落盘 → 按组重建受影响客户端——
//! asr/tts/llm 三组各重建、dialog 组就地改参 [`dialog::Rr3Dialog::max_utterance_ms`]）；
//! `model/list|set|test` 模型行面（[`rpc::rr3_model_list`] /
//! [`rpc::rr3_model_set_arm`] 落盘 + llm 组重建 / [`rpc::rr3_model_test_arm`] 单次探测）；
//! 未知 topic 返 Err 由 serve 折 `{"error": …}` 应答（Python 主循环兜底分支同 wire）。
//!
//! 事件面 on_event 两臂（自 Python composite.py 主循环事件分支逐语义迁入）：
//! pcm → [`dialog::Rr3Dialog`] 积累/预滚；vad_start → onset 起轮 / offset 判停
//! 触发整轮（[`Rr3BrainDispatcher::rr3_finish_turn`]：ASR → LLM → TTS → 分帧发布
//! tts_pcm → interact 播放点名 → intent 发布）。节拍面 on_tick 一臂：
//! utterance_expired 时长判停触发整轮（与 offset 臂两路共用同一 rr3_finish_turn，
//! ADR-0016 增补裁定 2）。订阅面外事件本地过滤静默（Python 主循环 if/elif 同形状）。
//!
//! 错误语义两档（rr3mod 同款）：on_rpc 返 Err = 业务性失败折 `{"error": …}` 应答；
//! on_event/on_tick 返 Err = 传输性/设备级失败上抛致命（云端 4xx/5xx/空文本/被调
//! 方超时臂内弃轮返 Ok，发布/应答写不出去上抛，模块不装"活着"，死交 supervisor
//! 按 restart_policy 裁定）。

use std::time::{Duration, Instant};

use rr_bus_api::Rr3TimeBase;
use rr_ipc::rr3_base64_encode;
use serde_json::{json, Value};

use rr3mod::{BusHandle, Dispatcher};

use crate::boot::{rr3_cloud_cfg_of, Rr3BrainCfg, Rr3WakeCfg};
use crate::cloud::{Rr3AsrClient, Rr3LlmClient, Rr3TtsClient};
use crate::dialog::{Rr3Dialog, Rr3DialogTurn};
use crate::intent::rr3_extract_actions;
use crate::rpc::{
    rr3_model_list, rr3_model_row_of, rr3_model_set_arm, rr3_model_test_arm, rr3_ops_set_arm,
    rr3_ops_snapshot, rr3_py_str,
};

/// 本模块 exposes 的 completion 点名（module.toml [rpc].exposes 同源）
pub(crate) const RR3_COMPLETION_TOPIC: &str = "rr3-brain/llm/completion";

/// ops 槽快照读口（批N4 件六二 B-12）
pub(crate) const RR3_OPS_LIST_TOPIC: &str = "rr3-brain/ops/list";

/// ops 槽写口（批N4 件九三 配置面）
pub(crate) const RR3_OPS_SET_TOPIC: &str = "rr3-brain/ops/set";

/// 模型行列表读口（ADR-0023 面板模型面）
pub(crate) const RR3_MODEL_LIST_TOPIC: &str = "rr3-brain/model/list";

/// 模型行 CRUD 写口（upsert/activate/delete；落盘 + live 生效）
pub(crate) const RR3_MODEL_SET_TOPIC: &str = "rr3-brain/model/set";

/// 模型行连通性测试口（按名探测一次，不写盘不改内存）
pub(crate) const RR3_MODEL_TEST_TOPIC: &str = "rr3-brain/model/test";

/// exposes 申报面（module.toml [rpc].exposes 同源六件；SURFACE 自述与未知 topic
/// 兜底文案共此一源——列表 repr 是 Python RPC_EXPOSES 的形状）
pub(crate) const RR3_RPC_EXPOSES: [&str; 6] = [
    "rr3-brain/llm/completion",
    "rr3-brain/ops/list",
    "rr3-brain/ops/set",
    "rr3-brain/model/list",
    "rr3-brain/model/set",
    "rr3-brain/model/test",
];

/// 申报发布面（与 module.toml [events].publishes 同源双写，composite.py PUBLISHES 同义）
const RR3_PUBLISHES: [&str; 2] = ["rr3-brain/intent/result", "rr3-brain/audio/tts_pcm"];

/// 申报订阅面（与 module.toml [events].subscribes 同源双写，C14 实然面：
/// pcm + vad_start 是语音主链入口；消费编排随迁移件四落）
const RR3_SUBSCRIBES: [&str; 2] = ["rr3-sense/audio/pcm", "rr3-sense/audio/vad_start"];

/// 启动自述（Python composite.py `rr3_brain_run` 打印序列逐字不变）：架构面
/// "这个模块管什么"的说明，版本随 Python `__version__` 逐字。机制面自述归
/// dispatch（装配入口只留启动编排，ADR-0017 五层表 ≤80 行门）。
pub(crate) fn rr3_brain_surface() {
    eprintln!("rr3-brain 0.1.0 (M4 实装)");
    rr3_surface_echo("  publish   ", &RR3_PUBLISHES);
    rr3_surface_echo("  subscribe ", &RR3_SUBSCRIBES);
    rr3_surface_echo("  rpc       ", &RR3_RPC_EXPOSES);
}

/// 自述行回声：前缀连对齐空白逐字沿用（三面共用一柄）。
fn rr3_surface_echo(prefix: &str, topics: &[&str]) {
    for topic in topics {
        eprintln!("{prefix}{topic}");
    }
}

/// 订阅消费面两臂事件（module.toml [events].subscribes 同源）
pub(crate) const RR3_TOPIC_PCM: &str = "rr3-sense/audio/pcm";
pub(crate) const RR3_TOPIC_VAD: &str = "rr3-sense/audio/vad_start";

/// 事务面点名（interact exposes 的播放口；Python composite TOPIC_TTS 同源）
pub(crate) const RR3_TOPIC_TTS: &str = "rr3-interact/tts/text";

/// 发布面两件（module.toml [events].publishes 同源）
pub(crate) const RR3_TOPIC_TTS_PCM: &str = "rr3-brain/audio/tts_pcm";
pub(crate) const RR3_TOPIC_INTENT: &str = "rr3-brain/intent/result";

/// TTS 下行流分帧规格（MCP-1 定形）：逐字段复用上行 rr3-sense/audio/pcm 的事件体，
/// 同 20ms 一块（@16k/mono/s16le = 320 样点 = 640 字节），base64 后 ≈856 字符。
/// 采样规格与 sense/interact 同一张表（16k/mono/16bit）。
pub(crate) const RR3_TTS_SAMPLE_RATE: u32 = 16_000;
pub(crate) const RR3_TTS_PCM_FRAME_BYTES: usize = 640;

mod arms;
#[cfg(test)]
mod tests;
mod tts;
mod turn;
mod wake;

use arms::*;
use tts::*;
use turn::*;
use wake::*;

/// 主循环状态整体持有：config 文档树（ops/set live 真值）＋ boot 投影＋cloud 三
/// 客户端（completion 臂消费、ops/set 写后重建）＋对话状态机（utterance 边界与
/// 历史登记，ops/set dialog 组就地改参）＋流水位（trace_seq 轮次号、pcm_seq TTS
/// 下行流 seq——模块级单调不随轮复位）＋单调时基（onset 记点与 expired 判定同锚
/// [`Rr3TimeBase`]，0.0.5 批I A-2 同款）。
pub(crate) struct Rr3BrainDispatcher {
    cfg: Rr3BrainCfg,
    clients: Rr3CloudClients,
    dialog: Rr3Dialog,
    trace_seq: u64,
    pcm_seq: u64,
    tb: Rr3TimeBase,
    /// 最近一次接受轮（Ack/Command/FollowUp）的时刻（ms）——唤醒后跟进窗基准；
    /// `None` = 从未唤醒（非唤醒文本在窗外一律 Ignore）。
    last_wake_ms: Option<u64>,
    /// 固定唤醒应答的 PCM：启动时预合成，唤醒轮直接复用，不再每次等待云端 TTS。
    wake_ack_pcm: Option<Vec<u8>>,
}

impl Rr3BrainDispatcher {
    /// 装配构造（main.rs 装配段调用）：config 解析产物整体入表＋三客户端装配＋
    /// 状态机以 config 话轮封顶起轮（Python Rr3Dialog(max_utterance_ms=…) 同形状）。
    pub(crate) fn new(cfg: Rr3BrainCfg) -> Self {
        let clients = Rr3CloudClients::from_cfg(&cfg);
        let dialog = Rr3Dialog::rr3_new(cfg.max_utterance_ms);
        let mut dispatcher = Self {
            cfg,
            clients,
            dialog,
            trace_seq: 0,
            pcm_seq: 0,
            tb: Rr3TimeBase::rr3_time_base_new(),
            last_wake_ms: None,
            wake_ack_pcm: None,
        };
        dispatcher.rr3_prewarm_wake_ack();
        dispatcher
    }

    /// 固定唤醒语只合成一次；失败不阻断模块启动，首个唤醒轮仍会按需重试。
    fn rr3_prewarm_wake_ack(&mut self) {
        if !self.cfg.wake.enable || self.cfg.wake.ack.trim().is_empty() {
            return;
        }
        let (res, started) = self.rr3_ack_synth();
        self.rr3_ack_take(res, started);
    }

    /// 固定唤醒语合成一次（返回结果与起算时刻，落账归调用方）。
    fn rr3_ack_synth(&self) -> (Result<Vec<u8>, String>, Instant) {
        let started = Instant::now();
        let res = self
            .clients
            .tts
            .rr3_synthesize(&self.cfg.wake.ack, RR3_TTS_SAMPLE_RATE);
        (res, started)
    }

    /// 预合成结果落账：非空 PCM 入表，空/失败只留痕（首轮按需重试）。
    fn rr3_ack_take(&mut self, res: Result<Vec<u8>, String>, started: Instant) {
        if let Some(pcm) = rr3_ack_pcm_of(started, res) {
            self.wake_ack_pcm = Some(pcm);
        }
    }

    /// 订阅面两臂（Python 主循环事件分支逐语义）：pcm → 提取事件体 pcm_b64/ms
    /// 字段喂状态机积累/预滚（机制吃适配后的字段值——S-7 同款，坏 base64 上抛）；
    /// vad_start → 提取 edge 字段判停真值触发整轮。泛型写口使事件臂与节拍臂纯
    /// 夹具可测（[`Rr3TurnBus`]）。
    fn rr3_event_arm<B: Rr3TurnBus>(
        &mut self,
        topic: &str,
        body: &Value,
        bus: &B,
    ) -> Result<(), String> {
        if topic == RR3_TOPIC_PCM {
            // Python: body.get("pcm_b64") 直取（sense 事件体保证在册）＋
            // ms 缺席折 0（攒窗按 0ms 帧计，不虚占预滚窗）
            let pcm_b64 = body
                .get("pcm_b64")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let ms = body
                .get("ms")
                .and_then(Value::as_f64)
                .map(|f| f as i64)
                .unwrap_or(0);
            self.dialog.rr3_feed_pcm(pcm_b64, ms)
        } else if topic == RR3_TOPIC_VAD {
            // Python: edge = body.get("edge", "")——不认的边沿由状态机静默忽略
            let edge = body.get("edge").and_then(Value::as_str).unwrap_or("");
            if self.dialog.rr3_feed_vad(edge, self.tb.rr3_now_ms()) {
                self.rr3_finish_turn(bus)?;
            }
            Ok(())
        } else {
            // 订阅面外事件：本地过滤静默（module.toml 申报面之外的来件不消费）
            Ok(())
        }
    }

    /// 节拍面一臂：utterance_expired 时长判停触发整轮（与 offset 臂共用
    /// rr3_finish_turn——连续语音无 offset 时也要有界）。
    fn rr3_tick_arm<B: Rr3TurnBus>(&mut self, bus: &B) -> Result<(), String> {
        if self.dialog.rr3_utt_expired(self.tb.rr3_now_ms()) {
            self.rr3_finish_turn(bus)?;
        }
        Ok(())
    }

    /// 判停后的一整轮（自 Python composite.py `rr3_finish_turn` 逐语义迁入）：
    /// ASR → LLM → TTS → 下发 PCM 流 → interact 播放 → intent 发布。
    ///
    /// 业务失败（云端错误 / 空文本 / 被调方超时）折日志弃轮返 Ok；传输失败
    /// （总线写不出去）Err 上抛由 serve 收口致命。trace_seq/pcm_seq 流水位在
    /// Dispatcher 上跨轮延续——pcm_seq 模块级单调不随轮次复位。
    /// 计账拆两本（B-16①，不许一个词盖两件事）：`rounds_replied` = LLM 应答发布
    /// 完毕（rr3_record 成功后 bump）；`rounds_audible` = interact 回执 played_ms>0
    /// （点名失败/播出 0ms 不计）。
    fn rr3_finish_turn<B: Rr3TurnBus>(&mut self, bus: &B) -> Result<(), String> {
        let Some((pcm, trace)) = self.rr3_turn_utt() else {
            return Ok(());
        };

        let asr_started = Instant::now();
        let Some((user_text, asr_cost_ms)) = self.rr3_turn_asr(bus, trace, &pcm, asr_started)
        else {
            return Ok(());
        };

        let Some(reply) = self.rr3_turn_reply(bus, trace, &user_text, asr_cost_ms) else {
            return Ok(());
        };
        let reply_text = reply.text;
        let llm_cost_ms = reply.llm_ms;
        let is_wake_ack = reply.ack;

        let Some((mut reply_pcm, tts_cost_ms)) =
            self.rr3_turn_tts(bus, trace, &reply_text, is_wake_ack)
        else {
            return Ok(());
        };
        // ready_ms 与拆分前同刻：ASR 起点到 TTS 完成（含唤醒判决与 LLM）。
        let ready_ms = asr_started.elapsed().as_millis();
        let tts_ms = Self::rr3_pcm_ms(&mut reply_pcm);

        let played = self.rr3_turn_play(bus, trace, &reply_text, &reply_pcm)?;

        // 意图→动作映射在 brain（策略层唯一归属，0.0.3 Q1 裁定）：结构化 actions
        // 随 intent/result 广播，执行器按 exposes 归属门各自认领（brain 不单播不
        // 等待）；只喂 user_text，机器人自己的 reply_text 不参与匹配
        let actions_n = self.rr3_turn_broadcast(bus, trace, &user_text, &reply_text, tts_ms)?;
        self.rr3_round_record(bus, trace, &user_text, &reply_text);
        eprintln!(
            "对话轮完成: trace={trace} user='{user_text}' reply='{reply_text}' asr_ms={asr_cost_ms} llm_ms={llm_cost_ms} tts_cost_ms={tts_cost_ms} ready_ms={ready_ms} tts_pcm_ms={tts_ms}{played} actions={actions_n}"
        );
        // 跟进窗基准 = 本轮完成（播放回执后）时刻：基准若停在轮内 ASR 完成点，
        // LLM/TTS/整段播放会吃掉用户的 30s 窗——2026-10-31 实机两轮紧邻追问
        // （"再说一下…"/"…推荐一下？"）因此被误判窗外静默。完成时刻起算，
        // 用户在机器人说完后拥有完整 followup_ms。
        self.last_wake_ms = Some(self.tb.rr3_now_ms());
        Ok(())
    }

    /// 轮末入账：对话历史登记 + 应答轮计数（计数点 = rr3_record 成功，方案 §4
    /// 归属表）。键名拆账（B-16①）：`rounds_replied` 只记"应答发布完毕"。
    fn rr3_round_record<B: Rr3TurnBus>(&mut self, bus: &B, trace: u64, user: &str, reply: &str) {
        self.dialog.rr3_record(Rr3DialogTurn {
            user_text: user.to_string(),
            reply_text: reply.to_string(),
            trace_id: trace,
        });
        bus.counter_bump("rounds_replied", 1);
    }

    /// 轮末意图广播：结构化 actions 随 intent/result 广播，执行器按 exposes
    /// 归属门各自认领（brain 不单播不等待）；只喂 user_text。回 actions 条数。
    fn rr3_turn_broadcast<B: Rr3TurnBus>(
        &self,
        bus: &B,
        trace: u64,
        user_text: &str,
        reply_text: &str,
        tts_ms: u64,
    ) -> Result<usize, String> {
        let actions = rr3_extract_actions(user_text);
        let actions_n = actions.len();
        bus.publish(
            RR3_TOPIC_INTENT,
            Self::rr3_intent_body(trace, user_text, reply_text, tts_ms, actions),
        )?;
        Ok(actions_n)
    }

    /// 轮末意图广播体（一行对账所需的五个字段原样透传）。
    fn rr3_intent_body(
        trace: u64,
        user_text: &str,
        reply_text: &str,
        tts_ms: u64,
        actions: Vec<Value>,
    ) -> Value {
        json!({
            "trace": trace,
            "user_text": user_text,
            "reply_text": reply_text,
            "tts_ms": tts_ms,
            "actions": actions,
        })
    }

    /// 应答 PCM 折算毫秒：奇数长度截尾（Python `reply_pcm[:-1]` 同形状）后按 16k 计。
    fn rr3_pcm_ms(pcm: &mut Vec<u8>) -> u64 {
        if pcm.len() % 2 == 1 {
            pcm.pop();
        }
        let samples = pcm.len() as u64 / 2;
        samples / 16_000 * 1000 + samples % 16_000 * 1000 / 16_000
    }

    /// 取本轮 utterance 并定格轮次号：空缓冲 = 无轮可开（不占 trace 号）。
    /// `RR3_BRAIN_DUMP_UTT` 诊断落盘（env 门控，默认关）随取随做。
    fn rr3_turn_utt(&mut self) -> Option<(Vec<u8>, u64)> {
        let pcm = self.dialog.rr3_take_utterance();
        if pcm.is_empty() {
            return None;
        }
        self.trace_seq += 1;
        let trace = self.trace_seq;
        Self::rr3_dump_utt(&pcm, trace);
        Some((pcm, trace))
    }

    /// 诊断面（2026-10-31，env 门控，默认关）：把本轮**送 ASR 的 utterance**
    /// 落盘成裸 PCM（s16le 16k mono，字节数/32 = 毫秒）——唤醒词被截断/"只识别
    /// 到你好"类问题，音频证据比转写文本可靠。空值/未设 = 零落盘。
    fn rr3_dump_utt(pcm: &[u8], trace: u64) {
        let Ok(dir) = std::env::var("RR3_BRAIN_DUMP_UTT") else {
            return;
        };
        let dir = dir.trim();
        if !dir.is_empty() {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(format!("{dir}/utt-{trace}.pcm"), pcm);
        }
    }

    /// ASR 段：转写失败/空文本折日志弃轮（None），不占后续段。
    fn rr3_turn_asr<B: Rr3TurnBus>(
        &mut self,
        bus: &B,
        trace: u64,
        pcm: &[u8],
        started: Instant,
    ) -> Option<(String, u128)> {
        let user_text = self.rr3_asr_call(bus, trace, pcm, started)?;
        let asr_cost_ms = started.elapsed().as_millis();
        bus.counter_bump("asr_ok", 1);
        if user_text.trim().is_empty() {
            eprintln!("轮次 {trace}: 识别为空文本，弃轮（asr_ms={asr_cost_ms}）");
            return None;
        }
        Some((user_text, asr_cost_ms))
    }

    /// 识别一跳：失败即记账弃轮（None），文案带耗时与错因。
    fn rr3_asr_call<B: Rr3TurnBus>(
        &self,
        bus: &B,
        trace: u64,
        pcm: &[u8],
        started: Instant,
    ) -> Option<String> {
        match self.clients.asr.rr3_transcribe(pcm, RR3_TTS_SAMPLE_RATE) {
            Ok(t) => Some(t),
            Err(e) => {
                bus.counter_bump("asr_fail", 1);
                eprintln!(
                    "轮次 {trace}: ASR 失败，弃轮（asr_ms={}，{e}）",
                    started.elapsed().as_millis()
                );
                None
            }
        }
    }

    /// 唤醒门控 + LLM 段：命中唤醒 → 固定应答/命令进 LLM；窗外未命中 → 静默
    /// 丢弃（None）；窗内未命中 → 追问续轮。`enable=false` 时恒 FollowUp。
    fn rr3_turn_reply<B: Rr3TurnBus>(
        &mut self,
        bus: &B,
        trace: u64,
        user_text: &str,
        asr_cost_ms: u128,
    ) -> Option<Rr3TurnReply> {
        // 唤醒门控（2026-10-31）：命中 → 固定应答/命令进 LLM；窗外未命中 → 静默
        // 丢弃（不 LLM、不 TTS，用户听不到任何回声）；窗内未命中 → 追问续轮。
        // `enable=false` 时 `rr3_wake_decide` 恒 FollowUp（既有行为逐点等价）。
        let now = self.tb.rr3_now_ms();
        let mut llm_input: Option<String> = None;
        let mut reply_text = String::new();
        let wake_action = rr3_wake_decide(user_text, &self.cfg.wake, now, self.last_wake_ms);
        let is_wake_ack = matches!(wake_action, Rr3WakeAction::Ack);
        match wake_action {
            Rr3WakeAction::Ignore => {
                // 带 ASR 原文：唤醒不中时唯一可判读面（识别变体/前置语气词/误识别）
                eprintln!(
                    "轮次 {trace}: 未命中唤醒词（窗外），静默（ASR='{user_text}'，asr_ms={asr_cost_ms}）"
                );
                return None;
            }
            Rr3WakeAction::Ack => {
                // 带 ASR 原文：Ack 轮是唯一不出"对话轮完成"user 文本的轮（固定
                // 应答），留一行可判读面——2026-10-31 实机排查 TTS 超时静默时
                // 曾只能靠轮次时序倒推是哪句。
                eprintln!("轮次 {trace}: 唤醒应答（ASR='{user_text}'）");
                self.last_wake_ms = Some(now);
                reply_text = self.cfg.wake.ack.clone();
            }
            Rr3WakeAction::Command(cmd) => {
                self.last_wake_ms = Some(now);
                llm_input = Some(cmd);
            }
            Rr3WakeAction::FollowUp => {
                self.last_wake_ms = Some(now);
                llm_input = Some(user_text.to_string());
            }
        }
        let mut llm_cost_ms = "-".to_string();
        if let Some(input) = llm_input {
            let llm_started = Instant::now();
            let completed = self.clients.llm.rr3_complete(&input);
            llm_cost_ms = llm_started.elapsed().as_millis().to_string();
            let t = completed
                .map_err(|e| {
                    bus.counter_bump("llm_fail", 1);
                    eprintln!(
                        "轮次 {trace}: LLM 失败，弃轮（user='{user_text}'，llm_ms={llm_cost_ms}，{e}）"
                    );
                    e
                })
                .ok()?;
            reply_text = t;
        }
        Some(Rr3TurnReply {
            text: reply_text,
            llm_ms: llm_cost_ms,
            ack: is_wake_ack,
        })
    }

    /// TTS 段：唤醒固定应答命中预合成 PCM 则复用；合成失败折日志弃轮（None）。
    fn rr3_turn_tts<B: Rr3TurnBus>(
        &mut self,
        bus: &B,
        trace: u64,
        reply_text: &str,
        is_wake_ack: bool,
    ) -> Option<(Vec<u8>, u128)> {
        let tts_started = Instant::now();
        let cached_ack = if is_wake_ack {
            self.wake_ack_pcm.clone()
        } else {
            None
        };
        let reply_pcm = if let Some(pcm) = cached_ack {
            pcm
        } else {
            self.clients
                .tts
                .rr3_synthesize(reply_text, RR3_TTS_SAMPLE_RATE)
                .map_err(|e| {
                    bus.counter_bump("tts_fail", 1);
                    eprintln!(
                        "轮次 {trace}: TTS 失败，弃轮（tts_cost_ms={}，{e}）",
                        tts_started.elapsed().as_millis()
                    );
                    e
                })
                .ok()?
        };
        let tts_cost_ms = tts_started.elapsed().as_millis();
        if is_wake_ack && self.wake_ack_pcm.is_none() && !reply_pcm.is_empty() {
            self.wake_ack_pcm = Some(reply_pcm.clone());
        }
        Some((reply_pcm, tts_cost_ms))
    }

    /// 语音下行：先按 MCP-1 的形状分帧发布 tts_pcm（stream），再走事务面点名
    /// interact 的 tts/text——点名是"该不该播"的**唯一判据**，流只是载荷通路
    /// （§20 第 1 条 (a)）。返回播放回显后缀（`played_ms=...` 或空）。
    fn rr3_turn_play<B: Rr3TurnBus>(
        &mut self,
        bus: &B,
        trace: u64,
        reply_text: &str,
        reply_pcm: &[u8],
    ) -> Result<String, String> {
        // 点名的 args 带 pcm_bytes，供播放面校验缓冲完整性（半流/空缓冲不静默，MC-1）。
        self.pcm_seq = rr3_publish_tts_pcm(bus, reply_pcm, self.pcm_seq)?;
        let mut played = String::new();
        match bus.call(
            RR3_TOPIC_TTS,
            &json!({ "text": reply_text, "pcm_bytes": reply_pcm.len() }),
            Duration::from_secs_f64(self.cfg.call_timeout_s),
        ) {
            Ok(reply) => {
                // audible 记账（B-16①）：播出且 >0ms 才算"用户听得见"——点名失败
                // 与 played_ms=0 都不进这本账。Number 判型 = Python isinstance
                // (int, float) 且非 bool 同形状（Null/串不进账）。
                let played_ms = reply.get("played_ms").cloned().unwrap_or(Value::Null);
                played = format!(" played_ms={}", rr3_py_str(&played_ms));
                if played_ms.as_f64().is_some_and(|ms| ms > 0.0) {
                    bus.counter_bump("rounds_audible", 1);
                }
            }
            Err(e) => eprintln!("轮次 {trace}: interact 播放点名失败（{e}）"),
        }
        Ok(played)
    }
}
