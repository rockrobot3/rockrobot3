//! rr3-brain 机制层注册面（批N32 迁移件1 立树；迁移件2 cloud 三客户端；迁移件4
//! 语音主链编排全接线）。
//!
//! [`Rr3BrainDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）。事务面
//! on_rpc 三臂（module.toml [rpc].exposes 三条全接线）：`llm/completion` 已激活
//! （[`cloud::Rr3LlmClient`]，mock 档回显/real 档带历史真调；业务失败 Err 折
//! `{"error": …}` 应答——Python try/except RuntimeError 同 wire）、`ops/list`
//! 快照（[`rpc::rr3_ops_snapshot`] 白名单组体）、`ops/set` 写臂（[`rpc::rr3_ops_set_arm`]
//! 全语义：白名单/类型域校验 → live 内存生效不落盘 → 按组重建受影响客户端——
//! asr/tts/llm 三组各重建、dialog 组就地改参 [`dialog::Rr3Dialog::max_utterance_ms`]）；
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

use crate::boot::{rr3_cloud_cfg_of, Rr3BrainCfg};
use crate::cloud::{Rr3AsrClient, Rr3LlmClient, Rr3TtsClient};
use crate::dialog::{Rr3Dialog, Rr3DialogTurn};
use crate::intent::rr3_extract_actions;
use crate::rpc::{rr3_model_row_of, rr3_ops_set_arm, rr3_ops_snapshot, rr3_py_str};

/// 本模块 exposes 的 completion 点名（module.toml [rpc].exposes 同源）
pub(crate) const RR3_COMPLETION_TOPIC: &str = "rr3-brain/llm/completion";

/// ops 槽快照读口（批N4 件六二 B-12）
pub(crate) const RR3_OPS_LIST_TOPIC: &str = "rr3-brain/ops/list";

/// ops 槽写口（批N4 件九三 配置面）
pub(crate) const RR3_OPS_SET_TOPIC: &str = "rr3-brain/ops/set";

/// exposes 申报面（module.toml [rpc].exposes 同源三件；SURFACE 自述与未知 topic
/// 兜底文案共此一源——列表 repr 是 Python RPC_EXPOSES 的形状）
pub(crate) const RR3_RPC_EXPOSES: [&str; 3] = [
    "rr3-brain/llm/completion",
    "rr3-brain/ops/list",
    "rr3-brain/ops/set",
];

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

/// 整轮编排面的总线写口窄化（Python rr3_finish_turn 首参 bus 的鸭子类型对位）：
/// 编排只认 counter/publish/call 三动作，记录型测试夹具与生产 [`BusHandle`] 同一
/// 契约（[`Rr3BrainDispatcher::rr3_finish_turn`] 泛型消费）。
pub(crate) trait Rr3TurnBus {
    fn counter_bump(&self, key: &str, by: u64);
    fn publish(&self, topic: &str, body: Value) -> Result<(), String>;
    fn call(&self, topic: &str, args: &Value, timeout: Duration) -> Result<Value, String>;
}

impl Rr3TurnBus for BusHandle {
    fn counter_bump(&self, key: &str, by: u64) {
        self.counter_bump(key, by)
    }

    fn publish(&self, topic: &str, body: Value) -> Result<(), String> {
        self.publish(topic, body)
    }

    fn call(&self, topic: &str, args: &Value, timeout: Duration) -> Result<Value, String> {
        self.call(topic, args, timeout)
    }
}

/// cloud 三客户端整体（批N32 迁移件2：Python composite 装配段同形状——asr/tts
/// 读 cloud 段、llm 读模型行实效；ops/set 写后按组重建的重建目标）。
pub(crate) struct Rr3CloudClients {
    pub(crate) asr: Rr3AsrClient,
    pub(crate) llm: Rr3LlmClient,
    pub(crate) tts: Rr3TtsClient,
}

impl Rr3CloudClients {
    /// 装配构造（main.rs 装配段与测试共用）：cloud 段四键＋模型行实效入表。
    /// boot 校验保证在册，取数失败 panic 点名（不可达路径 fail-closed）。
    pub(crate) fn from_cfg(cfg: &Rr3BrainCfg) -> Self {
        let cloud = rr3_cloud_cfg_of(&cfg.doc).expect("config [rr3-brain.cloud] 段 boot 校验在册");
        Self {
            asr: Rr3AsrClient::new(&cloud.base_url, &cloud.asr_model, cloud.timeout_s),
            llm: Rr3LlmClient::new(
                &cfg.model.base_url,
                &cfg.model.model_id,
                cloud.timeout_s,
                cfg.model.key_ref.clone(),
                cfg.model.mode,
            ),
            tts: Rr3TtsClient::new(&cloud.base_url, &cloud.tts_model, cloud.timeout_s),
        }
    }
}

