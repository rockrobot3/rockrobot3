//! unsafe 壳层：WiFi STA 起栈、httpd_start 与白名单 endpoint 注册。

use esp_idf_sys::httpd_req_get_hdr_value_str;

use super::*;

mod wifi;
use wifi::*;

mod start;
pub(crate) use start::rr3_httpd_spawn;

// ---------------------------------------------------------------------------
// HTTP 原语与 endpoint 处理臂；配置腿起栈面在 `start.rs`
// ---------------------------------------------------------------------------

/// httpd 任务栈（账面见模块文档；同 THREAD_STACK 口径）。
#[cfg(not(feature = "no-http"))]
pub(super) const HTTPD_STACK_SIZE: usize = 16384;
/// HTTP 监听口（板侧只读面，80 与 host 面板同档；端口号住常量＝资源参数
/// 同型病例的既有口径，本件不定新配置键）。
#[cfg(not(feature = "no-http"))]
pub(super) const HTTPD_SERVER_PORT: u16 = 80;

/// bus 单点（handler 裸壳取态的唯一入口——OnceLock 存 Arc clone，无裸指针
/// user_ctx；板侧单点先例 = [`crate::state::BOARD_BINDINGS`] 同形状）。
pub(super) static HTTPD_BUS: OnceLock<Arc<Mutex<Bus>>> = OnceLock::new();

/// 统一错误应答（`msg` 须含 NUL 结尾；文案由调用点给，逐字保留语义）。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
unsafe fn resp_err(req: *mut httpd_req_t, code: httpd_err_code_t, msg: &'static [u8]) -> i32 {
    httpd_resp_send_err(req, code, msg.as_ptr() as *const _)
}

/// JSON 应答（`application/json`；body 由调用点组好，出锁纪律在调用点）。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
unsafe fn resp_json(req: *mut httpd_req_t, body: &str) -> i32 {
    httpd_resp_set_type(req, c"application/json".as_ptr());
    httpd_resp_send(req, body.as_ptr() as *const _, body.len() as _)
}

/// body 有界收失败面（400 文案细分：上界 vs 收包——原 WifiWrite 逐字保留）。
enum BodyErr {
    Bounds,
    Recv,
}

/// 请求 body 有界收：`content_len` 是客户端可控的 Content-Length，超
/// [`RR3_WIFI_BODY_MAX`] 一律不读全（无上限 `vec![0u8; clen]` 在 4MB RAM 板上
/// 即 OOM abort——同网段一个请求打挂 httpd，静态审计 0.0.8b 实锤）。
/// 本轮审计：`httpd_req_recv` 单次返回可短于请求长度（TCP 分段/IDF 收包窗），
/// 原"单次读 + 满长判据"会把弱信号下的分段到达误判成坏包（配置写入随机 400）；
/// 循环读满或 IDF 收包超时（n ≤ 0）才回 Recv。
/// # Safety: req 为有效请求指针；content_len 由 IDF 填，函数按上界收包。
unsafe fn req_body_read(req: *mut httpd_req_t) -> Result<Vec<u8>, BodyErr> {
    req_body_read_max(req, RR3_WIFI_BODY_MAX)
}

/// 有界 body 收（上界按端点给：配网 512B、播放 128KB——同名 `BodyErr` 语义）。
///
/// # Safety: 同 [`req_body_read`]。
unsafe fn req_body_read_max(req: *mut httpd_req_t, max: usize) -> Result<Vec<u8>, BodyErr> {
    let clen = (*req).content_len;
    if clen == 0 || clen > max {
        return Err(BodyErr::Bounds);
    }
    let mut buf = vec![0u8; clen];
    let mut got = 0usize;
    while got < buf.len() {
        let n = httpd_req_recv(req, buf[got..].as_mut_ptr() as *mut _, buf.len() - got);
        if n <= 0 {
            return Err(BodyErr::Recv);
        }
        got += n as usize;
    }
    Ok(buf)
}

/// 请求 URI（失败模式不 panic：to_string_lossy 兜非 UTF-8）。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
unsafe fn req_uri(req: *mut httpd_req_t) -> String {
    std::ffi::CStr::from_ptr((*req).uri.as_ptr())
        .to_string_lossy()
        .into_owned()
}

