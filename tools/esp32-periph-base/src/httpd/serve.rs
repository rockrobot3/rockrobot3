//! unsafe 壳层：WiFi STA 起栈、httpd_start 与白名单 endpoint 注册。

use super::*;

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
/// STA netif 句柄（起栈时写一次——等连判据 `esp_netif_get_ip_info` 查 IP 用；
/// 存 `usize` 绕裸指针非 Send，解引用处均在 unsafe 集中段）。
static STA_NETIF: OnceLock<usize> = OnceLock::new();

/// v5.2.2 `WIFI_INIT_CONFIG_DEFAULT()` 宏展开手抄（[`wifi_init_config_t`]）——
/// 宏不进绑定，裸 sys 必须手抄；字段真值逐项对表**本项目生成 sdkconfig**
/// （target …/esp-idf-sys out/sdkconfig，版本组合已钉 v5.2.2）＋ esp_wifi.h
/// `:252` 宏体，升级 IDF 版本须同批重抄（版本钉死登记表口径）。
unsafe fn wifi_default_config() -> wifi_init_config_t {
    wifi_init_config_t {
        // &g_wifi_osi_funcs（esp_wifi.h :253）
        osi_funcs: std::ptr::addr_of_mut!(g_wifi_osi_funcs),
        // g_wifi_default_wpa_crypto_funcs（:254）
        wpa_crypto_funcs: g_wifi_default_wpa_crypto_funcs,
        static_rx_buf_num: 10,     // CONFIG_ESP_WIFI_STATIC_RX_BUFFER_NUM
        dynamic_rx_buf_num: 32,    // CONFIG_ESP_WIFI_DYNAMIC_RX_BUFFER_NUM
        tx_buf_type: 1,            // CONFIG_ESP_WIFI_TX_BUFFER_TYPE（动态）
        static_tx_buf_num: 0,      // CONFIG_ESP_WIFI_STATIC_TX_BUFFER 未设（动态模式，宏 → 0）
        dynamic_tx_buf_num: 32,    // CONFIG_ESP_WIFI_DYNAMIC_TX_BUFFER_NUM
        rx_mgmt_buf_type: 0,       // CONFIG_ESP_WIFI_DYNAMIC_RX_MGMT_BUF=0（静态）
        rx_mgmt_buf_num: 5,        // CONFIG_ESP_WIFI_RX_MGMT_BUF_NUM_DEF
        cache_tx_buf_num: 0,       // ESP32 无 CACHE_TX（宏 → 0）
        csi_enable: 0,             // CONFIG_ESP_WIFI_CSI_ENABLED 未设
        ampdu_rx_enable: 1,        // CONFIG_ESP_WIFI_AMPDU_RX_ENABLED=y
        ampdu_tx_enable: 1,        // CONFIG_ESP_WIFI_AMPDU_TX_ENABLED=y
        amsdu_tx_enable: 0,        // CONFIG_ESP_WIFI_AMSDU_TX_ENABLED 未设
        nvs_enable: 1,             // CONFIG_ESP_WIFI_NVS_ENABLED=y
        nano_enable: 0,            // CONFIG_ESP_WIFI_NANO_FORMAT_ENABLED 未设
        rx_ba_win: 6,              // CONFIG_ESP_WIFI_RX_BA_WIN
        wifi_task_core_id: 0,      // CONFIG_ESP_WIFI_TASK_PINNED_TO_CORE_0=y（宏 :196）
        beacon_max_len: 752,       // CONFIG_ESP_WIFI_SOFTAP_BEACON_MAX_LEN
        mgmt_sbuf_num: 32,         // CONFIG_ESP_WIFI_MGMT_SBUF_NUM
        feature_caps: 1,           // WIFI_FEATURE_CAPS = WPA3_SAE(1<<0)｜SPIRAM 0｜FTM×2 0
        sta_disconnected_pm: true, // CONFIG_ESP_WIFI_STA_DISCONNECTED_PM_ENABLE=y
        espnow_max_encrypt_num: 7, // CONFIG_ESP_WIFI_ESPNOW_MAX_ENCRYPT_NUM
        magic: WIFI_INIT_CONFIG_MAGIC as i32, // 0x1F2F3F4F——esp_wifi_init 魔法校验
    }
}

