//! wifi 原子件机制主体（五件套③，自原 src/wifi.rs 逐字迁入，逻辑零改动）。
//!
//! WiFi 链路：`rr3-connect/net/link` 状态机与周期流 + 配网面三 RPC（0.0.4 批D）。
//!
//! HAL 读数（`Rr3LinkState`）→ 状态机裁决（翻转必发 + 周期保底）→ 事件体上路。
//! 漫游/重连是**模块侧策略**，本轮不实装（roam_probe_enabled 只解析回显，与
//! rr3-power 的 cutoff 同款口径）；NetworkManager/D-Bus 属板上件（cfg 门骨架，
//! 0.0.2 方案 D7），sim 后端状态由环境注入定死——状态机的翻转分支只有纯函数
//! 单测能钉，e2e 只见首拍基线与周期流（e2e_power 的时序教训同款分工）。
//!
//! 配网面（0.0.4 方案 D4/Q3）：AP 配网状态机是**模块内策略**，不进 HAL——sim
//! 是"设备"不是"策略"，HAL 只如实报数（up/ssid/rssi），凭据态归
//! [`Rr3Provision`]，net/link 体以 `mode` 字段随流发布（契约只增不改，0.0.4
//! 批D append）。connect/forget 只改状态机与内存凭据：凭据不落盘（conventions
//! §3），真凭据持久化（wpa_supplicant/NVS）板上后置——重启即忘，如实声明。
//! scan 回 sim 模拟列表（`backend:"sim"` 诚实标注），板上换 NetworkManager。

use rr_hal::Rr3LinkState;
use serde_json::{json, Value};

/// net/link 状态机：跟踪上一次的 up 值。首拍与**翻转**返回 true（立即发，
/// 不等周期）；同态返回 false（交给主循环的周期拍点保底）。状态只有这一位。
pub struct Rr3LinkEdge {
    last: Option<bool>,
}

impl Rr3LinkEdge {
    pub fn rr3_link_edge_new() -> Self {
        Self { last: None }
    }

    /// 每拍 HAL 读数喂一次 up 值。返回 true = 此刻应立即发布（首拍基线 / 翻转）。
    pub fn rr3_link_edge_cross(&mut self, up: bool) -> bool {
        match self.last {
            None => {
                self.last = Some(up);
                true // 首拍基线：观察者无论何时接入，下一条周期流必然等不来"没发过"
            }
            Some(prev) if prev != up => {
                self.last = Some(up);
                true // 翻转必发：在线/离线边界是这条流唯一的"新闻"
            }
            Some(_) => false,
        }
    }
}

/// net/link 事件体：seq 是本流自己的计数（envelope 惯例，从 1 起）。
/// ssid/rssi 断链时是 null——设备面（Rr3SimLink）已抹掉，这里如实透传。
/// `mode` 是配网状态机（[`Rr3Provision`]）的当前模式，0.0.4 批D append——
/// 契约只增不改：既有 seq/up/ssid/rssi_dbm 四字段原样保留。
pub fn rr3_link_body(seq: u64, st: &Rr3LinkState, mode: &str) -> Value {
    json!({
        "seq": seq,
        "up": st.up,
        "ssid": st.ssid,
        "rssi_dbm": st.rssi_dbm,
        "mode": mode,
    })
}

// ---------- 配网面（0.0.4 批D）：AP 配网状态机 + 三 RPC ----------

/// AP 配网状态机（模块内策略，HAL 不知情）：内存凭据有 → `station`（客户端
/// 模式），没有 → `ap`（配网热点在开）。connect 存凭据、forget 清凭据，
/// 就这两步转移——"AP 态可模拟进出"由纯函数单测钉死（方案 D4 判据）。
/// 真凭据持久化（wpa_supplicant/NVS）板上后置，重启即忘，不 claim。
pub struct Rr3Provision {
    credentials: Option<(String, String)>,
}

impl Rr3Provision {
    pub fn rr3_provision_new() -> Self {
        Self { credentials: None }
    }

    /// 当前配网模式：`"ap"` | `"station"`（net/link 的 mode 字段词汇，只有两值）。
    pub fn rr3_provision_mode(&self) -> &'static str {
        match self.credentials {
            None => "ap",
            Some(_) => "station",
        }
    }

    /// 存凭据并转入 station（参数合法性由 [`rr3_wifi_connect_parse_args`] 裁决，
    /// 状态机只管转移）。
    pub fn rr3_provision_connect(&mut self, ssid: String, pass: String) {
        self.credentials = Some((ssid, pass));
    }

    /// 清凭据回 ap——legacy `/api/factory_reset` 的 WiFi 语义承接（≠ M6 整机
    /// 恢复出厂，partition-layout 两语义不合并）。
    pub fn rr3_provision_forget(&mut self) {
        self.credentials = None;
    }
}

