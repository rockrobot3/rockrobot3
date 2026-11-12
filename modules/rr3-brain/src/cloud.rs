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
//! 本仓全同步面；TLS 必须库承载）。B19 起改用进程级共享 Agent（空闲连接池）：
//! 一轮对话的 ASR/LLM/TTS 在同一 TCP＋TLS 会话内往返，不再每次调用重新握手；
//! 三家共用一条有界重试机制 [`rr3_http_try`]。与 Python 侧的三笔语义差如实登记：
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
/// ASR 语言策略（2026-10-31 实机）：短中文唤醒词偶发被云端误判语种（曾返回
/// 韩语 "안녕"），显式 `zh` 稳定中文识别；服务端若忽略该字段则无副作用。
pub(crate) const RR3_ASR_LANGUAGE: &str = "zh";
/// ASR 输入增益（2026-10-31 实机）：板麦语音 RMS 仅 ~1000/32767，云端 VAD 对
/// 短句偶发判空/幻觉；×4 实测能稳定出文本（×1/×8 为空、×2/×4 出文本）。只作用
/// 于送 ASR 的 PCM 副本，VAD/DoA/总线 PCM 均不变。
pub(crate) const RR3_ASR_INPUT_GAIN: i32 = 4;
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

/// 云端应答体策略帽（32 MiB）：ASR/LLM 应答 KB 级，TTS PCM 30s@48k 双声道约
/// 5.7MB；32 MiB 给足长语音余量，同时任何单次调用都不允许把板子内存顶穿。
const RR3_CLOUD_BODY_CAP: usize = 32 * 1024 * 1024;

/// 空闲连接池上界（连接复用机制的策略量）：三家端点同宿主不同路径，对话轮内
/// 同步串行，1 条即可复用；2 条给面板测连与对话轮并发的两路各留一条。总 4 条
/// 是"进程内至多两路并发 × 两条宿主"的上界，内存占用与连接数都可判定。
const RR3_CLOUD_IDLE_MAX: usize = 4;
const RR3_CLOUD_IDLE_PER_HOST: usize = 2;

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
        Ok(v) => rr3_cloud_mode_word(&v),
    }
}

/// 云档词表（real/mock；词表外点名拒）。
fn rr3_cloud_mode_word(v: &str) -> Result<Rr3CloudMode, String> {
    match v.trim().to_lowercase().as_str() {
        "real" => Ok(Rr3CloudMode::Real),
        "mock" => Ok(Rr3CloudMode::Mock),
        other => Err(format!(
            "{RR3_CLOUD_MODE_ENV} 取值不认: {other}（允许 real/mock）"
        )),
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
fn rr3_secrets_def_path() -> String {
    match std::env::var("HOME") {
        Ok(home) => format!("{home}/.rr3/secrets.json"),
        Err(_) => "~/.rr3/secrets.json".to_string(),
    }
}

/// secrets 文件现读 ref（面板维护面）：每次调用现读——面板填完下一轮即生效，无需
/// 重启。缺文件/缺键/坏 json/值非串一律 None，容忍与否由调用方裁定（real 档缺
/// key 报错只点名 ref，不回显任何值）。
pub(crate) fn rr3_secrets_get(reference: &str) -> Option<String> {
    let path = std::env::var(RR3_SECRETS_FILE_ENV).unwrap_or_else(|_| rr3_secrets_def_path());
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

/// 进程级共享 HTTP 代理（连接复用机制）：`ureq::post()` 顶层函数每次新建 Agent，
/// 即每次调用重做 TCP＋TLS 握手——一轮对话 ASR/LLM/TTS 三次握手，移动网络下多付
/// 数百 ms，且握手失败面按调用次数放大。共享 Agent 自带空闲连接池：同宿主请求
/// 串行复用同一条连接（keep-alive），池上界见 [`RR3_CLOUD_IDLE_MAX`]。
///
/// 失效连接语义如实登记：服务端在空闲期关闭连接后，客户端复用才发现的写失败由
/// ureq 自动换新连接重放（`send_prelude` 失败路径）；写成功而读响应前被关的窄
/// 竞态 ureq 不重放非幂等 POST 体，由调用方 [`rr3_http_try`] 的有界重试兜住。
fn rr3_http_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        eprintln!(
            "rr3-brain: 云端连接池就绪（空闲≤{RR3_CLOUD_IDLE_MAX}，每宿主≤{RR3_CLOUD_IDLE_PER_HOST}）"
        );
        ureq::AgentBuilder::new()
            .max_idle_connections(RR3_CLOUD_IDLE_MAX)
            .max_idle_connections_per_host(RR3_CLOUD_IDLE_PER_HOST)
            .build()
    })
}

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
    let req = rr3_http_agent()
        .post(url)
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
/// Content-Length、服务端发完即断，按流读尽即得）。读侧强制策略帽：云端/中间人
/// 返回超大响应时不能把板子的内存顶穿；读侧走 `take(cap+1)`，多读 1 字节
/// 专用于判"是否还有余量"，分配上界仍是 cap+1。
fn rr3_read_body(url: &str, resp: ureq::Response) -> Result<Vec<u8>, String> {
    rr3_read_capped_body(url, resp.into_reader(), RR3_CLOUD_BODY_CAP)
}

