//! 应答体解析与请求体装配（ASR 文本、LLM 补全体/应答收敛、mock 文本）。

use serde_json::{json, Value};

use super::cfg::{RR3_LLM_MAX_TOKENS, RR3_MOCK_TEXT, RR3_MOCK_TEXT_ENV};

/// mock 档识别文本（`RR3_BRAIN_MOCK_ASR_TEXT` 注入覆盖，空白回落固定文本）。
pub(super) fn rr3_mock_text() -> String {
    std::env::var(RR3_MOCK_TEXT_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| RR3_MOCK_TEXT.to_string())
}

/// 识别应答形状：坏 JSON / 缺 text 折 Err（B-16②：不逃逸护栏）。
pub(super) fn rr3_asr_text(resp: &[u8]) -> Result<String, String> {
    let data: Value = serde_json::from_slice(resp).map_err(|e| format!("识别应答非 JSON: {e}"))?;
    data.get("text")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| "识别应答缺 text 字段".to_string())
}

/// 连通性测试的最小补全体（`ping` 一句、`max_tokens=1`）。
pub(super) fn rr3_ping_body(model: &str) -> Vec<u8> {
    json!({
        "model": model,
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 1,
    })
    .to_string()
    .into_bytes()
}

/// 补全请求体：历史 → OpenAI 兼容 messages + 模型行 + 回复长度上限。
pub(super) fn rr3_chat_body(history: &crate::llm::Rr3LlmHistory, model: &str) -> Vec<u8> {
    let messages: Vec<Value> = history
        .rr3_messages()
        .iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    json!({
        "model": model,
        "messages": messages,
        "max_tokens": RR3_LLM_MAX_TOKENS,
    })
    .to_string()
    .into_bytes()
}

/// 应答形状收敛为一档：取不出 choices[0].message.content 串即 None（形状不符）。
pub(super) fn rr3_reply_text(resp: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(resp)
        .ok()
        .and_then(|v| rr3_choice_text(&v))
}

/// 应答信封取值：`choices[0].message.content`（形状不符即 None）。
pub(super) fn rr3_choice_text(v: &Value) -> Option<String> {
    v.get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}
