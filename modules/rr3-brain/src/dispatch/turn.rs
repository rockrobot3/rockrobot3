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
    text: String,
    llm_ms: String,
    ack: bool,
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
