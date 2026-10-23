//! rr3-brain 云端适配层（批N32 迁移件2，ADR-0017 五层表——自 Python cloud/
//! \_\_init\_\_.py + asr.py + llm.py + tts.py 四件逐语义迁入，ADR-0014 裁定 2）。
//!
//! 线上契约逐项对齐 legacy/core/{asr,llm,tts}（SiliconFlow /v1 形状）：
//! - ASR  POST {base}/v1/audio/transcriptions   multipart(model + WAV) → {"text": …}
//! - LLM  POST {base}/v1/chat/completions       {model, messages, max_tokens} → choices[0].message.content
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
//! 调用方（completion 臂折 error 应答、件四编排面折日志弃轮）分派。

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

/// legacy/core/llm/src/llm_config.h 的原值移植（M4 不改人设与参数）
pub(crate) const RR3_SYSTEM_PROMPT: &str =
    "你是一个中文语音助手，回答要口语化、简短，一般不超过50个字，不要用列表、符号和表情。";
/// 历史窗口轮数（一轮 = user + assistant 两条，裁剪只动最旧的成对话轮）
pub(crate) const RR3_LLM_HISTORY_ROUNDS: usize = 4;
/// legacy llm_config.h 原值
pub(crate) const RR3_LLM_MAX_TOKENS: i64 = 150;
/// TTS 女声 bella（男声改 alex——tts_config.h）
pub(crate) const RR3_TTS_VOICE_SUFFIX: &str = ":bella";
/// key_ref 双源语法前缀：`env:变量名` → 环境变量
pub(crate) const RR3_KEY_REF_ENV_PREFIX: &str = "env:";
/// mock TTS 正弦时长：每字符 80ms
const RR3_SINE_MS_PER_CHAR: u64 = 80;

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

