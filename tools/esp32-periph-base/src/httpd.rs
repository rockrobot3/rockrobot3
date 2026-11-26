// SPDX-License-Identifier: Apache-2.0
//! 板侧配置腿服务：WiFi 上的极简 HTTP 面（不经桥），载体是 ESP-IDF `esp_http_server`
//! （HTTP/1.1 子集，与 host 面板同形）。**TLS 不做**（局域网可信面、板侧无 TLS 栈
//! 现役、零新依赖）。
//!
//! 写口（配网半边）：`POST /api/board/wifi` 事务性配网——读旧值 → 写 NVS → STA
//! 重配置重连 → 有界等连 → 败即回退旧值 → 如实应答 `{ok, connected, reverted,
//! stage}`；`connected` 只取实测等连结果，永不因请求被接受而置真。`POST
//! /api/board/reboot` 应答先行落链再 `esp_restart`。事务全程不持 bus 锁、不碰
//! 观测面（零回显契约的第二道墙）。
//!
//! 应答送达半边物理不可达（reconnect/reboot 都会杀 TCP 会话），故客户端确认语义
//! 以旁证为准：写口 = 板重新可达后 `GET /api/board/state` 的 `connected`；reboot =
//! 复位后 boot 横幅"凭据源=…"点名。
//!
//! 分层：纯逻辑层（白名单路由表 [`RR3_BOARD_ROUTES`] + 查表 + JSON 组包/解析纯函数，
//! 单测可钉）；unsafe 壳层 [`rr3_httpd_spawn`]（WiFi STA 起栈 + `httpd_start` +
//! 注册 handler）。表即分派真值，无第二份。凭据读链：NVS 优先 → `option_env!`
//! 编译期回落（板上 newlib 的 `environ` 为空）；都缺 = 配置腿缺席，boot fail-honest。
//! 栈 16KB（[`HTTPD_STACK_SIZE`]）；写口瞬时缓冲 ≤512B，不新增常驻堆。

use std::sync::{Arc, Mutex, OnceLock};

use esp_idf_sys::{
    esp_event_loop_create_default, esp_netif_create_default_wifi_ap,
    esp_netif_create_default_wifi_sta, esp_netif_get_ip_info, esp_netif_init, esp_netif_ip_info_t,
    esp_netif_t, esp_restart, esp_wifi_connect, esp_wifi_disconnect, esp_wifi_init,
    esp_wifi_set_config, esp_wifi_set_mode, esp_wifi_start, g_wifi_default_wpa_crypto_funcs,
    g_wifi_osi_funcs, http_method_HTTP_GET, http_method_HTTP_POST, httpd_err_code_t,
    httpd_err_code_t_HTTPD_400_BAD_REQUEST, httpd_err_code_t_HTTPD_401_UNAUTHORIZED,
    httpd_err_code_t_HTTPD_404_NOT_FOUND, httpd_req_recv, httpd_req_t, httpd_resp_send,
    httpd_resp_send_err, httpd_resp_set_type, wifi_auth_mode_t_WIFI_AUTH_WPA2_PSK, wifi_config_t,
    wifi_init_config_t, wifi_interface_t_WIFI_IF_AP, wifi_interface_t_WIFI_IF_STA,
    wifi_mode_t_WIFI_MODE_APSTA, ESP_OK, WIFI_INIT_CONFIG_MAGIC,
};

use rr_ipc::Rr3BoardBindings;

use crate::state::{lock_bus, push_job, Bus, Job};

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

/// 板侧处理臂统一签名：请求 + 总线（不读总线的臂忽略第二参）。
/// SAFETY: 仅在 esp_http_server 回调内调用；`req` 非空且只活到本次回调结束。
pub(crate) type Rr3Handler = unsafe fn(*mut httpd_req_t, &Arc<Mutex<Bus>>) -> i32;

