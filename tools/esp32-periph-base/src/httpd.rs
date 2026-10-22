//! 板侧配置腿服务（N-13 服务本体，批N13 件一；connect 册 N-13 / 全案 §7 F-18）。
//!
//! rtos 形态"自己的配置/观测腿"——不经桥（[`crate::frame`] 的 `rr3.set_bindings`
//! 是 host **推给**板的另一条腿，基-24），板自己在 WiFi 上开极简 HTTP 面。
//! 载体选型随批N4 件二定案：ESP-IDF 内置组件 `esp_http_server`（HTTP/1.1 子集：
//! GET/POST + JSON，与 host 面板 [server.rs](../../../modules/rr3-interact/src/panel/server.rs)
//! 同形）；**TLS 不做**（四条理由在案：局域网可信面同 host 面板 Q3 裁定／板侧无
//! TLS 栈现役，引 mbedtls 堆账不可收／零新依赖纪律／公网暴露立项时与凭据面同批
//! 评审）。
//!
//! 写口（批N14 件二，N-10 ② 配网半边）：**POST /api/board/wifi** 事务性配网——
//! 读旧值 → 写 NVS → STA 重配置重连 → 有界等连 → 败即回退旧值重连 → 如实应答
//! `{ok, connected, reverted, stage}`（fail-safe 方向：消灭"写坏凭据板掉网"砖
//! 路径；`connected` 字段＝实测等连结果，**永不因请求被接受而置真**，同 connect
//! RPC 纪律）；**POST /api/board/reboot** 动作面——应答先行落链再 `esp_restart`。
//! 事务全程**不持 bus 锁**（不碰 [`crate::state::PeriphState`]，观测面结构上无
//! WiFi 字段——零回显契约的第二道墙，[`crate::cred`] 模块文档）。
//!
//! 分层（机制与策略分离）：
//! - **纯逻辑层**（单测可钉）：白名单路由表 [`RR3_BOARD_ROUTES`]（与 host
//!   `RR3_PANEL_ROUTES` 四列同构）＋路由纯函数 [`rr3_board_route`]＋快照 JSON
//!   组包纯函数＋写口 body 解析/应答组包纯函数。表↔分派零漂移由
//!   `route_table_matches_dispatch` 单测钉死（同 host `route_table_matches_dispatch`
//!   形状）。
//! - **unsafe 壳层**（[`rr3_board_httpd_spawn`]）：WiFi STA 起栈（裸 sys——
//!   [`wifi_init_config_t`] 手抄 v5.2.2 `WIFI_INIT_CONFIG_DEFAULT()` 宏展开，
//!   字段真值逐项对表本项目生成 sdkconfig，见 [`wifi_default_config`]）＋
//!   `httpd_start` ＋注册白名单 handler。
//!
//! 凭据：**NVS 优先 → 编译期注入回落**（批N14 件一读链；存储对象见 [`crate::cred`]，
//! namespace `"rr3"` 单真值——N-10 ② 主体半边）：
//! - **NVS 优先**：`rr3_cred_read()` 读时现取（配网写口批N14 件二落值的消费点）；
//! - **编译期回落**：`option_env!` 读 `RR3_BOARD_WIFI_SSID`/`RR3_BOARD_WIFI_PASS`
//!   （构建期烘焙进 .rodata，值不落仓——**批N14 订正**：批N13 码面误用
//!   `std::env::var` 运行期读，而板上 IDF newlib 把 `environ` 初始化为空数组
//!   （newlib_init.c :169-174），运行期 env 永远缺席＝配置腿真机永不装配——文档
//!   所述"编译期注入"与码面不符，本批按文档口径落实）；
//! - 都缺＝**配置腿缺席**：boot 点名后 fail-honest 返回，不阻塞既有链路。
//!
//! 账面：httpd 任务栈 16KB（[`HTTPD_STACK_SIZE`]，`task_stack_budget_bytes =
//! 65536` 容既有两 16KB 线程＋本面余量充足；起栈线程短命同栈）；WiFi/httpd
//! 常驻堆从批N4 件二预留 16384 出——**取值对账归真板**（`esp_get_free_heap_size`
//! 实测，`heap_reserved_bytes` 回填口径已注明，WiFi 静态缓冲 ~16KB 量级＋
//! httpd 会话面，预留是否充足以真板实测为准，本件如实留痕不冒领）。写口瞬时
//! 面（批N14 件二；批N15 件二真板订正：等连判据改 `esp_netif_ip_info_t`
//! DHCP 得 IP——`esp_wifi_sta_get_ap_info` 关联瞬态误报缺陷在案）：body 收包
//! 缓冲 ≤512B＋`esp_netif_ip_info_t` ~16B 栈上临时——16KB 栈富余；事务不新增
//! 常驻堆。