/// 带帽读体（帽参数显式化，单测用小帽钉边界）。
fn rr3_read_capped_body(url: &str, reader: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    reader
        .take(cap as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("云端不可达 {url}: {e}"))?;
    if buf.len() > cap {
        return Err(format!(
            "云端应答体超界（>{cap}B）: {url}（流式应答读尽策略帽，防内存耗尽）"
        ));
    }
    Ok(buf)
}

/// 云端调用有界重试：总尝试次数（首调 + 1 次重试，2026-10-31；TTS 同轨同批——
/// 实机唤醒 Ack 命中后 CosyVoice 读响应超时一次即弃轮，用户侧表现为"喊了没
/// 应答"，与 ASR 是同一类瞬时态）。只对**瞬时**失败重试——HTTP 429/5xx 与
/// 传输层（连接拒绝/超时/读断）；鉴权/请求形状 4xx、坏应答、缺密钥一律不重试
/// （重试只会重复同一结局，还把 401 拖成 2 倍时延）。同步调用发生在对话轮内，
/// 上限 2×`timeout_s`（当前配置 8s）——有界，不无限重试。
const RR3_CLOUD_ATTEMPTS: u32 = 2;
/// 两次尝试之间的固定短歇（对话轮内同步调用，不能长睡）。
const RR3_CLOUD_RETRY_PAUSE: Duration = Duration::from_millis(500);

/// 可重试判据（纯函数，单测钉域）：传输层「云端不可达 …」与 HTTP 429/5xx 重试；
/// 其余（4xx/坏应答/缺密钥/缺字段）不重试。`interface` 是各云端错误文案的接口
/// 前缀词（"识别接口"/"合成接口"/"大模型接口"），用于解析
/// `"{interface} HTTP {code}"`。
fn rr3_cloud_retryable(interface: &str, err: &str) -> bool {
    if err.starts_with("云端不可达") {
        return true;
    }
    let prefix = format!("{interface} HTTP ");
    match err
        .strip_prefix(&prefix)
        .and_then(|s| s.parse::<u16>().ok())
    {
        Some(code) => rr3_retry_code(code),
        None => false,
    }
}

/// 可重试状态码：限流（429）与服务端（5xx）。
fn rr3_retry_code(code: u16) -> bool {
    code == 429 || (500..=599).contains(&code)
}

