//! 板侧云 TTS 客户端（W4-2b）：NVS 凭据 → HTTPS `/v1/audio/speech` → 播放队列。
//!
//! 契约（docs/process/dev/单板全链云API契约-2026-11-19.md）：
//! - 只请求 `response_format=pcm`、16 kHz；不做 MP3/AAC 解码；
//! - 单次 8 s 超时、最多 2 次尝试、500 ms 间隔；429/5xx/传输失败可重试；
//! - key 只存 NVS、永不出日志/回显（零回显墙同 `cred` 模块）；
//! - 传输层用 IDF `esp_http_client` + mbedTLS 证书束，不引 ureq/rustls。
//!
//! 线程模型：一条常驻 worker（24 KB 栈——TLS 握手栈需求高于普通 IO 线程），
//! 有界文本队列（容量 2）→ 读配置 → 拉 PCM → 交 [`crate::playout::rr3_play`]。
//! 播放面负责半双工门与 `audio-out` 心跳；本模块只管取数与错误点名。
//!
//! 凭据写口（`POST /api/board/cloud`）与取数共用 [`nvs_get`]/[`nvs_set`]；
//! NVS 键名 ≤15 B（IDF 硬限），值上限见 `MAX_*`。

use std::ffi::CString;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_sys::{
    nvs_close, nvs_commit, nvs_get_str, nvs_handle_t, nvs_open, nvs_open_mode_t_NVS_READONLY,
    nvs_open_mode_t_NVS_READWRITE, nvs_set_str, ESP_OK,
};
use serde_json::Value;

use crate::cred::RR3_CRED_NAMESPACE;
use crate::state::Bus;

mod http;
pub(crate) use http::{rr3_post_retry, rr3_tts_fetch, MAX_JSON};

/// NVS 键（≤15 B）：base URL / key / TTS 模型行 / 音色 / ASR 与 LLM 模型行。
const K_BASE: &str = "cloud.base";
const K_KEY: &str = "cloud.key";
const K_MODEL: &str = "tts.model";
const K_VOICE: &str = "tts.voice";
/// W6-2：对话链两跳的模型行（供应商模型名归配置，不编译进固件）。
const K_ASR_MODEL: &str = "asr.model";
const K_LLM_MODEL: &str = "llm.model";
/// LLM 基址（**完整 API 根，含版本段**；与 cloud.base 主机根不同义——Pi 侧
/// `[rr3-brain.model].base_url` 同口径）。缺席时回退 `{cloud.base}/v1`。
const K_LLM_BASE: &str = "llm.base";
/// 值上限（写口与读口同门；超形即拒，防 NVS 被大值撑爆）。
const MAX_BASE: usize = 128;
const MAX_KEY: usize = 160;
const MAX_MODEL: usize = 64;
const MAX_VOICE: usize = 64;
/// 单条朗读文本上限（与 `/say` body 的 512 B 上界同源）。
const MAX_TEXT: usize = 512;
/// TTS PCM 响应上限：512 KB ≈ 16 s @16k mono s16（2026-11-20 真板实锤：LLM
/// 45 字回复的 CosyVoice PCM ≈9 s/288 KB，旧 256 KB 上限把正常回复判失败）。
const MAX_PCM: usize = 524_288;
/// 待朗读队列容量（条；满即拒，不阻塞 HTTP 任务）。
const TTS_Q_CAP: usize = 2;
/// worker 栈：mbedTLS 握手 + esp_http_client 缓冲。
const TTS_STACK: usize = 24_576;
/// worker 空转周期（只为及时退出，不承担心跳——dialog 归 W6）。
const IDLE_MS: u64 = 2_000;

/// 云端 worker 作业（TTS 朗读 / 对话整轮；同一条 TLS 线程，省一份 24 KB 栈）。
pub(crate) enum Rr3CloudJob {
    Say(String),
    Turn(Vec<i16>),
}

/// 云端 worker 发送端（未装配 = None）。
static TTS_TX: Mutex<Option<SyncSender<Rr3CloudJob>>> = Mutex::new(None);

/// 云 TTS 配置（key 私有——零回显：不派生 `Debug`）。
struct Rr3CloudCfg {
    base: String,
    key: String,
    model: String,
    voice: String,
}

impl Rr3CloudCfg {
    /// 端点 URL（base 尾部斜杠容错）。
    fn url(&self) -> String {
        format!("{}/v1/audio/speech", self.base.trim_end_matches('/'))
    }
}