/// wifi/scan 点名参数裁决：无参动作，`null` 与空对象放行，多余字段点名拒。
pub fn rr3_wifi_scan_parse_args(args: &Value) -> Result<(), String> {
    match args {
        Value::Null => Ok(()),
        Value::Object(m) if m.is_empty() => Ok(()),
        _ => Err("wifi/scan 无参数（收到多余字段）".into()),
    }
}

/// wifi/scan 结果（sim 后端）：确定性三行模拟列表，元素形状 = legacy panel.c
/// h_scan 的 `{"ssid","rssi","auth"}`（auth=是否加密，bool）；`backend:"sim"`
/// 诚实标注——sim 列表只保证形状与确定性，不保证"周围真有这些网"，板上换
/// NetworkManager 真扫描（cfg 门，方案 P4）。
pub fn rr3_wifi_scan_result() -> Value {
    json!({
        "backend": "sim",
        "items": [
            { "ssid": "rock-sim",       "rssi": -42, "auth": true },
            { "ssid": "rock-sim-guest", "rssi": -58, "auth": true },
            { "ssid": "neighbour-2g",   "rssi": -71, "auth": false },
        ],
    })
}

/// wifi/connect 点名参数裁决（legacy panel.c h_connect 语义照搬）：ssid 非空、
/// 密码至少 8 位，文案点名（"密码至少 8 位"是 legacy 原词，配网页依赖它）。
/// 字段名沿用 legacy 的 `pass`——面板表单同名透传，零翻译。
pub fn rr3_wifi_connect_parse_args(args: &Value) -> Result<(String, String), String> {
    let ssid = args
        .get("ssid")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let pass = args.get("pass").and_then(Value::as_str).unwrap_or("");
    if ssid.is_empty() {
        return Err("缺 ssid（非空字符串）".into());
    }
    if pass.len() < 8 {
        return Err("密码至少 8 位（legacy 配网语义）".into());
    }
    Ok((ssid.to_string(), pass.to_string()))
}

/// wifi/connect 结果：状态机已转 station；`connected` 如实取链路读数（sim 后端
/// 由 env 定死，不 fake"连上了"）——legacy"等连后回真实结果"的等待语义由
/// net/link 流承载，RPC 只回执状态机转移与当下读数。
pub fn rr3_wifi_connect_result(st: &Rr3LinkState, ssid: &str) -> Value {
    json!({
        "ok": true,
        "connected": st.up,
        "mode": "station",
        "ssid": ssid,
    })
}

/// wifi/forget 点名参数裁决：无参动作（同 scan）。
pub fn rr3_wifi_forget_parse_args(args: &Value) -> Result<(), String> {
    match args {
        Value::Null => Ok(()),
        Value::Object(m) if m.is_empty() => Ok(()),
        _ => Err("wifi/forget 无参数（收到多余字段）".into()),
    }
}