/// 有界重试机制（策略由调用方注入：接口前缀词＋台账标签＋单次尝试闭包）。三家
/// 客户端共用同一循环，重试判据只有 [`rr3_cloud_retryable`] 一处——重复率与
/// 圈复杂度的唯一落点。闭包每次调用重发同一请求体（体在闭包外装配一次）。
fn rr3_http_try<T>(
    interface: &str,
    label: &str,
    mut attempt_fn: impl FnMut() -> Result<T, String>,
) -> Result<T, String> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match attempt_fn() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if rr3_cloud_retryable(interface, &e) && attempt < RR3_CLOUD_ATTEMPTS {
                    eprintln!(
                        "rr3-brain: {label} 第 {attempt}/{RR3_CLOUD_ATTEMPTS} 次失败（{e}），{}ms 后重试",
                        RR3_CLOUD_RETRY_PAUSE.as_millis()
                    );
                    std::thread::sleep(RR3_CLOUD_RETRY_PAUSE);
                    continue;
                }
                // 重试过的最终失败补尝试数（首调失败文案逐字不变——台账/单测依赖）
                return if attempt > 1 {
                    Err(format!("{e}（{label} 共尝试 {attempt} 次）"))
                } else {
                    Err(e)
                };
            }
        }
    }
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
            return Ok(rr3_mock_text());
        }
        let resp = self.rr3_transcribe_net(pcm, sample_rate)?;
        rr3_asr_text(&resp)
    }

    /// 真云路径：multipart 一次装配、有界重试，只回 200 应答体。
    fn rr3_transcribe_net(&self, pcm: &[u8], sample_rate: u32) -> Result<Vec<u8>, String> {
        let key = rr3_cloud_api_key()?;
        let boundary = rr3_boundary_hex();
        let boosted = crate::asr::rr3_asr_boost_pcm(pcm, RR3_ASR_INPUT_GAIN);
        let body = crate::asr::rr3_multipart_body(
            &self.model,
            &boosted,
            sample_rate,
            &boundary,
            Some(RR3_ASR_LANGUAGE),
        );
        let url = format!("{}/v1/audio/transcriptions", self.base_url);
        let content_type = format!("multipart/form-data; boundary={boundary}");
        // 有界重试（见 [`rr3_http_try`]）：multipart 体一次装配、多轮复用；
        // 只对瞬时失败续打，非瞬时/耗尽即按原形状回错。
        rr3_http_try("识别接口", "ASR", || {
            rr3_http_post(&url, &key, &body, &content_type, self.timeout_s).and_then(
                |(status, resp)| {
                    if status == 200 {
                        Ok(resp)
                    } else {
                        Err(format!("识别接口 HTTP {status}"))
                    }
                },
            )
        })
    }
}

/// mock 档识别文本（`RR3_BRAIN_MOCK_ASR_TEXT` 注入覆盖，空白回落固定文本）。
fn rr3_mock_text() -> String {
    std::env::var(RR3_MOCK_TEXT_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| RR3_MOCK_TEXT.to_string())
}

/// 识别应答形状：坏 JSON / 缺 text 折 Err（B-16②：不逃逸护栏）。
fn rr3_asr_text(resp: &[u8]) -> Result<String, String> {
    let data: Value = serde_json::from_slice(resp).map_err(|e| format!("识别应答非 JSON: {e}"))?;
    data.get("text")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| "识别应答缺 text 字段".to_string())
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
            Some(r) => rr3_ref_key(r),
        }
    }
}

/// 裸名 ref 取值：`env:VAR` → 环境变量；裸名 → secrets 文件（每次调用现读）。
fn rr3_ref_key(r: &str) -> Result<String, String> {
    if let Some(var) = r.strip_prefix(RR3_KEY_REF_ENV_PREFIX) {
        return rr3_env_ref_key(var, r);
    }
    rr3_secrets_get(r).ok_or_else(|| format!("secrets 缺引用 {r}（面板填 key 后下一轮生效）"))
}

/// `env:VAR` 分支取值（空值点名，不回显凭据）。
fn rr3_env_ref_key(var: &str, r: &str) -> Result<String, String> {
    let val = std::env::var(var).unwrap_or_default();
    let val = val.trim();
    if val.is_empty() {
        return Err(format!("缺少环境变量 {var}（模型行 key_ref={r}）"));
    }
    Ok(val.to_string())
}

impl Rr3LlmClient {
    /// 连通性测试（面板"测连"，ADR-0023）：mock 档不触网直接成功（`mock:true`、
    /// 0ms）；real 档发一条 `max_tokens=1` 的最小补全，只判 HTTP 2xx 与耗时。
    /// **不写对话历史**——测连是只读探针，不带任何会话副作用。
    pub(crate) fn rr3_probe(&self) -> Result<(u64, bool), String> {
        if rr3_cloud_mode()? == Rr3CloudMode::Mock {
            return Ok((0, true));
        }
        self.rr3_probe_real()
    }