/// 写口 body：base/model/voice 必填，key/asr_model/llm_model 可选（缺席＝保留旧值）。
struct Rr3CloudWrite {
    base: String,
    model: String,
    voice: String,
    key: Option<String>,
    asr_model: Option<String>,
    llm_model: Option<String>,
    llm_base: Option<String>,
}

/// 对话链配置（W6：ASR/LLM 两跳；key 零回显，不派生 Debug）。
pub(crate) struct Rr3DialogCfg {
    /// 主机根（ASR 拼 `/v1/...`）。
    pub(crate) base: String,
    /// 完整 API 根（LLM 拼 `/chat/completions`）。
    pub(crate) llm_base: String,
    pub(crate) key: String,
    pub(crate) asr_model: String,
    pub(crate) llm_model: String,
}

// ---------------------------------------------------------------------------
// NVS 读写（namespace 与 WiFi 凭据同库；键名级点名，永不出值）
// ---------------------------------------------------------------------------

/// 单键读出：缺失/超形/坏 UTF-8 一律 None（fail-honest，不猜）。
fn nvs_get(key: &str, max: usize) -> Option<String> {
    let Ok(ns) = CString::new(RR3_CRED_NAMESPACE) else {
        return None;
    };
    let Ok(k) = CString::new(key) else {
        return None;
    };
    let mut h: nvs_handle_t = 0;
    // SAFETY：h 为本地零初始化句柄槽；ns/k 活到调用结束；open 失败不解引用 h。
    let opened = unsafe { nvs_open(ns.as_ptr(), nvs_open_mode_t_NVS_READONLY, &mut h) } == ESP_OK;
    let mut buf = vec![0u8; max + 1];
    let mut len = buf.len();
    // SAFETY：h 由刚成功的 open 写入；缓冲按 max+1 可写；长度指针初值为容量。
    let read = opened
        && unsafe { nvs_get_str(h, k.as_ptr(), buf.as_mut_ptr() as *mut _, &mut len) } == ESP_OK;
    // SAFETY：无论读成败，句柄都由本函数成对关闭。
    let _ = opened.then(|| unsafe { nvs_close(h) });
    read.then(|| {
        let end = buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(len.min(buf.len()));
        String::from_utf8(buf[..end].to_vec()).ok()
    })
    .flatten()
}

/// 单键写入 + commit（值含 NUL/写失败/commit 失败都回错误码）。
fn nvs_set(key: &str, val: &str) -> Result<(), i32> {
    let Ok(ns) = CString::new(RR3_CRED_NAMESPACE) else {
        return Err(-1);
    };
    let Ok(k) = CString::new(key) else {
        return Err(-1);
    };
    let Ok(v) = CString::new(val) else {
        return Err(-1);
    };
    let mut h: nvs_handle_t = 0;
    // SAFETY：h 本地零初始化；ns/k/v 活到调用结束；open 成功才进入写路径。
    let opened = unsafe { nvs_open(ns.as_ptr(), nvs_open_mode_t_NVS_READWRITE, &mut h) } == ESP_OK;
    let true = opened else { return Err(-3) };
    // SAFETY：h 已成功打开；键值均为有效 C 串。
    let r = unsafe { nvs_set_str(h, k.as_ptr(), v.as_ptr()) };
    // SAFETY：h 有效；commit 失败回原错误码。
    let r = if r == ESP_OK {
        unsafe { nvs_commit(h) }
    } else {
        r
    };
    // SAFETY：句柄成对关闭。
    unsafe { nvs_close(h) };
    (r == ESP_OK).then_some(()).ok_or_else(|| {
        // 键名级点名（不出值），err 码供真板排障对表 IDF nvs.h。
        eprintln!("[cloud] NVS 写 {key} 失败 err={r:#x}");
        r
    })
}

/// 读云 TTS 配置（每次取数现读——面板写入下一轮生效，不缓存）。
fn rr3_cloud_cfg() -> Result<Rr3CloudCfg, &'static str> {
    let Some(base) = nvs_get(K_BASE, MAX_BASE) else {
        return Err("缺 cloud.base");
    };
    let Some(model) = nvs_get(K_MODEL, MAX_MODEL) else {
        return Err("缺 tts.model");
    };
    let Some(voice) = nvs_get(K_VOICE, MAX_VOICE) else {
        return Err("缺 tts.voice");
    };
    let key = nvs_get(K_KEY, MAX_KEY).unwrap_or_default();
    let ok = base.starts_with("https://") && !model.is_empty() && !voice.is_empty();
    let true = ok else {
        return Err("云配置不完整或 base 非 https");
    };
    Ok(Rr3CloudCfg {
        base,
        key,
        model,
        voice,
    })
}