/// 44B WAV 头（PCM s16le 单声道），字节序对齐 legacy asr.c 的 build_wav_header。
pub(crate) fn rr3_wav_header(sample_rate: u32, data_len: u32) -> [u8; 44] {
    let mut b = [0u8; 44];
    b[0..4].copy_from_slice(b"RIFF");
    b[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    b[8..16].copy_from_slice(b"WAVEfmt ");
    b[16..20].copy_from_slice(&16u32.to_le_bytes());
    b[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    b[22..24].copy_from_slice(&1u16.to_le_bytes()); // 单声道
    b[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    b[28..32].copy_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    b[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    b[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
    b[36..40].copy_from_slice(b"data");
    b[40..44].copy_from_slice(&data_len.to_le_bytes());
    b
}

/// mock TTS 的确定性 PCM：440Hz 正弦，时长随文本长度（s16le；振幅 12000）。
pub(crate) fn rr3_sine_pcm(text: &str, sample_rate: u32) -> Vec<u8> {
    rr3_sine_pcm_ms(text, sample_rate, RR3_SINE_MS_PER_CHAR)
}

/// [`rr3_sine_pcm`] 的参数化底座（ms_per_char 可调——真云冒烟夹具喂 125ms）。
pub(crate) fn rr3_sine_pcm_ms(text: &str, sample_rate: u32, ms_per_char: u64) -> Vec<u8> {
    let n = text.trim().chars().count().max(1) as u64;
    let total = (sample_rate as u64 * n * ms_per_char / 1000) as usize;
    let mut pcm = Vec::with_capacity(total * 2);
    for i in 0..total {
        let v = (12000.0
            * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sample_rate as f64).sin())
            as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    pcm
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

// ---------- ASR 适配器（自 cloud/asr.py 迁入：SenseVoice，整段 WAV 一次上传——
// ADR-0014 裁定 2；流式是 legacy ESP32 内存约束的产物，本机不复刻——见该 ADR
// "明确不做"第 3 条） ----------

pub(crate) struct Rr3AsrClient {
    base_url: String,
    model: String,
    timeout_s: f64,
}

impl Rr3AsrClient {
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
            // 注入键取值不设/置空回落固定文本
            let injected = std::env::var(RR3_MOCK_TEXT_ENV).unwrap_or_default();
            let injected = injected.trim();
            return Ok(if injected.is_empty() {
                RR3_MOCK_TEXT.to_string()
            } else {
                injected.to_string()
            });
        }
        let key = rr3_cloud_api_key()?;
        let header = rr3_wav_header(sample_rate, pcm.len() as u32);
        let boundary = rr3_boundary_hex();
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{}\r\n",
                self.model
            )
            .as_bytes(),
        );
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(&header);
        body.extend_from_slice(pcm);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let (status, resp) = rr3_http_post(
            &format!("{}/v1/audio/transcriptions", self.base_url),
            &key,
            &body,
            &format!("multipart/form-data; boundary={boundary}"),
            self.timeout_s,
        )?;
        if status != 200 {
            return Err(format!("识别接口 HTTP {status}"));
        }
        // json 解析进护栏（B-16②）：非 JSON/非 dict/缺 text/非串一律 Err（语义
        // 差③——Python 非 dict 体 AttributeError 逃逸，此处收敛）
        let parsed: Value =
            serde_json::from_slice(&resp).map_err(|e| format!("识别应答非 JSON: {e}"))?;
        match parsed.get("text").and_then(Value::as_str) {
            Some(t) => Ok(t.to_string()),
            None => Err("识别应答缺 text 字段".to_string()),
        }
    }
}

// ---------- LLM 适配器（自 cloud/llm.py 迁入：OpenAI Chat Completions 形状，
// 0.0.6 模型连接抽象批——连接参数由 config [[rr3-brain.model]] 行承载，换云只改
// 配置不改码。历史策略移植 legacy/core/llm/src/llm.c：system + 最近
// RR3_LLM_HISTORY_ROUNDS 轮，裁剪只动最旧的成对话轮） ----------

pub(crate) struct Rr3LlmClient {
    base_url: String,
    model: String,
    timeout_s: f64,
    /// None = 走通用环境变量键（旧路径）；`env:VAR` → 环境变量；裸名 → secrets 文件
    key_ref: Option<String>,
    /// None = 随全局档（RR3_BRAIN_CLOUD）；Some(Local) = 免鉴权
    mode: Option<Rr3LlmMode>,
    /// 对话历史：[(role, content)]，首条固定 system 人设
    history: Vec<(String, String)>,
}

impl Rr3LlmClient {
    pub(crate) fn new(
        base_url: &str,
        model: &str,
        timeout_s: f64,
        key_ref: Option<String>,
        mode: Option<Rr3LlmMode>,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            timeout_s,
            key_ref,
            mode,
            history: vec![("system".to_string(), RR3_SYSTEM_PROMPT.to_string())],
        }
    }

    /// 本轮密钥；local 免鉴权回空串（http_post 空串 → 无 Authorization 头）。
    fn rr3_resolve_key(&self) -> Result<String, String> {
        if self.mode == Some(Rr3LlmMode::Local) {
            return Ok(String::new());
        }
        match &self.key_ref {
            None => rr3_cloud_api_key(), // 旧路径：通用环境变量键
            Some(kr) => {
                if let Some(var) = kr.strip_prefix(RR3_KEY_REF_ENV_PREFIX) {
                    let val = std::env::var(var).unwrap_or_default();
                    let val = val.trim();
                    if val.is_empty() {
                        return Err(format!("缺少环境变量 {var}（模型行 key_ref={kr}）"));
                    }
                    Ok(val.to_string())
                } else {
                    rr3_secrets_get(kr)
                        .ok_or_else(|| format!("secrets 缺引用 {kr}（面板填 key 后下一轮生效）"))
                }
            }
        }
    }

    /// 对话历史在册（测试面/编排面只读——完成轮回账在调用方）。
    #[cfg(test)]
    pub(crate) fn rr3_history(&self) -> &[(String, String)] {
        &self.history
    }

    /// 一轮补全：追加 user →（带历史）请求 → 回文本并追加 assistant。
    pub(crate) fn rr3_complete(&mut self, prompt: &str) -> Result<String, String> {
        if rr3_cloud_mode()? == Rr3CloudMode::Mock {
            let reply = format!("（mock 回复）{prompt}");
            self.history.push(("user".to_string(), prompt.to_string()));
            self.history.push(("assistant".to_string(), reply.clone()));
            return Ok(reply);
        }
        let key = self.rr3_resolve_key()?;
        self.history.push(("user".to_string(), prompt.to_string()));
        // 裁剪只动最旧的成对话轮（history[1..3]——system 恒守首位）
        while self.history.len() > 1 + RR3_LLM_HISTORY_ROUNDS * 2 {
            self.history.drain(1..3);
        }
        let messages: Vec<Value> = self
            .history
            .iter()
            .map(|(role, content)| json!({"role": role, "content": content}))
            .collect();
        let body = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": RR3_LLM_MAX_TOKENS,
        })
        .to_string();
        let (status, resp) = rr3_http_post(
            &format!("{}/v1/chat/completions", self.base_url),
            &key,
            body.as_bytes(),
            "application/json",
            self.timeout_s,
        )?;
        if status != 200 {
            self.history.pop(); // 失败轮不入账（对位 legacy：只有成功才追加 assistant）
            return Err(format!("大模型接口 HTTP {status}"));
        }
        // 应答形状护栏（B-16②）：非 JSON/形状不符/非串 content 一律 Err 且不入账
        // （语义差③——Python content 非 str 照 append，此处 fail-closed 收敛）
        let parsed: Value = match serde_json::from_slice(&resp) {
            Ok(v) => v,
            Err(e) => {
                self.history.pop();
                return Err(format!("大模型应答形状不符: {e}"));
            }
        };
        let reply = match parsed
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
        {
            Some(t) => t.to_string(),
            None => {
                self.history.pop();
                return Err("大模型应答形状不符: 缺 choices[0].message.content".to_string());
            }
        };
        self.history
            .push(("assistant".to_string(), reply.clone()));
        Ok(reply)
    }
}

