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
//! 动作面应答送达半边（批N16 件二裁定：**旁证契约化**，decisions §3）：事务内
//! reconnect 与 reboot 的 `esp_restart` 都会杀 TCP 会话——四字段应答**取读数半边**
//! 真板成立、**送达半边物理不可达**（实现已是"事务完成后应答"，无更延迟的应答位
//! 可取；批N15 真板实录：坏凭据 POST curl 35s 超时应答丢失／reboot 应答先行落链
//! 仍不可达）。客户端**确认语义以旁证为准**：写口＝板重新可达后
//! `GET /api/board/state` 的 `connected` 字段（新凭据连上＝生效／板仍以旧凭据可达
//! ＝回退生效——砖路径消灭实证语义）；reboot＝复位后 boot 横幅「凭据源=…」点名。
//!
//! 分层（机制与策略分离）：
//! - **纯逻辑层**（单测可钉）：白名单路由表 [`RR3_BOARD_ROUTES`]（与 host
//!   `RR3_PANEL_ROUTES` 四列同构）＋路由纯函数 [`rr3_board_route`]＋快照 JSON
//!   组包纯函数＋写口 body 解析/应答组包纯函数。表↔分派零漂移由
//!   `route_table_matches_dispatch` 单测钉死（同 host `route_table_matches_dispatch`
//!   形状）。
//! - **unsafe 壳层**（[`rr3_httpd_spawn`]）：WiFi STA 起栈（裸 sys——
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
    esp_event_loop_create_default, esp_netif_create_default_wifi_ap,
    esp_netif_create_default_wifi_sta, esp_netif_get_ip_info, esp_netif_init, esp_netif_ip_info_t,
    esp_netif_t, esp_restart, esp_wifi_connect, esp_wifi_disconnect, esp_wifi_init,
    esp_wifi_set_config, esp_wifi_set_mode, esp_wifi_start, g_wifi_default_wpa_crypto_funcs,
    g_wifi_osi_funcs, http_method_HTTP_GET, http_method_HTTP_POST, httpd_config_t,
    httpd_err_code_t, httpd_err_code_t_HTTPD_400_BAD_REQUEST,
    httpd_err_code_t_HTTPD_401_UNAUTHORIZED, httpd_err_code_t_HTTPD_404_NOT_FOUND, httpd_handle_t,
    httpd_register_uri_handler, httpd_req_recv, httpd_req_t, httpd_resp_send, httpd_resp_send_err,
    httpd_resp_set_type, httpd_start, httpd_uri_t, nvs_flash_init,
    wifi_auth_mode_t_WIFI_AUTH_WPA2_PSK, wifi_config_t, wifi_init_config_t,
    wifi_interface_t_WIFI_IF_AP, wifi_interface_t_WIFI_IF_STA, wifi_mode_t_WIFI_MODE_APSTA, ESP_OK,
    WIFI_INIT_CONFIG_MAGIC,
};

use rr_ipc::Rr3BoardBindings;

use crate::state::{lock_bus, Bus};

/// 板侧 HTTP 写口管理令牌（P1-3）：POST /api/board/wifi 与 /api/board/reboot
/// 须在 JSON body 带 `token` 字段且与本值相等，否则 401。生产构建必须经
/// `RR3_BOARD_ADMIN_TOKEN` 显式给值；缺省公开值只允许在显式声明"开发/CI
/// 构建"时编入（下方 const 断言执法）。
const RR3_BOARD_ADMIN_TOKEN: &str = match option_env!("RR3_BOARD_ADMIN_TOKEN") {
    Some(t) if !t.is_empty() => t,
    _ => "rock3-board-admin-dev",
};

/// 令牌配置门（fail-closed，编译期）：没有生产令牌、又没有显式
/// `RR3_ALLOW_DEV_ADMIN_TOKEN=1`（开发/CI）时直接编译失败——公开缺省令牌
/// 不得静默进入任何镜像（SoftAP 口令是仓内公开的，令牌是写口唯一门槛）。
const _: () = assert!(
    matches!(option_env!("RR3_BOARD_ADMIN_TOKEN"), Some(t) if !t.is_empty())
        || option_env!("RR3_ALLOW_DEV_ADMIN_TOKEN").is_some(),
    "缺少生产管理令牌：请设置 RR3_BOARD_ADMIN_TOKEN=<强随机值> 后重编；仅开发/CI 可设 RR3_ALLOW_DEV_ADMIN_TOKEN=1 使用公开缺省值"
);

