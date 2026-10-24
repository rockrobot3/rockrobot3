//! rr3-brain 云端共享原语（组合件级，批N32 件四拆分定形）：云档/密钥双源/
//! secrets/HTTP/boundary 随机源 + asr/llm/tts 三客户端——IO/JSON/随机性是策略
//! 面，留在组合件（⑧-b 依赖闭包纪律）；字节级纯机制（WAV 头/multipart 装配、
//! 历史窗口、voice 命名/正弦 PCM）下沉 atoms/{asr,llm,tts} 各件 mech.rs，客户端
//! 经 `crate::{asr,llm,tts}` 按名委托。dialog 件不触网不经此层。
//!
//! 线上契约逐项对齐 legacy/core/{asr,llm,tts}（SiliconFlow /v1 形状）：
//! - ASR  POST {base}/v1/audio/transcriptions   multipart(model + WAV) → {"text": …}
//! - LLM  POST {api_root}/chat/completions      {model, messages, max_tokens} → choices[0].message.content
//! - TTS  POST {base}/v1/audio/speech           {model, input, voice, response_format, sample_rate} → 原始 PCM
//!
//! 密钥**不进代码/仓库**，双源解析（0.0.6 模型连接抽象批）：ref 以 `env:` 开头 →
//! 环境变量；裸名 → 面板维护的 secrets 文件（`RR3_SECRETS_FILE` 可改路径，缺省
//! `~/.rr3/secrets.json`，0600，仓外）每次调用现读——面板填完下一轮即生效。行
//! `mode="local"` 免鉴权：请求**不带 Authorization 头**。任何日志、错误文案都不得
//! 带出密钥值（错误只点名 ref/环境变量名）。
//!
//! mock 后端：环境变量 `RR3_BRAIN_CLOUD=mock`（默认 `real`）时三家换确定性行为
//! （ASR 固定文本/LLM 回显/TTS 正弦 PCM）——测试注入点，不是产品路径（ADR-0016
//! 裁定 2.1 逐语义保留）；缺 key 的 real 调用照报缺环境变量，不静默降级。
//!
//! HTTP/TLS 选型（批N32 裁定 5）：[`ureq`] 同步阻塞客户端＋rustls（零异步栈，
//! 本仓全同步面；TLS 必须库承载）。与 Python 侧的三笔语义差如实登记：
//! ①超时语义——urllib 是 per-op 上界（connect/read 各算），ureq `.timeout()` 是
//! 整请求上界（更严，无害收敛）；②TLS 根证书——urllib 用系统 CA，rustls 用
//! webpki-roots 内置（公共 CA 全覆盖，板上无系统 CA 库亦可复现）；③应答字段
//! 收敛——Python `json.loads().get(...)` 对非 dict 体抛 AttributeError 逃逸护栏
//! （asr.py try 只包 JSONDecodeError），LLM `content` 非 str 也照 append——Rust
//! 侧一律折 Err 点名（fail-closed 优于静默传播，同 B-16② 收敛方向）。
//!
//! 错误语义一个类型（String，B-16② 承续）：业务失败（4xx/5xx/应答形状不符/缺密钥）
//! 与传输失败（连接拒绝/超时/读断）都折 Err 文案——结局只剩两种且都有计数，由
//! 调用方（completion 臂折 error 应答、编排面折日志弃轮）分派。

use std::io::Read;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use crate::boot::Rr3LlmMode;

// ---------- 常量（自 cloud/__init__.py + asr.py + llm.py + tts.py 迁入，逐字） ----------

/// 通用密钥环境变量（`env:` 前缀外的旧路径缺省键）
pub(crate) const RR3_API_KEY_ENV: &str = "RR3_CLOUD_API_KEY";
/// 云档判据环境变量（real / mock）
pub(crate) const RR3_CLOUD_MODE_ENV: &str = "RR3_BRAIN_CLOUD";
/// secrets 文件路径覆盖环境变量
pub(crate) const RR3_SECRETS_FILE_ENV: &str = "RR3_SECRETS_FILE";
/// mock ASR 的文本注入键（批F2）：判向-转向闭环 e2e 用它注入"向左/向右"类识别
/// 文本驱动意图面。取值不设/置空回落固定文本——mock 是测试注入点，真云路径永不
/// 读它（与 [`RR3_CLOUD_MODE_ENV`] 的注入点分工同一纪律）。
pub(crate) const RR3_MOCK_TEXT_ENV: &str = "RR3_BRAIN_MOCK_ASR_TEXT";
/// mock ASR 的固定识别文本
pub(crate) const RR3_MOCK_TEXT: &str = "你好，今天天气怎么样";
/// TTS 宿主兜底后端开关（`RR3_TTS_BACKEND=mac_say`）：硅基流动 /v1/audio/speech
/// 2026-10-24 起对合法请求零字节挂死（ASR/LLM 正常），宿主开发档改走 macOS `say`
/// 合成 WAV 再抽 PCM——只换 TTS 一跳，总线契约与 ASR/LLM 零改动。
pub(crate) const RR3_TTS_BACKEND_ENV: &str = "RR3_TTS_BACKEND";
const RR3_TTS_BACKEND_MAC_SAY: &str = "mac_say";
/// 宿主合成音色（macOS 内置 zh_CN 女声）
const RR3_SAY_VOICE: &str = "Ting-Ting";

