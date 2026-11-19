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

use std::ffi::{CStr, CString};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use esp_idf_sys::{
    esp_crt_bundle_attach, esp_http_client_cleanup, esp_http_client_config_t,
    esp_http_client_fetch_headers, esp_http_client_get_status_code, esp_http_client_handle_t,
    esp_http_client_init, esp_http_client_open, esp_http_client_read, esp_http_client_set_header,
    esp_http_client_write, nvs_close, nvs_commit, nvs_get_str, nvs_handle_t, nvs_open,
    nvs_open_mode_t_NVS_READONLY, nvs_open_mode_t_NVS_READWRITE, nvs_set_str, ESP_OK,
};
use serde_json::Value;

use crate::cred::RR3_CRED_NAMESPACE;

/// NVS 键（≤15 B）：base URL / key / TTS 模型行 / 音色。
const K_BASE: &str = "cloud.base";
const K_KEY: &str = "cloud.key";
const K_MODEL: &str = "tts.model";
const K_VOICE: &str = "tts.voice";
/// 值上限（写口与读口同门；超形即拒，防 NVS 被大值撑爆）。
const MAX_BASE: usize = 128;
const MAX_KEY: usize = 160;
const MAX_MODEL: usize = 64;
const MAX_VOICE: usize = 64;
/// 单条朗读文本上限（与 `/say` body 的 512 B 上界同源）。
const MAX_TEXT: usize = 512;
/// TTS PCM 响应上限：256 KB ≈ 8 s @16k mono s16。
const MAX_PCM: usize = 262_144;
/// 待朗读队列容量（条；满即拒，不阻塞 HTTP 任务）。
const TTS_Q_CAP: usize = 2;
/// worker 栈：mbedTLS 握手 + esp_http_client 缓冲。
const TTS_STACK: usize = 24_576;
/// 单次 HTTP 超时（契约 §4）。
const HTTP_TIMEOUT_MS: i32 = 8_000;
/// 重试间隔（契约 §4）。
const RETRY_DELAY_MS: u64 = 500;
/// IDF `esp_http_client_method_t` 的 POST 枚举值（GET=0、POST=1）。
const HTTP_METHOD_POST: u32 = 1;
/// worker 空转周期（只为及时退出，不承担心跳——dialog 归 W6）。
const IDLE_MS: u64 = 2_000;

/// 播放队列发送端（未装配 = None）。
static TTS_TX: Mutex<Option<SyncSender<String>>> = Mutex::new(None);

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

/// 写口 body：base/model/voice 必填，key 可选（缺席＝保留旧值）。
struct Rr3CloudWrite {
    base: String,
    model: String,
    voice: String,
    key: Option<String>,
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
    opened.then(|| unsafe { nvs_close(h) });
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
    (r == ESP_OK).then_some(()).ok_or(r)
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

// ---------------------------------------------------------------------------
// 写口：body 解析 / 形制 / 落 NVS
// ---------------------------------------------------------------------------

/// JSON 取串字段（缺失/非串统一错误）。
fn rr3_field<'a>(v: &'a Value, k: &str) -> Result<&'a str, &'static str> {
    v.get(k).and_then(Value::as_str).ok_or("字段缺席或非串")
}

/// 写口 body 解析（纯函数，单测钉域）。
fn rr3_cloud_body(body: &[u8]) -> Result<Rr3CloudWrite, &'static str> {
    let v: Value = serde_json::from_slice(body).map_err(|_| "body 非 JSON")?;
    Ok(Rr3CloudWrite {
        base: rr3_field(&v, "base")?.to_string(),
        model: rr3_field(&v, "model")?.to_string(),
        voice: rr3_field(&v, "voice")?.to_string(),
        key: v.get("key").and_then(Value::as_str).map(str::to_string),
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
        c.key.as_ref().map_or(true, |k| k.len() <= MAX_KEY),
    ];
    let true = checks.iter().all(|b| *b) else {
        return Err("云配置形制不合（https/长度门）");
    };
    Ok(())
}

/// 落 NVS：三必填键 + 可选 key（缺席保留旧值）。
fn rr3_cloud_store(c: &Rr3CloudWrite) -> Result<(), i32> {
    let Ok(()) = nvs_set(K_BASE, &c.base) else {
        return Err(-2);
    };
    let Ok(()) = nvs_set(K_MODEL, &c.model) else {
        return Err(-2);
    };
    let Ok(()) = nvs_set(K_VOICE, &c.voice) else {
        return Err(-2);
    };
    c.key.as_ref().map_or(Ok(()), |k| nvs_set(K_KEY, k))
}