// ---------- TTS 适配器（自 cloud/tts.py 迁入：CosyVoice2，文本 → PCM 字节流。
// M4 的 PCM 是验证性消费（非空 + 时长换算入日志）；真播放随 M5 板上 ALSA sink
// （ADR-0014 裁定 3）。响应无 Content-Length、服务端发完即断（read-until-close）
// —— rr3_read_body 的按流读尽天然就是这个语义） ----------

pub(crate) struct Rr3TtsClient {
    base_url: String,
    model: String,
    timeout_s: f64,
}

impl Rr3TtsClient {
    pub(crate) fn new(base_url: &str, model: &str, timeout_s: f64) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            timeout_s,
        }
    }

    /// 文本 → PCM 字节流（s16le）。
    pub(crate) fn rr3_synthesize(&self, text: &str, sample_rate: u32) -> Result<Vec<u8>, String> {
        if rr3_cloud_mode()? == Rr3CloudMode::Mock {
            return Ok(rr3_sine_pcm(text, sample_rate));
        }
        let key = rr3_cloud_api_key()?;
        let body = json!({
            "model": self.model,
            "input": text,
            "voice": format!("{}{RR3_TTS_VOICE_SUFFIX}", self.model),
            "response_format": "pcm",
            "sample_rate": sample_rate,
        })
        .to_string();
        let (status, resp) = rr3_http_post(
            &format!("{}/v1/audio/speech", self.base_url),
            &key,
            body.as_bytes(),
            "application/json",
            self.timeout_s,
        )?;
        if status != 200 {
            return Err(format!("合成接口 HTTP {status}"));
        }
        Ok(resp)
    }
}

// ---------- 测试互斥柄（仅测试编译）：RR3_BRAIN_CLOUD 等键是进程级全局，凡动
// 它的测试必须全 crate 一个锁串行——cloud.rs 与 dispatch.rs 两个测试域共此一柄。