/// 按组重建受影响客户端（自 Python composite.py ops/set 分支的重建段逐语义迁入）：
/// asr/tts 组读 cloud 段现值、llm 组重解析模型行实效（超时恒读 cloud 段 timeout_s
/// ——Python float(cloud["timeout_s"]) 同形状）；dialog 组无客户端动作，就地改参
/// max_utterance_ms（live 生效不落盘——expired 判定现读新值，不丢开轮状态）。
/// 取数失败返 Err——Python 同路径 RuntimeError 逃逸主循环 exit(1)，此处由 on_rpc
/// 折 error 应答（写已生效；该路径写面已拦结构错误、实际不可达，收敛宽一笔如实
/// 登记）。
fn rr3_rebuild_clients(
    cfg: &Rr3BrainCfg,
    group: &str,
    new: &Value,
    dialog: &mut Rr3Dialog,
    clients: &mut Rr3CloudClients,
) -> Result<(), String> {
    match group {
        "asr" => {
            let cloud = rr3_cloud_cfg_of(&cfg.doc)?;
            clients.asr = Rr3AsrClient::new(&cloud.base_url, &cloud.asr_model, cloud.timeout_s);
        }
        "tts" => {
            let cloud = rr3_cloud_cfg_of(&cfg.doc)?;
            clients.tts = Rr3TtsClient::new(&cloud.base_url, &cloud.tts_model, cloud.timeout_s);
        }
        "llm" => {
            let row = rr3_model_row_of(cfg)?;
            let timeout_s = rr3_cloud_cfg_of(&cfg.doc)?.timeout_s;
            clients.llm = Rr3LlmClient::new(
                &row.base_url,
                &row.model_id,
                timeout_s,
                row.key_ref,
                row.mode,
            );
        }
        // dialog（rr3_ops_write 白名单保证四组，其余即 dialog）：Python
        // dialog.max_utterance_ms = int(new) 同位——new 是写臂类型域校验的整数
        _ => {
            dialog.max_utterance_ms = new
                .as_i64()
                .expect("ops/set 写臂类型域校验保证 dialog 组 new 是整数");
        }
    }
    Ok(())
}

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
}

impl Rr3BrainDispatcher {
    /// 装配构造（main.rs 装配段调用）：config 解析产物整体入表＋三客户端装配＋
    /// 状态机以 config 话轮封顶起轮（Python Rr3Dialog(max_utterance_ms=…) 同形状）。
    pub(crate) fn new(cfg: Rr3BrainCfg) -> Self {
        let clients = Rr3CloudClients::from_cfg(&cfg);
        let dialog = Rr3Dialog::rr3_new(cfg.max_utterance_ms);
        Self {
            cfg,
            clients,
            dialog,
            trace_seq: 0,
            pcm_seq: 0,
            tb: Rr3TimeBase::rr3_time_base_new(),
        }
    }

    /// 订阅面两臂（Python 主循环事件分支逐语义）：pcm → 状态机积累/预滚（坏
    /// base64 上抛）；vad_start → feed_vad 判停真值触发整轮。泛型写口使事件臂
    /// 与节拍臂纯夹具可测（[`Rr3TurnBus`]）。
    fn rr3_event_arm<B: Rr3TurnBus>(
        &mut self,
        topic: &str,
        body: &Value,
        bus: &B,
    ) -> Result<(), String> {
        match topic {
            RR3_TOPIC_PCM => self.dialog.rr3_feed_pcm(body),
            RR3_TOPIC_VAD => {
                if self.dialog.rr3_feed_vad(body, self.tb.rr3_now_ms()) {
                    self.rr3_finish_turn(bus)?;
                }
                Ok(())
            }
            // 订阅面外事件：本地过滤静默（module.toml 申报面之外的来件不消费）
            _ => Ok(()),
        }
    }

