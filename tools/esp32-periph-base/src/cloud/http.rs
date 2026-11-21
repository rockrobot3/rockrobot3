//! 云 HTTP 传输面（W6-2 自 cloud.rs 抽出；W7-b2 增流式读）：`esp_http_client` +
//! mbedTLS 证书束，单轮串行。请求体装配与应答解析在 `rr-cloud-proto`，本件只做
//! 字节收发与状态码收敛；错误文案形状是重试判据的输入（`rr3_cloud_retryable` 同源）。
//!
//! 两条读路：整段 `rr3_http_read`（ASR/LLM 小 JSON）与流式 `rr3_http_post_stream`
//! （TTS 4 KB 边收边播）。流式一旦向 sink 投出数据就不再重试——重放音频比失败更糟；
//! 首块之前仍按契约最多 2 次、500 ms。
//!
//! 只被 `cloud`（TTS）与 `dialog`（ASR/LLM）调用；key 空＝不带 Authorization 头
//! （local 免鉴权），key 值不出日志。

use std::ffi::{CStr, CString};
use std::thread;
use std::time::Duration;

use esp_idf_sys::esp_http_client_method_t_HTTP_METHOD_POST;
use esp_idf_sys::{
    esp_crt_bundle_attach, esp_http_client_cleanup, esp_http_client_config_t,
    esp_http_client_fetch_headers, esp_http_client_get_content_length,
    esp_http_client_get_status_code, esp_http_client_handle_t, esp_http_client_init,
    esp_http_client_open, esp_http_client_read, esp_http_client_set_header, esp_http_client_write,
    ESP_OK,
};

/// 单轮耗时/重试口径（契约 §4：8 s、最多 2 次、500 ms 间隔）。
const HTTP_TIMEOUT_MS: i32 = 8_000;
const RETRY_DELAY_MS: u64 = 500;
/// ASR/LLM JSON 应答上限（64 KB；识别长文本与补全回复的合理上界）。
pub(crate) const MAX_JSON: usize = 65_536;
/// 流式读块（TTS 4 KB 搬运窗；栈上）。
const STREAM_BUF: usize = 4_096;

/// 流式应答接收端（TTS 边收边播的机制缝）：重试前 `reset`，头到齐 `begin`，
/// 每块 `chunk`；一旦 `started`（已有播放副作用），上层不得重试。
pub(crate) trait Rr3StreamSink {
    fn rr3_reset(&mut self);
    fn rr3_begin(&mut self, content_len: Option<usize>);
    fn rr3_chunk(&mut self, data: &[u8]) -> Result<(), String>;
    fn rr3_started(&self) -> bool;
    /// 中止已开始的播放（失败路径必调；未开始则无操作）。
    fn rr3_abort(&mut self) {}
}

/// 三个 C 串（url/auth/content-type）；NUL 显式拒。
fn rr3_cstrs(
    url: &str,
    key: &str,
    content_type: &str,
) -> Result<(CString, Option<CString>, CString), String> {
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
    Ok((url_c, auth, ctype))
}

/// 初始化客户端并挂头（URL/auth/ctype 的 C 串由调用方保活到 cleanup）。
fn rr3_client(
    url_c: &CStr,
    auth: Option<&CStr>,
    ctype: &CStr,
) -> Result<esp_http_client_handle_t, String> {
    let cfg = esp_http_client_config_t {
        url: url_c.as_ptr(),
        method: esp_http_client_method_t_HTTP_METHOD_POST,
        timeout_ms: HTTP_TIMEOUT_MS,
        crt_bundle_attach: Some(esp_crt_bundle_attach),
        ..Default::default()
    };
    // SAFETY：cfg/url_c/ctype 活到 cleanup；handle 配对 init/cleanup。
    let client = unsafe { esp_http_client_init(&cfg) };
    let Some(client) = (!client.is_null()).then_some(client) else {
        return Err("云端不可达: HTTP 客户端初始化失败（堆不足？）".into());
    };
    // SAFETY：client 有效；头名/值为 C 串且活到调用结束。
    let h1 =
        unsafe { esp_http_client_set_header(client, c"Content-Type".as_ptr(), ctype.as_ptr()) };
    let h2 = auth.map_or(ESP_OK, |a| unsafe {
        esp_http_client_set_header(client, c"Authorization".as_ptr(), a.as_ptr())
    });
    if h1 == ESP_OK && h2 == ESP_OK {
        Ok(client)
    } else {
        // SAFETY：client 刚 init，失败路径成对 cleanup。
        unsafe { esp_http_client_cleanup(client) };
        Err("云端不可达: 请求头设置失败".into())
    }
}

