//! HTTP 传输（共享 Agent/读体策略帽）与 asr/llm/tts 三家云客户端、multipart 随机源。

use std::io::Read;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::json;

use crate::boot::Rr3LlmMode;

use super::cfg::{
    rr3_cloud_api_key, rr3_cloud_mode, rr3_ref_key, Rr3CloudMode, RR3_ASR_INPUT_GAIN,
    RR3_ASR_LANGUAGE, RR3_SAY_VOICE, RR3_SYSTEM_PROMPT, RR3_TTS_BACKEND_ENV,
    RR3_TTS_BACKEND_MAC_SAY,
};
use super::payload::{rr3_asr_text, rr3_chat_body, rr3_mock_text, rr3_ping_body, rr3_reply_text};
use super::retry::{rr3_http_try, rr3_ok_resp};

/// 云端应答体策略帽（32 MiB）：ASR/LLM 应答 KB 级，TTS PCM 30s@48k 双声道约
/// 5.7MB；32 MiB 给足长语音余量，同时任何单次调用都不允许把板子内存顶穿。
const RR3_CLOUD_BODY_CAP: usize = 32 * 1024 * 1024;

/// 空闲连接池上界（连接复用机制的策略量）：三家端点同宿主不同路径，对话轮内
/// 同步串行，1 条即可复用；2 条给面板测连与对话轮并发的两路各留一条。总 4 条
/// 是"进程内至多两路并发 × 两条宿主"的上界，内存占用与连接数都可判定。
const RR3_CLOUD_IDLE_MAX: usize = 4;
const RR3_CLOUD_IDLE_PER_HOST: usize = 2;

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
    let (status, resp) = match req.send_bytes(body) {
        Ok(resp) => (resp.status(), resp),
        // 4xx/5xx：urllib HTTPError 对位——状态码与错误体照回（调用方折业务失败）
        Err(ureq::Error::Status(code, resp)) => (code, resp),
        Err(e) => return Err(format!("云端不可达 {url}: {e}")),
    };
    Ok((status, rr3_read_body(url, resp)?))
}
/// 应答体读尽（read-until-close 语义对 urllib resp.read() 同形状——TTS 响应无
/// Content-Length、服务端发完即断，按流读尽即得）。读侧强制策略帽：云端/中间人
/// 返回超大响应时不能把板子的内存顶穿；读侧走 `take(cap+1)`，多读 1 字节
/// 专用于判"是否还有余量"，分配上界仍是 cap+1。
fn rr3_read_body(url: &str, resp: ureq::Response) -> Result<Vec<u8>, String> {
    rr3_read_capped_body(url, resp.into_reader(), RR3_CLOUD_BODY_CAP)
}

/// 带帽读体（帽参数显式化，单测用小帽钉边界）。
pub(crate) fn rr3_read_capped_body(
    url: &str,
    reader: impl Read,
    cap: usize,
) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    rr3_read_take(url, reader, cap, &mut buf)?;
    if buf.len() > cap {
        return Err(format!(
            "云端应答体超界（>{cap}B）: {url}（流式应答读尽策略帽，防内存耗尽）"
        ));
    }
    Ok(buf)
}