    /// 节拍面一臂：utterance_expired 时长判停触发整轮（与 offset 臂共用
    /// rr3_finish_turn——连续语音无 offset 时也要有界）。
    fn rr3_tick_arm<B: Rr3TurnBus>(&mut self, bus: &B) -> Result<(), String> {
        if self.dialog.rr3_utterance_expired(self.tb.rr3_now_ms()) {
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
        let pcm = self.dialog.rr3_take_utterance();
        if pcm.is_empty() {
            return Ok(());
        }
        self.trace_seq += 1;
        let trace = self.trace_seq;

        let user_text = match self
            .clients
            .asr
            .rr3_transcribe(&pcm, RR3_TTS_SAMPLE_RATE)
        {
            Ok(t) => t,
            Err(e) => {
                bus.counter_bump("asr_fail", 1);
                eprintln!("轮次 {trace}: ASR 失败，弃轮（{e}）");
                return Ok(());
            }
        };
        bus.counter_bump("asr_ok", 1);
        if user_text.trim().is_empty() {
            eprintln!("轮次 {trace}: 识别为空文本，弃轮");
            return Ok(());
        }

        let reply_text = match self.clients.llm.rr3_complete(&user_text) {
            Ok(t) => t,
            Err(e) => {
                bus.counter_bump("llm_fail", 1);
                eprintln!("轮次 {trace}: LLM 失败，弃轮（user='{user_text}'，{e}）");
                return Ok(());
            }
        };

        let mut reply_pcm = match self
            .clients
            .tts
            .rr3_synthesize(&reply_text, RR3_TTS_SAMPLE_RATE)
        {
            Ok(p) => p,
            Err(e) => {
                bus.counter_bump("tts_fail", 1);
                eprintln!("轮次 {trace}: TTS 失败，弃轮（{e}）");
                return Ok(());
            }
        };
        if reply_pcm.len() % 2 == 1 {
            reply_pcm.pop(); // 奇数长度截尾（Python reply_pcm[:-1] 同形状）
        }
        let samples = reply_pcm.len() as u64 / 2;
        let tts_ms = samples / 16_000 * 1000 + samples % 16_000 * 1000 / 16_000;

        // 语音下行：先把 PCM 按 MCP-1 的形状分帧发布到 rr3-brain/audio/tts_pcm
        // （stream），再走事务面点名 interact 的 tts/text——点名是"该不该播"的
        // **唯一判据**，流只是载荷通路（§20 第 1 条 (a)）。点名的 args 带
        // pcm_bytes，供播放面校验缓冲完整性（半流/空缓冲不静默，MC-1）。
        self.pcm_seq = rr3_publish_tts_pcm(bus, &reply_pcm, self.pcm_seq)?;
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
                if matches!(played_ms, Value::Number(_))
                    && played_ms.as_f64().unwrap_or(0.0) > 0.0
                {
                    bus.counter_bump("rounds_audible", 1);
                }
            }
            Err(e) => eprintln!("轮次 {trace}: interact 播放点名失败（{e}）"),
        }

        // 意图→动作映射在 brain（策略层唯一归属，0.0.3 Q1 裁定）：结构化 actions
        // 随 intent/result 广播，执行器按 exposes 归属门各自认领（brain 不单播不
        // 等待）；只喂 user_text，机器人自己的 reply_text 不参与匹配
        let actions = rr3_extract_actions(&user_text);
        let actions_n = actions.len();
        bus.publish(
            RR3_TOPIC_INTENT,
            json!({
                "trace": trace,
                "user_text": user_text,
                "reply_text": reply_text,
                "tts_ms": tts_ms,
                "actions": actions,
            }),
        )?;
        self.dialog.rr3_record(Rr3DialogTurn {
            user_text: user_text.clone(),
            reply_text: reply_text.clone(),
            trace_id: trace,
        });
        // 完成一轮才记账（计数点 = rr3_record 成功，方案 §4 归属表）。键名拆账
        // （B-16①）：chat_rounds 一个词盖"应答"与"听见"两件事，退役拆两本
        bus.counter_bump("rounds_replied", 1);
        eprintln!(
            "对话轮完成: trace={trace} user='{user_text}' reply='{reply_text}' tts_pcm_ms={tts_ms}{played} actions={actions_n}"
        );
        Ok(())
    }
}

impl Dispatcher for Rr3BrainDispatcher {
    /// rr3.call 点名应答（事务面三臂，module.toml exposes 同源）。薄委托
    /// [`rr3_rpc_arm`]——三臂是纯内存面不触总线（bus 参数留给 trait 形状），
    /// 纯函数即测试真值入口（仓外侧无法凭空构造 BusHandle）。
    fn on_rpc(&mut self, topic: &str, args: &Value, _bus: &BusHandle) -> Result<Value, String> {
        rr3_rpc_arm(
            &mut self.cfg,
            &mut self.clients,
            &mut self.dialog,
            topic,
            args,
        )
    }

    /// 订阅面两臂（pcm 积累＋vad_start 判停触发整轮；module.toml subscribes
    /// 两条是申报面，事件过滤在臂内本地做）。
    fn on_event(&mut self, topic: &str, body: &Value, bus: &BusHandle) -> Result<(), String> {
        self.rr3_event_arm(topic, body, bus)
    }

    /// 节拍面一臂（utterance_expired 时长判停触发整轮）。
    fn on_tick(&mut self, _now: Instant, bus: &BusHandle) -> Result<(), String> {
        self.rr3_tick_arm(bus)
    }
}

