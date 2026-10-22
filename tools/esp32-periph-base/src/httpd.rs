//! 板侧配置腿服务（N-13 服务本体，批N13 件一；connect 册 N-13 / 全案 §7 F-18）。
//!
//! rtos 形态"自己的配置/观测腿"——不经桥（[`crate::frame`] 的 `rr3.set_bindings`
//! 是 host **推给**板的另一条腿，基-24），板自己在 WiFi 上开极简 HTTP 只读面。
//! 载体选型随批N4 件二定案：ESP-IDF 内置组件 `esp_http_server`（HTTP/1.1 子集：
//! GET + JSON，与 host 面板 [server.rs](../../../modules/rr3-interact/src/panel/server.rs)
//! 同形）；**TLS 不做**（四条理由在案：局域网可信面同 host 面板 Q3 裁定／板侧无
//! TLS 栈现役，引 mbedtls 堆账不可收／零新依赖纪律／公网暴露立项时与凭据面同批
//! 评审）——凭据面本件**不接 N-11**。
//!
//! 分层（机制与策略分离）：
//! - **纯逻辑层**（单测可钉）：白名单路由表 [`RR3_BOARD_ROUTES`]（与 host
//!   `RR3_PANEL_ROUTES` 四列同构）＋路由纯函数 [`rr3_board_route`]＋快照 JSON
//!   组包纯函数。表↔分派零漂移由 `route_table_matches_dispatch` 单测钉死
//!   （同 host `route_table_matches_dispatch` 形状）。
//! - **unsafe 壳层**（[`rr3_board_httpd_spawn`]）：WiFi STA 起栈（裸 sys——
//!   [`wifi_init_config_t`] 手抄 v5.2.2 `WIFI_INIT_CONFIG_DEFAULT()` 宏展开，
//!   字段真值逐项对表本项目生成 sdkconfig，见 [`wifi_default_config`]）＋
//!   `httpd_start` ＋注册白名单 handler。
//!
//! 凭据：**编译期注入**（`RR3_BOARD_WIFI_SSID` / `RR3_BOARD_WIFI_PASS` 环境变量，
//! `option_env!` 读，值不落仓）——板侧凭据存储对象是 N-10 ②/N-11 的工单（另批），
//! 本件不引入运行时凭据面。**未装配（缺省/空）＝配置腿缺席**：boot 点名后
//! fail-honest 返回，不阻塞既有链路（与"不立做不到的判据"同纪律——行为取证
//! 归真板验证窗）。
//!
//! 账面：httpd 任务栈 16KB（[`HTTPD_STACK_SIZE`]，`task_stack_budget_bytes =
//! 65536` 容既有两 16KB 线程＋本面余量充足；起栈线程短命同栈）；WiFi/httpd
//! 常驻堆从批N4 件二预留 16384 出——**取值对账归真板**（`esp_get_free_heap_size`
//! 实测，`heap_reserved_bytes` 回填口径已注明，WiFi 静态缓冲 ~16KB 量级＋
//! httpd 会话面，预留是否充足以真板实测为准，本件如实留痕不冒领）。

use std::ffi::CString;
use std::sync::{Arc, Mutex, OnceLock};

use esp_idf_sys::{
    esp_event_loop_create_default, esp_netif_create_default_wifi_sta, esp_netif_init,
    esp_wifi_init, esp_wifi_set_config, esp_wifi_set_mode, esp_wifi_start, g_wifi_default_wpa_crypto_funcs,
    g_wifi_osi_funcs, http_method_HTTP_GET, httpd_config_t, httpd_err_code_t_HTTPD_404_NOT_FOUND,
    httpd_handle_t, httpd_register_uri_handler, httpd_req_t,
    httpd_resp_send, httpd_resp_send_err, httpd_resp_set_type, httpd_start, httpd_uri_t,
    nvs_flash_init, wifi_config_t, wifi_init_config_t, wifi_interface_t_WIFI_IF_STA,
    wifi_mode_t_WIFI_MODE_STA, WIFI_INIT_CONFIG_MAGIC, ESP_OK,
};

use crate::state::{lock_bus, Bus};

