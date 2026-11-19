//! 云对话协议机制核：ASR/LLM/TTS 请求体装配、应答解析与重试判据的纯函数。
//!
//! 自 `rr3-brain/src/cloud` 抽出（W2-1）：Pi 与 MCU 共用同一份请求字节形状，
//! 传输层（`ureq` / `esp_http_client`）只做收发。零 IO、零环境读取、零平台依赖；
//! 只依赖 `serde_json`（alloc 档）与 `rr-audio-mech` 的 WAV 头。
//!
//! 口径来源：[单板全链云 API 契约](../../../../docs/process/dev/单板全链云API契约-2026-11-19.md)。

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde_json::{json, Value};

/// ASR 应答解析：坏 JSON / 缺 `text` 折 Err。
pub fn rr3_asr_text(resp: &[u8]) -> Result<String, String> {
    let data: Value = serde_json::from_slice(resp).map_err(|e| format!("识别应答非 JSON: {e}"))?;
    data.get("text")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| "识别应答缺 text 字段".to_string())
}

/// ASR multipart 请求体：model + language（可选）+ file（WAV 头仓内自建）。
pub fn rr3_multipart_body(
    model: &str,
    pcm: &[u8],
    sample_rate: u32,
    boundary: &str,
    language: Option<&str>,
) -> Vec<u8> {
    let header = rr_audio_mech::rr3_wav_header(sample_rate, pcm.len() as u32);
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n"
        )
        .as_bytes(),
    );
    if let Some(lang) = language {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\n{lang}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&header);
    body.extend_from_slice(pcm);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// LLM 请求体：OpenAI 兼容 messages + 模型行 + 回复长度上限。
pub fn rr3_chat_body(model: &str, messages: &[(String, String)], max_tokens: i64) -> Vec<u8> {
    let messages: Vec<Value> = messages
        .iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens,
    })
    .to_string()
    .into_bytes()
}

/// 连通性测试的最小补全体（`ping` 一句、`max_tokens=1`）。
pub fn rr3_ping_body(model: &str) -> Vec<u8> {
    json!({
        "model": model,
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 1,
    })
    .to_string()
    .into_bytes()
}

/// LLM 应答收敛：取不出 `choices[0].message.content` 串即 None。
pub fn rr3_reply_text(resp: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(resp)
        .ok()
        .and_then(|v| rr3_choice_text(&v))
}

/// 应答信封取值：`choices[0].message.content`（形状不符即 None）。
pub fn rr3_choice_text(v: &Value) -> Option<String> {
    v.get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}

/// TTS 请求体：模型/文本/音色/格式/采样率。
pub fn rr3_tts_body(model: &str, input: &str, voice: &str, sample_rate: u32) -> Vec<u8> {
    json!({
        "model": model,
        "input": input,
        "voice": voice,
        "response_format": "pcm",
        "sample_rate": sample_rate,
    })
    .to_string()
    .into_bytes()
}

/// 可重试判据（纯函数）：传输层「云端不可达 …」与 HTTP 429/5xx 重试；
/// 其余（4xx/坏应答/缺密钥/缺字段）不重试。
pub fn rr3_cloud_retryable(interface: &str, err: &str) -> bool {
    if err.starts_with("云端不可达") {
        return true;
    }
    rr3_http_code_of(interface, err).is_some_and(rr3_retry_code)
}

/// 从 `"{interface} HTTP {code}"` 解析状态码（形状不符即 None）。
fn rr3_http_code_of(interface: &str, err: &str) -> Option<u16> {
    let prefix = format!("{interface} HTTP ");
    err.strip_prefix(&prefix)
        .and_then(|s| s.parse::<u16>().ok())
}

/// 可重试状态码：限流（429）与服务端（5xx）。
fn rr3_retry_code(code: u16) -> bool {
    code == 429 || (500..=599).contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn asr_text_parses_and_rejects_bad_shapes() {
        assert_eq!(
            rr3_asr_text(r#"{"text":"你好"}"#.as_bytes()),
            Ok("你好".to_string())
        );
        assert!(rr3_asr_text(br#"{"nope":1}"#)
            .unwrap_err()
            .contains("缺 text"));
        assert!(rr3_asr_text(b"not json").unwrap_err().contains("非 JSON"));
    }

    #[test]
    fn multipart_carries_model_language_file_and_wav() {
        let body = rr3_multipart_body("m", b"\x01\x02", 16000, "bd1", Some("zh"));
        assert!(body.windows(12).any(|w| w == b"name=\"model\""));
        assert!(body.windows(15).any(|w| w == b"name=\"language\""));
        assert!(body.windows(16).any(|w| w == b"filename=\"a.wav\""));
        assert!(body.windows(4).any(|w| w == b"RIFF"));
        assert!(body.ends_with(b"\r\n--bd1--\r\n"));
        let no_lang = rr3_multipart_body("m", b"", 16000, "bd1", None);
        assert!(!no_lang.windows(15).any(|w| w == b"name=\"language\""));
    }

    #[test]
    fn chat_and_ping_bodies_have_openai_shape() {
        let messages = vec![
            ("system".to_string(), "sys".to_string()),
            ("user".to_string(), "hi".to_string()),
        ];
        let body = rr3_chat_body("m", &messages, 150);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["model"], "m");
        assert_eq!(v["max_tokens"], 150);
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][1]["content"], "hi");
        let ping: Value = serde_json::from_slice(&rr3_ping_body("m")).unwrap();
        assert_eq!(ping["max_tokens"], 1);
        assert_eq!(ping["messages"][0]["content"], "ping");
    }

    #[test]
    fn reply_text_reads_choice_content() {
        let body = r#"{"choices":[{"message":{"content":"好"}}]}"#.as_bytes();
        assert_eq!(rr3_reply_text(body), Some("好".to_string()));
        assert_eq!(rr3_reply_text(br#"{"choices":[]}"#), None);
    }

    #[test]
    fn tts_body_requests_pcm_and_sample_rate() {
        let v: Value = serde_json::from_slice(&rr3_tts_body("m", "hi", "m:bella", 16000)).unwrap();
        assert_eq!(v["response_format"], "pcm");
        assert_eq!(v["sample_rate"], 16000);
        assert_eq!(v["voice"], "m:bella");
    }

    #[test]
    fn retry_predicate_covers_transient_and_rejects_client_errors() {
        assert!(rr3_cloud_retryable("识别接口", "云端不可达 x: timeout"));
        assert!(rr3_cloud_retryable("识别接口", "识别接口 HTTP 429"));
        assert!(rr3_cloud_retryable("识别接口", "识别接口 HTTP 503"));
        assert!(!rr3_cloud_retryable("识别接口", "识别接口 HTTP 401"));
        assert!(!rr3_cloud_retryable("识别接口", "识别接口 HTTP 400"));
        assert!(!rr3_cloud_retryable("识别接口", "缺 key"));
    }
}