/// 读对话链配置（ASR/LLM；与 TTS 同库同 base，key 可空＝local 免鉴权）。
pub(crate) fn rr3_dialog_cfg() -> Result<Rr3DialogCfg, &'static str> {
    let Some(base) = nvs_get(K_BASE, MAX_BASE) else {
        return Err("缺 cloud.base");
    };
    let Some(asr_model) = nvs_get(K_ASR_MODEL, MAX_MODEL) else {
        return Err("缺 asr.model");
    };
    let Some(llm_model) = nvs_get(K_LLM_MODEL, MAX_MODEL) else {
        return Err("缺 llm.model");
    };
    // LLM 基址：显式键优先，缺席回退主机根 + /v1（SiliconFlow 等常见同源部署）。
    let llm_base = nvs_get(K_LLM_BASE, MAX_BASE)
        .unwrap_or_else(|| format!("{}/v1", base.trim_end_matches('/')));
    let ok = base.starts_with("https://")
        && llm_base.starts_with("https://")
        && !asr_model.is_empty()
        && !llm_model.is_empty();
    let true = ok else {
        return Err("对话配置不完整或 base 非 https");
    };
    Ok(Rr3DialogCfg {
        base,
        llm_base,
        key: nvs_get(K_KEY, MAX_KEY).unwrap_or_default(),
        asr_model,
        llm_model,
    })
}

// ---------------------------------------------------------------------------
// 写口：body 解析 / 形制 / 落 NVS
// ---------------------------------------------------------------------------

/// JSON 取串字段（缺失/非串统一错误）。
fn rr3_field<'a>(v: &'a Value, k: &str) -> Result<&'a str, &'static str> {
    v.get(k).and_then(Value::as_str).ok_or("字段缺席或非串")
}

/// JSON 可选串字段：缺席＝None，非串＝Err。
fn rr3_opt_field<'a>(v: &'a Value, k: &str) -> Result<Option<&'a str>, &'static str> {
    match v.get(k) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err("字段非串"),
    }
}

/// 写口 body 解析：字节面 JSON 门（纯函数，单测钉域）。
fn rr3_cloud_body(body: &[u8]) -> Result<Rr3CloudWrite, &'static str> {
    let v: Value = serde_json::from_slice(body).map_err(|_| "body 非 JSON")?;
    rr3_cloud_write(&v)
}

/// 写口字段面：三必填 + 四可选（缺席保留旧值）。
fn rr3_cloud_write(v: &Value) -> Result<Rr3CloudWrite, &'static str> {
    Ok(Rr3CloudWrite {
        base: rr3_field(v, "base")?.to_string(),
        model: rr3_field(v, "model")?.to_string(),
        voice: rr3_field(v, "voice")?.to_string(),
        key: rr3_opt_field(v, "key")?.map(str::to_string),
        asr_model: rr3_opt_field(v, "asr_model")?.map(str::to_string),
        llm_model: rr3_opt_field(v, "llm_model")?.map(str::to_string),
        llm_base: rr3_opt_field(v, "llm_base")?.map(str::to_string),
    })
}

/// 形制门（清单式：https 前缀、非空、长度上限；key 可缺省）。
fn rr3_cloud_check(c: &Rr3CloudWrite) -> Result<(), &'static str> {
    let checks = [
        c.base.starts_with("https://"),
        c.base.len() <= MAX_BASE,
        !c.model.is_empty(),
        c.model.len() <= MAX_MODEL,
        !c.voice.is_empty(),
        c.voice.len() <= MAX_VOICE,
        c.key.as_ref().is_none_or(|k| k.len() <= MAX_KEY),
        rr3_opt_len_ok(&c.asr_model),
        rr3_opt_len_ok(&c.llm_model),
        c.llm_base
            .as_ref()
            .is_none_or(|b| b.starts_with("https://") && b.len() <= MAX_BASE),
    ];
    let true = checks.iter().all(|b| *b) else {
        return Err("云配置形制不合（https/长度门）");
    };
    Ok(())
}