/// 事务面三臂（自 Python composite.py 主循环 RPC 分支逐语义迁入）：completion
/// （[`Rr3LlmClient`]）＋ops/list 快照＋ops/set 写臂（写 doc → 按组重建受影响
/// 客户端——dialog 组就地改参）；未知 topic 返 Err 由 serve 折 `{"error": …}`
/// 应答（与 Python 主循环兜底分支同 wire）。
pub(crate) fn rr3_rpc_arm(
    cfg: &mut Rr3BrainCfg,
    clients: &mut Rr3CloudClients,
    dialog: &mut Rr3Dialog,
    topic: &str,
    args: &Value,
) -> Result<Value, String> {
    match topic {
        // completion 臂：业务失败（缺密钥/HTTP 4xx-5xx/应答形状不符）Err 折 error
        // 应答——Python try/except RuntimeError 同 wire（传输异常已在 cloud 层折
        // 同型 Err，同一次云不可用结局只剩一种：弃轮/错误应答，进程不死）
        RR3_COMPLETION_TOPIC => {
            // Python: text = str(params.get("args", {}).get("text", ""))
            let text = args.get("text").map(rr3_py_str).unwrap_or_default();
            Ok(json!({ "text": clients.llm.rr3_complete(&text)? }))
        }
        RR3_OPS_LIST_TOPIC => Ok(json!({ "ops": rr3_ops_snapshot(cfg)? })),
        RR3_OPS_SET_TOPIC => {
            let reply = rr3_ops_set_arm(cfg, args)?;
            // Python: group = str(args.get("group", ""))（同 ops/set 臂的 coercion）；
            // new 是写臂类型域校验产物（dialog 组消费其整数值）
            let group = args.get("group").map(rr3_py_str).unwrap_or_default();
            let new = reply.get("new").cloned().unwrap_or(Value::Null);
            rr3_rebuild_clients(cfg, &group, &new, dialog, clients)?;
            Ok(reply)
        }
        _ => {
            let list: Vec<String> = RR3_RPC_EXPOSES.iter().map(|t| format!("'{t}'")).collect();
            Err(format!(
                "rr3-brain 只 exposes [{}]，收到 {topic}",
                list.join(", ")
            ))
        }
    }
}

/// 把一轮 TTS 的 PCM 分帧为 `rr3-brain/audio/tts_pcm` 的载荷体列表（纯函数，自
/// composite.py `rr3_tts_pcm_frames` 逐语义迁入）。
///
/// 逐字段复用上行 `rr3-sense/audio/pcm` 已定的形状（`rr3_pcm_event_body` 为形状
/// 事实源）：`{seq, format:"s16le", rate, channels, ms, pcm_b64}`，20ms 一块
/// （640 字节）。`seq` 从入参水位起逐帧 +1（**模块级单调，不随轮次复位**）；`ms`
/// 由块内样点数导出（尾块不足 20ms 时按实算）。`topic` 不含（由发布并入）。返回
/// `(frames, 新 seq 水位)`——总线发布与跨语言一致性用例共用同一份组装。
pub(crate) fn rr3_tts_pcm_frames(pcm: &[u8], mut seq: u64) -> (Vec<Value>, u64) {
    let mut frames = Vec::new();
    for chunk in pcm.chunks(RR3_TTS_PCM_FRAME_BYTES) {
        seq += 1;
        let samples = chunk.len() as u64 / 2;
        frames.push(json!({
            "seq": seq,
            "format": "s16le",
            "rate": RR3_TTS_SAMPLE_RATE,
            "channels": 1,
            "ms": samples * 1000 / RR3_TTS_SAMPLE_RATE as u64,
            "pcm_b64": rr3_base64_encode(chunk),
        }));
    }
    (frames, seq)
}

