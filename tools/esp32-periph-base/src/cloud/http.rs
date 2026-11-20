//! 云 HTTP 传输面（W6-2 自 cloud.rs 抽出）：`esp_http_client` + mbedTLS 证书束，
//! 单轮串行。请求体装配与应答解析在 `rr-cloud-proto`，本件只做字节收发与状态码
//! 收敛；错误文案形状是重试判据的输入（`rr3_cloud_retryable` 同源）。
//!
//! 只被 `cloud`（TTS）与 `dialog`（ASR/LLM）调用；key 空＝不带 Authorization 头
//! （local 免鉴权），key 值不出日志。

use std::ffi::{CStr, CString};
use std::thread;
use std::time::Duration;

use esp_idf_sys::esp_http_client_method_t_HTTP_METHOD_POST;
use esp_idf_sys::{
    esp_crt_bundle_attach, esp_http_client_cleanup, esp_http_client_config_t,
    esp_http_client_fetch_headers, esp_http_client_get_status_code, esp_http_client_handle_t,
    esp_http_client_init, esp_http_client_open, esp_http_client_read, esp_http_client_set_header,
    esp_http_client_write, ESP_OK,
};

use super::{rr3_cloud_cfg, MAX_PCM};

/// 单轮耗时/重试口径（契约 §4：8 s、最多 2 次、500 ms 间隔）。
const HTTP_TIMEOUT_MS: i32 = 8_000;
const RETRY_DELAY_MS: u64 = 500;
/// ASR/LLM JSON 应答上限（64 KB；识别长文本与补全回复的合理上界）。
pub(crate) const MAX_JSON: usize = 65_536;

/// 一次 HTTPS POST（原始字节响应）。错误串按契约分两类：传输层
/// `云端不可达 ...`、HTTP 状态 `{interface} HTTP {code}`（重试判据同源）。
pub(crate) fn rr3_http_post(
    interface: &str,
    url: &str,
    key: &str,
    body: &[u8],
    content_type: &str,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    let Ok(url_c) = CString::new(url) else {
        return Err("url 含 NUL".into());
    };
    let Ok(ctype) = CString::new(content_type) else {
        return Err("content-type 含 NUL".into());
    };
    let auth = if key.is_empty() {
        None
    } else {
        let Ok(a) = CString::new(format!("Bearer {key}")) else {
            return Err("key 含 NUL".into());
        };
        Some(a)
    };
    let cfg = esp_http_client_config_t {
        url: url_c.as_ptr(),
        method: esp_http_client_method_t_HTTP_METHOD_POST,
        timeout_ms: HTTP_TIMEOUT_MS,
        crt_bundle_attach: Some(esp_crt_bundle_attach),
        ..Default::default()
    };
    // SAFETY：cfg/url_c/ctype 活到 cleanup；handle 在本函数内成对 init/cleanup。
    let client = unsafe { esp_http_client_init(&cfg) };
    let Some(client) = (!client.is_null()).then_some(client) else {
        return Err("云端不可达: HTTP 客户端初始化失败（堆不足？）".into());
    };
    let rc = rr3_http_do(client, auth.as_deref(), &ctype, body, interface, max_body);
    // SAFETY：client 由本函数 init 且只在此处 cleanup。
    unsafe { esp_http_client_cleanup(client) };
    rc
}

/// 建连接 → 写请求体 → 取状态码 → 读响应体（header/auth 由调用方备好）。
fn rr3_http_do(
    client: esp_http_client_handle_t,
    auth: Option<&CStr>,
    ctype: &CStr,
    body: &[u8],
    interface: &str,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    // SAFETY：client 有效；头名/值为 C 串，ctype/auth 活到调用结束。
    let h1 =
        unsafe { esp_http_client_set_header(client, c"Content-Type".as_ptr(), ctype.as_ptr()) };
    let h2 = auth.map_or(ESP_OK, |a| unsafe {
        esp_http_client_set_header(client, c"Authorization".as_ptr(), a.as_ptr())
    });
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
        return Err(format!("{interface} HTTP {status}"));
    };
    rr3_http_read(client, interface, max_body)
}

/// 读响应体（≤ `max_body`；n<0 传输错、n==0 EOF）。
fn rr3_http_read(
    client: esp_http_client_handle_t,
    interface: &str,
    max_body: usize,
) -> Result<Vec<u8>, String> {
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
        let true = out.len() + n <= max_body else {
            return Err(format!("{interface} 应答超上限"));
        };
        out.extend_from_slice(&buf[..n]);
    }
    let true = !out.is_empty() else {
        return Err(format!("{interface} 应答为空"));
    };
    Ok(out)
}

/// 有界重试（契约 §4：最多 2 次尝试、500 ms；只对瞬时失败续打）。
pub(crate) fn rr3_post_retry(
    interface: &str,
    url: &str,
    key: &str,
    body: &[u8],
    content_type: &str,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for _ in 0..2 {
        match rr3_http_post(interface, url, key, body, content_type, max_body) {
            Ok(v) => return Ok(v),
            Err(e) => {
                let true = rr_cloud_proto::rr3_cloud_retryable(interface, &e) else {
                    return Err(e);
                };
                last = e;
            }
        }
        thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
    }
    Err(last)
}

/// 取一条 TTS PCM：读配置 → 组请求体（共享纯函数）→ 有界重试。
pub(crate) fn rr3_tts_fetch(text: &str) -> Result<Vec<u8>, String> {
    let cfg = rr3_cloud_cfg().map_err(|e| format!("云配置缺席: {e}"))?;
    let body = rr_cloud_proto::rr3_tts_body(&cfg.model, text, &cfg.voice, 16_000);
    rr3_post_retry(
        "TTS接口",
        &cfg.url(),
        &cfg.key,
        &body,
        "application/json",
        MAX_PCM,
    )
}