/// 写口令牌校验：body JSON 须含 `token` 字段且等于 [`RR3_BOARD_ADMIN_TOKEN`]。
/// 纯函数，单测可钉；不读 env、不碰全局。
fn rr3_token_check(body: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    v.get("token")
        .and_then(|t| t.as_str())
        .map(|t| t == RR3_BOARD_ADMIN_TOKEN)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// 纯逻辑层：白名单路由表 + 路由纯函数 + 快照 JSON 组包（单测可钉）
// ---------------------------------------------------------------------------

/// 端点白名单（与 host `RR3_PANEL_ROUTES` 四列同构：method / path / covers / applies）。
/// 批N14 件二起四行：两读（GET state/bindings）＋两写（POST wifi/reboot）。
/// covers 列＝该端点读的 ⑩ 配置模型字段（`<路>/<字段>`）；本表端点读的是板侧
/// 运行态/注入绑定（**不在** ⑩ 分母五路——绑定十一键是 rr-ipc 注入键非配置模型
/// [[pin]] 词表）/写的是板侧 NVS 凭据（同样不在五路），covers 全空如实登记；
/// applies 列＝该端点改的配置对象（词表 `live`/空）：配网写口改的是"活配置"
/// （STA 立即重连）＝`live`；reboot 是动作面（不改任何配置对象）＝空，同 host
/// 面板 POST /api/relay 动作面先例。申报面进 check.sh ⑩ 扫描（批N13 件二
/// 已把本表区段扩进扫描源），执法位＝未来板侧端点读五路字段而不申报即红
/// （对拍判据 ⊆ 分母五路并集）。
#[rustfmt::skip]
pub(crate) const RR3_BOARD_ROUTES: &[(&str, &str, &[&str], &str)] = &[
    ("GET", "/", &[], ""),
    ("GET", "/api/board/state", &[], ""),
    ("GET", "/api/board/bindings", &[], ""),
    ("POST", "/api/board/wifi", &[], "live"),
    ("POST", "/api/board/reboot", &[], ""),
];

/// 路由分派产物（表内端点；`None` = 白名单外，fail-closed 回 404）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rr3BoardRoute {
    /// GET /——板端配网/状态网页（2026-10-31：手机连 SoftAP 即可配网，无需 curl）
    Index,
    /// GET /api/board/state——板侧运行态观测快照（`PeriphState` 观测字段，
    /// 批N12 件一观测面含其中）
    State,
    /// GET /api/board/bindings——注入绑定十一键读数（批N9 件五对账锚同源；批N17
    /// 件四 cred_store 随键——非 GPIO 存储源标记，读数面可见）
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
        ("GET", "/") => Some(Rr3BoardRoute::Index),
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
    pub(crate) rail_valid: bool,
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
        rail_valid: st.rail_valid,
        rail_tier: if st.rail_valid {
            st.rail_tier.name()
        } else {
            "unknown"
        },
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
        "rail_valid": s.rail_valid,
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

/// 注入绑定快照（state.rs 读窄缝自持锁、Clone 出整体 [`Rr3BoardBindings`]——
/// 批N17 件四起 cred_store 随 struct 透传，键名不再本侧手抄第二份）。
fn rr3_bindings_snap() -> Option<Rr3BoardBindings> {
    crate::state::rr3_board_binds_snap()
}

/// 绑定快照 → JSON（纯函数；未注入整体 `null`，对账锚读数语义同源）。键名用
/// rr-ipc 在册常量（键真值单点不抄第二份）；值域：GPIO 十键 → 数字、cred_store
/// → 字符串（批N17 件四——非 GPIO 存储源标记，与桥上注入载荷同域不同形：注入
/// 侧值全字符串、读数侧 GPIO 数字化，两侧键域同源 rr-ipc 在册表）。
pub(crate) fn rr3_bindings_json(b: Option<&Rr3BoardBindings>) -> String {
    use rr_ipc::{
        RR3_BOARD_BINDING_ADC_RAIL_PIN, RR3_BOARD_BINDING_CRED_STORE, RR3_BOARD_BINDING_I2C_SCL,
        RR3_BOARD_BINDING_I2C_SDA, RR3_BOARD_BINDING_I2S_BCLK, RR3_BOARD_BINDING_I2S_SD,
        RR3_BOARD_BINDING_I2S_WS, RR3_BOARD_BINDING_LEDC_PAN, RR3_BOARD_BINDING_LEDC_TILT,
        RR3_BOARD_BINDING_UART2_RX, RR3_BOARD_BINDING_UART2_TX,
    };
    let Some(b) = b else {
        return "null".into();
    };
    let mut obj = serde_json::Map::new();
    let mut put_gpio = |k: &'static str, v: Option<u8>| {
        obj.insert(
            k.to_string(),
            v.map_or(serde_json::Value::Null, serde_json::Value::from),
        );
    };
    put_gpio(RR3_BOARD_BINDING_I2C_SDA, b.i2c_sda);
    put_gpio(RR3_BOARD_BINDING_I2C_SCL, b.i2c_scl);
    put_gpio(RR3_BOARD_BINDING_ADC_RAIL_PIN, b.adc_rail_pin);
    put_gpio(RR3_BOARD_BINDING_UART2_TX, b.uart2_tx);
    put_gpio(RR3_BOARD_BINDING_UART2_RX, b.uart2_rx);
    put_gpio(RR3_BOARD_BINDING_LEDC_PAN, b.ledc_pan);
    put_gpio(RR3_BOARD_BINDING_LEDC_TILT, b.ledc_tilt);
    put_gpio(RR3_BOARD_BINDING_I2S_BCLK, b.i2s_bclk);
    put_gpio(RR3_BOARD_BINDING_I2S_SD, b.i2s_sd);
    put_gpio(RR3_BOARD_BINDING_I2S_WS, b.i2s_ws);
    obj.insert(
        RR3_BOARD_BINDING_CRED_STORE.to_string(),
        b.cred_store.as_ref().map_or(serde_json::Value::Null, |s| {
            serde_json::Value::from(s.as_str())
        }),
    );
    serde_json::Value::Object(obj).to_string()
}

