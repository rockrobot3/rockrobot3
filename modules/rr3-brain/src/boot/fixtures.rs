//! 与仓内 config.toml 同形状的测试夹具（boot/rpc/config_store 三侧共用一份真值）。

/// 与仓内 config.toml 同形状的夹具段真值（boot/rpc/config_store 三侧测试共用，
/// 一份文本一份判据）：cloud 四元组段 + [[model]] 行表段 + bus/dialog 尾段。
/// 有无 `[rr3-brain]` 头、注释行由各夹具自定（重写保留判据的注释底稿在 config_store）。
#[cfg(test)]
pub(crate) const RR3_CLOUD_TOML: &str = r#"[rr3-brain.cloud]
base_url = "https://api.siliconflow.cn"
asr_model = "iic/SenseVoiceSmall"
llm_model = "deepseek-ai/DeepSeek-V3"
tts_model = "FunAudioLLM/CosyVoice2-0.5B"
timeout_s = 20
"#;

#[cfg(test)]
pub(crate) const RR3_MODEL_ROWS_TOML: &str = r#"[[rr3-brain.model]]
name = "DeepSeek官方"
mode = "api"
api_format = "openai"
base_url = "https://api.deepseek.com/v1"
model_id = "deepseek-chat"
api_key_ref = "DeepSeek官方"
active = true
"#;

#[cfg(test)]
pub(crate) const RR3_CFG_TAIL_TOML: &str = r#"[rr3-brain.bus]
call_timeout_s = 30.0

[rr3-brain.dialog]
max_utterance_ms = 8000
"#;

/// 与仓内 config.toml 同形状的夹具文本（boot/rpc 两侧测试共用真值）：
/// `[rr3-brain]` 头 + 三段常量拼装（concat! 只吃字面量，常量拼装须走函数）
#[cfg(test)]
pub(crate) fn rr3_repo_cfg_fixture() -> String {
    format!(
        "[rr3-brain]\nlog_level = \"info\"\n\n{}\n{}\n{}",
        RR3_CLOUD_TOML, RR3_MODEL_ROWS_TOML, RR3_CFG_TAIL_TOML
    )
}

/// 无 [[model]] 行表的 fallback 变体（cloud + dialog 段，bus 缺省回落由用例自证）
#[cfg(test)]
pub(crate) fn rr3_fallback_cfg() -> String {
    let full = rr3_repo_cfg_fixture();
    let head = full.split("[[rr3-brain.model]]").next().unwrap();
    let tail = full.split("[rr3-brain.dialog]").nth(1).unwrap();
    format!("{head}[rr3-brain.dialog]{tail}")
}