/// 带帽读尽：`take(cap+1)` 多读 1 字节专门用于判"是否还有余量"。
fn rr3_read_take(
    url: &str,
    reader: impl Read,
    cap: usize,
    buf: &mut Vec<u8>,
) -> Result<(), String> {
    reader
        .take(cap as u64 + 1)
        .read_to_end(buf)
        .map_err(|e| format!("云端不可达 {url}: {e}"))?;
    Ok(())
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
        let (url, body, content_type) = self.rr3_asr_wire(pcm, sample_rate);
        // 有界重试（见 [`rr3_http_try`]）：multipart 体一次装配、多轮复用；
        // 只对瞬时失败续打，非瞬时/耗尽即按原形状回错。
        rr3_http_try("识别接口", "ASR", || {
            rr3_http_post(&url, &key, &body, &content_type, self.timeout_s)
                .and_then(|(status, resp)| rr3_ok_resp("识别接口", status, resp))
        })
    }

    /// 识别请求面装配：boundary/输入增益/WAV 体/URL/类型一次成型。
    fn rr3_asr_wire(&self, pcm: &[u8], sample_rate: u32) -> (String, Vec<u8>, String) {
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
        (url, body, content_type)
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
        self.key_ref
            .as_deref()
            .map_or_else(rr3_cloud_api_key, rr3_ref_key)
    }
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
        let body = rr3_ping_body(&self.model_id);
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
        let resp = self
            .rr3_llm_post(&url, &key, &body)
            .inspect_err(|_| self.history.rr3_rollback_last())?;
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
            rr3_http_post(url, key, body, "application/json", self.timeout_s)
                .and_then(|(status, resp)| rr3_ok_resp("大模型接口", status, resp))
        })
    }

    /// 应答形状收敛 + 历史入账：取不出即回滚并报形状不符。
    fn rr3_take_reply(&mut self, resp: &[u8]) -> Result<String, String> {
        let Some(t) = rr3_reply_text(resp) else {
            return Err(self.rr3_shape_fail());
        };
        self.history.rr3_assistant_turn(&t);
        Ok(t)
    }

    /// 应答形状不符：回滚本轮 user 入账并回拒因文案（历史不入账）。
    fn rr3_shape_fail(&mut self) -> String {
        self.history.rr3_rollback_last();
        "大模型应答形状不符: 缺 choices[0].message.content（或应答非 JSON）".to_string()
    }

    /// 对话历史在册（仅测试编译：mock/裁窗/回滚断言的观测面）。
    #[cfg(test)]
    pub(crate) fn rr3_history(&self) -> &[(String, String)] {
        self.history.rr3_messages()
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
        match rr3_tts_local(text, sample_rate)? {
            Some(pcm) => Ok(pcm),
            None => self.rr3_tts_cloud(text, sample_rate),
        }
    }

    /// 真云 TTS 一跳：请求体装配 + 有界重试，只回 200 应答体。
    fn rr3_tts_cloud(&self, text: &str, sample_rate: u32) -> Result<Vec<u8>, String> {
        let key = rr3_cloud_api_key()?;
        let body = self.rr3_tts_body(text, sample_rate);
        let url = format!("{}/v1/audio/speech", self.base_url);
        // 有界重试（与 ASR/LLM 同轨，见 [`rr3_http_try`]）：CosyVoice 读响应超时
        // 是瞬时态，一次即弃会让唤醒 Ack 静默（用户喊了没应答）。
        rr3_http_try("合成接口", "TTS", || {
            rr3_http_post(&url, &key, &body, "application/json", self.timeout_s)
                .and_then(|(status, resp)| rr3_ok_resp("合成接口", status, resp))
        })
    }

    /// 合成请求体（模型/文本/音色/格式/采样率）。
    fn rr3_tts_body(&self, text: &str, sample_rate: u32) -> Vec<u8> {
        json!({
            "model": self.model,
            "input": text,
            "voice": crate::tts::rr3_tts_voice(&self.model),
            "response_format": "pcm",
            "sample_rate": sample_rate,
        })
        .to_string()
        .into_bytes()
    }
}

/// 宿主合成临时文件序号（同进程内多次合成不撞名）
static RR3_SAY_SEQ: AtomicU32 = AtomicU32::new(0);

/// 宿主 `say` 合成（仅 `RR3_TTS_BACKEND=mac_say` 档）：一次写临时 WAV（请求
/// 采样率的 s16le 单声道）→ 抽 `data` 块 → 返回裸 PCM。退出非零/文件缺失/
/// 无 data 块一律 Err（文案带 stderr，不静默降级）。
fn rr3_say_pcm(text: &str, sample_rate: u32) -> Result<Vec<u8>, String> {
    let path = rr3_say_path();
    let out = rr3_say_run(text, sample_rate, &path)?;
    rr3_say_check(&path, &out)?;
    let bytes = std::fs::read(&path);
    let _ = std::fs::remove_file(&path);
    let bytes = bytes.map_err(|e| format!("读宿主合成 WAV 失败: {e}"))?;
    rr3_say_pcm_of(&bytes)
}

/// 免网兜底两档（mock 正弦 / 宿主 `say`）；real 云档回 None 交调用方触网。
fn rr3_tts_local(text: &str, sample_rate: u32) -> Result<Option<Vec<u8>>, String> {
    if rr3_cloud_mode()? == Rr3CloudMode::Mock {
        return Ok(Some(crate::tts::rr3_sine_pcm(text, sample_rate)));
    }
    if std::env::var(RR3_TTS_BACKEND_ENV).as_deref() == Ok(RR3_TTS_BACKEND_MAC_SAY) {
        return rr3_say_pcm(text, sample_rate).map(Some);
    }
    Ok(None)
}

/// 宿主合成临时文件路径（同进程内多次合成不撞名）。
fn rr3_say_path() -> std::path::PathBuf {
    let seq = RR3_SAY_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("rr3-tts-{}-{seq}.wav", std::process::id()))
}

/// 宿主 `say` 一次执行（参数面：音色/波形格式/采样率/输出文件/文本）。
fn rr3_say_run(
    text: &str,
    sample_rate: u32,
    path: &std::path::Path,
) -> Result<std::process::Output, String> {
    std::process::Command::new("/usr/bin/say")
        .arg("-v")
        .arg(RR3_SAY_VOICE)
        .arg("--file-format=WAVE")
        .arg(format!("--data-format=LEI16@{sample_rate}"))
        .arg("-o")
        .arg(path)
        .arg(text)
        .output()
        .map_err(|e| format!("宿主合成不可用（/usr/bin/say）: {e}"))
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