#[cfg(test)]
pub(crate) static RR3_TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use std::net::TcpStream;
    use std::sync::{Arc, Mutex};

    /// env 动作的测试互斥（全 crate 一柄，见 RR3_TEST_ENV_LOCK）
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        RR3_TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---------- 纯函数面 ----------

    /// 44B WAV 头逐字段（对位 test_cloud.py test_wav_header_shape）
    #[test]
    fn wav_header_shape() {
        let hdr = rr3_wav_header(16000, 3200);
        assert_eq!(&hdr[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(hdr[4..8].try_into().unwrap()), 36 + 3200);
        assert_eq!(&hdr[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(hdr[20..22].try_into().unwrap()), 1); // PCM
        assert_eq!(u16::from_le_bytes(hdr[22..24].try_into().unwrap()), 1); // mono
        assert_eq!(u32::from_le_bytes(hdr[24..28].try_into().unwrap()), 16000);
        assert_eq!(&hdr[36..40], b"data");
        assert_eq!(u32::from_le_bytes(hdr[40..44].try_into().unwrap()), 3200);
    }

    /// mock 正弦确定性、偶数字节长、非空、振幅不越 12000（对位 test_sine_pcm_…）
    #[test]
    fn sine_pcm_deterministic_and_sized() {
        let a = rr3_sine_pcm_ms("你好", 16000, 80);
        let b = rr3_sine_pcm_ms("你好", 16000, 80);
        assert_eq!(a, b);
        assert_eq!(a.len() % 2, 0);
        assert!(!a.is_empty());
        for chunk in a.chunks(2) {
            let v = i16::from_le_bytes(chunk.try_into().unwrap());
            assert!(v.unsigned_abs() <= 12000);
        }
    }

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

    /// 密钥缺失点名环境变量、不回显任何值；secrets 双源各自点名 ref
    #[test]
    fn key_resolution_errors_name_source_not_value() {
        let _g = env_lock();
        std::env::remove_var(RR3_API_KEY_ENV);
        let e = rr3_cloud_api_key().unwrap_err();
        assert_eq!(e, "缺少环境变量 RR3_CLOUD_API_KEY，云端链路不可用");
        // env: 前缀缺变量 → 点名变量名与 key_ref
        let client = Rr3LlmClient::new("http://127.0.0.1:1", "m", 1.0, Some("env:RR3_NO_SUCH_VAR".into()), None);
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        let e = client.rr3_resolve_key().unwrap_err();
        assert_eq!(e, "缺少环境变量 RR3_NO_SUCH_VAR（模型行 key_ref=env:RR3_NO_SUCH_VAR）");
        // 裸名 → secrets 文件缺引用 → 点名 ref
        let client = Rr3LlmClient::new("http://127.0.0.1:1", "m", 1.0, Some("不存在的引用".into()), None);
        let e = client.rr3_resolve_key().unwrap_err();
        assert!(e.contains("secrets 缺引用 不存在的引用"), "点名 ref: {e}");
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

    // ---------- 本地假端点（对位 test_cloud.py FakeSiliconFlow：记录请求、回罐头；
    // std TcpListener 手写微型 HTTP/1.1——仓内首个 HTTP 服务端测试夹具，仅测试编译） ----—

    struct FakeCloud {
        base: String,
        requests: Arc<Mutex<Vec<(String, String, Vec<u8>)>>>, // (path, authorization, body)
        fail_path: Arc<Mutex<Option<String>>>,
        bad_json_path: Arc<Mutex<Option<String>>>,
        missing_field_path: Arc<Mutex<Option<String>>>,
    }

    impl FakeCloud {
        fn start() -> Self {
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

        fn last(&self) -> (String, String, Vec<u8>) {
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

    // ---------- 真路径（本地假端点；密钥经环境变量注入，仓里不落任何凭据） ----------

    /// ASR：multipart 形状逐项——path、Bearer 头、model 字段、WAV 头仓内自建、
    /// filename="a.wav"（对位 test_asr_sends_multipart_with_model_and_wav）
    #[test]
    fn asr_sends_multipart_with_model_and_wav() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        let client = Rr3AsrClient::new(&srv.base, "iic/SenseVoiceSmall", 5.0);
        let text = client.rr3_transcribe(&vec![0u8, 1].repeat(16), 16000).unwrap();
        assert_eq!(text, "你好");
        let (path, auth, body) = srv.last();
        assert_eq!(path, "/v1/audio/transcriptions");
        assert_eq!(auth, "Bearer test-key");
        assert!(body.windows(12).any(|w| w == b"name=\"model\""));
        assert!(body.windows(19).any(|w| w == b"iic/SenseVoiceSmall"));
        assert!(body.windows(4).any(|w| w == b"RIFF"));
        assert!(body.windows(16).any(|w| w == b"filename=\"a.wav\""));
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// ASR：非 200 折「识别接口 HTTP 500」；200 坏体折「非 JSON」；200 缺字段折
    /// 「缺 text 字段」（B-16② 三径，对位 test_asr_non_json_body_is_runtime_error…）
    #[test]
    fn asr_error_folds_are_named() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        let client = Rr3AsrClient::new(&srv.base, "m", 5.0);
        *srv.fail_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
        let e = client.rr3_transcribe(&[0u8; 8], 16000).unwrap_err();
        assert_eq!(e, "识别接口 HTTP 500");
        *srv.fail_path.lock().unwrap() = None;
        *srv.bad_json_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
        let e = client.rr3_transcribe(&[0u8; 8], 16000).unwrap_err();
        assert!(e.contains("非 JSON"), "坏体点名: {e}");
        *srv.bad_json_path.lock().unwrap() = None;
        *srv.missing_field_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
        let e = client.rr3_transcribe(&[0u8; 8], 16000).unwrap_err();
        assert_eq!(e, "识别应答缺 text 字段");
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// LLM：历史裁剪（system + 最近 4 轮）与失败轮回滚 -2（对位
    /// test_llm_history_trim_and_failure_rollback）
    #[test]
    fn llm_history_trim_and_failure_rollback() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        let mut client = Rr3LlmClient::new(&srv.base, "deepseek-ai/DeepSeek-V3", 5.0, None, None);
        for i in 0..RR3_LLM_HISTORY_ROUNDS + 2 {
            assert_eq!(client.rr3_complete(&format!("问{i}")).unwrap(), "好的");
        }
        // system + 最近 4 轮（一轮两条）；最旧两条已裁（问0/问1 出列）
        assert_eq!(client.rr3_history().len(), 1 + RR3_LLM_HISTORY_ROUNDS * 2);
        assert_eq!(client.rr3_history()[0].0, "system");
        assert_eq!(client.rr3_history()[1].1, "问2");
        let (path, _, body) = srv.last();
        assert_eq!(path, "/v1/chat/completions");
        let sent: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(sent["max_tokens"], 150);
        // 失败轮不入账：user 先入列、失败后回滚（裁剪 -2：失败轮的 user 一并出列）
        let before = client.rr3_history().len();
        *srv.fail_path.lock().unwrap() = Some("/v1/chat/completions".into());
        let e = client.rr3_complete("会失败").unwrap_err();
        assert_eq!(e, "大模型接口 HTTP 500");
        *srv.fail_path.lock().unwrap() = None;
        assert_eq!(client.rr3_history().len(), before - 2);
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// LLM：200 坏体折「形状不符」且不入账；local 行免鉴权（无 Authorization 头）
    #[test]
    fn llm_bad_shape_folds_and_local_mode_omits_auth() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        // real 档 key 检查先于请求（对位 test_cloud.py TestTransportErrorConvergence
        // setUp 的 os.environ[API_KEY_ENV]——坏体测试也得先有 key）
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        let mut client = Rr3LlmClient::new(&srv.base, "m", 5.0, None, None);
        *srv.bad_json_path.lock().unwrap() = Some("/v1/chat/completions".into());
        let e = client.rr3_complete("在吗").unwrap_err();
        assert!(e.contains("大模型应答形状不符"), "坏体点名: {e}");
        assert_eq!(client.rr3_history().len(), 1, "失败轮不入账（user 已回滚）");
        *srv.bad_json_path.lock().unwrap() = None;
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
        // local 免鉴权：请求不带 Authorization 头，key 缺席也不报
        let mut local = Rr3LlmClient::new(&srv.base, "m", 5.0, None, Some(Rr3LlmMode::Local));
        assert_eq!(local.rr3_complete("你好").unwrap(), "好的");
        let (_, auth, _) = srv.last();
        assert_eq!(auth, "", "local 行无 Authorization 头");
    }

    /// TTS：请求体形状（voice 后缀/response_format/sample_rate）与 PCM 原样回传
    /// （对位 test_tts_returns_pcm_stream）
    #[test]
    fn tts_returns_pcm_stream() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        let client = Rr3TtsClient::new(&srv.base, "FunAudioLLM/CosyVoice2-0.5B", 5.0);
        let pcm = client.rr3_synthesize("好的", 16000).unwrap();
        assert_eq!(pcm, vec![1u8, 0].repeat(1600));
        let (path, _, body) = srv.last();
        assert_eq!(path, "/v1/audio/speech");
        let sent: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(sent["voice"], "FunAudioLLM/CosyVoice2-0.5B:bella");
        assert_eq!(sent["response_format"], "pcm");
        assert_eq!(sent["sample_rate"], 16000);
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    // ---------- mock 路径（不要求 key：显式清掉，证明不静默取 key） ----------

    #[test]
    fn mock_asr_fixed_text_without_key() {
        let _g = env_lock();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_MOCK_TEXT_ENV);
        let client = Rr3AsrClient::new("http://127.0.0.1:1", "m", 1.0);
        assert_eq!(client.rr3_transcribe(&[0u8; 4], 16000).unwrap(), RR3_MOCK_TEXT);
        // 注入键优先（批F2 判向-转向闭环的注入点）
        std::env::set_var(RR3_MOCK_TEXT_ENV, "向左");
        assert_eq!(client.rr3_transcribe(&[0u8; 4], 16000).unwrap(), "向左");
        std::env::remove_var(RR3_MOCK_TEXT_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    #[test]
    fn mock_llm_echo_and_history() {
        let _g = env_lock();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
        std::env::remove_var(RR3_API_KEY_ENV);
        let mut client = Rr3LlmClient::new("http://127.0.0.1:1", "m", 1.0, None, None);
        assert_eq!(client.rr3_complete("在吗").unwrap(), "（mock 回复）在吗");
        let history = client.rr3_history();
        assert_eq!(history.last().unwrap().0, "assistant");
        assert_eq!(history.last().unwrap().1, "（mock 回复）在吗");
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    #[test]
    fn mock_tts_sine_without_key() {
        let _g = env_lock();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
        std::env::remove_var(RR3_API_KEY_ENV);
        let client = Rr3TtsClient::new("http://127.0.0.1:1", "m", 1.0);
        let pcm = client.rr3_synthesize("hi", 16000).unwrap();
        assert!(!pcm.is_empty());
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }
}