/// 统一错误应答（`msg` 须含 NUL 结尾；文案由调用点给，逐字保留语义）。
unsafe fn resp_err(req: *mut httpd_req_t, code: httpd_err_code_t, msg: &'static [u8]) -> i32 {
    httpd_resp_send_err(req, code, msg.as_ptr() as *const _)
}

/// JSON 应答（`application/json`；body 由调用点组好，出锁纪律在调用点）。
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
unsafe fn req_uri(req: *mut httpd_req_t) -> String {
    std::ffi::CStr::from_ptr((*req).uri.as_ptr())
        .to_string_lossy()
        .into_owned()
}

/// 请求 method 归一（批N14 件二：注册面挂 GET/POST 两行——注册面未挂的 method
/// 判 "?"，路由纯函数必 None fail-closed 404；req.method 是 c_int，绑定常量是
/// http_method=u32——比较处 as i32 对齐）。
unsafe fn req_method(req: *mut httpd_req_t) -> &'static str {
    match (*req).method {
        m if m == http_method_HTTP_GET as i32 => "GET",
        m if m == http_method_HTTP_POST as i32 => "POST",
        _ => "?",
    }
}

/// GET /——板端配网页。
unsafe fn handle_index(req: *mut httpd_req_t) -> i32 {
    httpd_resp_set_type(req, c"text/html; charset=utf-8".as_ptr());
    httpd_resp_send(
        req,
        RR3_BOARD_INDEX_HTML.as_ptr() as *const _,
        RR3_BOARD_INDEX_HTML.len() as _,
    )
}

/// GET /api/board/state——出锁组 JSON（快照取窗内不组串，模块文档纪律）。
unsafe fn handle_state(req: *mut httpd_req_t, bus: &Arc<Mutex<Bus>>) -> i32 {
    resp_json(req, &rr3_state_json(&rr3_state_snapshot(bus)))
}

/// GET /api/board/bindings——注入绑定读数。
unsafe fn handle_bindings(req: *mut httpd_req_t) -> i32 {
    let snap = rr3_bindings_snap();
    resp_json(req, &rr3_bindings_json(snap.as_ref()))
}

/// POST /api/board/wifi——事务性配网写口（批N14 件二）：body 有界收（fail-closed）
/// → 鉴权 → 解析 → 形制 → 事务；HTTP 层失败 400/401 拒收，业务层失败一律 200
/// JSON 如实应答。
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

/// STA 凭据装配（start_httpd 与写口 apply/回退共用；union 起底——sta 臂其余
/// 字段零 = 默认；调用前形制已验 32/64 上限＝`sta.ssid[32]`/`sta.password[64]`
/// 数组尺寸，copy_from_slice 不越界）。返回 esp_err。
/// SoftAP 配置（2026-10-31 独立形态配网面）：AP 常开，SSID/口令由编译期
/// `RR3_BOARD_AP_SSID`/`RR3_BOARD_AP_PASS` 回落缺省（"RR3-BOARD"/"rock3robot"，
/// 非秘密、仓内公开——目的只是局域网配网入口，不承载业务凭据）。
/// 返回 AP 实际 SSID 便于 boot 横幅点名；`Err(i32)` = 某步 ESP-IDF 拒。
unsafe fn wifi_ap_configure() -> Result<&'static str, i32> {
    let ssid = option_env!("RR3_BOARD_AP_SSID").unwrap_or("RR3-BOARD");
    let pass = option_env!("RR3_BOARD_AP_PASS").unwrap_or("rock3robot");
    let s = ssid.as_bytes();
    let p = pass.as_bytes();
    // 形制：SSID ≤32B（IDF 硬限）、WPA2 口令 8..=63B
    if s.is_empty() || s.len() > 32 || p.len() < 8 || p.len() > 63 {
        return Err(-1);
    }
    if esp_netif_create_default_wifi_ap().is_null() {
        return Err(-1);
    }
    let mut cfg: wifi_config_t = std::mem::zeroed();
    cfg.ap.ssid[..s.len()].copy_from_slice(s);
    cfg.ap.password[..p.len()].copy_from_slice(p);
    cfg.ap.ssid_len = s.len() as u8;
    cfg.ap.channel = 1;
    cfg.ap.authmode = wifi_auth_mode_t_WIFI_AUTH_WPA2_PSK;
    cfg.ap.max_connection = 4;
    let r = esp_wifi_set_config(wifi_interface_t_WIFI_IF_AP, &mut cfg);
    if r != ESP_OK {
        return Err(r);
    }
    Ok(ssid)
}

