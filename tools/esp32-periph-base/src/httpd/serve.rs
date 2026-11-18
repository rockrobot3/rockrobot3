//! unsafe 壳层：WiFi STA 起栈、httpd_start 与白名单 endpoint 注册。

use super::*;

mod wifi;
use wifi::*;

// ---------------------------------------------------------------------------
// unsafe 壳层：WiFi STA 起栈（裸 sys）+ httpd_start + 白名单注册
// ---------------------------------------------------------------------------

/// httpd 任务栈（账面见模块文档；同 THREAD_STACK 口径）。
const HTTPD_STACK_SIZE: usize = 16384;
/// HTTP 监听口（板侧只读面，80 与 host 面板同档；端口号住常量＝资源参数
/// 同型病例的既有口径，本件不定新配置键）。
const HTTPD_SERVER_PORT: u16 = 80;

/// bus 单点（handler 裸壳取态的唯一入口——OnceLock 存 Arc clone，无裸指针
/// user_ctx；板侧单点先例 = [`crate::state::BOARD_BINDINGS`] 同形状）。
static HTTPD_BUS: OnceLock<Arc<Mutex<Bus>>> = OnceLock::new();

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
    let clen = (*req).content_len;
    if clen == 0 || clen > RR3_WIFI_BODY_MAX {
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
    match (*req).method {
        m if m == http_method_HTTP_GET as i32 => "GET",
        m if m == http_method_HTTP_POST as i32 => "POST",
        _ => "?",
    }
}

