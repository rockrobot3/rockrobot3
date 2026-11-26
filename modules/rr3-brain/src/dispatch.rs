// SPDX-License-Identifier: Apache-2.0
//! rr3-brain 机制层注册面：[`Rr3BrainDispatcher`] 实现 rr3mod [`Dispatcher`]。
//!
//! `on_rpc` 六臂（module.toml `[rpc].exposes` 全接线）：`llm/completion`（mock 回显 /
//! real 带历史真调，业务失败 Err 折 `{"error": …}`）；`ops/list` 白名单快照；`ops/set`
//! （白名单/类型域 → live 生效不落盘 → 按组重建 asr/tts/llm 客户端，dialog 组就地改参）；
//! `model/list|set|test`（落盘 + llm 组重建 / 单次探测）。未知 topic 返 Err，由 serve 折
//! `{"error": …}`。
//!
//! `on_event` 两臂：pcm → [`dialog::Rr3Dialog`] 积累/预滚；vad_start → onset 起轮 /
//! offset 判停触发整轮（[`Rr3BrainDispatcher::rr3_finish_turn`]：ASR → LLM → TTS →
//! 分帧发布 tts_pcm → interact 播放点名 → intent 发布）。`on_tick` 一臂：
//! utterance_expired 时长判停（与 offset 共用同一 finish_turn）；订阅面外事件本地
//! 过滤静默。
//!
//! 错误语义两档（rr3mod 同款）：`on_rpc` 返 Err = 业务失败折错误应答；`on_event`/
//! `on_tick` 返 Err = 传输/设备级失败上抛致命（云端 4xx/5xx/空文本/被调方超时臂内
//! 弃轮返 Ok；发布/应答写不出去上抛，模块不装"活着"，死交 supervisor）。

use std::time::{Duration, Instant};

use rr_bus_api::Rr3TimeBase;
use rr_ipc::rr3_base64_encode;
use serde_json::{json, Value};

use rr3mod::{BusHandle, Dispatcher};

use crate::boot::Rr3BrainCfg;
use crate::dialog::{Rr3Dialog, Rr3DialogTurn};
use crate::intent::rr3_extract_actions;
use crate::rpc::{
    rr3_model_list, rr3_model_set_arm, rr3_model_test_arm, rr3_ops_set_arm, rr3_ops_snapshot,
    rr3_py_str,
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
mod cloud_clients;
#[cfg(test)]
mod tests;
mod tts;
mod turn;
mod wake;

use arms::*;
use cloud_clients::*;
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
    /// 状态机以 config 话轮封顶起轮（Python Rr3Dialog(max_utterance_ms=…) 同形状）；
    /// 客户端装配失败回 Err，由装配段点名退出。
    pub(crate) fn new(cfg: Rr3BrainCfg) -> Result<Self, String> {
        let clients = Rr3CloudClients::from_cfg(&cfg)?;
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
        Ok(dispatcher)
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
}