unsafe fn wifi_sta_set_cred(ssid: &[u8], pass: &[u8]) -> i32 {
    // FFI 边界自守（静态审计 B16）：定长数组越界复制＝panic＝整板 abort，这里把
    // "上层忘了校形"折成错误码走事务回退，永不 panic（读窄缝已同域校验，本关是
    // 第二道墙）。
    if ssid.len() > crate::cred::RR3_CRED_SSID_MAX || pass.len() > crate::cred::RR3_CRED_PASS_MAX {
        return -1; // 形制域内错误（非 esp_err 码空间，同 cred 模块口径）
    }
    let mut wcfg: wifi_config_t = std::mem::zeroed();
    wcfg.sta.ssid[..ssid.len()].copy_from_slice(ssid);
    wcfg.sta.password[..pass.len()].copy_from_slice(pass);
    esp_wifi_set_config(wifi_interface_t_WIFI_IF_STA, &mut wcfg)
}

/// 有界等连（连上＝**DHCP 得 IP**，判据见函数内注记）：[`WIFI_CONNECT_WAIT_MS`]
/// 上限、[`WIFI_POLL_MS`] 步进；超窗即 false（调用方如实应答——不猜不造）。
unsafe fn wifi_sta_wait_conn() -> bool {
    // 判据＝**DHCP 得 IP**（`esp_netif_get_ip_info` 的 ip.addr 非 0），批N15 件二
    // 真板取证实锤的缺陷修复：原判据 `esp_wifi_sta_get_ap_info` 在关联瞬态
    // （auth/assoc→run，4-way 握手前）即返 ESP_OK——坏凭据被误判成功→不回退→
    // NVS 残留坏凭据＝真砖路径（真板波形：关联 run 后 3s 被 AP 踢回 init 反复）。
    // 密码错永远到不了 DHCP，本判据物理可靠。
    let netif = match STA_NETIF.get() {
        Some(p) => *p as *mut esp_netif_t,
        None => return false, // netif 未建（时序防御，常态不可达）
    };
    for _ in 0..(WIFI_CONNECT_WAIT_MS / WIFI_POLL_MS) {
        let mut info: esp_netif_ip_info_t = std::mem::zeroed();
        if esp_netif_get_ip_info(netif, &mut info) == ESP_OK && info.ip.addr != 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(WIFI_POLL_MS));
    }
    false
}