/// 请求 method 归一（批N14 件二：注册面挂 GET/POST 两行——注册面未挂的 method
/// 判 "?"，路由纯函数必 None fail-closed 404；req.method 是 c_int，绑定常量是
/// http_method=u32——比较处 as i32 对齐）。
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
unsafe fn req_method(req: *mut httpd_req_t) -> &'static str {
    // 注册面只挂 GET/POST 两行；词表外判 "?"（查表必不命中 → 404 fail-closed）。
    const WORDS: [(i32, &str); 2] = [
        (http_method_HTTP_GET as i32, "GET"),
        (http_method_HTTP_POST as i32, "POST"),
    ];
    let m = (*req).method;
    WORDS
        .iter()
        .find(|(code, _)| *code == m)
        .map_or("?", |(_, w)| *w)
}

/// GET /——板端配网页。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
pub(crate) unsafe fn handle_index(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    httpd_resp_set_type(req, c"text/html; charset=utf-8".as_ptr());
    httpd_resp_send(
        req,
        RR3_BOARD_INDEX_HTML.as_ptr() as *const _,
        RR3_BOARD_INDEX_HTML.len() as _,
    )
}

/// GET /api/board/state——出锁组 JSON（快照取窗内不组串，模块文档纪律）。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
pub(crate) unsafe fn handle_state(req: *mut httpd_req_t, bus: &Arc<Mutex<Bus>>) -> i32 {
    resp_json(req, &rr3_state_json(&rr3_state_snapshot(bus)))
}

/// GET /api/board/health——自治监督壳快照（W3-2b）。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
pub(crate) unsafe fn handle_health(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    resp_json(req, &crate::autonomy::rr3_health_json())
}

/// GET /api/board/bindings——注入绑定读数。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
pub(crate) unsafe fn handle_bindings(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    let snap = rr3_bindings_snap();
    resp_json(req, &rr3_bindings_json(snap.as_ref()))
}

/// POST /api/board/wifi——事务性配网写口（批N14 件二）：body 有界收（fail-closed）
/// → 鉴权 → 解析 → 形制 → 事务；HTTP 层失败 400/401 拒收，业务层失败一律 200
/// JSON 如实应答。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
pub(crate) unsafe fn handle_wifi_write(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    let body = match req_body_read(req) {
        Ok(b) => b,
        Err(BodyErr::Bounds) => {
            println!("[periph] 写口 wifi 拒收：body 空或超 {RR3_WIFI_BODY_MAX}B");
            return resp_err(
                req,
                httpd_err_code_t_HTTPD_400_BAD_REQUEST,
                b"wifi body out of bounds\0",
            );
        }
        Err(BodyErr::Recv) => {
            println!("[periph] 写口 wifi 拒收：body 收包不完整");
            return resp_err(
                req,
                httpd_err_code_t_HTTPD_400_BAD_REQUEST,
                b"wifi body recv failed\0",
            );
        }
    };
    if !rr3_token_check(&body) {
        println!("[periph] 写口 wifi 拒收：admin token 缺失或错误");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_401_UNAUTHORIZED,
            b"admin token missing or invalid\0",
        );
    }
    // 阶段名保持契约词（"body"/"form"）：解析/形制错误各自映射，不把详细文案
    // 漏进 `stage` 字段（应答四字段是客户端判据）。
    let result = rr3_wifi_write_body(&body)
        .map_err(|_| "body")
        .and_then(|(s, p)| {
            crate::cred::rr3_cred_form_check(&s, &p)
                .map(|()| (s, p))
                .map_err(|_| "form")
        })
        .map_or_else(Rr3WifiWriteResult::rejected, |(s, p)| {
            rr3_wifi_write_apply(&s, &p)
        });
    resp_json(req, &rr3_wifi_write_json(&result))
}

/// 播放 body 上界（W4-2）：128 KB ≈ 4 s @16k mono s16；超限 fail-closed 400。
#[cfg(feature = "tts")]
const RR3_PLAY_BODY_MAX: usize = 131_072;