/// 端点白名单（前四列与 host `RR3_PANEL_ROUTES` 同构：method / path / covers /
/// applies；第五列是**处理臂**——表即分派真值，表加行＝分派生效，结构上不存在
/// "表加行忘加臂"的漂移面）。
/// covers 列＝该端点读的 ⑩ 配置模型字段（`<路>/<字段>`）；本表端点读的是板侧
/// 运行态/注入绑定（**不在** ⑩ 分母五路——绑定十一键是 rr-ipc 注入键非配置模型
/// [[pin]] 词表）/写的是板侧 NVS 凭据（同样不在五路），covers 全空如实登记；
/// applies 列＝该端点改的配置对象（词表 `live`/空）：配网写口改的是"活配置"
/// （STA 立即重连）＝`live`；其余写口是动作面（不改任何配置对象）＝空。申报面
/// 进 check.sh ⑩ 扫描（批N13 件二已把本表区段扩进扫描源）。
#[rustfmt::skip]
pub(crate) const RR3_BOARD_ROUTES: &[(&str, &str, &[&str], &str, Rr3Handler)] = &[
    ("GET", "/", &[], "", serve::handle_index),
    ("GET", "/api/board/state", &[], "", serve::handle_state),
    ("GET", "/api/board/health", &[], "", serve::handle_health),
    ("GET", "/api/board/bindings", &[], "", serve::handle_bindings),
    ("POST", "/api/board/wifi", &[], "live", serve::handle_wifi_write),
    ("POST", "/api/board/reboot", &[], "", serve::handle_reboot),
    // 调试/标定提示面：把一行 ASCII 写进 OLED 三行文本区（DoA 3×10 等现场测试
    // 的提示屏；不依赖 UART2，WiFi 可达即可）。
    ("POST", "/api/board/oled", &[], "", serve::handle_oled),
    // W4-2 播放写口：raw PCM/WAV body，令牌走 `X-RR3-Token` 头（非 JSON）。
    #[cfg(feature = "tts")]
    ("POST", "/api/board/play", &[], "", serve::handle_play),
    // W4-2b 云 TTS：配置写口（NVS）+ 朗读写口（云取数→播放队列）。
    #[cfg(feature = "tts")]
    ("POST", "/api/board/cloud", &[], "", serve::handle_cloud),
    #[cfg(feature = "tts")]
    ("POST", "/api/board/say", &[], "", serve::handle_say),
    // W6 对话轮手动触发：raw 16 kHz mono WAV/PCM → ASR→LLM→TTS→播放（VAD 未接时
    // 的逐跳验收入口；令牌同 play 走 `X-RR3-Token` 头）。
    #[cfg(feature = "tts")]
    ("POST", "/api/board/dialog", &[], "", serve::handle_dialog),
];

/// 路由查表：method + path 全等才命中（fail-closed，同 host 面板语义；
/// 405 细分不做——method 不在表内一律白名单外）。
pub(crate) fn rr3_route_lookup(method: &str, path: &str) -> Option<Rr3Handler> {
    RR3_BOARD_ROUTES
        .iter()
        .find(|(m, p, _, _, _)| *m == method && *p == path)
        .map(|(_, _, _, _, handler)| *handler)
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
    pub(crate) tts_gate: bool,
    pub(crate) tts_played: u64,
    pub(crate) tts_fail: u32,
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
        tts_gate: st.tts_gate,
        tts_played: st.tts_played,
        tts_fail: st.tts_fail,
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
        "tts_gate": s.tts_gate,
        "tts_played": s.tts_played,
        "tts_fail": s.tts_fail,
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

/// OLED 提示写口解析结果：文本 + 是否保持（hold=true 时 dialog/DoA 的表情
/// 切换不覆盖提示，用于 3×10 等现场标定）。
pub(crate) struct Rr3OledWrite {
    pub(crate) text: String,
    pub(crate) hold: bool,
}

/// OLED 提示文本（3 行 ×21 列 ASCII 字体；中文不在字模内，现场提示用英文）。
pub(crate) fn rr3_oled_body(body: &[u8]) -> Result<Rr3OledWrite, &'static str> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|_| "body 非 JSON")?;
    let text = v
        .get("text")
        .and_then(serde_json::Value::as_str)
        .ok_or("text 字段缺席或非串")?;
    let ok = !text.is_empty() && text.len() <= 63;
    let true = ok else {
        return Err("text 空或超 63B");
    };
    let hold = match v.get("hold") {
        None => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return Err("hold 非布尔"),
    };
    Ok(Rr3OledWrite {
        text: text.to_string(),
        hold,
    })
}

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
body:JSON.stringify({token:tok.value,ssid:ssid.value,pass:pass.value})})).json();
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