    /// real 档最小补全：只判 HTTP 2xx 与耗时，不写对话历史。
    fn rr3_probe_real(&self) -> Result<(u64, bool), String> {
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
            return Ok(self.rr3_mock_reply(prompt));
        }
        let key = self.rr3_resolve_key()?;
        self.history.rr3_user_turn(prompt);
        let body = rr3_chat_body(&self.history, &self.model_id);
        // base_url = 完整 OpenAI 兼容 API 根（含版本段，模型行配置承载），客户端
        // 只拼 `/chat/completions`——GLM `/api/paas/v4`、Qwen `/compatible-mode/v1`
        // 与 DeepSeek `/v1` 同形接入，不按厂商猜版本段（ADR-0023）。
        let url = format!("{}/chat/completions", self.base_url);
        let resp = match self.rr3_llm_post(&url, &key, &body) {
            Ok(resp) => resp,
            Err(e) => {
                self.history.rr3_rollback_last();
                return Err(e);
            }
        };
        self.rr3_take_reply(&resp)
    }

    /// mock 档：回显应答入账后返回。
    fn rr3_mock_reply(&mut self, prompt: &str) -> String {
        let reply = format!("（mock 回复）{prompt}");
        self.history.rr3_user_turn(prompt);
        self.history.rr3_assistant_turn(&reply);
        reply
    }

    /// 补全真调：重试与 ASR/TTS 同轨：连接池复用引入"写成功、读前被关"的窄竞态
    /// 需要一次重放，5xx/429 同属瞬时态。代价如实登记——补全非幂等，极端竞态下
    /// 同轮可能被云端处理两次（OpenAI 兼容面无幂等键）；会话语义只采纳一次应答、
    /// 历史只入账一次，换来"问了不回"这一生产不可接受结局的收敛。
    fn rr3_llm_post(&self, url: &str, key: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        rr3_http_try("大模型接口", "LLM", || {
            rr3_http_post(url, key, body, "application/json", self.timeout_s).and_then(
                |(status, resp)| {
                    if status == 200 {
                        Ok(resp)
                    } else {
                        Err(format!("大模型接口 HTTP {status}"))
                    }
                },
            )
        })
    }

    /// 应答形状收敛 + 历史入账：取不出即回滚并报形状不符。
    fn rr3_take_reply(&mut self, resp: &[u8]) -> Result<String, String> {
        match rr3_reply_text(resp) {
            Some(t) => {
                self.history.rr3_assistant_turn(&t);
                Ok(t)
            }
            None => {
                self.history.rr3_rollback_last();
                Err(
                    "大模型应答形状不符: 缺 choices[0].message.content（或应答非 JSON）"
                        .to_string(),
                )
            }
        }
    }

    /// 对话历史在册（仅测试编译：mock/裁窗/回滚断言的观测面）。
    #[cfg(test)]
    fn rr3_history(&self) -> &[(String, String)] {
        self.history.rr3_messages()
    }
}