/// 可选模型行形制：缺席合法；在场须非空且 ≤ [`MAX_MODEL`]。
fn rr3_opt_len_ok(v: &Option<String>) -> bool {
    v.as_ref()
        .is_none_or(|s| !s.is_empty() && s.len() <= MAX_MODEL)
}

/// 落 NVS：三必填键 + 可选 key/asr_model/llm_model（缺席保留旧值）。
fn rr3_cloud_store(c: &Rr3CloudWrite) -> Result<(), i32> {
    for (key, val) in [
        (K_BASE, Some(c.base.as_str())),
        (K_MODEL, Some(c.model.as_str())),
        (K_VOICE, Some(c.voice.as_str())),
        (K_KEY, c.key.as_deref()),
        (K_ASR_MODEL, c.asr_model.as_deref()),
        (K_LLM_MODEL, c.llm_model.as_deref()),
        (K_LLM_BASE, c.llm_base.as_deref()),
    ] {
        if let Some(val) = val {
            let Ok(()) = nvs_set(key, val) else {
                return Err(-2);
            };
        }
    }
    Ok(())
}

/// 写口总入口（handler 只做 token/应答）：解析 → 形制 → NVS。
pub(crate) fn rr3_cloud_write(body: &[u8]) -> Result<(), &'static str> {
    let req = rr3_cloud_body(body)?;
    rr3_cloud_check(&req)?;
    let Ok(()) = rr3_cloud_store(&req) else {
        return Err("NVS 写入失败（串口有 err 码）");
    };
    println!(
        "[cloud] 配置已写入 NVS（key={}，asr_model={}，llm_model={}，llm_base={}）",
        req.key.as_ref().map_or("保留", |_| "更新"),
        req.asr_model.as_ref().map_or("保留", |_| "更新"),
        req.llm_model.as_ref().map_or("保留", |_| "更新"),
        req.llm_base.as_ref().map_or("缺省/保留", |_| "更新")
    );
    Ok(())
}

/// `/api/board/say` 的 body 解析（text 非空且 ≤512 B）。
pub(crate) fn rr3_say_body(body: &[u8]) -> Result<String, &'static str> {
    let v: Value = serde_json::from_slice(body).map_err(|_| "body 非 JSON")?;
    let text = rr3_field(&v, "text")?;
    let ok = !text.is_empty() && text.len() <= MAX_TEXT;
    let true = ok else {
        return Err("text 空或超 512B");
    };
    Ok(text.to_string())
}

// ---------------------------------------------------------------------------
// worker（有界作业队列：TTS 朗读 / 对话整轮；单条 TLS 线程）
// ---------------------------------------------------------------------------

/// 对话整轮策略入口（由装配面注入，cloud 只持机制：队列/传输/线程）。
pub(crate) type Rr3TurnFn = fn(&Arc<Mutex<Bus>>, Vec<i16>);

/// 起云端 worker（httpd 之前调用，避免 `/say` 与 `/dialog` 撞"未装配"窗）。
/// `turn` 由调用方注入（`dialog::rr3_turn`）——cloud 不反向依赖策略层。
pub(crate) fn spawn_tts(bus: Arc<Mutex<Bus>>, turn: Rr3TurnFn) {
    let (tx, rx) = sync_channel(TTS_Q_CAP);
    let _ = TTS_TX.lock().map(|mut slot| *slot = Some(tx));
    println!("[cloud] 云端 worker 起（TTS 朗读 + 对话整轮；单条 TLS 栈）");
    let _ = thread::Builder::new()
        .name("cloud-dialog".into())
        .stack_size(TTS_STACK)
        .spawn(move || cloud_run(rx, bus, turn))
        .map_err(|e| println!("[cloud] 起云端线程失败: {e:?}"));
}

/// 入队一条朗读请求（满/未装配/线程退出都如实回错，绝不阻塞 HTTP 任务）。
pub(crate) fn rr3_say(text: String) -> Result<(), &'static str> {
    rr3_enqueue(Rr3CloudJob::Say(text))
}

/// 入队一整轮对话（mic 切段 / `/api/board/dialog` 共用；不阻塞调用方）。
pub(crate) fn rr3_turn(pcm: Vec<i16>) -> Result<(), &'static str> {
    rr3_enqueue(Rr3CloudJob::Turn(pcm))
}