/// 打开连接 → 写请求体 → 取状态码（200 之外按契约回 `{interface} HTTP {code}`）。
fn rr3_send(client: esp_http_client_handle_t, body: &[u8], interface: &str) -> Result<(), String> {
    // SAFETY：client 有效；写长由 body 长度给定。
    let opened = unsafe { esp_http_client_open(client, body.len() as i32) };
    if opened != ESP_OK {
        return Err(format!("云端不可达: open err={opened}"));
    }
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
        if n <= 0 {
            return Err("云端不可达: 请求体写失败".into());
        }
        off += n as usize;
    }
    // SAFETY：client 有效；返回长度不参与判定（状态码另取）。
    unsafe { esp_http_client_fetch_headers(client) };
    // SAFETY：头已取回，状态码只读。
    let status = unsafe { esp_http_client_get_status_code(client) };
    if status != 200 {
        return Err(format!("{interface} HTTP {status}"));
    }
    Ok(())
}

/// 一次 HTTPS POST（原始字节响应）。
pub(crate) fn rr3_http_post(
    interface: &str,
    url: &str,
    key: &str,
    body: &[u8],
    content_type: &str,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    let (url_c, auth, ctype) = rr3_cstrs(url, key, content_type)?;
    let client = rr3_client(&url_c, auth.as_deref(), &ctype)?;
    crate::mem::rr3_heap_mark(interface, "http-open");
    let rc =
        rr3_send(client, body, interface).and_then(|()| rr3_http_read(client, interface, max_body));
    crate::mem::rr3_heap_mark(interface, "http-done");
    // SAFETY：client 由上面 init 且只在此处 cleanup。
    unsafe { esp_http_client_cleanup(client) };
    rc
}