/// 写口 body 上界（fail-closed：形制上限 32+64B 的 JSON 富余 8 倍——超长 body
/// 一律不读全，400 拒收）。
const RR3_WIFI_BODY_MAX: usize = 512;

/// 板端配网页（GET /，2026-10-31）：手机连上 SoftAP 后直接填 SSID/口令即可配网，
/// 不必依赖 curl；同时轮询 `/api/board/state` 显示实时状态。页面本体零外部依赖
/// （无 CDN、离线可用），所有请求只打本板白名单端点。
const RR3_BOARD_INDEX_HTML: &str = r#"<!doctype html><html lang="zh"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>RR3 机器人</title><style>
body{font-family:system-ui,-apple-system,sans-serif;margin:16px;max-width:560px}
h1{font-size:20px}h2{font-size:15px;margin-top:22px}
input,button{font-size:16px;padding:9px;margin:5px 0;width:100%;box-sizing:border-box}
pre{background:#f4f4f5;padding:10px;border-radius:6px;overflow:auto;font-size:12px}
</style></head><body>
<h1>RR3 机器人 · 板载面板</h1>
<h2>实时状态</h2><pre id="s">加载中…</pre>
<h2>WiFi 配置（换网后在这里重配）</h2>
<form id="f">
<input id="tok" type="password" placeholder="管理令牌 (token)" required>
<input id="ssid" placeholder="WiFi 名称 (SSID)" required>
<input id="pass" type="password" placeholder="WiFi 密码">
<button>保存并连接</button></form>
<h2>结果</h2><pre id="r">—</pre>
<h2>动作</h2><button id="rb">重启板子</button>
<script>
const S=document.getElementById('s'),R=document.getElementById('r');
async function st(){try{const j=await (await fetch('/api/board/state')).json();
S.textContent=JSON.stringify(j,null,1)}catch(e){S.textContent='读取失败: '+e}}
st();setInterval(st,3000);
document.getElementById('f').onsubmit=async e=>{e.preventDefault();R.textContent='提交中…';
try{const j=await (await fetch('/api/board/wifi',{method:'POST',
headers:{'Content-Type':'application/json'},
body:JSON.stringify({token:tok.value,ssid:ssid.value,password:pass.value})})).json();
R.textContent=JSON.stringify(j,null,1);
if(j.connected){R.textContent+='\n✅ 已加入新 WiFi（本页可断开，改用新 IP 访问）'}
else if(j.reverted){R.textContent+='\n⚠ 新凭据未连上，已回滚旧凭据'}
}catch(e){R.textContent='请求失败: '+e}st()};
document.getElementById('rb').onclick=async()=>{if(!confirm('确认重启？'))return;
R.textContent='重启指令已发…';try{await fetch('/api/board/reboot',{method:'POST',
headers:{'Content-Type':'application/json'},
body:JSON.stringify({token:tok.value})})}catch(e){}};
</script></body></html>"#;
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

mod serve;
#[cfg(test)]
mod tests;

pub(crate) use serve::rr3_httpd_spawn;