/// 按 [`rr3_tts_pcm_frames`] 分帧发布 tts_pcm 流，返回新的 seq 水位
/// （发布写不出去 Err 上抛——传输性失败致命）。
fn rr3_publish_tts_pcm<B: Rr3TurnBus>(bus: &B, pcm: &[u8], seq: u64) -> Result<u64, String> {
    let (frames, seq) = rr3_tts_pcm_frames(pcm, seq);
    for body in frames {
        bus.publish(RR3_TOPIC_TTS_PCM, body)?;
    }
    Ok(seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::rr3_brain_config_parse;
    use rr_ipc::rr3_base64_decode;

    /// env 动作的测试互斥（全 crate 一柄，与 cloud.rs tests 共享——RR3_BRAIN_CLOUD
    /// 等键是进程级全局，并行测试互相污染）
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::cloud::RR3_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// 与仓内 config.toml 同形状的最小配置夹具
    fn cfg() -> Rr3BrainCfg {
        rr3_brain_config_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n",
        )
        .unwrap()
    }

    /// 带点名超时的配置夹具（B-13① 超时注入判据用）
    fn cfg_call_timeout(s: f64) -> Rr3BrainCfg {
        rr3_brain_config_parse(&format!(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n[rr3-brain.bus]\ncall_timeout_s = {s}\n"
        ))
        .unwrap()
    }

    /// mock 档三客户端夹具（completion 臂激活测试用——mock 不触网不读 key）。
    /// 返回锁柄随测试全程持有：mock 判据是现读 env（rr3_cloud_mode），释锁后
    /// 其他测试改档会让臂半途换档。
    fn mock_clients(cfg: &Rr3BrainCfg) -> (Rr3CloudClients, std::sync::MutexGuard<'static, ()>) {
        let g = env_lock();
        std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "mock");
        (Rr3CloudClients::from_cfg(cfg), g)
    }

    /// mock 档整装 Dispatcher 夹具（编排面测试用：三客户端 mock 档＋config 话轮
    /// 封顶入状态机）。锁柄随测试全程持有（env 纪律同 mock_clients）。
    fn mock_dispatcher(cfg: Rr3BrainCfg) -> (Rr3BrainDispatcher, std::sync::MutexGuard<'static, ()>) {
        let g = env_lock();
        std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "mock");
        (Rr3BrainDispatcher::new(cfg), g)
    }

    /// rr3-sense/audio/pcm 事件体夹具（rr3_pcm_event_body 同形状）
    fn pcm_event_body(payload: &[u8], ms: i64) -> Value {
        json!({
            "seq": 1,
            "format": "s16le",
            "rate": 16000,
            "channels": 1,
            "ms": ms,
            "pcm_b64": rr3_base64_encode(payload),
        })
    }

    /// 记录型整轮编排夹具（Python test_composite._FakeBus 同体例）：计数/发布/
    /// 点名三动作记账，点名应答/报错可编程——拆账判据的观测面。账本 RefCell
    /// 内可变（Rr3TurnBus 取 &self——生产 BusHandle 内部带锁同形状）。
    struct FakeTurnBus {
        bumps: std::cell::RefCell<Vec<(String, u64)>>,
        published: std::cell::RefCell<Vec<(String, Value)>>,
        calls: std::cell::RefCell<Vec<(String, Value, Duration)>>,
        reply: Value,
        call_error: Option<String>,
    }

    impl FakeTurnBus {
        fn reply(reply: Value) -> Self {
            Self {
                bumps: Default::default(),
                published: Default::default(),
                calls: Default::default(),
                reply,
                call_error: None,
            }
        }

        fn failing_call(error: &str) -> Self {
            Self {
                bumps: Default::default(),
                published: Default::default(),
                calls: Default::default(),
                reply: Value::Null,
                call_error: Some(error.to_string()),
            }
        }

        fn bumps_of(&self, key: &str) -> u64 {
            self.bumps
                .borrow()
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, by)| by)
                .sum()
        }

        fn published_of(&self, topic: &str) -> Vec<Value> {
            self.published
                .borrow()
                .iter()
                .filter(|(t, _)| t == topic)
                .map(|(_, b)| b.clone())
                .collect()
        }
    }

    impl Rr3TurnBus for FakeTurnBus {
        fn counter_bump(&self, key: &str, by: u64) {
            self.bumps.borrow_mut().push((key.to_string(), by));
        }

        fn publish(&self, topic: &str, body: Value) -> Result<(), String> {
            self.published.borrow_mut().push((topic.to_string(), body));
            Ok(())
        }

        fn call(&self, topic: &str, args: &Value, timeout: Duration) -> Result<Value, String> {
            self.calls.borrow_mut().push((topic.to_string(), args.clone(), timeout));
            match &self.call_error {
                Some(e) => Err(e.clone()),
                None => Ok(self.reply.clone()),
            }
        }
    }

    /// 未知 topic 兜底文案逐字（Python f"rr3-brain 只 exposes {RPC_EXPOSES}，收到
    /// {rpc_topic}" 同形——列表 repr 是 Python 形状）
    #[test]
    fn unknown_topic_error_names_exposes_list() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let mut d = Rr3Dialog::rr3_new(8000);
        let e = rr3_rpc_arm(&mut c, &mut cl, &mut d, "rr3-brain/other", &Value::Null)
            .unwrap_err();
        assert_eq!(
            e,
            "rr3-brain 只 exposes ['rr3-brain/llm/completion', 'rr3-brain/ops/list', 'rr3-brain/ops/set']，收到 rr3-brain/other"
        );
    }

    /// completion 臂激活（迁移件2）：mock 档回显＋{"text": …} 应答体（Python
    /// result = {"text": llm.complete(text)} 同 wire）
    #[test]
    fn completion_arm_mocks_echo_reply() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let mut d = Rr3Dialog::rr3_new(8000);
        let v = rr3_rpc_arm(
            &mut c,
            &mut cl,
            &mut d,
            RR3_COMPLETION_TOPIC,
            &serde_json::json!({"text": "在吗"}),
        )
        .unwrap();
        assert_eq!(v, serde_json::json!({"text": "（mock 回复）在吗"}));
    }

    /// completion 臂 text 参数 str() coercion（Python str(args.get("text", ""))：
    /// 缺键回落空串、非串折词——mock 回显即观测面）
    #[test]
    fn completion_arm_coerces_text_like_python_str() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let mut d = Rr3Dialog::rr3_new(8000);
        let v = rr3_rpc_arm(&mut c, &mut cl, &mut d, RR3_COMPLETION_TOPIC, &serde_json::json!({}))
            .unwrap();
        assert_eq!(v, serde_json::json!({"text": "（mock 回复）"}), "缺 text 回落空串");
        let v = rr3_rpc_arm(
            &mut c,
            &mut cl,
            &mut d,
            RR3_COMPLETION_TOPIC,
            &serde_json::json!({"text": 5}),
        )
        .unwrap();
        assert_eq!(v, serde_json::json!({"text": "（mock 回复）5"}), "str(5)='5' coercion");
    }

    /// completion 臂 real 档缺密钥：业务失败 Err 折 error 应答（Python try/except
    /// RuntimeError 同 wire——文案点名环境变量，不静默降级）
    #[test]
    fn completion_arm_real_without_key_folds_error() {
        let mut c = cfg();
        let _g = env_lock();
        let mut cl = Rr3CloudClients::from_cfg(&c);
        let mut d = Rr3Dialog::rr3_new(8000);
        std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "real");
        std::env::remove_var(crate::cloud::RR3_API_KEY_ENV);
        let e = rr3_rpc_arm(
            &mut c,
            &mut cl,
            &mut d,
            RR3_COMPLETION_TOPIC,
            &serde_json::json!({"text": "hi"}),
        )
        .unwrap_err();
        assert_eq!(e, "缺少环境变量 RR3_CLOUD_API_KEY，云端链路不可用");
        std::env::remove_var(crate::cloud::RR3_CLOUD_MODE_ENV);
    }

    /// ops/list 臂经 rr3_rpc_arm 全链路可达（快照组体在 rpc.rs 逐键钉，此处钉臂位）
    #[test]
    fn ops_list_arm_reachable_via_rpc_arm() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let mut d = Rr3Dialog::rr3_new(8000);
        let v = rr3_rpc_arm(&mut c, &mut cl, &mut d, RR3_OPS_LIST_TOPIC, &Value::Null).unwrap();
        assert_eq!(v["ops"]["dialog"]["max_utterance_ms"], 8000);
    }

    /// ops/set 臂写后按组重建：llm 组写 model_id 后重建不红（重建取数在
    /// rpc.rs model_row_of_reflects_live_write 钉真值），mock 档 complete 照走
    #[test]
    fn ops_set_arm_rebuilds_llm_client_after_write() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let mut d = Rr3Dialog::rr3_new(8000);
        let v = rr3_rpc_arm(
            &mut c,
            &mut cl,
            &mut d,
            RR3_OPS_SET_TOPIC,
            &serde_json::json!({"group": "llm", "slot": "llm_model", "value": "m2"}),
        )
        .unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["new"], "m2");
        // 重建后 completion 臂照常可用（mock 档回显不依赖模型行，行为连续性观测）
        let v = rr3_rpc_arm(
            &mut c,
            &mut cl,
            &mut d,
            RR3_COMPLETION_TOPIC,
            &serde_json::json!({"text": "hi"}),
        )
        .unwrap();
        assert_eq!(v, serde_json::json!({"text": "（mock 回复）hi"}));
    }

    /// ops/set dialog 组就地改参（迁移件四接线）：写臂应答后 max_utterance_ms
    /// live 生效（doc 真值同拍写入——重启回 config.toml 真值）
    #[test]
    fn ops_set_arm_dialog_group_updates_live_param() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let mut d = Rr3Dialog::rr3_new(8000);
        let v = rr3_rpc_arm(
            &mut c,
            &mut cl,
            &mut d,
            RR3_OPS_SET_TOPIC,
            &serde_json::json!({"group": "dialog", "slot": "max_utterance_ms", "value": 9000}),
        )
        .unwrap();
        assert_eq!(v["new"], 9000);
        assert_eq!(d.max_utterance_ms, 9000, "状态机话轮封顶 live 生效");
        assert_eq!(
            c.doc["rr3-brain"]["dialog"]["max_utterance_ms"],
            serde_json::json!(9000),
            "doc 真值已写"
        );
    }

    /// 事件臂订阅面外事件静默过滤（Python 主循环 if/elif 兜底同形状）
    #[test]
    fn event_arm_ignores_unsubscribed_topics() {
        let (mut dp, _g) = mock_dispatcher(cfg());
        let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
        dp.rr3_event_arm("rr3-other/topic", &json!({"x": 1}), &bus).unwrap();
        assert!(bus.published.borrow().is_empty());
        assert!(bus.bumps.borrow().is_empty());
    }

    /// 事件臂全链路：pcm 预滚/积累 + vad onset/offset 判停 → 触发整轮
    /// （mock 档三客户端走通 ASR→LLM→TTS→分帧→点名→intent 发布）
    #[test]
    fn event_arm_vad_offset_triggers_full_turn() {
        let (mut dp, _g) = mock_dispatcher(cfg());
        let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
        dp.rr3_event_arm(RR3_TOPIC_PCM, &pcm_event_body(b"\x01\x02", 200), &bus)
            .unwrap(); // onset 前判据段（预滚）
        dp.rr3_event_arm(RR3_TOPIC_VAD, &json!({"edge": "onset"}), &bus).unwrap();
        dp.rr3_event_arm(RR3_TOPIC_PCM, &pcm_event_body(b"\x03\x04", 32), &bus)
            .unwrap();
        dp.rr3_event_arm(RR3_TOPIC_VAD, &json!({"edge": "offset"}), &bus).unwrap();
        // intent 发布 = 整轮走通的观测面（trace 从 1 起）
        let intent = &dp_published_intent(&bus);
        assert_eq!(intent["trace"], 1);
        assert!(
            intent["user_text"].as_str().unwrap().contains("你好"),
            "mock ASR 文本入轮"
        );
        // tts_pcm 流已分帧发布（载荷通路），点名恰好一次
        assert!(!bus.published_of(RR3_TOPIC_TTS_PCM).is_empty());
        assert_eq!(bus.calls.borrow().len(), 1);
        assert_eq!(bus.calls.borrow()[0].0, RR3_TOPIC_TTS);
        // 拆账两本都有（played_ms=120 > 0）
        assert_eq!(bus.bumps_of("rounds_replied"), 1);
        assert_eq!(bus.bumps_of("rounds_audible"), 1);
    }

    /// intent 发布体取用（整轮走通的观测面；恰一次发布）
    fn dp_published_intent(bus: &FakeTurnBus) -> Value {
        let intents = bus.published_of(RR3_TOPIC_INTENT);
        assert_eq!(intents.len(), 1, "整轮恰发布一次 intent");
        intents.into_iter().next().unwrap()
    }

    /// 节拍臂时长判停触发整轮：0 封顶 onset 即刻到点（Python
    /// test_duration_cap_expires 同款零等待技巧）
    #[test]
    fn tick_arm_expired_triggers_full_turn() {
        let mut c = cfg();
        c.max_utterance_ms = 0;
        let (mut dp, _g) = mock_dispatcher(c);
        let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
        dp.rr3_event_arm(RR3_TOPIC_PCM, &pcm_event_body(b"\x01\x02", 32), &bus)
            .unwrap();
        dp.rr3_event_arm(RR3_TOPIC_VAD, &json!({"edge": "onset"}), &bus).unwrap();
        dp.rr3_tick_arm(&bus).unwrap();
        assert!(
            !bus.published_of(RR3_TOPIC_INTENT).is_empty(),
            "时长判停应触发整轮"
        );
        // 复位后同一拍不再触发（take 交空直接返回）
        dp.rr3_tick_arm(&bus).unwrap();
        assert_eq!(bus.bumps_of("rounds_replied"), 1, "一拍只走一轮");
    }

    // ---------- TTS 下行流分帧（Python test_composite.TestTtsPcmFrames 对位） ----------

    /// 确定性伪 PCM（s16le，字节值随下标变化，便于逐块断言原样切分）
    fn pcm_bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    /// 空 PCM 零帧且水位不动
    #[test]
    fn frames_empty_yields_no_frames_and_seq_unchanged() {
        let (frames, seq) = rr3_tts_pcm_frames(b"", 7);
        assert!(frames.is_empty());
        assert_eq!(seq, 7);
    }

    /// 整块切分：640×3 → 三帧 seq 1..3、每帧 20ms、b64 解码恰一整块
    #[test]
    fn frames_exact_multiple_is_split_at_20ms_blocks() {
        let (frames, seq) = rr3_tts_pcm_frames(&pcm_bytes(RR3_TTS_PCM_FRAME_BYTES * 3), 0);
        assert_eq!(
            frames.iter().map(|f| f["seq"].as_u64().unwrap()).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(seq, 3);
        for f in &frames {
            assert_eq!(f["format"], "s16le");
            assert_eq!(f["rate"], 16000);
            assert_eq!(f["channels"], 1);
            assert_eq!(f["ms"], 20, "320 样点 @16k");
            assert_eq!(rr3_base64_decode(f["pcm_b64"].as_str().unwrap()).unwrap().len(), RR3_TTS_PCM_FRAME_BYTES);
        }
    }

    /// 尾块 ms 按实算：640 + 320 → 尾块 160 样点 = 10ms；逐块拼回原流无损
    #[test]
    fn frames_tail_block_ms_derived_from_actual_samples() {
        let raw = pcm_bytes(RR3_TTS_PCM_FRAME_BYTES + RR3_TTS_PCM_FRAME_BYTES / 2);
        let (frames, seq) = rr3_tts_pcm_frames(&raw, 0);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["ms"], 20);
        assert_eq!(frames[1]["ms"], 10);
        assert_eq!(seq, 2);
        let mut joined = Vec::new();
        for f in &frames {
            joined.extend(rr3_base64_decode(f["pcm_b64"].as_str().unwrap()).unwrap());
        }
        assert_eq!(joined, raw, "切分无损");
    }

    /// seq 模块级单调：跨轮续接不复位
    #[test]
    fn frames_seq_monotonic_across_calls() {
        let (_, seq) = rr3_tts_pcm_frames(&pcm_bytes(RR3_TTS_PCM_FRAME_BYTES), 5);
        let (frames, seq) = rr3_tts_pcm_frames(&pcm_bytes(RR3_TTS_PCM_FRAME_BYTES * 2), seq);
        assert_eq!(
            frames.iter().map(|f| f["seq"].as_u64().unwrap()).collect::<Vec<_>>(),
            vec![7, 8]
        );
        assert_eq!(seq, 8);
    }

    /// 跨语言一致性（MC-1③）：`contracts/golden/tts-pcm-frames.json` 是 Python 腿
    /// （本模块退役前的分帧真值）与 rr3-interact 收流腿共读的同一份基准——分帧
    /// 输出与共享基准逐字段一致，帧列 b64 拼接还原基准 PCM
    #[test]
    fn frames_match_shared_golden_vector() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/golden/tts-pcm-frames.json");
        let golden: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let raw = hex_to_bytes(golden["pcm_hex"].as_str().unwrap());
        let (frames, seq) = rr3_tts_pcm_frames(&raw, 0);
        assert_eq!(
            Value::Array(frames.clone()),
            golden["frames"],
            "分帧输出与共享基准逐字段一致"
        );
        assert_eq!(seq, golden["frames"].as_array().unwrap().len() as u64);
        let mut joined = Vec::new();
        for f in &frames {
            joined.extend(rr3_base64_decode(f["pcm_b64"].as_str().unwrap()).unwrap());
        }
        assert_eq!(joined, raw, "拼回无损");
    }

    /// hex 文本 → 字节列（golden 基准的 pcm_hex 载体形状）
    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    // ---------- 整轮计账拆分（Python test_composite.TestFinishTurnLedger 对位） ----------

    /// played_ms>0：两本账各记一次、点名超时经 config 注入（B-13①）、意图照发
    #[test]
    fn played_rounds_counted_in_both_ledgers_and_timeout_injected() {
        let (mut dp, _g) = mock_dispatcher(cfg_call_timeout(12.5));
        let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
        dp.dialog.rr3_feed_vad(&json!({"edge": "onset"}), 0);
        dp.dialog
            .rr3_feed_pcm(&pcm_event_body(&pcm_bytes(320), 32))
            .unwrap();
        dp.rr3_finish_turn(&bus).unwrap();
        let intent = dp_published_intent(&bus);
        assert_eq!(intent["trace"], 1);
        assert_eq!(bus.bumps_of("rounds_replied"), 1, "应答一本账");
        assert_eq!(bus.bumps_of("rounds_audible"), 1, "听见一本账");
        assert_eq!(bus.bumps_of("chat_rounds"), 0, "旧键退役不再记");
        assert_eq!(
            bus.calls.borrow()[0].2,
            Duration::from_millis(12500),
            "点名超时经 config 注入（B-13①）"
        );
    }

    /// 播放点名失败：只进应答账，听见账不动——"完成一轮"与"用户听见"自此可分
    /// （不许一个词盖两件事）；意图照发（动作不丢）
    #[test]
    fn call_failure_counts_replied_only_and_intent_still_published() {
        let (mut dp, _g) = mock_dispatcher(cfg());
        let bus = FakeTurnBus::failing_call("tts 播出 0ms（…），不得记 ok（I-21①）");
        dp.dialog.rr3_feed_vad(&json!({"edge": "onset"}), 0);
        dp.dialog
            .rr3_feed_pcm(&pcm_event_body(&pcm_bytes(320), 32))
            .unwrap();
        dp.rr3_finish_turn(&bus).unwrap();
        assert_eq!(bus.bumps_of("rounds_replied"), 1);
        assert_eq!(bus.bumps_of("rounds_audible"), 0);
        assert!(
            !bus.published_of(RR3_TOPIC_INTENT).is_empty(),
            "点名失败意图照发（动作不丢）"
        );
    }

    /// played_ms=0 不算听见（interact 修后 0ms 折 error；此处防御异常形状仍不进账）
    #[test]
    fn zero_played_ms_does_not_count_audible() {
        let (mut dp, _g) = mock_dispatcher(cfg());
        let bus = FakeTurnBus::reply(json!({"played_ms": 0}));
        dp.dialog.rr3_feed_vad(&json!({"edge": "onset"}), 0);
        dp.dialog
            .rr3_feed_pcm(&pcm_event_body(&pcm_bytes(320), 32))
            .unwrap();
        dp.rr3_finish_turn(&bus).unwrap();
        assert_eq!(bus.bumps_of("rounds_replied"), 1);
        assert_eq!(bus.bumps_of("rounds_audible"), 0, "0ms 不算听见");
    }

    /// 云端 ASR 失败弃轮：两本账都不动（既有 asr_fail 计数语义不变）——real 档
    /// 缺密钥即 ASR 确定性失败（不触网）
    #[test]
    fn asr_failure_counts_neither_ledger() {
        let (mut dp, g) = mock_dispatcher(cfg());
        // 整装后换 real 档并撤密钥：mock 判据现读 env，半途换档即生效
        std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "real");
        std::env::remove_var(crate::cloud::RR3_API_KEY_ENV);
        let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
        dp.dialog.rr3_feed_vad(&json!({"edge": "onset"}), 0);
        dp.dialog
            .rr3_feed_pcm(&pcm_event_body(&pcm_bytes(320), 32))
            .unwrap();
        dp.rr3_finish_turn(&bus).unwrap();
        std::env::remove_var(crate::cloud::RR3_CLOUD_MODE_ENV);
        drop(g);
        assert_eq!(bus.bumps_of("rounds_replied"), 0);
        assert_eq!(bus.bumps_of("rounds_audible"), 0);
        assert_eq!(bus.bumps_of("asr_fail"), 1, "弃轮仍有失败计数");
    }
}