// ---------------------------------------------------------------------------
// 纯逻辑层：白名单路由表 + 路由纯函数 + 快照 JSON 组包（单测可钉）
// ---------------------------------------------------------------------------

/// 端点白名单（与 host `RR3_PANEL_ROUTES` 四列同构：method / path / covers / applies）。
/// **首笔只读**：写口（POST 改配置）并发面与凭据面另批。covers 列＝该端点读的
/// ⑩ 配置模型字段（`<路>/<字段>`）；本批两端点读的是板侧运行态与注入绑定
/// （**不在** ⑩ 分母五路——绑定十键是 rr-ipc 注入键非配置模型 [[pin]] 词表），
/// covers 全空＋applies 全空如实登记；件二把本表区段扩进 check.sh ⑩ 扫描源，
/// 执法位＝未来板侧端点读五路字段而不申报即红（对拍判据 ⊆ 分母五路并集）。
pub(crate) const RR3_BOARD_ROUTES: &[(&str, &str, &[&str], &str)] = &[
    ("GET", "/api/board/state", &[], ""),
    ("GET", "/api/board/bindings", &[], ""),
];

/// 路由分派产物（表内端点；`None` = 白名单外，fail-closed 回 404）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rr3BoardGet {
    /// 板侧运行态观测快照（`PeriphState` 观测字段，批N12 件一观测面含其中）
    State,
    /// 注入绑定十键读数（批N9 件五对账锚同源）
    Bindings,
}

/// 路由纯函数：method + path → 分派。**白名单 = [`RR3_BOARD_ROUTES`]**——
/// method 或 path 任一不在表内即 `None`（fail-closed，同 host 面板语义；
/// 405 细分不做——首笔只读，GET 之外的 method 一律白名单外）。
pub(crate) fn rr3_board_route(method: &str, path: &str) -> Option<Rr3BoardGet> {
    match (method, path) {
        ("GET", "/api/board/state") => Some(Rr3BoardGet::State),
        ("GET", "/api/board/bindings") => Some(Rr3BoardGet::Bindings),
        _ => None,
    }
}

/// 板侧运行态快照（锁窗内 Copy 出，出锁组 JSON——**不持锁出 JSON** 纪律）。
pub(crate) struct Rr3BoardStateSnapshot {
    pub(crate) pan_deg: u32,
    pub(crate) tilt_deg: u32,
    pub(crate) frame_mode: bool,
    pub(crate) expr_mode: bool,
    pub(crate) wander_on: bool,
    pub(crate) rail_mv: u16,
    pub(crate) rail_raw: u16,
    pub(crate) rail_tier: &'static str,
    pub(crate) rail_low_events: u32,
    pub(crate) move_fail_events: u32,
    pub(crate) selfheal_skipped: u32,
    pub(crate) binding_mismatch: u32,
    pub(crate) audio_on: bool,
    pub(crate) audio_seq: u32,
    pub(crate) audio_sent: u64,
    pub(crate) audio_dropped: u64,
}

/// 快照取窗（锁内只 Copy/查名，不组串）。
fn rr3_state_snapshot(bus: &Arc<Mutex<Bus>>) -> Rr3BoardStateSnapshot {
    let b = lock_bus(bus);
    let st = &b.st;
    Rr3BoardStateSnapshot {
        pan_deg: st.pan_deg,
        tilt_deg: st.tilt_deg,
        frame_mode: st.frame_mode,
        expr_mode: st.expr_mode,
        wander_on: st.wander_on,
        rail_mv: st.rail_mv,
        rail_raw: st.rail_raw,
        rail_tier: st.rail_tier.name(),
        rail_low_events: st.rail_low_events,
        move_fail_events: st.move_fail_events,
        selfheal_skipped: st.selfheal_skipped,
        binding_mismatch: st.binding_mismatch,
        audio_on: st.audio_on,
        audio_seq: st.audio_seq,
        audio_sent: st.audio_sent,
        audio_dropped: st.audio_dropped,
    }
}