/// wifi/forget 结果：清凭据回 ap，回执转移后的模式。
pub fn rr3_wifi_forget_result() -> Value {
    json!({ "ok": true, "mode": "ap" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(up: bool) -> Rr3LinkState {
        Rr3LinkState {
            up,
            ssid: up.then(|| "rock-sim".to_string()),
            rssi_dbm: up.then_some(-50),
        }
    }

    /// 状态机主语义：首拍基线必发、翻转必发、同态不发——"翻转必发 + 周期保底"
    /// 的"必发"一半钉在这里（sim 后端状态恒定，e2e 看不见翻转，只有单测能钉）。
    #[test]
    fn link_edge_fires_on_first_and_flip_only() {
        let mut e = Rr3LinkEdge::rr3_link_edge_new();
        assert!(e.rr3_link_edge_cross(true), "首拍基线必发");
        assert!(!e.rr3_link_edge_cross(true), "同态不发（周期拍点保底）");
        assert!(!e.rr3_link_edge_cross(true), "同态继续不发");
        assert!(e.rr3_link_edge_cross(false), "在线→离线翻转必发");
        assert!(!e.rr3_link_edge_cross(false), "翻转后同态不发");
        assert!(e.rr3_link_edge_cross(true), "离线→在线翻转必发");
    }

    #[test]
    fn link_body_carries_full_state_with_own_seq() {
        let v = rr3_link_body(3, &st(true), "station");
        assert_eq!(v["seq"], 3);
        assert_eq!(v["up"], true);
        assert_eq!(v["ssid"], "rock-sim");
        assert_eq!(v["rssi_dbm"], -50);
        assert_eq!(v["mode"], "station", "0.0.4 批D append：mode 随流发布");
        let v = rr3_link_body(4, &st(false), "ap");
        assert_eq!(v["up"], false);
        assert_eq!(v["mode"], "ap");
        assert!(
            v["ssid"].is_null() && v["rssi_dbm"].is_null(),
            "断链时 ssid/rssi 必须是 null（设备面已抹掉）: {v}"
        );
    }

    /// 配网状态机进出（方案 D4 判据"AP 态可模拟进出"钉在纯函数单测）：
    /// 出厂 ap → connect 转 station → forget 回 ap，凭据只存内存。
    #[test]
    fn provision_roundtrips_ap_station() {
        let mut p = Rr3Provision::rr3_provision_new();
        assert_eq!(p.rr3_provision_mode(), "ap", "出厂无凭据 = 配网热点在开");
        p.rr3_provision_connect("home".into(), "password8".into());
        assert_eq!(
            p.rr3_provision_mode(),
            "station",
            "connect 存凭据即转 station"
        );
        p.rr3_provision_forget();
        assert_eq!(p.rr3_provision_mode(), "ap", "forget 清凭据即回 ap");
    }

    /// wifi/connect 参数裁决（legacy 语义）：合法放行；缺 ssid / 空 ssid /
    /// 缺 pass / 密码 <8 位 全部点名拒，"密码至少 8 位"是 legacy 原词。
    #[test]
    fn wifi_connect_args_parse_legacy_semantics() {
        let (ssid, pass) =
            rr3_wifi_connect_parse_args(&json!({"ssid": " home ", "pass": "password8"})).unwrap();
        assert_eq!(
            (ssid.as_str(), pass.as_str()),
            ("home", "password8"),
            "ssid 去首尾空白"
        );
        let e = rr3_wifi_connect_parse_args(&json!({"pass": "password8"})).unwrap_err();
        assert!(e.contains("ssid"), "缺 ssid 点名: {e}");
        let e =
            rr3_wifi_connect_parse_args(&json!({"ssid": "  ", "pass": "password8"})).unwrap_err();
        assert!(e.contains("ssid"), "空 ssid 点名: {e}");
        let e = rr3_wifi_connect_parse_args(&json!({"ssid": "home"})).unwrap_err();
        assert!(e.contains("8 位"), "缺 pass 点名: {e}");
        let e = rr3_wifi_connect_parse_args(&json!({"ssid": "home", "pass": "short"})).unwrap_err();
        assert!(
            e.contains("密码至少 8 位"),
            "短密码点名（legacy 原词）: {e}"
        );
    }

    /// wifi/scan|forget 无参裁决 + 结果形状：sim 诚实标注、items 三行、
    /// 元素三字段（legacy panel.c h_scan 形状）；forget 回执 ap。
    #[test]
    fn wifi_scan_and_forget_shapes() {
        assert_eq!(rr3_wifi_scan_parse_args(&Value::Null), Ok(()));
        assert_eq!(rr3_wifi_scan_parse_args(&json!({})), Ok(()));
        assert!(
            rr3_wifi_scan_parse_args(&json!({"n": 5})).is_err(),
            "多余字段点名拒"
        );
        assert_eq!(rr3_wifi_forget_parse_args(&json!({})), Ok(()));
        assert!(rr3_wifi_forget_parse_args(&json!({"x": 1})).is_err());

        let v = rr3_wifi_scan_result();
        assert_eq!(v["backend"], "sim", "sim 后端诚实标注");
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        for it in items {
            assert!(it.get("ssid").and_then(Value::as_str).is_some());
            assert!(it.get("rssi").and_then(Value::as_i64).is_some());
            assert!(it.get("auth").and_then(Value::as_bool).is_some());
        }
        assert_eq!(rr3_wifi_forget_result()["mode"], "ap");
    }

    /// wifi/connect 结果：connected 如实透传链路读数（不 fake"连上了"），
    /// mode 恒 station、ssid 回显。
    #[test]
    fn wifi_connect_result_reports_link_honestly() {
        let v = rr3_wifi_connect_result(&st(true), "home");
        assert_eq!(v["ok"], true);
        assert_eq!(v["connected"], true);
        assert_eq!(v["mode"], "station");
        assert_eq!(v["ssid"], "home");
        let v = rr3_wifi_connect_result(&st(false), "home");
        assert_eq!(v["connected"], false, "链路 down 时 connected 如实 false");
    }
}
