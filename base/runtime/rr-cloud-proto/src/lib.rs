// SPDX-License-Identifier: Apache-2.0
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

/// ASR multipart 分段计划（W7-b3 流式上行）：头/ WAV 头/尾与总长分离，
/// PCM 由调用方按 4 KB 分块写 `esp_http_client_write`，不再聚合成整段请求体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AsrPlan {
    /// model/language/file 三段 multipart 头（不含 WAV 头）。
    pub head: Vec<u8>,
    /// 44B WAV 头（`data` 长度 = PCM 字节数）。
    pub wav: [u8; 44],
    /// 结尾 boundary。
    pub tail: Vec<u8>,
    /// 请求体总长（head + wav + pcm_len + tail）。
    pub content_len: usize,
}

/// 生成 ASR 分段计划；与 [`rr3_multipart_body`] 逐字节同源（测试钉住）。
pub fn rr3_asr_plan(
    model: &str,
    pcm_len: usize,
    sample_rate: u32,
    boundary: &str,
    language: Option<&str>,
) -> Rr3AsrPlan {
    let mut head = Vec::new();
    head.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n"
        )
        .as_bytes(),
    );
    if let Some(lang) = language {
        head.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\n{lang}\r\n"
            )
            .as_bytes(),
        );
    }
    head.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    let wav = rr_audio_mech::rr3_wav_header(sample_rate, pcm_len as u32);
    let tail = format!("\r\n--{boundary}--\r\n").into_bytes();
    let content_len = head.len() + wav.len() + pcm_len + tail.len();
    Rr3AsrPlan {
        head,
        wav,
        tail,
        content_len,
    }
}

/// ASR multipart 请求体：model + language（可选）+ file（WAV 头仓内自建）。
pub fn rr3_multipart_body(
    model: &str,
    pcm: &[u8],
    sample_rate: u32,
    boundary: &str,
    language: Option<&str>,
) -> Vec<u8> {
    let plan = rr3_asr_plan(model, pcm.len(), sample_rate, boundary, language);
    let mut body = Vec::with_capacity(plan.content_len);
    body.extend_from_slice(&plan.head);
    body.extend_from_slice(&plan.wav);
    body.extend_from_slice(pcm);
    body.extend_from_slice(&plan.tail);
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

/// 从 `"{interface} HTTP {code}"` 解析状态码（形状不符即 None；`-1` 等负值是
/// 传输层无响应，保留符号——按传输失败判可重试）。
fn rr3_http_code_of(interface: &str, err: &str) -> Option<i32> {
    let prefix = format!("{interface} HTTP ");
    err.strip_prefix(&prefix)
        .and_then(|s| s.parse::<i32>().ok())
}

/// 可重试状态码：传输层无响应（`code <= 0`）、限流（429）与服务端（5xx）。
/// 旧判据只认 429/5xx，`HTTP -1`（连接/读超时）被当成不可重试——弱网下
/// 明明可重试却直接弃轮（0015 实测 30 次）。
fn rr3_retry_code(code: i32) -> bool {
    code <= 0 || code == 429 || (500..=599).contains(&code)
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
    fn asr_plan_concatenates_to_legacy_body() {
        let pcm = b"\x01\x02\x03\x04";
        for lang in [Some("zh"), None] {
            let plan = rr3_asr_plan("m", pcm.len(), 16000, "bd1", lang);
            let mut joined = Vec::new();
            joined.extend_from_slice(&plan.head);
            joined.extend_from_slice(&plan.wav);
            joined.extend_from_slice(pcm);
            joined.extend_from_slice(&plan.tail);
            assert_eq!(joined, rr3_multipart_body("m", pcm, 16000, "bd1", lang));
            assert_eq!(plan.content_len, joined.len());
        }
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
        // 传输层无响应（esp_http_client 状态 -1）= 可重试（旧判据漏掉，弱网弃轮 30 次）
        assert!(rr3_cloud_retryable("识别接口", "识别接口 HTTP -1"));
        assert!(!rr3_cloud_retryable("识别接口", "识别接口 HTTP 401"));
        assert!(!rr3_cloud_retryable("识别接口", "识别接口 HTTP 400"));
        assert!(!rr3_cloud_retryable("识别接口", "缺 key"));
    }

    /// W2-2：请求体逐字节冻结——Pi 与 MCU 必须发出同一串字节（含 JSON 键序）。
    #[test]
    fn request_bytes_match_frozen_golden() {
        let messages = vec![
            ("system".to_string(), "sys".to_string()),
            ("user".to_string(), "hi".to_string()),
        ];
        assert_eq!(
            rr3_chat_body("m", &messages, 150),
            br#"{"max_tokens":150,"messages":[{"content":"sys","role":"system"},{"content":"hi","role":"user"}],"model":"m"}"#
                .to_vec()
        );
        assert_eq!(
            rr3_ping_body("m"),
            br#"{"max_tokens":1,"messages":[{"content":"ping","role":"user"}],"model":"m"}"#
                .to_vec()
        );
        assert_eq!(
            rr3_tts_body("m", "hi", "m:bella", 16000),
            br#"{"input":"hi","model":"m","response_format":"pcm","sample_rate":16000,"voice":"m:bella"}"#
                .to_vec()
        );

        let mut want = Vec::new();
        want.extend_from_slice(
            b"--b\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nm\r\n",
        );
        want.extend_from_slice(
            b"--b\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nzh\r\n",
        );
        want.extend_from_slice(b"--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n");
        want.extend_from_slice(&rr_audio_mech::rr3_wav_header(16000, 2));
        want.extend_from_slice(b"\x01\x02");
        want.extend_from_slice(b"\r\n--b--\r\n");
        assert_eq!(
            rr3_multipart_body("m", b"\x01\x02", 16000, "b", Some("zh")),
            want
        );
    }
}