/// legacy/core/llm/src/llm_config.h 的原值移植（M4 不改人设与参数）
pub(crate) const RR3_SYSTEM_PROMPT: &str =
    "你是一个中文语音助手，回答要口语化、简短，一般不超过50个字，不要用列表、符号和表情。";
/// legacy llm_config.h 原值
pub(crate) const RR3_LLM_MAX_TOKENS: i64 = 150;
/// key_ref 双源语法前缀：`env:变量名` → 环境变量
pub(crate) const RR3_KEY_REF_ENV_PREFIX: &str = "env:";

// ---------- 模式与密钥（自 cloud/__init__.py 迁入） ----------

/// 云档（`RR3_BRAIN_CLOUD` 判据，每次调用现读——Python `rr3_brain_cloud_mode` 同形状）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rr3CloudMode {
    Real,
    Mock,
}

/// 云档解析：键缺席 → real（Python get(env, "real") 同形状）；键在但不认（含空串）
/// → Err 点名（fail-closed，不静默夹值）。
pub(crate) fn rr3_cloud_mode() -> Result<Rr3CloudMode, String> {
    match std::env::var(RR3_CLOUD_MODE_ENV) {
        Err(_) => Ok(Rr3CloudMode::Real),
        Ok(v) => match v.trim().to_lowercase().as_str() {
            "real" => Ok(Rr3CloudMode::Real),
            "mock" => Ok(Rr3CloudMode::Mock),
            other => Err(format!(
                "{RR3_CLOUD_MODE_ENV} 取值不认: {other}（允许 real/mock）"
            )),
        },
    }
}

/// 取密钥；缺失即报错。错误文案只点名环境变量，不回显任何凭据。
pub(crate) fn rr3_cloud_api_key() -> Result<String, String> {
    let key = std::env::var(RR3_API_KEY_ENV).unwrap_or_default();
    let key = key.trim();
    if key.is_empty() {
        return Err(format!("缺少环境变量 {RR3_API_KEY_ENV}，云端链路不可用"));
    }
    Ok(key.to_string())
}

/// secrets 缺省路径（Python `expanduser(~/.rr3/secrets.json)` 同形状：HOME 缺失
/// 保留 `~` 字面——后续打开必然失败折 None，语义一致）。
fn rr3_secrets_default_path() -> String {
    match std::env::var("HOME") {
        Ok(home) => format!("{home}/.rr3/secrets.json"),
        Err(_) => "~/.rr3/secrets.json".to_string(),
    }
}

/// secrets 文件现读 ref（面板维护面）：每次调用现读——面板填完下一轮即生效，无需
/// 重启。缺文件/缺键/坏 json/值非串一律 None，容忍与否由调用方裁定（real 档缺
/// key 报错只点名 ref，不回显任何值）。
pub(crate) fn rr3_secrets_get(reference: &str) -> Option<String> {
    let path = std::env::var(RR3_SECRETS_FILE_ENV).unwrap_or_else(|_| rr3_secrets_default_path());
    let text = std::fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    let val = data.get("keys")?.get(reference)?.as_str()?;
    let val = val.trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

// ---------- HTTP 与编码纯函数（自 cloud/__init__.py 迁入） ----------

/// 一次 POST；返回 (status, body)。4xx/5xx 也带回状态码与错误体（调用方折业务失败）。
///
/// key 空串 → 不带 Authorization 头（local 免鉴权路径）；real 档缺 key 由调用方
/// 先报错，不在此静默放行。传输异常（连接拒绝/超时/读断——Python OSError 族对位）
/// 与读体失败统一折「云端不可达 {url}: …」（URL 不含密钥——Authorization 只进
/// header，文案可安全带出）。
pub(crate) fn rr3_http_post(
    url: &str,
    key: &str,
    body: &[u8],
    content_type: &str,
    timeout_s: f64,
) -> Result<(u16, Vec<u8>), String> {
    let req = ureq::post(url)
        .set("Content-Type", content_type)
        .timeout(Duration::from_secs_f64(timeout_s));
    let req = if key.is_empty() {
        req
    } else {
        req.set("Authorization", &format!("Bearer {key}"))
    };
    match req.send_bytes(body) {
        Ok(resp) => Ok((resp.status(), rr3_read_body(url, resp)?)),
        // 4xx/5xx：urllib HTTPError 对位——状态码与错误体照回（调用方折业务失败）
        Err(ureq::Error::Status(code, resp)) => Ok((code, rr3_read_body(url, resp)?)),
        Err(e) => Err(format!("云端不可达 {url}: {e}")),
    }
}

/// 应答体读尽（read-until-close 语义对 urllib resp.read() 同形状——TTS 响应无
/// Content-Length、服务端发完即断，按流读尽即得）。
fn rr3_read_body(url: &str, resp: ureq::Response) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("云端不可达 {url}: {e}"))?;
    Ok(buf)
}