/// POST /api/board/play——raw PCM/WAV body（W4-2）：令牌走 `X-RR3-Token` 头，
/// 入队即应答（播放异步；完成/失败行见串口 `[spk]`）。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
#[cfg(feature = "tts")]
pub(crate) unsafe fn handle_play(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    let true = req_hdr_token_ok(req) else {
        println!("[periph] 写口 play 拒收：admin token 缺失或错误");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_401_UNAUTHORIZED,
            b"admin token missing or invalid\0",
        );
    };
    let Ok(body) = req_body_read_max(req, RR3_PLAY_BODY_MAX) else {
        println!("[periph] 写口 play 拒收：body 空/超 {RR3_PLAY_BODY_MAX}B 或收包不完整");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_400_BAD_REQUEST,
            b"play body out of bounds or recv failed\0",
        );
    };
    let n = body.len();
    let reply = match crate::cloud::rr3_play_async(body) {
        Ok(()) => format!("{{\"ok\":true,\"bytes\":{n}}}"),
        Err(e) => format!("{{\"ok\":false,\"err\":\"{e}\"}}"),
    };
    resp_json(req, &reply)
}

/// 云配置/朗读写口 body 上界（1024 B：512 B 文本 + JSON 键壳 + key 值）。
#[cfg(feature = "tts")]
const RR3_CLOUD_BODY_MAX: usize = 1024;

/// POST /api/board/cloud——云 TTS 配置写口（base/model/voice + 可选 key → NVS）。
/// key 只进 NVS，应答与日志均不回显值。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
#[cfg(feature = "tts")]
pub(crate) unsafe fn handle_cloud(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    let Ok(body) = req_body_read_max(req, RR3_CLOUD_BODY_MAX) else {
        println!("[periph] 写口 cloud 拒收：body 空/超 {RR3_CLOUD_BODY_MAX}B 或收包不完整");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_400_BAD_REQUEST,
            b"cloud body out of bounds or recv failed\0",
        );
    };
    let true = rr3_token_check(&body) else {
        println!("[periph] 写口 cloud 拒收：admin token 缺失或错误");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_401_UNAUTHORIZED,
            b"admin token missing or invalid\0",
        );
    };
    let reply = crate::cloud::rr3_cloud_write(&body).map_or_else(
        |e| format!("{{\"ok\":false,\"err\":\"{e}\"}}"),
        |()| "{\"ok\":true}".to_string(),
    );
    resp_json(req, &reply)
}

/// POST /api/board/say——朗读写口（JSON `{token,text}` → 云 TTS → 播放队列）。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
#[cfg(feature = "tts")]
pub(crate) unsafe fn handle_say(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    let Ok(body) = req_body_read_max(req, RR3_CLOUD_BODY_MAX) else {
        println!("[periph] 写口 say 拒收：body 空/超 {RR3_CLOUD_BODY_MAX}B 或收包不完整");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_400_BAD_REQUEST,
            b"say body out of bounds or recv failed\0",
        );
    };
    let true = rr3_token_check(&body) else {
        println!("[periph] 写口 say 拒收：admin token 缺失或错误");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_401_UNAUTHORIZED,
            b"admin token missing or invalid\0",
        );
    };
    let reply = crate::cloud::rr3_say_body(&body).map_or_else(
        |e| format!("{{\"ok\":false,\"err\":\"{e}\"}}"),
        |text| match crate::cloud::rr3_say(text) {
            Ok(()) => "{\"ok\":true}".to_string(),
            Err(e) => format!("{{\"ok\":false,\"err\":\"{e}\"}}"),
        },
    );
    resp_json(req, &reply)
}

/// 对话 body 上界（W6）：128 KB ≈ 4 s @16k mono s16（手动验收入口；麦克风路径
/// 走 PSRAM 缓冲，不占 HTTP body）。
#[cfg(feature = "tts")]
const RR3_DIALOG_BODY_MAX: usize = 131_072;

/// POST /api/board/dialog——手动触发一整轮（W6）：raw 16 kHz mono WAV/PCM，
/// 令牌走 `X-RR3-Token` 头；入队即应答（识别/补全/播放异步，进度见串口 `[dialog]`）。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
#[cfg(feature = "tts")]
pub(crate) unsafe fn handle_dialog(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    let true = req_hdr_token_ok(req) else {
        println!("[periph] 写口 dialog 拒收：admin token 缺失或错误");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_401_UNAUTHORIZED,
            b"admin token missing or invalid\0",
        );
    };
    let Ok(body) = req_body_read_max(req, RR3_DIALOG_BODY_MAX) else {
        println!("[periph] 写口 dialog 拒收：body 空/超 {RR3_DIALOG_BODY_MAX}B 或收包不完整");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_400_BAD_REQUEST,
            b"dialog body out of bounds or recv failed\0",
        );
    };
    let reply = match crate::dialog::rr3_dialog_body(&body)
        .and_then(|pcm| crate::cloud::rr3_turn(pcm).map_err(|e| e.to_string()))
    {
        Ok(()) => serde_json::json!({"ok": true}).to_string(),
        Err(e) => serde_json::json!({"ok": false, "err": e}).to_string(),
    };
    resp_json(req, &reply)
}