use std::ffi::CString;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use esp_idf_sys::{
    esp_event_loop_create_default, esp_netif_create_default_wifi_sta, esp_netif_get_ip_info,
    esp_netif_init, esp_netif_ip_info_t, esp_netif_t, esp_restart, esp_wifi_connect,
    esp_wifi_disconnect, esp_wifi_init, esp_wifi_set_config, esp_wifi_set_mode, esp_wifi_start,
    g_wifi_default_wpa_crypto_funcs, g_wifi_osi_funcs, httpd_config_t,
    httpd_err_code_t_HTTPD_400_BAD_REQUEST, httpd_err_code_t_HTTPD_404_NOT_FOUND, httpd_handle_t,
    httpd_register_uri_handler, httpd_req_recv, httpd_req_t, httpd_resp_send,
    httpd_resp_send_err, httpd_resp_set_type, httpd_start, httpd_uri_t, http_method_HTTP_GET,
    http_method_HTTP_POST, nvs_flash_init, wifi_config_t, wifi_init_config_t,
    wifi_interface_t_WIFI_IF_STA, wifi_mode_t_WIFI_MODE_STA, WIFI_INIT_CONFIG_MAGIC, ESP_OK,
};

use crate::state::{lock_bus, Bus};

// ---------------------------------------------------------------------------
// 纯逻辑层：白名单路由表 + 路由纯函数 + 快照 JSON 组包（单测可钉）
// ---------------------------------------------------------------------------

/// 端点白名单（与 host `RR3_PANEL_ROUTES` 四列同构：method / path / covers / applies）。
/// 批N14 件二起四行：两读（GET state/bindings）＋两写（POST wifi/reboot）。
/// covers 列＝该端点读的 ⑩ 配置模型字段（`<路>/<字段>`）；本表端点读的是板侧
/// 运行态/注入绑定（**不在** ⑩ 分母五路——绑定十键是 rr-ipc 注入键非配置模型
/// [[pin]] 词表）/写的是板侧 NVS 凭据（同样不在五路），covers 全空如实登记；
/// applies 列＝该端点改的配置对象（词表 `live`/空）：配网写口改的是"活配置"
/// （STA 立即重连）＝`live`；reboot 是动作面（不改任何配置对象）＝空，同 host
/// 面板 POST /api/relay 动作面先例。申报面进 check.sh ⑩ 扫描（批N13 件二
/// 已把本表区段扩进扫描源），执法位＝未来板侧端点读五路字段而不申报即红
/// （对拍判据 ⊆ 分母五路并集）。
pub(crate) const RR3_BOARD_ROUTES: &[(&str, &str, &[&str], &str)] = &[
    ("GET", "/api/board/state", &[], ""),
    ("GET", "/api/board/bindings", &[], ""),
    ("POST", "/api/board/wifi", &[], "live"),
    ("POST", "/api/board/reboot", &[], ""),
];

/// 路由分派产物（表内端点；`None` = 白名单外，fail-closed 回 404）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rr3BoardRoute {
    /// GET /api/board/state——板侧运行态观测快照（`PeriphState` 观测字段，
    /// 批N12 件一观测面含其中）
    State,
    /// GET /api/board/bindings——注入绑定十键读数（批N9 件五对账锚同源）
    Bindings,
    /// POST /api/board/wifi——事务性配网写口（批N14 件二，N-10 ② 配网半边）
    WifiWrite,
    /// POST /api/board/reboot——动作面（应答先行落链再重启）
    Reboot,
}