/// GET /——板端配网页。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
unsafe fn handle_index(req: *mut httpd_req_t) -> i32 {
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
unsafe fn handle_state(req: *mut httpd_req_t, bus: &Arc<Mutex<Bus>>) -> i32 {
    resp_json(req, &rr3_state_json(&rr3_state_snapshot(bus)))
}

/// GET /api/board/bindings——注入绑定读数。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
unsafe fn handle_bindings(req: *mut httpd_req_t) -> i32 {
    let snap = rr3_bindings_snap();
    resp_json(req, &rr3_bindings_json(snap.as_ref()))
}

/// POST /api/board/wifi——事务性配网写口（批N14 件二）：body 有界收（fail-closed）
/// → 鉴权 → 解析 → 形制 → 事务；HTTP 层失败 400/401 拒收，业务层失败一律 200
/// JSON 如实应答。
///
/// # Safety: req 为 esp_http_server 传入的非空有效指针，仅本次回调内使用。
unsafe fn handle_wifi_write(req: *mut httpd_req_t) -> i32 {
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
    let result = match rr3_wifi_write_body(&body) {
        Ok((s, p)) => match crate::cred::rr3_cred_form_check(&s, &p) {
            Ok(()) => rr3_wifi_write_apply(&s, &p),
            Err(_) => Rr3WifiWriteResult::rejected("form"),
        },
        Err(_) => Rr3WifiWriteResult::rejected("body"),
    };
    resp_json(req, &rr3_wifi_write_json(&result))
}

/// POST /api/board/reboot——动作面：应答先行落链再重启（`esp_restart` 不返回；
/// 重启即断链，故先 send 后 restart）。鉴权失败一律 401。
///
/// # Safety: req 为有效请求指针；esp_restart 不返回，调用后不再依赖本次请求。
unsafe fn handle_reboot(req: *mut httpd_req_t) -> i32 {
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
unsafe extern "C" fn rr3_httpd_dispatch(req: *mut httpd_req_t) -> i32 {
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
    let Some(route) = rr3_board_route(method, &path) else {
        println!("[periph] HTTP 白名单外拒收 method={method} path={path}");
        return resp_err(
            req,
            httpd_err_code_t_HTTPD_404_NOT_FOUND,
            b"not in board route whitelist\0",
        );
    };
    match route {
        Rr3BoardRoute::Index => handle_index(req),
        Rr3BoardRoute::State => handle_state(req, bus),
        Rr3BoardRoute::Bindings => handle_bindings(req),
        Rr3BoardRoute::WifiWrite => handle_wifi_write(req),
        Rr3BoardRoute::Reboot => handle_reboot(req),
    }
}

/// 起板侧配置腿（main 挂载点；**fail-honest**——任何一步失败点名后返回，
/// 配置腿缺席不阻塞既有链路；凭据未装配同口径）。
///
/// 凭据读链（批N14 件一，存储对象见 [`crate::cred`]）：**NVS 优先 → 编译期
/// `option_env!` 回落 → 都缺＝缺席照常**（NVS 空时与批N13 逐字零回归）。
/// NVS init 前移到本段（读链前件）——失败＝起栈链无健康前件，点名缺席
/// （不 erase——不毁既有分区）；真机 env 永远缺席的机制订正见模块文档凭据节。
pub(crate) fn rr3_httpd_spawn(bus: &Arc<Mutex<Bus>>) {
    let bus = Arc::clone(bus);
    // ① NVS init 前移（批N14 件一）：凭据读链前件，兼 WiFi 校准数据存储
    // SAFETY：nvs_flash_init 是 IDF 的一次性初始化，配置腿线程起栈前调用；
    // 失败即返回，不重复调用、不 erase 既有分区。
    if unsafe { nvs_flash_init() } != ESP_OK {
        println!("[periph] 配置腿未装配（nvs_flash_init 失败）——N-13 服务面缺席，板其余功能照常");
        return;
    }
    // ② 凭据读链：NVS 优先（读取窄缝 [`crate::cred::rr3_cred_read`]，读时现取）
    let (ssid, pass, cred_source) = match crate::cred::rr3_cred_read() {
        Some(c) => {
            let pass = c.pass().to_string(); // 先经窄缝取密码（借用），再 move ssid
            (c.ssid, pass, "nvs")
        }
        // 编译期回落（option_env! 需字面量——构建期烘焙 .rodata，值不落仓）
        None => match (
            option_env!("RR3_BOARD_WIFI_SSID"),
            option_env!("RR3_BOARD_WIFI_PASS"),
        ) {
            (Some(s), p) if !s.is_empty() => {
                (s.to_string(), p.unwrap_or_default().to_string(), "编译期")
            }
            _ => {
                // **2026-10-31 SoftAP 配网面**：原实现在"两源都缺"时直接返回（配置腿
                // 缺席），板因此无法自身上线——必须先有 WiiFi 凭据才能用面板配网，
                // 而配网入口本身要网络，构成鸡生蛋死锁。改为 AP-only 起栈：无凭据
                // 也起 SoftAP（SSID 见 [`wifi_ap_configure`]），手机/笔记本可连上板
                // 自己的面板完成配网（`POST /api/board/wifi`），配网成功即 STA 上线。
                // 凭据来源如实点名 "ap-only"（boot 横幅可辨，永不冒领已配网）。
                println!(
                    "[periph] 无 WiFi 凭据（NVS 空且 RR3_BOARD_WIFI_SSID 未注入）→ 转 SoftAP 配网模式（AP-only，配置腿照常可达）"
                );
                (String::new(), String::new(), "ap-only")
            }
        },
    };
    // ③ 形制校验（两源同门——[`crate::cred::rr3_cred_form_check`]；错误不出值只出长度）。
    // ap-only 档无凭据可校，跳过（STA 侧不配置、不连接）。
    if !ssid.is_empty() {
        if let Err(e) = crate::cred::rr3_cred_form_check(&ssid, &pass) {
            println!(
                "[periph] 配置腿装配失败（{e}：SSID {}B/密码 {}B）——缺席照常",
                ssid.len(),
                pass.len()
            );
            return;
        }
    }
    let ssid = ssid.as_bytes().to_vec();
    let pass = pass.as_bytes().to_vec();
    thread_spawn(bus, cred_source, ssid, pass);
}

/// 起栈线程（与控制/运行面同 16KB 口径；短命——起完 httpd 即退；
/// `cred_source`＝凭据来源点名串（"nvs"/"编译期"），boot 日志用，永不出值）。
fn thread_spawn(bus: Arc<Mutex<Bus>>, cred_source: &'static str, ssid: Vec<u8>, pass: Vec<u8>) {
    std::thread::Builder::new()
        .name("periph-httpd".into())
        .stack_size(crate::board::THREAD_STACK)
        // SAFETY：闭包体整段是 unsafe 壳层调用（start_httpd），线程独占起栈链；
        // 捕获的 Arc/Vec 均 Send，无裸指针跨线程传递。
        .spawn(move || unsafe {
            if let Err(e) = start_httpd(&bus, cred_source, &ssid, &pass) {
                println!("[periph] 配置腿启动失败 esp_err={e}（缺席照常；负值＝IDF 错误码）");
            }
        })
        .expect("起配置腿线程失败");
}

/// esp_err → Result（fail-honest 逐步：非 ESP_OK 即 Err，点名在调用方）。
fn esp_ok(r: i32) -> Result<(), i32> {
    if r == ESP_OK {
        Ok(())
    } else {
        Err(r)
    }
}

/// httpd 起 + 白名单注册（手填 config：HTTPD_DEFAULT_CONFIG 真值——宏默认也是
/// false/null/0 的字段由全零起底等价覆盖，差异字段逐项对表 esp_http_server.h :53；
/// OnceLock 先写——handler 可达即 bus 在位；method 映射随表行 GET/POST，表内词表
/// 由 covers_wordform_shape 单测钉死，词表外 panic 不可达）。
///
/// # Safety
///
/// 只能在 `wifi_bringup` 成功后调用一次；`HTTPD_BUS` 先写后注册 handler，
/// 保证回调可达时 bus 必在位；注册表路径 CString 活到函数返回。
unsafe fn httpd_register(bus: &Arc<Mutex<Bus>>) -> Result<(), i32> {
    let mut h: httpd_handle_t = std::ptr::null_mut();
    let conf = httpd_config_t {
        task_priority: 6, // tskIDLE_PRIORITY+5
        stack_size: HTTPD_STACK_SIZE,
        core_id: 0x7fff_ffff, // tskNO_AFFINITY
        server_port: HTTPD_SERVER_PORT,
        ctrl_port: 32768, // ESP_HTTPD_DEF_CTRL_PORT
        max_open_sockets: 7,
        max_uri_handlers: 8,
        max_resp_headers: 8,
        backlog_conn: 5,
        recv_wait_timeout: 5,
        send_wait_timeout: 5,
        ..Default::default()
    };
    esp_ok(httpd_start(&mut h, &conf))?;
    let _ = HTTPD_BUS.set(Arc::clone(bus));
    for (method, path, _, _) in RR3_BOARD_ROUTES {
        let cpath = CString::new(*path).expect("白名单 path 不含 NUL");
        let m = match *method {
            "GET" => http_method_HTTP_GET,
            "POST" => http_method_HTTP_POST,
            _ => panic!("白名单 method 词表外: {method}"),
        };
        let uri = httpd_uri_t {
            uri: cpath.as_ptr(),
            method: m,
            handler: Some(rr3_httpd_dispatch),
            user_ctx: std::ptr::null_mut(),
        };
        esp_ok(httpd_register_uri_handler(h, &uri))?;
    }
    Ok(())
}

/// WiFi 起栈 + httpd 起与注册（unsafe 集中段；NVS init 已前移到
/// [`rr3_httpd_spawn`]——本段只做起栈链与服务工作面）。
///
/// # Safety
///
/// 只能由配置腿线程调用一次；整段是 IDF 起栈链，重复或并发调用会与 IDF
/// 全局状态冲突（本函数不提供幂等保证）。
unsafe fn start_httpd(
    bus: &Arc<Mutex<Bus>>,
    cred_source: &str,
    ssid: &[u8],
    pass: &[u8],
) -> Result<(), i32> {
    let ap_ssid = wifi_bringup(ssid, pass)?;
    httpd_register(bus)?;
    println!(
        "[periph] 配置腿起（N-13，凭据源={cred_source}）: SoftAP SSID={ap_ssid}（WPA2，口令见 wifi_ap_configure 注）+ http://0.0.0.0:{}/api/board/*（白名单 {} 端点＝GET state/bindings＋POST wifi/reboot）",
        HTTPD_SERVER_PORT,
        RR3_BOARD_ROUTES.len()
    );
    Ok(())
}