/// 配网事务（批N14 件二，fail-safe 方向——消灭"写坏凭据板掉网"砖路径）：
/// 读旧值 → 写 NVS → apply（disconnect→set→connect）→ 有界等连 → 败即回退
/// 旧值（Some→写回 / None→erase 回原空态）重连 → 如实应答。全程**不持 bus 锁**
/// （不碰 [`crate::state::PeriphState`]）；httpd 服务任务串行处理请求＝两写口
/// 天然互斥。回退写败＝砖路径残余（新凭据在 NVS 但未连——重启后读链装配新凭据
/// 重试，AP 不可达则掉网）：NVS 写败罕见链，如实点名 stage 不冒领。
unsafe fn rr3_wifi_write_apply(ssid: &str, pass: &str) -> Rr3WifiWriteResult {
    let old = crate::cred::rr3_cred_read(); // None＝原空态（回退目标＝erase）
                                            // 写 NVS（cred 模块内复验形制——毒不双入；败＝NVS 未动无需回退）
    if crate::cred::rr3_cred_write(ssid, pass).is_err() {
        return Rr3WifiWriteResult::rejected("nvs");
    }
    // apply 新凭据（disconnect 返回值不看——未连态不阻断 apply 链；
    // set/connect 任一败一律进回退分支）
    esp_wifi_disconnect();
    let mut apply_ok = wifi_sta_set_cred(ssid.as_bytes(), pass.as_bytes()) == ESP_OK;
    if apply_ok {
        apply_ok = esp_wifi_connect() == ESP_OK;
    }
    if apply_ok && wifi_sta_wait_conn() {
        return Rr3WifiWriteResult {
            ok: true,
            connected: true,
            reverted: false,
            stage: "apply",
        };
    }
    // 新凭据未连——回退旧值（Some→写回；None→erase；cred 模块幂等语义在案）
    let revert_ok = match &old {
        Some(c) => crate::cred::rr3_cred_write(&c.ssid, c.pass()).is_ok(),
        None => crate::cred::rr3_cred_erase().is_ok(),
    };
    if !revert_ok {
        return Rr3WifiWriteResult {
            ok: false,
            connected: false,
            reverted: false,
            stage: "revert",
        };
    }
    // 回退写成功——apply 旧凭据重连（原空态＝停在断态，不 connect）
    match &old {
        Some(c) => {
            esp_wifi_disconnect();
            if wifi_sta_set_cred(c.ssid.as_bytes(), c.pass().as_bytes()) == ESP_OK {
                let _ = esp_wifi_connect();
            }
        }
        None => {
            esp_wifi_disconnect();
        }
    }
    let connected = wifi_sta_wait_conn();
    Rr3WifiWriteResult {
        ok: true,
        connected,
        reverted: true,
        stage: "reverted",
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

/// WiFi 起栈链（netif/事件循环/宏配置/APSTA/AP/STA/start/connect），返回 AP 实际
/// SSID 供 boot 横幅点名；逐步 fail-honest（错误码由调用方点名）。
unsafe fn wifi_bringup(ssid: &[u8], pass: &[u8]) -> Result<&'static str, i32> {
    // ① netif + 事件循环 + STA 缺省 netif（句柄留存——等连判据查 IP 用）
    esp_ok(esp_netif_init())?;
    esp_ok(esp_event_loop_create_default())?;
    let _ = STA_NETIF.set(esp_netif_create_default_wifi_sta() as usize);
    // ② WiFi 起栈（手抄宏配置见 wifi_default_config）
    let cfg = wifi_default_config();
    esp_ok(esp_wifi_init(&cfg))?;
    // **模式必须先于接口配置**（2026-10-31 真板排查）：IDF 的 esp_wifi_set_config
    // 按接口查"该接口是否已在当前模式里启用"，模式仍是缺省 STA 时配 AP 接口返回
    // ESP_ERR_INVALID_ARG——表现为配置腿整段起不来、板侧 AP 不可见，而其它功能
    // 全部正常（板活着但不配网，排查极易误判成 SoftAP 支持未编入）。
    esp_ok(esp_wifi_set_mode(wifi_mode_t_WIFI_MODE_APSTA))?;
    // AP 常开（SoftAP 配网面，见 wifi_ap_configure 注）：netif 与 config 须在
    // esp_wifi_start 前就位——IDF 在 start 时才拉起 AP/DHCP 服务。
    let ap_ssid = wifi_ap_configure()?;
    // STA 凭据（helper 共用——写口 apply/回退同款；位形已在 spawn 前验）；
    // ap-only 档空凭据不配置 STA，只起 AP
    if !ssid.is_empty() {
        esp_ok(wifi_sta_set_cred(ssid, pass))?;
    }
    esp_ok(esp_wifi_start())?;
    // STA 关联发起（批N15 件二真板取证实锤的缺陷修复：esp_wifi_start 只起驱动
    // 不自动连 AP——缺此步板永不关联、无 got ip；写口 apply/回退两路本就有
    // connect，唯 boot 路漏）。ap-only 档无 STA 目标，跳过。
    if !ssid.is_empty() {
        esp_ok(esp_wifi_connect())?;
    }
    Ok(ap_ssid)
}

/// httpd 起 + 白名单注册（手填 config：HTTPD_DEFAULT_CONFIG 真值——宏默认也是
/// false/null/0 的字段由全零起底等价覆盖，差异字段逐项对表 esp_http_server.h :53；
/// OnceLock 先写——handler 可达即 bus 在位；method 映射随表行 GET/POST，表内词表
/// 由 covers_wordform_shape 单测钉死，词表外 panic 不可达）。
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