/// 读响应体（≤ `max_body`；n<0 传输错、n==0 EOF）。
fn rr3_http_read(
    client: esp_http_client_handle_t,
    interface: &str,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    let mut first = true;
    loop {
        // SAFETY：client 有效；缓冲 4 KB 可写；长度按缓冲大小给。
        let n =
            unsafe { esp_http_client_read(client, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
        if n < 0 {
            return Err("云端不可达: 读响应失败".into());
        }
        if n == 0 {
            break;
        }
        let n = n as usize;
        if out.len() + n > max_body {
            return Err(format!("{interface} 应答超上限"));
        }
        out.extend_from_slice(&buf[..n]);
        if first {
            crate::mem::rr3_heap_mark(interface, "rx-first");
            first = false;
        }
    }
    if out.is_empty() {
        return Err(format!("{interface} 应答为空"));
    }
    crate::mem::rr3_heap_mark(interface, "rx-done");
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
        // 单次 HTTP 上限 8 s < dialog 心跳窗 10 s；每次尝试前后各续一拍，
        // 长云调用（ASR/LLM/TTS）不会把 dialog 任务误判 dead。
        crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
        let attempt = rr3_http_post(interface, url, key, body, content_type, max_body);
        crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
        match attempt {
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

/// 流式 POST：响应体按 4 KB 读块交给 sink；返回总字节数。
///
/// 重试纪律：sink 尚未开始播（`rr3_started=false`）时沿用契约重试；一旦开始播，
/// 任何失败立即返回并 `rr3_abort`，绝不重放已播音频。
pub(crate) fn rr3_http_post_stream(
    interface: &str,
    url: &str,
    key: &str,
    body: &[u8],
    content_type: &str,
    sink: &mut dyn Rr3StreamSink,
    max_body: usize,
) -> Result<usize, String> {
    let (url_c, auth, ctype) = rr3_cstrs(url, key, content_type)?;
    let mut last = String::new();
    for _ in 0..2 {
        sink.rr3_reset();
        crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
        let attempt = rr3_stream_try(
            &url_c,
            auth.as_deref(),
            &ctype,
            body,
            interface,
            max_body,
            sink,
        );
        crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
        match attempt {
            Ok(n) => return Ok(n),
            Err(e) => {
                if sink.rr3_started() {
                    sink.rr3_abort();
                    return Err(e);
                }
                if !rr_cloud_proto::rr3_cloud_retryable(interface, &e) {
                    return Err(e);
                }
                last = e;
            }
        }
        thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
    }
    Err(last)
}

/// 单次流式尝试：init → send → 读块 → sink；cleanup 成对。
fn rr3_stream_try(
    url_c: &CStr,
    auth: Option<&CStr>,
    ctype: &CStr,
    body: &[u8],
    interface: &str,
    max_body: usize,
    sink: &mut dyn Rr3StreamSink,
) -> Result<usize, String> {
    let client = rr3_client(url_c, auth, ctype)?;
    crate::mem::rr3_heap_mark(interface, "http-open");
    let rc = rr3_stream_read(client, body, interface, max_body, sink);
    crate::mem::rr3_heap_mark(interface, "http-done");
    // SAFETY：client 由刚 init 且只在此处 cleanup。
    unsafe { esp_http_client_cleanup(client) };
    rc
}

/// 状态码后逐块读：长度已知则对账完整性，未知（chunked）只按上限收敛。
fn rr3_stream_read(
    client: esp_http_client_handle_t,
    body: &[u8],
    interface: &str,
    max_body: usize,
    sink: &mut dyn Rr3StreamSink,
) -> Result<usize, String> {
    rr3_send(client, body, interface)?;
    // SAFETY：头已取回，内容长度只读；-1 = chunked/未知。
    let len = unsafe { esp_http_client_get_content_length(client) };
    let total = (len > 0).then_some(len as usize);
    sink.rr3_begin(total);
    let got = rr3_read_chunks(client, interface, max_body, sink)?;
    rr3_stream_check(interface, got, total)?;
    crate::mem::rr3_heap_mark(interface, "rx-done");
    Ok(got)
}

/// 逐块读（4 KB）并交 sink；返回总字节数。
fn rr3_read_chunks(
    client: esp_http_client_handle_t,
    interface: &str,
    max_body: usize,
    sink: &mut dyn Rr3StreamSink,
) -> Result<usize, String> {
    let mut buf = [0u8; STREAM_BUF];
    let mut got = 0usize;
    let mut first = true;
    loop {
        // SAFETY：client 有效；缓冲 4 KB 可写；长度按缓冲大小给。
        let n =
            unsafe { esp_http_client_read(client, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
        if n < 0 {
            return Err("云端不可达: 读响应失败".into());
        }
        if n == 0 {
            break;
        }
        let n = n as usize;
        got += n;
        if got > max_body {
            return Err(format!("{interface} 应答超上限"));
        }
        sink.rr3_chunk(&buf[..n])
            .map_err(|e| format!("{interface} 播放链路失败: {e}"))?;
        if first {
            crate::mem::rr3_heap_mark(interface, "rx-first");
            first = false;
        }
    }
    Ok(got)
}

/// 收尾判据：空回包显式拒；长度已知时必须逐字节收满。
fn rr3_stream_check(interface: &str, got: usize, total: Option<usize>) -> Result<(), String> {
    if got == 0 {
        return Err(format!("{interface} 应答为空"));
    }
    if let Some(total) = total {
        if got != total {
            return Err(format!("{interface} 回包不完整 {got}/{total}B"));
        }
    }
    Ok(())
}