/// POST /api/board/oled——OLED 提示文本（DoA 3×10 现场提示；ASCII ≤63 B）。
/// 写 `st.last_text` 并切回文本模式，运行面下一次 `RedrawOled` 上屏。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
pub(crate) unsafe fn handle_oled(req: *mut httpd_req_t, bus: &Arc<Mutex<Bus>>) -> i32 {
    let Ok(body) = req_body_read(req) else {
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_400_BAD_REQUEST,
            b"oled body out of bounds\0",
        );
    };
    let true = rr3_token_check(&body) else {
        println!("[periph] 写口 oled 拒收：admin token 缺失或错误");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_401_UNAUTHORIZED,
            b"admin token missing or invalid\0",
        );
    };
    let reply = match rr3_oled_body(&body) {
        Ok(w) => {
            {
                let mut b = lock_bus(bus);
                b.st.last_text = w.text;
                b.st.expr_mode = false;
                b.st.frame_mode = false;
                b.st.oled_hold = w.hold;
            }
            push_job(bus, Job::RedrawOled);
            "{\"ok\":true}".to_string()
        }
        Err(e) => serde_json::json!({"ok": false, "err": e}).to_string(),
    };
    resp_json(req, &reply)
}

/// `X-RR3-Token` 头校验（raw body 端点用；令牌上限 128 B，超长即拒）。
///
/// # Safety: req 为有效请求指针；`httpd_req_get_hdr_value_str` 只读请求头。
#[cfg(feature = "tts")]
unsafe fn req_hdr_token_ok(req: *mut httpd_req_t) -> bool {
    let mut buf = [0u8; 128];
    if httpd_req_get_hdr_value_str(
        req,
        c"X-RR3-Token".as_ptr(),
        buf.as_mut_ptr() as *mut _,
        buf.len(),
    ) != ESP_OK
    {
        return false;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    std::str::from_utf8(&buf[..end])
        .map(|s| s == RR3_BOARD_ADMIN_TOKEN)
        .unwrap_or(false)
}

/// POST /api/board/reboot——动作面：应答先行落链再重启（`esp_restart` 不返回；
/// 重启即断链，故先 send 后 restart）。鉴权失败一律 401。
///
/// # Safety: req 为有效请求指针；esp_restart 不返回，调用后不再依赖本次请求。
pub(crate) unsafe fn handle_reboot(req: *mut httpd_req_t, _bus: &Arc<Mutex<Bus>>) -> i32 {
    let authed = req_body_read(req)
        .map(|b| rr3_token_check(&b))
        .unwrap_or(false);
    if !authed {
        println!("[periph] 写口 reboot 拒收：admin token 缺失或错误");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_401_UNAUTHORIZED,
            b"admin token missing or invalid\0",
        );
    }
    let _ = resp_json(req, "{\"ok\":true}");
    println!("[periph] 写口 reboot 应答已发——esp_restart");
    esp_restart()
}

/// 白名单统一 handler：req→路由纯函数→分派到各端点处理臂。表外 404 fail-closed。
///
/// # Safety: IDF 回调契约——req 有效且仅回调期间；不保存、不跨线程传递。
pub(super) unsafe extern "C" fn rr3_httpd_dispatch(req: *mut httpd_req_t) -> i32 {
    let Some(bus) = HTTPD_BUS.get() else {
        // 未装配即被请求到（时序防御）：OnceLock 在 httpd_start 前写入，常态不可达
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_404_NOT_FOUND,
            b"board bus unavailable\0",
        );
    };
    let method = req_method(req);
    let path = req_uri(req);
    rr3_route_lookup(method, &path).map_or_else(
        || {
            println!("[periph] HTTP 白名单外拒收 method={method} path={path}");
            resp_err(
                req,
                httpd_err_code_t_HTTPD_404_NOT_FOUND,
                b"not in board route whitelist\0",
            )
        },
        |handler| handler(req, bus),
    )
}
