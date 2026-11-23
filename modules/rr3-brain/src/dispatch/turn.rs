// SPDX-License-Identifier: Apache-2.0
//! 整轮编排的总线窄化、应答体与云端客户端重建。

use super::*;

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

/// 唤醒判决 + LLM 段的产物（`rr3_turn_reply` 出、主编排消费）：`ack` 标记本轮是
/// 固定应答（TTS 段可复用预合成 PCM），`llm_ms` 为 `-` 表示本轮未进 LLM。
pub(super) struct Rr3TurnReply {
    pub(super) text: String,
    pub(super) llm_ms: String,
    pub(super) ack: bool,
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
pub(super) fn rr3_rebuild_clients(
    cfg: &Rr3BrainCfg,
    group: &str,
    new: &Value,
    dialog: &mut Rr3Dialog,
    clients: &mut Rr3CloudClients,
) -> Result<(), String> {
    match group {
        "asr" => rr3_rebuild_asr(cfg, clients),
        "tts" => rr3_rebuild_tts(cfg, clients),
        "llm" => rr3_rebuild_llm(cfg, clients),
        // dialog（rr3_ops_write 白名单保证四组，其余即 dialog）：Python
        // dialog.max_utterance_ms = int(new) 同位——new 是写臂类型域校验的整数
        _ => {
            dialog.max_utterance_ms = new
                .as_i64()
                .ok_or_else(|| "ops/set dialog 组 new 须为整数".to_string())?;
            Ok(())
        }
    }
}

/// asr 组：读 cloud 段现值重建 ASR 客户端。
fn rr3_rebuild_asr(cfg: &Rr3BrainCfg, clients: &mut Rr3CloudClients) -> Result<(), String> {
    let cloud = rr3_cloud_cfg_of(&cfg.doc)?;
    clients.asr = Rr3AsrClient::new(&cloud.base_url, &cloud.asr_model, cloud.timeout_s);
    Ok(())
}

/// tts 组：读 cloud 段现值重建 TTS 客户端。
fn rr3_rebuild_tts(cfg: &Rr3BrainCfg, clients: &mut Rr3CloudClients) -> Result<(), String> {
    let cloud = rr3_cloud_cfg_of(&cfg.doc)?;
    clients.tts = Rr3TtsClient::new(&cloud.base_url, &cloud.tts_model, cloud.timeout_s);
    Ok(())
}

/// llm 组：重解析模型行实效 + cloud 段超时，重建 LLM 客户端。
fn rr3_rebuild_llm(cfg: &Rr3BrainCfg, clients: &mut Rr3CloudClients) -> Result<(), String> {
    let row = rr3_model_row_of(cfg)?;
    let timeout_s = rr3_cloud_cfg_of(&cfg.doc)?.timeout_s;
    clients.llm = Rr3LlmClient::new(
        &row.base_url,
        &row.model_id,
        timeout_s,
        row.key_ref,
        row.mode,
    );
    Ok(())
}

impl Rr3BrainDispatcher {
    /// 订阅面两臂（Python 主循环事件分支逐语义）：pcm → 提取事件体 pcm_b64/ms
    /// 字段喂状态机积累/预滚（机制吃适配后的字段值——S-7 同款，坏 base64 上抛）；
    /// vad_start → 提取 edge 字段判停真值触发整轮。泛型写口使事件臂与节拍臂纯
    /// 夹具可测（[`Rr3TurnBus`]）。
    pub(super) fn rr3_event_arm<B: Rr3TurnBus>(
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
    pub(super) fn rr3_tick_arm<B: Rr3TurnBus>(&mut self, bus: &B) -> Result<(), String> {
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
    pub(super) fn rr3_finish_turn<B: Rr3TurnBus>(&mut self, bus: &B) -> Result<(), String> {
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
    /// 唤醒门控段（自 [`rr3_turn_reply`] 抽出）：命中 → 固定应答/命令进 LLM；
    /// 窗外未命中 → 静默丢弃；窗内未命中 → 追问续轮。`enable=false` 恒 FollowUp。
    /// 返回 (LLM 输入, 固定应答文本, 是否唤醒应答)；None = 本轮回声都不出。
    fn rr3_wake_gate(
        &mut self,
        trace: u64,
        user_text: &str,
        asr_cost_ms: u128,
    ) -> Option<(Option<String>, String, bool)> {
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
        Some((llm_input, reply_text, is_wake_ack))
    }

    fn rr3_turn_reply<B: Rr3TurnBus>(
        &mut self,
        bus: &B,
        trace: u64,
        user_text: &str,
        asr_cost_ms: u128,
    ) -> Option<Rr3TurnReply> {
        let (llm_input, mut reply_text, is_wake_ack) =
            self.rr3_wake_gate(trace, user_text, asr_cost_ms)?;
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
