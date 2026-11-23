// SPDX-License-Identifier: Apache-2.0
//! cloud 三客户端整体与按组重建（自 `turn.rs` 抽出）：ops/set 写后重建的目标，
//! asr/tts 组读 cloud 段现值、llm 组重解析模型行实效。

use serde_json::Value;

use crate::boot::{rr3_cloud_cfg_of, Rr3BrainCfg};
use crate::cloud::{Rr3AsrClient, Rr3LlmClient, Rr3TtsClient};
use crate::dialog::Rr3Dialog;
use crate::rpc::rr3_model_row_of;

/// cloud 三客户端整体（批N32 迁移件2：Python composite 装配段同形状——asr/tts
/// 读 cloud 段、llm 读模型行实效；ops/set 写后按组重建的重建目标）。
pub(crate) struct Rr3CloudClients {
    pub(crate) asr: Rr3AsrClient,
    pub(crate) llm: Rr3LlmClient,
    pub(crate) tts: Rr3TtsClient,
}

impl Rr3CloudClients {
    /// 装配构造（main.rs 装配段与测试共用）：cloud 段四键＋模型行实效入表。
    /// boot 校验保证在册；取数失败回 Err（装配段点名后退出，不 panic）。
    pub(crate) fn from_cfg(cfg: &Rr3BrainCfg) -> Result<Self, String> {
        let cloud = rr3_cloud_cfg_of(&cfg.doc)?;
        Ok(Self {
            asr: Rr3AsrClient::new(&cloud.base_url, &cloud.asr_model, cloud.timeout_s),
            llm: Rr3LlmClient::new(
                &cfg.model.base_url,
                &cfg.model.model_id,
                cloud.timeout_s,
                cfg.model.key_ref.clone(),
                cfg.model.mode,
            ),
            tts: Rr3TtsClient::new(&cloud.base_url, &cloud.tts_model, cloud.timeout_s),
        })
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