/// 识别客户端（SenseVoice）：整段 WAV 一次上传（ADR-0014 裁定 2；流式是 legacy
/// ESP32 内存约束的产物，本机不复刻）。WAV 头/multipart 装配机制在
/// [`crate::asr`]，本客户端负责云档/密钥/HTTP/应答解析（策略与 IO 面）。
pub(crate) struct Rr3AsrClient {
    base_url: String,
    model: String,
    timeout_s: f64,
}

impl Rr3AsrClient {
    /// 装配构造（Python `base_url.rstrip("/")` 同形状）。
    pub(crate) fn new(base_url: &str, model: &str, timeout_s: f64) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            timeout_s,
        }
    }

    /// PCM（s16le 单声道）→ 识别文本。云端失败/非 200 折 Err（业务失败）。
    pub(crate) fn rr3_transcribe(&self, pcm: &[u8], sample_rate: u32) -> Result<String, String> {
        if rr3_cloud_mode()? == Rr3CloudMode::Mock {
            return Ok(std::env::var(RR3_MOCK_TEXT_ENV)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| RR3_MOCK_TEXT.to_string()));
        }
        let key = rr3_cloud_api_key()?;
        let boundary = rr3_boundary_hex();
        let body = crate::asr::rr3_asr_multipart_body(&self.model, pcm, sample_rate, &boundary);
        let url = format!("{}/v1/audio/transcriptions", self.base_url);
        let (status, resp) = rr3_http_post(
            &url,
            &key,
            &body,
            &format!("multipart/form-data; boundary={boundary}"),
            self.timeout_s,
        )?;
        if status != 200 {
            return Err(format!("识别接口 HTTP {status}"));
        }
        // json.loads 进护栏（B-16②）：坏 JSON 折 Err 不逃逸（Python try 同形状）
        let data: Value = serde_json::from_slice(&resp)
            .map_err(|e| format!("识别应答非 JSON: {e}"))?;
        data.get("text")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .ok_or_else(|| "识别应答缺 text 字段".to_string())
    }
}

/// LLM 连接与历史整体（0.0.6 模型连接抽象批逐语义迁入）：OpenAI Chat
/// Completions 形状；历史窗口机制（system 恒守首位 + 最近 N 轮 + 失败回滚）在
/// [`crate::llm::Rr3LlmHistory`]，本客户端持之并负责 IO/JSON 面装配。
pub(crate) struct Rr3LlmClient {
    base_url: String,
    model_id: String,
    timeout_s: f64,
    key_ref: Option<String>,
    mode: Option<Rr3LlmMode>,
    history: crate::llm::Rr3LlmHistory,
}