/// 补全请求体：历史 → OpenAI 兼容 messages + 模型行 + 回复长度上限。
fn rr3_chat_body(history: &crate::llm::Rr3LlmHistory, model: &str) -> Vec<u8> {
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
fn rr3_reply_text(resp: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(resp).ok().and_then(|v| {
        v.get("choices")?
            .get(0)?
            .get("message")?
            .get("content")?
            .as_str()
            .map(|s| s.to_string())
    })
}

/// 模型行密钥是否就位（面板模型列表只回布尔 `key_set`，值零回显）：local 免鉴权
/// 恒 true；`env:` 引用看环境变量非空；裸名看 secrets 现读；无引用走通用环境变量键。
pub(crate) fn rr3_key_set_of(key_ref: &Option<String>, mode: Option<Rr3LlmMode>) -> bool {
    if mode == Some(Rr3LlmMode::Local) {
        return true;
    }
    match key_ref {
        None => rr3_cloud_api_key().is_ok(),
        Some(r) => rr3_key_ref_set(r),
    }
}

/// 引用式 key_ref 是否就位：`env:` 看环境变量非空；裸名看 secrets 现读。
fn rr3_key_ref_set(r: &str) -> bool {
    match r.strip_prefix(RR3_KEY_REF_ENV_PREFIX) {
        Some(var) => std::env::var(var)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false),
        None => rr3_secrets_get(r).is_some(),
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
        self.rr3_tts_cloud(text, sample_rate)
    }

    /// 真云 TTS 一跳：请求体装配 + 有界重试，只回 200 应答体。
    fn rr3_tts_cloud(&self, text: &str, sample_rate: u32) -> Result<Vec<u8>, String> {
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
        // 有界重试（与 ASR/LLM 同轨，见 [`rr3_http_try`]）：CosyVoice 读响应超时
        // 是瞬时态，一次即弃会让唤醒 Ack 静默（用户喊了没应答）。
        rr3_http_try("合成接口", "TTS", || {
            rr3_http_post(&url, &key, &body, "application/json", self.timeout_s).and_then(
                |(status, resp)| {
                    if status == 200 {
                        Ok(resp)
                    } else {
                        Err(format!("合成接口 HTTP {status}"))
                    }
                },
            )
        })
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
    rr3_say_check(&path, &out)?;
    let bytes = std::fs::read(&path);
    let _ = std::fs::remove_file(&path);
    let bytes = bytes.map_err(|e| format!("读宿主合成 WAV 失败: {e}"))?;
    rr3_say_pcm_of(&bytes)
}

/// WAV data 块 → 裸 PCM（无 data 块点名）。
fn rr3_say_pcm_of(bytes: &[u8]) -> Result<Vec<u8>, String> {
    crate::tts::rr3_wav_data_chunk(bytes)
        .map(|b| b.to_vec())
        .ok_or_else(|| "宿主合成 WAV 无 data 块".to_string())
}

/// say 退出码判据：非零即清临时文件并点名 stderr（不静默降级）。
fn rr3_say_check(path: &std::path::Path, out: &std::process::Output) -> Result<(), String> {
    if out.status.success() {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    let err = String::from_utf8_lossy(&out.stderr);
    Err(format!(
        "宿主合成失败（say {}）: {}",
        out.status,
        err.trim()
    ))
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
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::RR3_TEST_ENV_LOCK;

    /// env 动作的测试互斥（全 crate 一柄，见 [`super::RR3_TEST_ENV_LOCK`]）
    pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        RR3_TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 测试 HTTP 服务收帧记账：(path, authorization, body)
    pub(crate) type FakeReqs = Arc<Mutex<Vec<(String, String, Vec<u8>)>>>;
    /// 可选路径开关（一次性 5xx / 坏 JSON / 缺字段夹具共用）
    pub(crate) type FakePath = Arc<Mutex<Option<String>>>;

    pub(crate) struct FakeCloud {
        pub(crate) base: String,
        pub(crate) requests: FakeReqs,
        pub(crate) fail_path: FakePath,
        /// 一次性 5xx（首请求 500、随即复位）——ASR 有界重试的正门夹具
        pub(crate) fail_once_path: FakePath,
        pub(crate) bad_json_path: FakePath,
        pub(crate) missing_field_path: FakePath,
        /// 累计 accept 次数（连接复用断言：N 请求 = 1 accept 即证明复用）
        pub(crate) conns: Arc<AtomicU32>,
    }

    impl FakeCloud {
        /// 默认档：应答带 `Connection: close`——每请求一条连接（对位既有断言）
        pub(crate) fn start() -> Self {
            Self::start_with(false)
        }

        /// keep-alive 档：同连接连续服务多请求（连接复用回归的观测面）
        pub(crate) fn start_keep_alive() -> Self {
            Self::start_with(true)
        }

        fn start_with(keep_alive: bool) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let fail_path = Arc::new(Mutex::new(None));
            let fail_once_path = Arc::new(Mutex::new(None));
            let bad_json_path = Arc::new(Mutex::new(None));
            let missing_field_path = Arc::new(Mutex::new(None));
            let conns = Arc::new(AtomicU32::new(0));
            let (r, f, fo, b, m, c) = (
                requests.clone(),
                fail_path.clone(),
                fail_once_path.clone(),
                bad_json_path.clone(),
                missing_field_path.clone(),
                conns.clone(),
            );
            // 线程随测试进程退出回收（cargo test 主线程等所有 #[test] 完成，本
            // detached 线程不阻塞收尾；每测试独立端口，线程间零干扰）
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    c.fetch_add(1, Ordering::Relaxed);
                    // keep-alive 档在同一条流上连续读数个请求；读到 EOF 即收
                    while let Some((path, headers, body)) = read_request(&mut stream) {
                        r.lock().unwrap().push((
                            path.clone(),
                            headers.get("authorization").cloned().unwrap_or_default(),
                            body,
                        ));
                        let fail = f.lock().unwrap().clone();
                        let fail_once = fo.lock().unwrap().clone();
                        let bad = b.lock().unwrap().clone();
                        let missing = m.lock().unwrap().clone();
                        let (status, payload): (&str, Vec<u8>) = if Some(&path)
                            == fail_once.as_ref()
                        {
                            *fo.lock().unwrap() = None; // 一次性：下一请求恢复罐头
                            ("500 Internal Server Error", br#"{"error":"boom"}"#.to_vec())
                        } else if Some(&path) == fail.as_ref() {
                            ("500 Internal Server Error", br#"{"error":"boom"}"#.to_vec())
                        } else if Some(&path) == bad.as_ref() {
                            // 200 + 非 JSON 体（B-16②：云 200 坏体的静默假成功口）
                            ("200 OK", b"not-json-at-all".to_vec())
                        } else if Some(&path) == missing.as_ref() {
                            ("200 OK", b"{}".to_vec())
                        } else if path == "/v1/audio/transcriptions" {
                            ("200 OK", "{\"text\": \"你好\"}".as_bytes().to_vec())
                        } else if path == "/v1/chat/completions" {
                            (
                                "200 OK",
                                json!({"choices": [{"message": {"role": "assistant", "content": "好的"}}]})
                                    .to_string()
                                    .into_bytes(),
                            )
                        } else {
                            // /v1/audio/speech → 原始 PCM
                            ("200 OK", [1u8, 0].repeat(1600))
                        };
                        respond(&mut stream, status, &payload, keep_alive);
                        if !keep_alive {
                            break;
                        }
                    }
                }
            });
            Self {
                base: format!("http://127.0.0.1:{port}"),
                requests,
                fail_path,
                fail_once_path,
                bad_json_path,
                missing_field_path,
                conns,
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

    fn respond(stream: &mut TcpStream, status: &str, body: &[u8], keep_alive: bool) {
        let conn = if keep_alive { "keep-alive" } else { "close" };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {conn}\r\n\r\n",
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

    /// 应答体策略帽（本轮审计）：帽内原样返回，超帽 1 字节即拒——云端/中间人
    /// 返回超大响应时不得把板子内存顶穿。
    #[test]
    fn response_body_cap_bounds_memory() {
        let ok = vec![7u8; 16];
        assert_eq!(
            rr3_read_capped_body("u", std::io::Cursor::new(ok.clone()), 16).unwrap(),
            ok
        );
        let e = rr3_read_capped_body("u", std::io::Cursor::new(vec![7u8; 17]), 16)
            .expect_err("超帽 1 字节必须拒");
        assert!(e.contains("应答体超界"), "{e}");
    }

    /// boundary 32 位小写 hex、两次调用不同
    #[test]
    fn boundary_hex_shape() {
        let a = rr3_boundary_hex();
        let b = rr3_boundary_hex();
        assert_eq!(a.len(), 32);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
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
        std::fs::write(
            &path,
            r#"{"keys": {"DeepSeek官方": "  sk-test  ", "空": "  "}}"#,
        )
        .unwrap();
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
        let e = rr3_http_post(
            "http://127.0.0.1:1/v1/audio/transcriptions",
            "k",
            b"x",
            "text/plain",
            1.0,
        )
        .unwrap_err();
        assert!(e.contains("云端不可达"), "传输失败折固定文案: {e}");
        assert!(!e.contains("Bearer"), "文案不得带出密钥");
    }

    /// 连接复用（B19）：keep-alive 服务端连续两请求只 accept 一次——进程级共享
    /// Agent 的池化生效（改造前 `ureq::post` 每次新建 Agent，必然两条连接）。
    #[test]
    fn http_post_reuses_pooled_connection() {
        let srv = FakeCloud::start_keep_alive();
        let url = format!("{}/v1/chat/completions", srv.base);
        for _ in 0..2 {
            let (status, _) = rr3_http_post(&url, "k", b"{}", "application/json", 5.0).unwrap();
            assert_eq!(status, 200);
        }
        assert_eq!(srv.requests.lock().unwrap().len(), 2, "两请求都应到达");
        assert_eq!(
            srv.conns.load(Ordering::Relaxed),
            1,
            "两请求应复用同一条 TCP 连接"
        );
    }

    /// 云端重试判据（2026-10-31 有界重试，ASR/LLM/TTS 同轨）：429/5xx/传输层可重试；
    /// 4xx/坏应答/缺密钥不重试（重试只会重复同一结局，还把 401 拖成 2 倍时延）。
    #[test]
    fn cloud_error_retry_classification() {
        for (iface, e) in [
            ("识别接口", "识别接口 HTTP 429"),
            ("识别接口", "识别接口 HTTP 500"),
            ("识别接口", "识别接口 HTTP 502"),
            ("识别接口", "识别接口 HTTP 503"),
            (
                "识别接口",
                "云端不可达 http://x: timed out reading response",
            ),
            ("合成接口", "合成接口 HTTP 429"),
            ("合成接口", "合成接口 HTTP 500"),
            ("合成接口", "合成接口 HTTP 503"),
            (
                "合成接口",
                "云端不可达 http://x: timed out reading response",
            ),
        ] {
            assert!(rr3_cloud_retryable(iface, e), "应重试: {e}");
        }
        for (iface, e) in [
            ("识别接口", "识别接口 HTTP 400"),
            ("识别接口", "识别接口 HTTP 401"),
            ("识别接口", "识别接口 HTTP 403"),
            ("识别接口", "识别应答非 JSON: expected value at line 1"),
            ("识别接口", "识别应答缺 text 字段"),
            ("识别接口", "缺少环境变量 RR3_CLOUD_API_KEY，云端链路不可用"),
            // 接口前缀词错配不重试：识别文案不能拿合成前缀解析，反之亦然
            ("合成接口", "识别接口 HTTP 500"),
            ("识别接口", "合成接口 HTTP 500"),
            ("合成接口", "合成应答非 JSON: expected value at line 1"),
            ("合成接口", "缺少环境变量 RR3_CLOUD_API_KEY，云端链路不可用"),
        ] {
            assert!(!rr3_cloud_retryable(iface, e), "不应重试: {e}");
        }
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
        let text = client
            .rr3_transcribe(&b"\x00\x01".repeat(16), 16000)
            .unwrap();
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
        assert_eq!(
            srv.requests.lock().unwrap().len(),
            1,
            "坏应答不重试（只打一次）"
        );
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// ASR 有界重试（2026-10-31）：首请求 500 → 500ms 后重试一次 → 第二次 200
    /// 成功；请求数恰 2（有界，不无限重试）。
    #[test]
    fn asr_retries_5xx_once_then_succeeds() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        *srv.fail_once_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
        let client = Rr3AsrClient::new(&srv.base, "m", 5.0);
        assert_eq!(
            client
                .rr3_transcribe(&b"\x00\x00".repeat(100), 16000)
                .unwrap(),
            "你好"
        );
        assert_eq!(
            srv.requests.lock().unwrap().len(),
            2,
            "首调 + 1 次重试恰两次（有界）"
        );
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// 5xx 持续失败：有界重试只多打一次（总 2 次），最终错误回带尝试数。
    #[test]
    fn asr_5xx_exhausts_bounded_attempts() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        *srv.fail_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
        let client = Rr3AsrClient::new(&srv.base, "m", 5.0);
        let e = client
            .rr3_transcribe(&b"\x00\x00".repeat(100), 16000)
            .unwrap_err();
        assert!(e.contains("识别接口 HTTP 500"), "{e}");
        assert!(e.contains("ASR 共尝试 2 次"), "{e}");
        assert_eq!(
            srv.requests.lock().unwrap().len(),
            2,
            "有界：总尝试 2 次后不再打"
        );
        std::env::remove_var(RR3_API_KEY_ENV);
        std::env::remove_var(RR3_CLOUD_MODE_ENV);
    }

    /// LLM 有界重试（B19，与 ASR/TTS 同轨）：首请求 500 → 重试一次 → 200；
    /// 请求数恰 2，历史只入账一轮（重试不产生额外对话轮）。
    #[test]
    fn llm_retries_5xx_once_then_succeeds() {
        let _g = env_lock();
        let srv = FakeCloud::start();
        std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
        std::env::set_var(RR3_API_KEY_ENV, "test-key");
        *srv.fail_once_path.lock().unwrap() = Some("/v1/chat/completions".into());
        let llm_root = format!("{}/v1", srv.base);
        let mut client = Rr3LlmClient::new(&llm_root, "m", 5.0, None, None);
        assert_eq!(client.rr3_complete("在吗").unwrap(), "好的");
        assert_eq!(
            srv.requests.lock().unwrap().len(),
            2,
            "首调 + 1 次重试（有界）"
        );
        assert_eq!(
            client.rr3_history().len(),
            3,
            "system + user + assistant 各一条（重试不额外入账）"
        );
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
        assert_eq!(pcm, [1u8, 0].repeat(1600));
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