/// 写口总入口（handler 只做 token/应答）：解析 → 形制 → NVS。
pub(crate) fn rr3_cloud_write(body: &[u8]) -> Result<(), &'static str> {
    let req = rr3_cloud_body(body)?;
    rr3_cloud_check(&req)?;
    let Ok(()) = rr3_cloud_store(&req) else {
        return Err("NVS 写入失败（串口有 err 码）");
    };
    println!(
        "[cloud] 配置已写入 NVS（key={}，base/model/voice 已更新）",
        req.key.as_ref().map_or("保留", |_| "更新")
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
// HTTPS 传输（esp_http_client + mbedTLS 证书束；单轮串行）
// ---------------------------------------------------------------------------

/// 一次 HTTPS POST（JSON 请求、原始字节响应）。错误串按契约分两类：
/// 传输层 `云端不可达 ...`、HTTP 状态 `TTS接口 HTTP {code}`（重试判据同源）。
fn rr3_http_post(url: &str, key: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    let Ok(url_c) = CString::new(url) else {
        return Err("url 含 NUL".into());
    };
    let cfg = esp_http_client_config_t {
        url: url_c.as_ptr(),
        method: HTTP_METHOD_POST,
        timeout_ms: HTTP_TIMEOUT_MS,
        crt_bundle_attach: Some(esp_crt_bundle_attach),
        ..Default::default()
    };
    // SAFETY：cfg/url_c 活到 cleanup；handle 在本函数内成对 init/cleanup。
    let client = unsafe { esp_http_client_init(&cfg) };
    let Some(client) = (!client.is_null()).then_some(client) else {
        return Err("云端不可达: HTTP 客户端初始化失败（堆不足？）".into());
    };
    let Ok(auth) = CString::new(format!("Bearer {key}")) else {
        // SAFETY: client 由本函数 init，此路径不再使用。
        unsafe { esp_http_client_cleanup(client) };
        return Err("key 含 NUL".into());
    };
    let rc = rr3_http_do(client, &auth, body);
    // SAFETY：client 由本函数 init 且只在此处 cleanup。
    unsafe { esp_http_client_cleanup(client) };
    rc
}

/// 建连接 → 写请求体 → 取状态码 → 读响应体（header/auth 由调用方备好）。
fn rr3_http_do(
    client: esp_http_client_handle_t,
    auth: &CStr,
    body: &[u8],
) -> Result<Vec<u8>, String> {
    // SAFETY：client 有效；两个头名/值为静态 C 串，auth 活到调用结束。
    let h1 = unsafe {
        esp_http_client_set_header(
            client,
            c"Content-Type".as_ptr(),
            c"application/json".as_ptr(),
        )
    };
    let h2 =
        unsafe { esp_http_client_set_header(client, c"Authorization".as_ptr(), auth.as_ptr()) };
    let true = (h1 == ESP_OK && h2 == ESP_OK) else {
        return Err("云端不可达: 请求头设置失败".into());
    };
    // SAFETY：client 有效；写长由 body 长度给定。
    let opened = unsafe { esp_http_client_open(client, body.len() as i32) };
    let true = opened == ESP_OK else {
        return Err(format!("云端不可达: open err={opened}"));
    };
    let mut off = 0usize;
    while off < body.len() {
        // SAFETY：client 已 open；切片剩余长度由 off 保证不超过 body。
        let n = unsafe {
            esp_http_client_write(
                client,
                body[off..].as_ptr() as *const _,
                (body.len() - off) as i32,
            )
        };
        let true = n > 0 else {
            return Err("云端不可达: 请求体写失败".into());
        };
        off += n as usize;
    }
    // SAFETY：client 有效；返回长度不参与判定（状态码另取）。
    unsafe { esp_http_client_fetch_headers(client) };
    // SAFETY：头已取回，状态码只读。
    let status = unsafe { esp_http_client_get_status_code(client) };
    let true = status == 200 else {
        return Err(format!("TTS接口 HTTP {status}"));
    };
    rr3_http_read(client)
}

/// 读响应体（≤[`MAX_PCM`]；n<0 传输错、n==0 EOF）。
fn rr3_http_read(client: esp_http_client_handle_t) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        // SAFETY：client 有效；缓冲 4 KB 可写；长度按缓冲大小给。
        let n =
            unsafe { esp_http_client_read(client, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
        let true = n >= 0 else {
            return Err("云端不可达: 读响应失败".into());
        };
        let true = n > 0 else { break };
        let n = n as usize;
        let true = out.len() + n <= MAX_PCM else {
            return Err("TTS 响应超上限".into());
        };
        out.extend_from_slice(&buf[..n]);
    }
    let true = !out.is_empty() else {
        return Err("TTS 响应为空".into());
    };
    Ok(out)
}

/// 取一条 TTS PCM：读配置 → 组请求体（共享纯函数）→ 至多两次尝试。
fn rr3_tts_fetch(text: &str) -> Result<Vec<u8>, String> {
    let cfg = rr3_cloud_cfg().map_err(|e| format!("云配置缺席: {e}"))?;
    let body = rr3_cloud_proto::rr3_tts_body(&cfg.model, text, &cfg.voice, 16_000);
    let url = cfg.url();
    let mut last = String::new();
    for _ in 0..2 {
        match rr3_http_post(&url, &cfg.key, &body) {
            Ok(pcm) => return Ok(pcm),
            Err(e) => {
                let true = rr3_cloud_proto::rr3_cloud_retryable("TTS接口", &e) else {
                    return Err(e);
                };
                last = e;
            }
        }
        thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
    }
    Err(last)
}

// ---------------------------------------------------------------------------
// worker（有界文本队列 → TTS → 播放队列）
// ---------------------------------------------------------------------------

/// 起 TTS worker（main 在 httpd 之前调用，避免 `/say` 撞"未装配"窗）。
pub(crate) fn spawn_tts() {
    let (tx, rx) = sync_channel(TTS_Q_CAP);
    let _ = TTS_TX.lock().map(|mut slot| *slot = Some(tx));
    let _ = thread::Builder::new()
        .name("tts-cloud".into())
        .stack_size(TTS_STACK)
        .spawn(move || tts_run(rx))
        .map_err(|e| println!("[cloud] 起 TTS 线程失败: {e:?}"));
}

/// 入队一条朗读请求（满/未装配/线程退出都如实回错，绝不阻塞 HTTP 任务）。
pub(crate) fn rr3_say(text: String) -> Result<(), &'static str> {
    let Ok(slot) = TTS_TX.lock() else {
        return Err("TTS 面锁中毒");
    };
    let Some(tx) = slot.as_ref() else {
        return Err("TTS 面未装配");
    };
    tx.try_send(text).map_err(|e| {
        if matches!(e, TrySendError::Full(_)) {
            "TTS 队列满"
        } else {
            "TTS 线程已退出"
        }
    })
}