impl Rr3LlmClient {
    /// 装配构造（历史首条固定 system 人设，文案 [`RR3_SYSTEM_PROMPT`]）。
    pub(crate) fn new(
        base_url: &str,
        model_id: &str,
        timeout_s: f64,
        key_ref: Option<String>,
        mode: Option<Rr3LlmMode>,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model_id: model_id.to_string(),
            timeout_s,
            key_ref,
            mode,
            history: crate::llm::Rr3LlmHistory::rr3_new(RR3_SYSTEM_PROMPT),
        }
    }

    /// 本轮密钥；local 免鉴权回空串（http_post 空串 → 无 Authorization 头）。
    /// 双源：`env:VAR` → 环境变量；裸名 → secrets 文件（每次调用现读）。错误文案
    /// 只点名 ref/环境变量名，不回显任何凭据值。
    fn rr3_resolve_key(&self) -> Result<String, String> {
        if self.mode == Some(Rr3LlmMode::Local) {
            return Ok(String::new());
        }
        match &self.key_ref {
            None => rr3_cloud_api_key(), // 旧路径：通用环境变量键
            Some(r) => {
                if let Some(var) = r.strip_prefix(RR3_KEY_REF_ENV_PREFIX) {
                    let val = std::env::var(var).unwrap_or_default();
                    let val = val.trim();
                    if val.is_empty() {
                        return Err(format!("缺少环境变量 {var}（模型行 key_ref={r}）"));
                    }
                    Ok(val.to_string())
                } else {
                    rr3_secrets_get(r).ok_or_else(|| {
                        format!("secrets 缺引用 {r}（面板填 key 后下一轮生效）")
                    })
                }
            }
        }
    }

    /// 连通性测试（面板"测连"，ADR-0023）：mock 档不触网直接成功（`mock:true`、
    /// 0ms）；real 档发一条 `max_tokens=1` 的最小补全，只判 HTTP 2xx 与耗时。
    /// **不写对话历史**——测连是只读探针，不带任何会话副作用。
    pub(crate) fn rr3_probe(&self) -> Result<(u64, bool), String> {
        if rr3_cloud_mode()? == Rr3CloudMode::Mock {
            return Ok((0, true));
        }
        let key = self.rr3_resolve_key()?;
        let body = json!({
            "model": self.model_id,
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 1,
        })
        .to_string()
        .into_bytes();
        let url = format!("{}/chat/completions", self.base_url);
        let started = std::time::Instant::now();
        let (status, _resp) = rr3_http_post(&url, &key, &body, "application/json", self.timeout_s)?;
        let latency_ms = started.elapsed().as_millis() as u64;
        if !(200..300).contains(&status) {
            return Err(format!("连通性测试 HTTP {status}"));
        }
        Ok((latency_ms, false))
    }

    /// 一轮补全：追加 user →（带历史）请求 → 回文本并追加 assistant；失败轮
    /// user 出列不入账（[`crate::llm::Rr3LlmHistory::rr3_rollback_last`]——
    /// 对位 legacy：只有成功才追加 assistant）。
    pub(crate) fn rr3_complete(&mut self, prompt: &str) -> Result<String, String> {
        if rr3_cloud_mode()? == Rr3CloudMode::Mock {
            let reply = format!("（mock 回复）{prompt}");
            self.history.rr3_user_turn(prompt);
            self.history.rr3_assistant_turn(&reply);
            return Ok(reply);
        }
        let key = self.rr3_resolve_key()?;
        self.history.rr3_user_turn(prompt);
        let messages: Vec<Value> = self
            .history
            .rr3_messages()
            .iter()
            .map(|(role, content)| json!({ "role": role, "content": content }))
            .collect();
        let body = json!({
            "model": self.model_id,
            "messages": messages,
            "max_tokens": RR3_LLM_MAX_TOKENS,
        })
        .to_string()
        .into_bytes();
        // base_url = 完整 OpenAI 兼容 API 根（含版本段，模型行配置承载），客户端
        // 只拼 `/chat/completions`——GLM `/api/paas/v4`、Qwen `/compatible-mode/v1`
        // 与 DeepSeek `/v1` 同形接入，不按厂商猜版本段（ADR-0023）。
        let url = format!("{}/chat/completions", self.base_url);
        let (status, resp) = rr3_http_post(&url, &key, &body, "application/json", self.timeout_s)?;
        if status != 200 {
            self.history.rr3_rollback_last();
            return Err(format!("大模型接口 HTTP {status}"));
        }
        // 应答形状面收敛为一档（Python KeyError/IndexError/JSONDecodeError 同一
        // except 折同文案）：取不出 choices[0].message.content 串即形状不符
        let reply = serde_json::from_slice::<Value>(&resp)
            .ok()
            .and_then(|v| {
                v.get("choices")?
                    .get(0)?
                    .get("message")?
                    .get("content")?
                    .as_str()
                    .map(|s| s.to_string())
            });
        match reply {
            Some(t) => {
                self.history.rr3_assistant_turn(&t);
                Ok(t)
            }
            None => {
                self.history.rr3_rollback_last();
                Err("大模型应答形状不符: 缺 choices[0].message.content（或应答非 JSON）".to_string())
            }
        }
    }

    /// 对话历史在册（仅测试编译：mock/裁窗/回滚断言的观测面）。
    #[cfg(test)]
    fn rr3_history(&self) -> &[(String, String)] {
        self.history.rr3_messages()
    }
}

/// 模型行密钥是否就位（面板模型列表只回布尔 `key_set`，值零回显）：local 免鉴权
/// 恒 true；`env:` 引用看环境变量非空；裸名看 secrets 现读；无引用走通用环境变量键。
pub(crate) fn rr3_key_set_of(key_ref: &Option<String>, mode: Option<Rr3LlmMode>) -> bool {
    if mode == Some(Rr3LlmMode::Local) {
        return true;
    }
    match key_ref {
        None => rr3_cloud_api_key().is_ok(),
        Some(r) => match r.strip_prefix(RR3_KEY_REF_ENV_PREFIX) {
            Some(var) => std::env::var(var)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false),
            None => rr3_secrets_get(r).is_some(),
        },
    }
}

/// TTS 合成客户端（CosyVoice2）：文本 → PCM 字节流（s16le）。M4 的 PCM 是验证性
/// 消费（非空 + 时长换算入日志）；真播放随 M5 板上 ALSA sink（ADR-0014 裁定 3）。
pub(crate) struct Rr3TtsClient {
    base_url: String,
    model: String,
    timeout_s: f64,
}