/// 公共入队（错误文案按 job 面口径统一）。
fn rr3_enqueue(job: Rr3CloudJob) -> Result<(), &'static str> {
    let Ok(slot) = TTS_TX.lock() else {
        return Err("云线程锁中毒");
    };
    let Some(tx) = slot.as_ref() else {
        return Err("云线程未装配");
    };
    tx.try_send(job).map_err(|e| {
        if matches!(e, TrySendError::Full(_)) {
            "云队列满"
        } else {
            "云线程已退出"
        }
    })
}

/// worker 主体：Say → 云端 TTS → 播放；Turn → 注入的整轮策略；空转续 dialog 心跳。
fn cloud_run(rx: Receiver<Rr3CloudJob>, bus: Arc<Mutex<Bus>>, turn: Rr3TurnFn) {
    loop {
        match rx.recv_timeout(Duration::from_millis(IDLE_MS)) {
            Ok(Rr3CloudJob::Say(text)) => tts_say(&text),
            Ok(Rr3CloudJob::Turn(pcm)) => {
                crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
                turn(&bus, pcm);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                println!("[cloud] 作业队列断开，worker 退出");
                return;
            }
        }
    }
}

/// 朗读一条：云端 TTS → 交播放面（半双工门由播放面收尾）。
fn tts_say(text: &str) {
    let chars = text.chars().count();
    match rr3_tts_fetch(text) {
        Ok(pcm) => {
            let bytes = pcm.len();
            let queued = crate::playout::rr3_play(pcm);
            println!("[cloud] TTS ok chars={chars} pcm={bytes}B 入队={queued:?}");
        }
        Err(e) => println!("[cloud] TTS 失败 chars={chars}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    #[test]
    fn cloud_body_requires_three_fields_and_optional_key() {
        let b = body(serde_json::json!({
            "base": "https://api.example.com",
            "model": "tts-1",
            "voice": "alloy",
            "asr_model": "sensevoice",
            "llm_model": "chat-1",
            "llm_base": "https://api.example.com/v1",
        }));
        let c = rr3_cloud_body(&b).unwrap();
        assert_eq!(c.base, "https://api.example.com");
        assert!(c.key.is_none(), "key 缺席＝保留旧值");
        assert_eq!(c.asr_model.as_deref(), Some("sensevoice"));
        assert_eq!(c.llm_model.as_deref(), Some("chat-1"));
        assert_eq!(c.llm_base.as_deref(), Some("https://api.example.com/v1"));
        // 不用 unwrap_err：Rr3CloudWrite 故意不派生 Debug（key 零回显墙）。
        assert!(matches!(rr3_cloud_body(b"{}"), Err(e) if e.contains("base")));
        assert!(matches!(rr3_cloud_body(b"not json"), Err(e) if e.contains("JSON")));
        let bad = body(serde_json::json!({
            "base": "https://a", "model": "m", "voice": "v", "asr_model": 1,
        }));
        assert!(matches!(rr3_cloud_body(&bad), Err(e) if e.contains("非串")));
    }

    #[test]
    fn cloud_check_rejects_http_and_overlong_values() {
        let ok = Rr3CloudWrite {
            base: "https://a".into(),
            model: "m".into(),
            voice: "v".into(),
            key: None,
            asr_model: None,
            llm_model: None,
            llm_base: None,
        };
        assert_eq!(rr3_cloud_check(&ok), Ok(()));
        let empty_asr = Rr3CloudWrite {
            asr_model: Some(String::new()),
            ..ok
        };
        assert!(rr3_cloud_check(&empty_asr).unwrap_err().contains("形制"));
        let http = Rr3CloudWrite {
            base: "http://a".into(),
            ..empty_asr
        };
        assert!(rr3_cloud_check(&http).unwrap_err().contains("形制"));
    }

    #[test]
    fn say_body_bounds_text() {
        assert_eq!(
            rr3_say_body("{\"text\":\"你好\"}".as_bytes()).unwrap(),
            "你好".to_string()
        );
        assert!(rr3_say_body(br#"{"text":""}"#).is_err());
        let long = format!("{{\"text\":\"{}\"}}", "a".repeat(MAX_TEXT + 1));
        assert!(rr3_say_body(long.as_bytes()).is_err());
    }

    #[test]
    fn url_trims_trailing_slash() {
        let cfg = Rr3CloudCfg {
            base: "https://x/".into(),
            key: String::new(),
            model: "m".into(),
            voice: "v".into(),
        };
        assert_eq!(cfg.url(), "https://x/v1/audio/speech");
    }
}