/// worker 主体：取文本 → 云端 TTS → 交播放面（半双工门由播放面收尾）。
fn tts_run(rx: Receiver<String>) {
    loop {
        let Ok(text) = rx.recv_timeout(Duration::from_millis(IDLE_MS)) else {
            continue;
        };
        let chars = text.chars().count();
        match rr3_tts_fetch(&text) {
            Ok(pcm) => {
                let bytes = pcm.len();
                let queued = crate::playout::rr3_play(pcm);
                println!("[cloud] TTS ok chars={chars} pcm={bytes}B 入队={queued:?}");
            }
            Err(e) => println!("[cloud] TTS 失败 chars={chars}: {e}"),
        }
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
        }));
        let c = rr3_cloud_body(&b).unwrap();
        assert_eq!(c.base, "https://api.example.com");
        assert!(c.key.is_none(), "key 缺席＝保留旧值");
        assert!(rr3_cloud_body(b"{}").unwrap_err().contains("base"));
        assert!(rr3_cloud_body(b"not json").unwrap_err().contains("JSON"));
    }

    #[test]
    fn cloud_check_rejects_http_and_overlong_values() {
        let ok = Rr3CloudWrite {
            base: "https://a".into(),
            model: "m".into(),
            voice: "v".into(),
            key: None,
        };
        assert_eq!(rr3_cloud_check(&ok), Ok(()));
        let http = Rr3CloudWrite {
            base: "http://a".into(),
            ..ok
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