/// 路由纯函数：method + path → 分派。**白名单 = [`RR3_BOARD_ROUTES`]**——
/// method 或 path 任一不在表内即 `None`（fail-closed，同 host 面板语义；
/// 405 细分不做——method 不在表内一律白名单外）。
pub(crate) fn rr3_board_route(method: &str, path: &str) -> Option<Rr3BoardRoute> {
    match (method, path) {
        ("GET", "/api/board/state") => Some(Rr3BoardRoute::State),
        ("GET", "/api/board/bindings") => Some(Rr3BoardRoute::Bindings),
        ("POST", "/api/board/wifi") => Some(Rr3BoardRoute::WifiWrite),
        ("POST", "/api/board/reboot") => Some(Rr3BoardRoute::Reboot),
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

/// 写口 body 上界（fail-closed：形制上限 32+64B 的 JSON 富余 8 倍——超长 body
/// 一律不读全，400 拒收）。
const RR3_WIFI_BODY_MAX: usize = 512;
/// 等连有界窗（事务与回退共用；步进 [`WIFI_POLL_MS`]——15s 上限，超窗＝未连
/// 如实应答，不猜不造）。
const WIFI_CONNECT_WAIT_MS: u64 = 15_000;
const WIFI_POLL_MS: u64 = 500;

/// 配网写口事务终态（应答 `{ok, connected, reverted, stage}`——`connected`＝
/// 实测等连结果**永不因请求被接受而置真**（同 connect RPC 纪律）；`stage`＝
/// 事务终态所在阶段名，点名不出值）。
pub(crate) struct Rr3WifiWriteResult {
    pub(crate) ok: bool,
    pub(crate) connected: bool,
    pub(crate) reverted: bool,
    pub(crate) stage: &'static str,
}

impl Rr3WifiWriteResult {
    /// 业务拒绝态（body/form/nvs——未触 STA，无回退发生）。
    fn rejected(stage: &'static str) -> Self {
        Self {
            ok: false,
            connected: false,
            reverted: false,
            stage,
        }
    }
}

/// 写口 body 解析（纯函数，单测钉域）：JSON object + `ssid`/`pass` 字符串字段；
/// 空密码合法（开放网络，同形制域口径）。缺字段/非串/坏 JSON 一律 Err（点名串
/// 不出值——零回显契约）。
pub(crate) fn rr3_wifi_write_body(body: &[u8]) -> Result<(String, String), &'static str> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|_| "body 非 JSON")?;
    let ssid = v
        .get("ssid")
        .and_then(serde_json::Value::as_str)
        .ok_or("ssid 字段缺席或非串")?;
    let pass = v
        .get("pass")
        .and_then(serde_json::Value::as_str)
        .ok_or("pass 字段缺席或非串")?;
    Ok((ssid.to_string(), pass.to_string()))
}

/// 事务终态 → 应答 JSON（纯函数，单测钉形状——四字段计数钉死防顺手加值字段）。
pub(crate) fn rr3_wifi_write_json(r: &Rr3WifiWriteResult) -> String {
    serde_json::json!({
        "ok": r.ok,
        "connected": r.connected,
        "reverted": r.reverted,
        "stage": r.stage,
    })
    .to_string()
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
        assert_eq!(rr3_board_route("POST", "/api/board/state"), None, "读端点不受 POST——写口白名单外 fail-closed");
        assert_eq!(rr3_board_route("GET", "/api/board/wifi"), None, "写端点不受 GET");
        assert_eq!(rr3_board_route("PUT", "/api/board/wifi"), None);
        assert_eq!(rr3_board_route("GET", "/api/brain/ops"), None);
        assert_eq!(rr3_board_route("GET", "/api/board/state/extra"), None);
        // 分派产物点名（enum 变体 ↔ 表行一一对应）
        assert_eq!(rr3_board_route("GET", "/api/board/state"), Some(Rr3BoardRoute::State));
        assert_eq!(rr3_board_route("GET", "/api/board/bindings"), Some(Rr3BoardRoute::Bindings));
        assert_eq!(rr3_board_route("POST", "/api/board/wifi"), Some(Rr3BoardRoute::WifiWrite));
        assert_eq!(rr3_board_route("POST", "/api/board/reboot"), Some(Rr3BoardRoute::Reboot));
    }

    /// covers 形制（同 check.sh ⑩ 扫描执法语义）：带斜杠且不以 `/` 开头；
    /// applies 只收 `live`/空；method 词表只收 GET/POST（注册面映射与 handler
    /// 识别共用同一词表——批N14 件二扩 POST 行时钉死）。
    #[test]
    fn covers_wordform_shape() {
        for (method, path, covers, applies) in RR3_BOARD_ROUTES {
            assert!(
                *method == "GET" || *method == "POST",
                "method 词表外: {path} {method}"
            );
            assert!(!path.is_empty(), "空 path 行");
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

    /// 写口 body 解析域（纯函数）：合法/空密码合法（开放网络）/坏 JSON/缺字段/
    /// 非串字段全点名。
    #[test]
    fn wifi_write_body_domain() {
        let (s, p) = rr3_wifi_write_body(br#"{"ssid":"ap","pass":"pw"}"#).unwrap();
        assert_eq!((s.as_str(), p.as_str()), ("ap", "pw"));
        assert!(rr3_wifi_write_body(br#"{"ssid":"ap","pass":""}"#).is_ok(), "空密码合法（开放网络）");
        assert!(rr3_wifi_write_body(b"not json").is_err(), "坏 JSON 必须红");
        assert!(rr3_wifi_write_body(br#"{"ssid":"ap"}"#).is_err(), "pass 缺席必须红");
        assert!(rr3_wifi_write_body(br#"{"pass":"pw"}"#).is_err(), "ssid 缺席必须红");
        assert!(rr3_wifi_write_body(br#"{"ssid":1,"pass":"pw"}"#).is_err(), "非串字段必须红");
    }

    /// 写口应答 JSON 形状：四字段名/类型钉死（拒绝态与成功态同形状——
    /// 宿主消费面契约面）。
    #[test]
    fn wifi_write_json_shape() {
        let mk = |r: &Rr3WifiWriteResult| {
            let v: serde_json::Value = serde_json::from_str(&rr3_wifi_write_json(r)).unwrap();
            v
        };
        let v = mk(&Rr3WifiWriteResult {
            ok: true,
            connected: true,
            reverted: false,
            stage: "apply",
        });
        assert_eq!(v["ok"], true);
        assert_eq!(v["connected"], true);
        assert_eq!(v["reverted"], false);
        assert_eq!(v["stage"], "apply");
        assert_eq!(v.as_object().unwrap().len(), 4);
        let v = mk(&Rr3WifiWriteResult::rejected("form"));
        assert_eq!(v["ok"], false);
        assert_eq!(v["connected"], false);
        assert_eq!(v["reverted"], false);
        assert_eq!(v["stage"], "form");
        assert_eq!(v.as_object().unwrap().len(), 4);
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

/// 白名单统一 handler：req→路由纯函数→快照/写口应答。表外 404 fail-closed。
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
    // 真实 method 从 req 读（批N14 件二：注册面挂 GET/POST 两行——注册面未挂的
    // method 判 "?"，路由纯函数必 None fail-closed 404；req.method 是 c_int，
    // 绑定常量是 http_method=u32——比较处 as i32 对齐）
    let method = match (*req).method {
        m if m == http_method_HTTP_GET as i32 => "GET",
        m if m == http_method_HTTP_POST as i32 => "POST",
        _ => "?",
    };
    let Some(route) = rr3_board_route(method, &path) else {
        return httpd_resp_send_err(
            req,
            httpd_err_code_t_HTTPD_404_NOT_FOUND,
            b"not in board route whitelist\0".as_ptr() as *const _,
        );
    };
    match route {
        // 出锁组 JSON（快照取窗内不组串——模块文档纪律）
        Rr3BoardRoute::State => {
            let body = rr3_state_json(&rr3_state_snapshot(bus));
            httpd_resp_set_type(req, c"application/json".as_ptr());
            httpd_resp_send(req, body.as_ptr() as *const _, body.len() as _)
        }
        Rr3BoardRoute::Bindings => {
            let body = rr3_bindings_json(rr3_bindings_snapshot());
            httpd_resp_set_type(req, c"application/json".as_ptr());
            httpd_resp_send(req, body.as_ptr() as *const _, body.len() as _)
        }
        // 事务性配网写口（批N14 件二）——body 有界收（fail-closed）→ 解析 →
        // 形制 → 事务；HTTP 层失败 400 拒收不读全，业务层失败一律 200 JSON 如实应答
        Rr3BoardRoute::WifiWrite => {
            let clen = (*req).content_len;
            if clen == 0 || clen > RR3_WIFI_BODY_MAX {
                return httpd_resp_send_err(
                    req,
                    httpd_err_code_t_HTTPD_400_BAD_REQUEST,
                    b"wifi body out of bounds\0".as_ptr() as *const _,
                );
            }
            let mut buf = vec![0u8; clen];
            let got = httpd_req_recv(req, buf.as_mut_ptr() as *mut _, buf.len());
            if got <= 0 || got as usize != buf.len() {
                return httpd_resp_send_err(
                    req,
                    httpd_err_code_t_HTTPD_400_BAD_REQUEST,
                    b"wifi body recv failed\0".as_ptr() as *const _,
                );
            }
            let result = match rr3_wifi_write_body(&buf) {
                Ok((s, p)) => match crate::cred::rr3_cred_form_check(&s, &p) {
                    Ok(()) => rr3_wifi_write_apply(&s, &p),
                    Err(_) => Rr3WifiWriteResult::rejected("form"),
                },
                Err(_) => Rr3WifiWriteResult::rejected("body"),
            };
            let body = rr3_wifi_write_json(&result);
            httpd_resp_set_type(req, c"application/json".as_ptr());
            httpd_resp_send(req, body.as_ptr() as *const _, body.len() as _)
        }
        // 动作面：应答先行落链再重启（esp_restart 不返回——重启即断链，应答
        // 不可达＝客户端误判超时，故先 send 后 restart）
        Rr3BoardRoute::Reboot => {
            let body = "{\"ok\":true}".to_string();
            httpd_resp_set_type(req, c"application/json".as_ptr());
            let _ = httpd_resp_send(req, body.as_ptr() as *const _, body.len() as _);
            println!("[periph] 写口 reboot 应答已发——esp_restart");
            esp_restart()
        }
    }
}

/// STA 凭据装配（start_httpd 与写口 apply/回退共用；union 起底——sta 臂其余
/// 字段零 = 默认；调用前形制已验 32/64 上限＝`sta.ssid[32]`/`sta.password[64]`
/// 数组尺寸，copy_from_slice 不越界）。返回 esp_err。
unsafe fn wifi_sta_set_credentials(ssid: &[u8], pass: &[u8]) -> i32 {
    let mut wcfg: wifi_config_t = std::mem::zeroed();
    wcfg.sta.ssid[..ssid.len()].copy_from_slice(ssid);
    wcfg.sta.password[..pass.len()].copy_from_slice(pass);
    esp_wifi_set_config(wifi_interface_t_WIFI_IF_STA, &mut wcfg)
}

/// 有界等连（连上＝**DHCP 得 IP**，判据见函数内注记）：[`WIFI_CONNECT_WAIT_MS`]
/// 上限、[`WIFI_POLL_MS`] 步进；超窗即 false（调用方如实应答——不猜不造）。
unsafe fn wifi_sta_wait_connected() -> bool {
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
    let mut apply_ok = wifi_sta_set_credentials(ssid.as_bytes(), pass.as_bytes()) == ESP_OK;
    if apply_ok {
        apply_ok = esp_wifi_connect() == ESP_OK;
    }
    if apply_ok && wifi_sta_wait_connected() {
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
            if wifi_sta_set_credentials(c.ssid.as_bytes(), c.pass().as_bytes()) == ESP_OK {
                let _ = esp_wifi_connect();
            }
        }
        None => {
            esp_wifi_disconnect();
        }
    }
    let connected = wifi_sta_wait_connected();
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
pub(crate) fn rr3_board_httpd_spawn(bus: &Arc<Mutex<Bus>>) {
    let bus = Arc::clone(bus);
    // ① NVS init 前移（批N14 件一）：凭据读链前件，兼 WiFi 校准数据存储
    if unsafe { nvs_flash_init() } != ESP_OK {
        eprintln!("[periph] 配置腿未装配（nvs_flash_init 失败）——N-13 服务面缺席，板其余功能照常");
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
                eprintln!(
                    "[periph] 配置腿未装配（NVS 无存档且 RR3_BOARD_WIFI_SSID 未注入）——N-13 服务面缺席，板其余功能照常"
                );
                return;
            }
        },
    };
    // ③ 形制校验（两源同门——[`crate::cred::rr3_cred_form_check`]；错误不出值只出长度）
    if let Err(e) = crate::cred::rr3_cred_form_check(&ssid, &pass) {
        eprintln!(
            "[periph] 配置腿装配失败（{e}：SSID {}B/密码 {}B）——缺席照常",
            ssid.len(),
            pass.len()
        );
        return;
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
                eprintln!("[periph] 配置腿启动失败（缺席照常）: {e:?}");
            }
        })
        .expect("起配置腿线程失败");
}

/// WiFi STA 起栈 + httpd_start + 白名单注册（unsafe 集中段；逐步 fail-honest；
/// NVS init 已前移到 [`rr3_board_httpd_spawn`]——本段只做起栈链）。
unsafe fn start_httpd(bus: &Arc<Mutex<Bus>>, cred_source: &str, ssid: &[u8], pass: &[u8]) -> Result<(), i32> {
    // 逐步 helper：非 ESP_OK 即 Err（fail-honest，点名在调用方）
    fn chk(r: i32) -> Result<(), i32> {
        if r == ESP_OK {
            Ok(())
        } else {
            Err(r)
        }
    }
    // ① netif + 事件循环 + STA 缺省 netif（句柄留存——等连判据查 IP 用）
    chk(esp_netif_init())?;
    chk(esp_event_loop_create_default())?;
    let _ = STA_NETIF.set(esp_netif_create_default_wifi_sta() as usize);
    // ② WiFi 起栈（手抄宏配置见 wifi_default_config）
    let cfg = wifi_default_config();
    chk(esp_wifi_init(&cfg))?;
    chk(esp_wifi_set_mode(wifi_mode_t_WIFI_MODE_STA))?;
    // STA 凭据（helper 共用——写口 apply/回退同款；位形已在 spawn 前验）
    chk(wifi_sta_set_credentials(ssid, pass))?;
    chk(esp_wifi_start())?;
    // STA 关联发起（批N15 件二真板取证实锤的缺陷修复：esp_wifi_start 只起驱动
    // 不自动连 AP——缺此步板永不关联、无 got ip；写口 apply/回退两路本就有
    // connect，唯 boot 路漏）
    chk(esp_wifi_connect())?;
    // ③ httpd 起（手填 config：HTTPD_DEFAULT_CONFIG 真值——宏默认也是
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
    // ④ 白名单注册（统一 handler；OnceLock 先写——handler 可达即 bus 在位；
    // method 映射随表行 GET/POST——表内词表由 covers_wordform_shape 单测钉死，
    // 词表外 panic 不可达）
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
            handler: Some(rr3_board_httpd_handler),
            user_ctx: std::ptr::null_mut(),
        };
        chk(httpd_register_uri_handler(h, &uri))?;
    }
    println!(
        "[periph] 配置腿起（N-13，凭据源={cred_source}）: http://0.0.0.0:{}/api/board/*（白名单 {} 端点＝GET state/bindings＋POST wifi/reboot）",
        HTTPD_SERVER_PORT,
        RR3_BOARD_ROUTES.len()
    );
    Ok(())
}