/// 运行态快照 → JSON（纯函数，单测钉形状）。
pub(crate) fn rr3_state_json(s: &Rr3BoardStateSnapshot) -> String {
    serde_json::json!({
        "pan_deg": s.pan_deg,
        "tilt_deg": s.tilt_deg,
        "frame_mode": s.frame_mode,
        "expr_mode": s.expr_mode,
        "wander_on": s.wander_on,
        "rail_mv": s.rail_mv,
        "rail_raw": s.rail_raw,
        "rail_tier": s.rail_tier,
        "rail_low_events": s.rail_low_events,
        "move_fail_events": s.move_fail_events,
        "selfheal_skipped": s.selfheal_skipped,
        "binding_mismatch": s.binding_mismatch,
        "audio_on": s.audio_on,
        "audio_seq": s.audio_seq,
        "audio_sent": s.audio_sent,
        "audio_dropped": s.audio_dropped,
    })
    .to_string()
}

/// 注入绑定十键快照（state.rs 读窄缝自持锁、逐键 Option<u8> 抄出；`None` = 该键未注入）。
fn rr3_bindings_snapshot() -> Option<[(&'static str, Option<u8>); 10]> {
    use crate::state::rr3_board_bindings_snapshot as raw;
    let b = raw()?;
    Some([
        ("i2c_sda", b.i2c_sda),
        ("i2c_scl", b.i2c_scl),
        ("adc_rail_pin", b.adc_rail_pin),
        ("uart2_tx", b.uart2_tx),
        ("uart2_rx", b.uart2_rx),
        ("ledc_pan", b.ledc_pan),
        ("ledc_tilt", b.ledc_tilt),
        ("i2s_bclk", b.i2s_bclk),
        ("i2s_sd", b.i2s_sd),
        ("i2s_ws", b.i2s_ws),
    ])
}

/// 绑定快照 → JSON（纯函数；未注入整体 `null`，对账锚读数语义同源）。
pub(crate) fn rr3_bindings_json(b: Option<[(&str, Option<u8>); 10]>) -> String {
    match b {
        None => "null".into(),
        Some(pairs) => {
            let obj: serde_json::Map<String, serde_json::Value> = pairs
                .into_iter()
                .map(|(k, v)| {
                    (
                        k.to_string(),
                        v.map_or(serde_json::Value::Null, serde_json::Value::from),
                    )
                })
                .collect();
            serde_json::Value::Object(obj).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表 ↔ 分派零漂移（同 host `route_table_matches_dispatch` 形状）：表内每行
    /// 必命中、表外 path/method 必 None——防"表加行忘加臂/臂漂移"。
    #[test]
    fn route_table_matches_dispatch() {
        for (method, path, _, _) in RR3_BOARD_ROUTES {
            let hit = rr3_board_route(method, path);
            assert!(hit.is_some(), "表内行未命中分派: {method} {path}");
        }
        assert_eq!(rr3_board_route("GET", "/api/board/nope"), None);
        assert_eq!(rr3_board_route("POST", "/api/board/state"), None);
        assert_eq!(rr3_board_route("GET", "/api/brain/ops"), None);
        assert_eq!(rr3_board_route("GET", "/api/board/state/extra"), None);
    }

    /// covers 形制（同 check.sh ⑩ 扫描执法语义）：带斜杠且不以 `/` 开头；
    /// applies 只收 `live`/空。本批全空＝平凡过，执法位防未来加错形状。
    #[test]
    fn covers_wordform_shape() {
        for (method, path, covers, applies) in RR3_BOARD_ROUTES {
            assert!(!method.is_empty() && !path.is_empty(), "空行: {method} {path}");
            assert!(
                applies.is_empty() || *applies == "live",
                "applies 形制乱写: {path} {applies}"
            );
            for c in *covers {
                assert!(
                    c.contains('/') && !c.starts_with('/'),
                    "covers 形制乱写: {path} {c}（应为 <路>/<字段>）"
                );
            }
        }
    }

    /// state JSON 形状：字段名/类型钉死（宿主消费面契约面）。
    #[test]
    fn state_json_shape() {
        let s = Rr3BoardStateSnapshot {
            pan_deg: 90,
            tilt_deg: 91,
            frame_mode: false,
            expr_mode: true,
            wander_on: false,
            rail_mv: 5123,
            rail_raw: 2048,
            rail_tier: "Ok",
            rail_low_events: 0,
            move_fail_events: 2,
            selfheal_skipped: 0,
            binding_mismatch: 1,
            audio_on: true,
            audio_seq: 7,
            audio_sent: 100,
            audio_dropped: 3,
        };
        let v: serde_json::Value = serde_json::from_str(&rr3_state_json(&s)).unwrap();
        assert_eq!(v["pan_deg"], 90);
        assert_eq!(v["rail_tier"], "Ok");
        assert_eq!(v["move_fail_events"], 2);
        assert_eq!(v["binding_mismatch"], 1);
        assert_eq!(v["audio_sent"], 100);
        assert_eq!(v["expr_mode"], true);
        assert_eq!(v.as_object().unwrap().len(), 16);
    }

    /// bindings JSON 形状：未注入整体 null；注入态十键全名＋None→null。
    #[test]
    fn bindings_json_shape() {
        assert_eq!(rr3_bindings_json(None), "null");
        let v: serde_json::Value = serde_json::from_str(&rr3_bindings_json(Some([
            ("i2c_sda", Some(21)),
            ("i2c_scl", Some(22)),
            ("adc_rail_pin", Some(34)),
            ("uart2_tx", Some(18)),
            ("uart2_rx", Some(19)),
            ("ledc_pan", Some(32)),
            ("ledc_tilt", Some(33)),
            ("i2s_bclk", Some(4)),
            ("i2s_sd", Some(27)),
            ("i2s_ws", None),
        ])))
        .unwrap();
        assert_eq!(v["i2c_sda"], 21);
        assert_eq!(v["i2s_ws"], serde_json::Value::Null);
        assert_eq!(v.as_object().unwrap().len(), 10);
    }
}

// ---------------------------------------------------------------------------
// unsafe 壳层：WiFi STA 起栈（裸 sys）+ httpd_start + 白名单注册
// ---------------------------------------------------------------------------

/// httpd 任务栈（账面见模块文档；同 THREAD_STACK 口径）。
const HTTPD_STACK_SIZE: usize = 16384;
/// HTTP 监听口（板侧只读面，80 与 host 面板同档；端口号住常量＝资源参数
/// 同型病例的既有口径，本件不定新配置键）。
const HTTPD_SERVER_PORT: u16 = 80;
/// WiFi 凭据环境变量（编译期注入，值不落仓——见模块文档凭据节）。
const RR3_BOARD_WIFI_SSID_ENV: &str = "RR3_BOARD_WIFI_SSID";
const RR3_BOARD_WIFI_PASS_ENV: &str = "RR3_BOARD_WIFI_PASS";

/// bus 单点（handler 裸壳取态的唯一入口——OnceLock 存 Arc clone，无裸指针
/// user_ctx；板侧单点先例 = [`crate::state::BOARD_BINDINGS`] 同形状）。
static HTTPD_BUS: OnceLock<Arc<Mutex<Bus>>> = OnceLock::new();

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
        static_rx_buf_num: 10,   // CONFIG_ESP_WIFI_STATIC_RX_BUFFER_NUM
        dynamic_rx_buf_num: 32,  // CONFIG_ESP_WIFI_DYNAMIC_RX_BUFFER_NUM
        tx_buf_type: 1,          // CONFIG_ESP_WIFI_TX_BUFFER_TYPE（动态）
        static_tx_buf_num: 0,    // CONFIG_ESP_WIFI_STATIC_TX_BUFFER 未设（动态模式，宏 → 0）
        dynamic_tx_buf_num: 32,  // CONFIG_ESP_WIFI_DYNAMIC_TX_BUFFER_NUM
        rx_mgmt_buf_type: 0,     // CONFIG_ESP_WIFI_DYNAMIC_RX_MGMT_BUF=0（静态）
        rx_mgmt_buf_num: 5,      // CONFIG_ESP_WIFI_RX_MGMT_BUF_NUM_DEF
        cache_tx_buf_num: 0,     // ESP32 无 CACHE_TX（宏 → 0）
        csi_enable: 0,           // CONFIG_ESP_WIFI_CSI_ENABLED 未设
        ampdu_rx_enable: 1,      // CONFIG_ESP_WIFI_AMPDU_RX_ENABLED=y
        ampdu_tx_enable: 1,      // CONFIG_ESP_WIFI_AMPDU_TX_ENABLED=y
        amsdu_tx_enable: 0,      // CONFIG_ESP_WIFI_AMSDU_TX_ENABLED 未设
        nvs_enable: 1,           // CONFIG_ESP_WIFI_NVS_ENABLED=y
        nano_enable: 0,          // CONFIG_ESP_WIFI_NANO_FORMAT_ENABLED 未设
        rx_ba_win: 6,            // CONFIG_ESP_WIFI_RX_BA_WIN
        wifi_task_core_id: 0,    // CONFIG_ESP_WIFI_TASK_PINNED_TO_CORE_0=y（宏 :196）
        beacon_max_len: 752,     // CONFIG_ESP_WIFI_SOFTAP_BEACON_MAX_LEN
        mgmt_sbuf_num: 32,       // CONFIG_ESP_WIFI_MGMT_SBUF_NUM
        feature_caps: 1,         // WIFI_FEATURE_CAPS = WPA3_SAE(1<<0)｜SPIRAM 0｜FTM×2 0
        sta_disconnected_pm: true, // CONFIG_ESP_WIFI_STA_DISCONNECTED_PM_ENABLE=y
        espnow_max_encrypt_num: 7, // CONFIG_ESP_WIFI_ESPNOW_MAX_ENCRYPT_NUM
        magic: WIFI_INIT_CONFIG_MAGIC as i32, // 0x1F2F3F4F——esp_wifi_init 魔法校验
    }
}

/// 白名单统一 handler：req→路由纯函数→快照 JSON 应答。表外 404 fail-closed。
unsafe extern "C" fn rr3_board_httpd_handler(req: *mut httpd_req_t) -> i32 {
    let Some(bus) = HTTPD_BUS.get() else {
        // 未装配即被请求到（时序防御）：OnceLock 在 httpd_start 前写入，常态不可达
        return httpd_resp_send_err(
            req,
            httpd_err_code_t_HTTPD_404_NOT_FOUND,
            b"board bus unavailable\0".as_ptr() as *const _,
        );
    };
    let path = std::ffi::CStr::from_ptr((*req).uri.as_ptr())
        .to_string_lossy()
        .into_owned();
    let method = "GET"; // 注册面只挂 GET 行——method 分派在注册面，此处恒 GET
    let Some(get) = rr3_board_route(method, &path) else {
        return httpd_resp_send_err(
            req,
            httpd_err_code_t_HTTPD_404_NOT_FOUND,
            b"not in board route whitelist\0".as_ptr() as *const _,
        );
    };
    // 出锁组 JSON（快照取窗内不组串——模块文档纪律）
    let body = match get {
        Rr3BoardGet::State => rr3_state_json(&rr3_state_snapshot(bus)),
        Rr3BoardGet::Bindings => rr3_bindings_json(rr3_bindings_snapshot()),
    };
    httpd_resp_set_type(req, c"application/json".as_ptr());
    httpd_resp_send(req, body.as_ptr() as *const _, body.len() as _)
}

/// 起板侧配置腿（main 挂载点；**fail-honest**——任何一步失败点名后返回，
/// 配置腿缺席不阻塞既有链路；凭据未装配同口径）。
pub(crate) fn rr3_board_httpd_spawn(bus: &Arc<Mutex<Bus>>) {
    let bus = Arc::clone(bus);
    // 凭据编译期注入检查（值不落仓）；未装配＝配置腿缺席，点名即可
    let ssid = match std::env::var(RR3_BOARD_WIFI_SSID_ENV) {
        Ok(s) if !s.is_empty() => s,
        _ => {
            eprintln!(
                "[periph] 配置腿未装配（{RR3_BOARD_WIFI_SSID_ENV} 未注入）——N-13 服务面缺席，板其余功能照常"
            );
            return;
        }
    };
    let pass = std::env::var(RR3_BOARD_WIFI_PASS_ENV).unwrap_or_default();
    if ssid.len() > 32 || pass.len() > 64 {
        eprintln!(
            "[periph] 配置腿装配失败（SSID {}B/密码 {}B 超位形 32/64）——缺席照常",
            ssid.len(),
            pass.len()
        );
        return;
    }
    let ssid = ssid.as_bytes().to_vec();
    let pass = pass.as_bytes().to_vec();
    thread_spawn(bus, ssid, pass);
}

/// 起栈线程（与控制/运行面同 16KB 口径；短命——起完 httpd 即退）。
fn thread_spawn(bus: Arc<Mutex<Bus>>, ssid: Vec<u8>, pass: Vec<u8>) {
    std::thread::Builder::new()
        .name("periph-httpd".into())
        .stack_size(crate::board::THREAD_STACK)
        .spawn(move || unsafe {
            if let Err(e) = start_httpd(&bus, &ssid, &pass) {
                eprintln!("[periph] 配置腿启动失败（缺席照常）: {e:?}");
            }
        })
        .expect("起配置腿线程失败");
}

/// WiFi STA 起栈 + httpd_start + 白名单注册（unsafe 集中段；逐步 fail-honest）。
unsafe fn start_httpd(bus: &Arc<Mutex<Bus>>, ssid: &[u8], pass: &[u8]) -> Result<(), i32> {
    // 逐步 helper：非 ESP_OK 即 Err（fail-honest，点名在调用方）
    fn chk(r: i32) -> Result<(), i32> {
        if r == ESP_OK {
            Ok(())
        } else {
            Err(r)
        }
    }
    // ① NVS（WiFi 校准/存储依赖；失败不 erase——不毁既有分区，点名缺席）
    chk(nvs_flash_init())?;
    // ② netif + 事件循环 + STA 缺省 netif
    chk(esp_netif_init())?;
    chk(esp_event_loop_create_default())?;
    esp_netif_create_default_wifi_sta();
    // ③ WiFi 起栈（手抄宏配置见 wifi_default_config）
    let cfg = wifi_default_config();
    chk(esp_wifi_init(&cfg))?;
    chk(esp_wifi_set_mode(wifi_mode_t_WIFI_MODE_STA))?;
    // STA 凭据（union 起底——sta 臂其余字段零 = 默认；位形已在 spawn 前验）
    let mut wcfg: wifi_config_t = std::mem::zeroed();
    wcfg.sta.ssid[..ssid.len()].copy_from_slice(ssid);
    wcfg.sta.password[..pass.len()].copy_from_slice(pass);
    chk(esp_wifi_set_config(wifi_interface_t_WIFI_IF_STA, &mut wcfg))?;
    chk(esp_wifi_start())?;
    // ④ httpd 起（手填 config：HTTPD_DEFAULT_CONFIG 真值——宏默认也是
    //   false/null/0 的字段由全零起底等价覆盖，差异字段逐项对表 esp_http_server.h :53）
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
    chk(httpd_start(&mut h, &conf))?;
    // ⑤ 白名单注册（统一 handler；OnceLock 先写——handler 可达即 bus 在位）
    let _ = HTTPD_BUS.set(Arc::clone(bus));
    for (method, path, _, _) in RR3_BOARD_ROUTES {
        debug_assert_eq!(*method, "GET", "首笔只读——注册面只挂 GET 行");
        let cpath = CString::new(*path).expect("白名单 path 不含 NUL");
        let uri = httpd_uri_t {
            uri: cpath.as_ptr(),
            method: http_method_HTTP_GET,
            handler: Some(rr3_board_httpd_handler),
            user_ctx: std::ptr::null_mut(),
        };
        chk(httpd_register_uri_handler(h, &uri))?;
    }
    println!(
        "[periph] 配置腿起（N-13）: http://0.0.0.0:{}/api/board/{{state,bindings}}（白名单 {} 端点，只读）",
        HTTPD_SERVER_PORT,
        RR3_BOARD_ROUTES.len()
    );
    Ok(())
}