impl Rr3TtsClient {
    /// 装配构造（Python `base_url.rstrip("/")` 同形状）。
    pub(crate) fn new(base_url: &str, model: &str, timeout_s: f64) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            timeout_s,
        }
    }

    /// 文本 → PCM 字节流。mock 档走 [`crate::tts::rr3_sine_pcm`]（正弦机制在
    /// atoms）；real 档响应为原始 PCM 流（read-until-close，[`rr3_read_body`]
    /// 读尽即得）。
    pub(crate) fn rr3_synthesize(&self, text: &str, sample_rate: u32) -> Result<Vec<u8>, String> {
        if rr3_cloud_mode()? == Rr3CloudMode::Mock {
            return Ok(crate::tts::rr3_sine_pcm(text, sample_rate));
        }
        if std::env::var(RR3_TTS_BACKEND_ENV).as_deref() == Ok(RR3_TTS_BACKEND_MAC_SAY) {
            return rr3_say_pcm(text, sample_rate);
        }
        let key = rr3_cloud_api_key()?;
        let body = json!({
            "model": self.model,
            "input": text,
            "voice": crate::tts::rr3_tts_voice(&self.model),
            "response_format": "pcm",
            "sample_rate": sample_rate,
        })
        .to_string()
        .into_bytes();
        let url = format!("{}/v1/audio/speech", self.base_url);
        let (status, resp) = rr3_http_post(&url, &key, &body, "application/json", self.timeout_s)?;
        if status != 200 {
            return Err(format!("合成接口 HTTP {status}"));
        }
        Ok(resp)
    }
}

/// 宿主合成临时文件序号（同进程内多次合成不撞名）
static RR3_SAY_SEQ: AtomicU32 = AtomicU32::new(0);

/// 宿主 `say` 合成（仅 `RR3_TTS_BACKEND=mac_say` 档）：一次写临时 WAV（请求
/// 采样率的 s16le 单声道）→ 抽 `data` 块 → 返回裸 PCM。退出非零/文件缺失/
/// 无 data 块一律 Err（文案带 stderr，不静默降级）。
fn rr3_say_pcm(text: &str, sample_rate: u32) -> Result<Vec<u8>, String> {
    let seq = RR3_SAY_SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("rr3-tts-{}-{seq}.wav", std::process::id()));
    let out = std::process::Command::new("/usr/bin/say")
        .arg("-v")
        .arg(RR3_SAY_VOICE)
        .arg("--file-format=WAVE")
        .arg(format!("--data-format=LEI16@{sample_rate}"))
        .arg("-o")
        .arg(&path)
        .arg(text)
        .output()
        .map_err(|e| format!("宿主合成不可用（/usr/bin/say）: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&path);
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "宿主合成失败（say {}）: {}",
            out.status,
            err.trim()
        ));
    }
    let bytes = std::fs::read(&path);
    let _ = std::fs::remove_file(&path);
    let bytes = bytes.map_err(|e| format!("读宿主合成 WAV 失败: {e}"))?;
    crate::tts::rr3_wav_data_chunk(&bytes)
        .map(|b| b.to_vec())
        .ok_or_else(|| "宿主合成 WAV 无 data 块".to_string())
}

/// splitmix64（纯 std 的 64bit 混淆——uuid crate 不引，裁定 5 最小依赖面）。
fn rr3_splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// multipart boundary（对位 asr.py `uuid.uuid4().hex`——32 位小写 hex）。裁定 5
/// 最小依赖面：uuid crate 不引，时间纳秒＋pid＋进程内序号三源混合 splitmix64
/// 产出 128bit——单进程串行调用的 multipart 分隔符唯一性足够（非密码学用途）。
pub(crate) fn rr3_boundary_hex() -> String {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) as u64;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = nanos ^ (std::process::id() as u64).rotate_left(32) ^ seq.rotate_left(16);
    let a = rr3_splitmix64(&mut state);
    let b = rr3_splitmix64(&mut state);
    format!("{a:016x}{b:016x}")
}

// ---------- 测试互斥柄（仅测试编译）：RR3_BRAIN_CLOUD 等键是进程级全局，凡动
// 它的测试必须全 crate 一个锁串行——cloud.rs 与 dispatch.rs 两个测试域共此一柄
// （env_lock 入口在下方 testkit）。

#[cfg(test)]
pub(crate) static RR3_TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---------- 测试共享夹具（仅测试编译）：asr/llm/tts 三家原子件 mech 测试与本
// 文件测试共用——云端拆件后夹具留在共享原语层，一处维护三家受益。 ----------

#[cfg(test)]
pub(crate) mod testkit {
    //! 本地假端点（对位 test_cloud.py FakeSiliconFlow：记录请求、回罐头；std
    //! TcpListener 手写微型 HTTP/1.1——仓内首个 HTTP 服务端测试夹具，仅测试编译）。
    //! asr/llm/tts 三家原子件 mech 测试与 cloud.rs 本域测试共用。

    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::RR3_TEST_ENV_LOCK;

    /// env 动作的测试互斥（全 crate 一柄，见 [`super::RR3_TEST_ENV_LOCK`]）
    pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        RR3_TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) struct FakeCloud {
        pub(crate) base: String,
        pub(crate) requests: Arc<Mutex<Vec<(String, String, Vec<u8>)>>>, // (path, authorization, body)
        pub(crate) fail_path: Arc<Mutex<Option<String>>>,
        pub(crate) bad_json_path: Arc<Mutex<Option<String>>>,
        pub(crate) missing_field_path: Arc<Mutex<Option<String>>>,
    }

    impl FakeCloud {
        pub(crate) fn start() -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let fail_path = Arc::new(Mutex::new(None));
            let bad_json_path = Arc::new(Mutex::new(None));
            let missing_field_path = Arc::new(Mutex::new(None));
            let (r, f, b, m) = (
                requests.clone(),
                fail_path.clone(),
                bad_json_path.clone(),
                missing_field_path.clone(),
            );
            // 线程随测试进程退出回收（cargo test 主线程等所有 #[test] 完成，本
            // detached 线程不阻塞收尾；每测试独立端口，线程间零干扰）
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    let Some((path, headers, body)) = read_request(&mut stream) else { continue };
                    r.lock().unwrap().push((
                        path.clone(),
                        headers.get("authorization").cloned().unwrap_or_default(),
                        body,
                    ));
                    let fail = f.lock().unwrap().clone();
                    let bad = b.lock().unwrap().clone();
                    let missing = m.lock().unwrap().clone();
                    if Some(&path) == fail.as_ref() {
                        respond(&mut stream, "500 Internal Server Error", b"{\"error\":\"boom\"}");
                    } else if Some(&path) == bad.as_ref() {
                        // 200 + 非 JSON 体（B-16②：云 200 坏体的静默假成功口）
                        respond(&mut stream, "200 OK", b"not-json-at-all");
                    } else if Some(&path) == missing.as_ref() {
                        respond(&mut stream, "200 OK", b"{}");
                    } else if path == "/v1/audio/transcriptions" {
                        respond(&mut stream, "200 OK", "{\"text\": \"你好\"}".as_bytes());
                    } else if path == "/v1/chat/completions" {
                        respond(
                            &mut stream,
                            "200 OK",
                            json!({"choices": [{"message": {"role": "assistant", "content": "好的"}}]})
                                .to_string()
                                .as_bytes(),
                        );
                    } else {
                        // /v1/audio/speech → 原始 PCM
                        respond(&mut stream, "200 OK", &vec![1u8, 0].repeat(1600));
                    }
                }
            });
            Self {
                base: format!("http://127.0.0.1:{port}"),
                requests,
                fail_path,
                bad_json_path,
                missing_field_path,
            }
        }

        pub(crate) fn last(&self) -> (String, String, Vec<u8>) {
            self.requests.lock().unwrap().last().unwrap().clone()
        }
    }

    /// 读一个 POST 请求（循环读至 Content-Length 满足——单次 read 不可靠）
    fn read_request(stream: &mut TcpStream) -> Option<(String, HashMap<String, String>, Vec<u8>)> {
        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        let header_end = loop {
            let n = stream.read(&mut buf).ok()?;
            if n == 0 {
                return None;
            }
            data.extend_from_slice(&buf[..n]);
            if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let text = String::from_utf8_lossy(&data[..header_end]).to_string();
        let mut lines = text.lines();
        let request_line = lines.next()?.to_string();
        let mut headers = HashMap::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
        let clen: usize = headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        while data.len() < header_end + clen {
            let n = stream.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        let path = request_line.split_whitespace().nth(1)?.to_string();
        Some((path, headers, data[header_end..header_end + clen].to_vec()))
    }

    fn respond(stream: &mut TcpStream, status: &str, body: &[u8]) {
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{env_lock, FakeCloud};
    use super::*;

    // ---------- 纯函数面 ----------

    /// boundary 32 位小写 hex、两次调用不同
    #[test]
    fn boundary_hex_shape() {
        let a = rr3_boundary_hex();
        let b = rr3_boundary_hex();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, b);
    }

    /// 云档解析四径：缺席 real、mock、bogus 红、空串红（Python 键在空串 not in 同红）
    #[test]
    fn cloud_mode_parse() {
        let _g = env_lock();
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
        assert_eq!(rr3_cloud_mode().unwrap(), Rr3CloudMode::Real);
        std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
        assert_eq!(rr3_cloud_mode().unwrap(), Rr3CloudMode::Mock);
        std::env::set_var(RR3_CLOUD_MODE_ENV, "bogus");
        let e = rr3_cloud_mode().unwrap_err();
        assert_eq!(e, "RR3_BRAIN_CLOUD 取值不认: bogus（允许 real/mock）");
        std::env::set_var(RR3_CLOUD_MODE_ENV, "");
        let e = rr3_cloud_mode().unwrap_err();
        assert!(e.contains("取值不认"), "空串同红: {e}");
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// secrets 现读：缺文件 None、缺键 None、有值 trim、空串 None
    #[test]
    fn secrets_get_reads_file_lazily() {
        let _g = env_lock();
        let dir = std::env::temp_dir().join(format!("rr3-brain-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secrets.json");
        std::fs::write(&path, r#"{"keys": {"DeepSeek官方": "  sk-test  ", "空": "  "}}"#).unwrap();
        std::env::set_var(RR3_SECRETS_FILE_ENV, &path);
        assert_eq!(rr3_secrets_get("DeepSeek官方").as_deref(), Some("sk-test"));
        assert_eq!(rr3_secrets_get("空"), None, "空白值折 None");
        assert_eq!(rr3_secrets_get("没有的"), None, "缺键 None");
        std::env::set_var(RR3_SECRETS_FILE_ENV, dir.join("不存在.json"));
        assert_eq!(rr3_secrets_get("任意"), None, "缺文件 None");
        std::env::remove_var(RR3_SECRETS_FILE_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 传输异常收敛（B-16②）：端口 1 无监听 → 连接拒绝折「云端不可达」（Python
    /// OSError 族折 RuntimeError 同语义）
    #[test]
    fn http_post_folds_unreachable_endpoint() {
        let e = rr3_http_post("http://127.0.0.1:1/v1/audio/transcriptions", "k", b"x", "text/plain", 1.0)
            .unwrap_err();
        assert!(e.contains("云端不可达"), "传输失败折固定文案: {e}");
        assert!(!e.contains("Bearer"), "文案不得带出密钥");
    }

    // ---------- 客户端面（asr/llm/tts 三家经本地假端点，对位 test_cloud.py
    // TestRealPathAgainstFakeEndpoint / TestMockPath / B-16② 收敛三组） ----------

    /// real 档 ASR 真调假端点：Bearer 鉴权、multipart 请求体带 model/WAV 头/
    /// filename="a.wav"（对位 test_asr_sends_multipart_with_model_and_wav）
    #[test]
    fn asr_real_sends_multipart_with_model_and_wav() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        let client = Rr3AsrClient::new(&srv.base, "iic/SenseVoiceSmall", 5.0);
        let text = client.rr3_transcribe(&b"\x00\x01".repeat(16), 16000).unwrap();
        assert_eq!(text, "你好");
        let (path, auth, body) = srv.last();
        assert_eq!(path, "/v1/audio/transcriptions");
        assert_eq!(auth, "Bearer test-key");
        assert!(body.windows(12).any(|w| w == b"name=\"model\""));
        assert!(body.windows(19).any(|w| w == b"iic/SenseVoiceSmall"));
        assert!(body.windows(4).any(|w| w == b"RIFF"), "WAV 头仓内自建");
        assert!(body.windows(16).any(|w| w == b"filename=\"a.wav\""));
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// mock 档 ASR 不触网不读 key：固定文本；RR3_BRAIN_MOCK_ASR_TEXT 注入覆盖
    /// （批F2 判向-转向闭环 e2e 的注入面）、空白回落固定文本
    #[test]
    fn asr_mock_fixed_text_and_env_injection_without_key() {
        let _g = env_lock();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
        std::env::remove_var(RR3_API_KEY_ENV);
        let client = Rr3AsrClient::new("http://127.0.0.1:1", "m", 1.0);
        assert_eq!(
            client.rr3_transcribe(b"\x00\x00", 16000).unwrap(),
            "你好，今天天气怎么样"
        );
        std::env::set_var(RR3_MOCK_TEXT_ENV, "向左");
        assert_eq!(client.rr3_transcribe(b"\x00\x00", 16000).unwrap(), "向左");
        std::env::set_var(RR3_MOCK_TEXT_ENV, "   ");
        assert_eq!(
            client.rr3_transcribe(b"\x00\x00", 16000).unwrap(),
            "你好，今天天气怎么样",
            "空白注入回落固定文本"
        );
        std::env::remove_var(RR3_MOCK_TEXT_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// real 档 ASR 200 + 非 JSON 体：折「识别应答非 JSON」不逃逸（B-16②，
    /// 对位 test_asr_non_json_body_is_runtime_error_not_escape）
    #[test]
    fn asr_non_json_body_folds_error() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        *srv.bad_json_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
        let client = Rr3AsrClient::new(&srv.base, "m", 5.0);
        let e = client
            .rr3_transcribe(&b"\x00\x00".repeat(100), 16000)
            .unwrap_err();
        assert!(e.contains("非 JSON"), "{e}");
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// real 档 LLM：多轮裁窗（system 恒守首位 + 最近 4 轮）、max_tokens=150、
    /// 失败轮回滚（user 一并出列——裁剪 -2＋回滚 -1 = before-2，Python 同账）
    #[test]
    fn llm_real_history_trim_and_failure_rollback() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        // 模型行 base_url = 完整 API 根（假端点在主机根之上挂 /v1）
        let llm_root = format!("{}/v1", srv.base);
        let mut client = Rr3LlmClient::new(&llm_root, "deepseek-ai/DeepSeek-V3", 5.0, None, None);
        for i in 0..crate::llm::RR3_LLM_HISTORY_ROUNDS + 2 {
            assert_eq!(client.rr3_complete(&format!("问{i}")).unwrap(), "好的");
        }
        let h = client.rr3_history();
        assert_eq!(h.len(), 1 + crate::llm::RR3_LLM_HISTORY_ROUNDS * 2);
        assert_eq!(h[0].0, "system");
        assert_eq!(h[1].1, "问2", "最旧一轮已裁");
        let (path, _, body) = srv.last();
        assert_eq!(path, "/v1/chat/completions");
        let req: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(req["max_tokens"], 150);
        // 失败轮不入账：user 先入列、失败后回滚（对位 legacy：只有成功才追加 assistant）
        let before = client.rr3_history().len();
        *srv.fail_path.lock().unwrap() = Some("/v1/chat/completions".into());
        assert!(client.rr3_complete("会失败").is_err());
        *srv.fail_path.lock().unwrap() = None;
        assert_eq!(client.rr3_history().len(), before - 2, "裁剪 + 回滚共 -2");
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// mock 档 LLM 回显且入账（history 尾条 = 回显应答；不触网不读 key）
    #[test]
    fn llm_mock_echo_and_history() {
        let _g = env_lock();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
        let mut client = Rr3LlmClient::new("http://127.0.0.1:1", "m", 1.0, None, None);
        assert_eq!(client.rr3_complete("在吗").unwrap(), "（mock 回复）在吗");
        assert_eq!(client.rr3_history().last().unwrap().1, "（mock 回复）在吗");
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// real 档 LLM 200 + 非 JSON 体：形状不符一档折 Err 且 user 回滚
    /// （B-16②，Python 单 except 同文案档）
    #[test]
    fn llm_non_json_body_folds_shape_error() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        let llm_root = format!("{}/v1", srv.base);
        let mut client = Rr3LlmClient::new(&llm_root, "m", 5.0, None, None);
        *srv.bad_json_path.lock().unwrap() = Some("/v1/chat/completions".into());
        let e = client.rr3_complete("在吗").unwrap_err();
        assert!(e.contains("形状不符"), "{e}");
        assert_eq!(client.rr3_history().len(), 1, "失败轮 user 已回滚");
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// real 档 LLM 200 + 缺字段体：形状不符同档（choices[0].message.content
    /// 取不出即红，Python KeyError 同 except 折同文案）
    #[test]
    fn llm_missing_field_folds_shape_error() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        let llm_root = format!("{}/v1", srv.base);
        let mut client = Rr3LlmClient::new(&llm_root, "m", 5.0, None, None);
        *srv.missing_field_path.lock().unwrap() = Some("/v1/chat/completions".into());
        let e = client.rr3_complete("在吗").unwrap_err();
        assert!(e.contains("形状不符"), "{e}");
        assert_eq!(client.rr3_history().len(), 1, "失败轮 user 已回滚");
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// real 档 TTS 真调：PCM 原样回流、请求体 voice=:bella/response_format/
    /// sample_rate（对位 test_tts_returns_pcm_stream）
    #[test]
    fn tts_real_returns_pcm_stream() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        let client = Rr3TtsClient::new(&srv.base, "FunAudioLLM/CosyVoice2-0.5B", 5.0);
        let pcm = client.rr3_synthesize("好的", 16000).unwrap();
        assert_eq!(pcm, vec![1u8, 0].repeat(1600));
        let (path, _, body) = srv.last();
        assert_eq!(path, "/v1/audio/speech");
        let req: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(req["voice"], "FunAudioLLM/CosyVoice2-0.5B:bella");
        assert_eq!(req["response_format"], "pcm");
        assert_eq!(req["sample_rate"], 16000);
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// 宿主兜底档：真云挂死时 `RR3_TTS_BACKEND=mac_say` 走本机 say，交出非空
    /// s16le PCM（仅 macOS；say 缺失/无 data 块即 Err，不静默降级）
    #[test]
    #[cfg(target_os = "macos")]
    fn tts_mac_say_backend_returns_pcm_without_cloud() {
        let _g = env_lock();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_TTS_BACKEND_ENV, RR3_TTS_BACKEND_MAC_SAY);
        std::env::remove_var(RR3_API_KEY_ENV);
        let client = Rr3TtsClient::new("http://127.0.0.1:9", "unused", 1.0);
        let pcm = client.rr3_synthesize("你好", 16000).unwrap();
        assert!(!pcm.is_empty());
        assert_eq!(pcm.len() % 2, 0);
        std::env::remove_var(RR3_TTS_BACKEND_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// mock 档 TTS 不触网不读 key：正弦 PCM 非空（机制在 atoms/tts）
    #[test]
    fn tts_mock_sine_without_key() {
        let _g = env_lock();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
        std::env::remove_var(RR3_API_KEY_ENV);
        let client = Rr3TtsClient::new("http://127.0.0.1:1", "m", 1.0);
        assert!(!client.rr3_synthesize("hi", 16000).unwrap().is_empty());
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }
}
