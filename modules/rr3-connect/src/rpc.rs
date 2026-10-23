//! rr3-connect 协议层（0.0.8 W-A 批2 拆分，ADR-0017 五层表协议层）。
//!
//! 四点名 handler（cloud/session / wifi/scan|connect|forget，自原 composite.rs
//! 迁入）＋载荷组装与参数下钻面（0.0.5 批I N-7 上移承载层，ADR-0019 修订 4：
//! json! 组装与 Value 下钻归本文件，件面 serde_json 清零；错误文案逐字，每句
//! 落在唯一一处）＋ ops/list 组体（批N4 件九二）。本层零机制重实现：链路读数/
//! 凭据存储经 HAL trait 注入，配网状态机经 wifi 原子件注入——"先过凭据存储
//! 后端再转状态机"（fail-closed）等编排语义在 handler 内原样保留。

use rr_hal::provision_store::Rr3WifiStore;
use rr_hal_api::{Rr3Link, Rr3LinkState};
use serde_json::{json, Value};

use crate::session;
use crate::wifi::{self, Rr3LinkCfg};

// ── 四点名 handler（自原 composite.rs 逐字迁入；0.0.8 批2 起由 dispatch.rs
// on_rpc 五臂调用）──

/// cloud/session 点名：参数裁决 → 链路裁决 → mock 会话应答（未就绪文案的
/// 配网态分流由 session 裁决面承接，D5——AP 态点名，不折叠成 up=false）
pub(crate) fn rr3_handle_session(
    dev: &dyn Rr3Link,
    mode: &str,
    args: &Value,
) -> Result<Value, String> {
    session::rr3_session_parse_args(args)?;
    let st = dev.state().map_err(|e| format!("链路读数失败: {e}"))?;
    let v = session::rr3_session_result(&st, mode)?;
    eprintln!("session: opened ssid={:?}", v["ssid"]);
    Ok(v)
}

/// wifi/scan 点名：无参裁决 → sim 模拟列表（backend=sim 诚实标注）
pub(crate) fn rr3_handle_wifi_scan(args: &Value) -> Result<Value, String> {
    rr3_wifi_scan_parse_args(args)?;
    Ok(rr3_wifi_scan_result())
}

/// wifi/connect 点名：参数裁决（legacy 语义）→ **先过凭据存储后端**（失败即
/// 折 error 应答、状态机不动，fail-closed）→ 存凭据转 station → connected
/// 如实取链路读数回执。
pub(crate) fn rr3_handle_wifi_connect(
    dev: &dyn Rr3Link,
    store: &dyn Rr3WifiStore,
    prov: &mut wifi::Rr3Provision,
    args: &Value,
    min_pass_len: u64,
) -> Result<Value, String> {
    let (ssid, pass) = rr3_wifi_connect_parse_args(args, min_pass_len)?;
    let st = dev.state().map_err(|e| format!("链路读数失败: {e}"))?;
    store
        .rr3_wifi_store_connect(&ssid, &pass)
        .map_err(|e| format!("凭据持久化失败: {e}"))?;
    prov.rr3_provision_connect(&ssid, &pass);
    eprintln!("wifi: connect ssid={ssid} mode=station（凭据由后端持久化，重启仍在册）");
    Ok(rr3_wifi_connect_result(&st, &ssid))
}

/// wifi/forget 点名：无参裁决 → **先真删后端配置**（失败折 error、状态机不动）
/// → 清凭据回 ap（恢复出厂 WiFi ≠ M6 整机恢复出厂）
pub(crate) fn rr3_handle_wifi_forget(
    store: &dyn Rr3WifiStore,
    prov: &mut wifi::Rr3Provision,
    args: &Value,
) -> Result<Value, String> {
    rr3_wifi_forget_parse_args(args)?;
    store
        .rr3_wifi_store_forget()
        .map_err(|e| format!("凭据删除失败: {e}"))?;
    prov.rr3_provision_forget();
    eprintln!("wifi: forget mode=ap（连接配置已真删）");
    Ok(rr3_wifi_forget_result())
}

// ── 载荷组装与参数下钻面（0.0.5 批I N-7，ADR-0019 修订 4：json! 组装与 Value
// 下钻上移承载层，件面 serde_json 清零；错误文案逐字，每句落在唯一一处）──

/// net/link 事件体：seq 是本流自己的计数（envelope 惯例，从 1 起）。
/// ssid/rssi 断链时是 null——设备面（Rr3SimLink）已抹掉，这里如实透传。
/// `mode` 是配网状态机（[`wifi::Rr3Provision`]）的当前模式，0.0.4 批D append——
/// 契约只增不改：既有 seq/up/ssid/rssi_dbm 四字段原样保留。
/// `up` 语义定案（增补轮六 D5 裁定，2026-10-19 对表落注）：只表"链路层已
/// 关联"，不表"可出网"——出网性由 `net.online` 通道（base 层恒在位格，本模块
/// 供数不占有）与运行期探测另证，本流不混载第二个事实的真值（一个事实一个
/// 真值点）。AP 态（mode=ap）恒 up=false：本机热点无上游关联。
/// 载荷形状契约 = contracts/events/connect-net-link.schema.json（随 D5 补齐）。
/// ssid 序列化面：契约值类型 `Rr3SsidText` 出定长缓冲取 &str（serde_json 不
/// 进契约类型，F1 定形）。（自 wifi 原子件上移）
pub fn rr3_link_body(seq: u64, st: &Rr3LinkState, mode: &str) -> Value {
    json!({
        "seq": seq,
        "up": st.up,
        "ssid": st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
        "rssi_dbm": st.rssi_dbm,
        "mode": mode,
    })
}

/// wifi/scan 点名参数裁决：无参动作，`null` 与空对象放行，多余字段点名拒。
/// （自 wifi 原子件上移）
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
/// NetworkManager 真扫描（cfg 门，方案 P4）。（自 wifi 原子件上移；假 AP 表
/// 整表迁 rr-hal sim 后端——0.0.5 批I N-9 落地，此处只组装形状，真值单点在
/// HAL 侧，首行 SSID 与链路 sim 默认 SSID 合流一处）
pub fn rr3_wifi_scan_result() -> Value {
    json!({
        "backend": "sim",
        "items": rr_hal::sim::rr3_sim_link_scan_list()
            .iter()
            .map(|(ssid, rssi, auth)| json!({ "ssid": ssid, "rssi": rssi, "auth": auth }))
            .collect::<Vec<Value>>(),
    })
}

/// wifi/connect 点名参数裁决（legacy panel.c h_connect 语义照搬）：ssid 非空、
/// 密码不短于下限，文案点名（"密码至少 8 位"是 legacy 原词，配网页依赖它——
/// 下限数据化后文案数字随 [`Rr3LinkCfg::min_pass_len`] 走，缺省 8 时不漂移）。
/// 字段名沿用 legacy 的 `pass`——面板表单同名透传，零翻译。（自 wifi 原子件上移）
pub fn rr3_wifi_connect_parse_args(
    args: &Value,
    min_pass_len: u64,
) -> Result<(String, String), String> {
    let ssid = args
        .get("ssid")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let pass = args.get("pass").and_then(Value::as_str).unwrap_or("");
    if ssid.is_empty() {
        return Err("缺 ssid（非空字符串）".into());
    }
    if (pass.len() as u64) < min_pass_len {
        return Err(format!("密码至少 {min_pass_len} 位（legacy 配网语义）"));
    }
    Ok((ssid.to_string(), pass.to_string()))
}

/// wifi/connect 结果：状态机已转 station；`connected` 如实取链路读数（sim 后端
/// 由 env 定死，不 fake"连上了"）——legacy"等连后回真实结果"的等待语义由
/// net/link 流承载，RPC 只回执状态机转移与当下读数。（自 wifi 原子件上移）
pub fn rr3_wifi_connect_result(st: &Rr3LinkState, ssid: &str) -> Value {
    json!({
        "ok": true,
        "connected": st.up,
        "mode": "station",
        "ssid": ssid,
    })
}

/// wifi/forget 点名参数裁决：无参动作（同 scan）。（自 wifi 原子件上移）
pub fn rr3_wifi_forget_parse_args(args: &Value) -> Result<(), String> {
    match args {
        Value::Null => Ok(()),
        Value::Object(m) if m.is_empty() => Ok(()),
        _ => Err("wifi/forget 无参数（收到多余字段）".into()),
    }
}

/// wifi/forget 结果：清凭据回 ap，回执转移后的模式。（自 wifi 原子件上移）
pub fn rr3_wifi_forget_result() -> Value {
    json!({ "ok": true, "mode": "ap" })
}

/// `rr3-connect/ops/list` 应答体（批N4 件九二，面板 /api/connect/ops 读口）：
/// ops 槽快照，**白名单组体**——槽名与 assemble.toml 槽表申报同源（bt/wifi 两组，
/// 门 d covers 申报的对账面）；配网凭据（ssid/pass）与链路瞬态（ssid/rssi 等
/// 运行态）不在快照面——快照只答 config 注入的策略面，瞬态走 net/link 事件流。
/// 签名随 0.0.8 批2 拆分适配两参（wifi_cfg/bt_up 自 Dispatcher 状态拆入），
/// json! 体逐字不变。
pub(crate) fn rr3_connect_ops_snapshot(wifi_cfg: &Rr3LinkCfg, bt_up: bool) -> Value {
    json!({
        "bt": { "up": bt_up },
        "wifi": {
            "link_report_period_ms": wifi_cfg.link_report_period_ms,
            "timeout_ms": wifi_cfg.timeout_ms,
            "min_pass_len": wifi_cfg.min_pass_len,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal_api::Rr3SsidText;

    fn st(up: bool) -> Rr3LinkState {
        Rr3LinkState {
            up,
            ssid: up.then(|| Rr3SsidText::rr3_ssid_text_new("rock-sim")),
            rssi_dbm: up.then_some(-50),
        }
    }

    // ── 载荷组装与下钻用例（自原 composite.rs 随载荷面迁入：件测"输入→结构体/
    // 判定"，承载层测"输入→字节"）──

    /// net/link 事件体逐字对齐契约：seq 自计数、断链时 ssid/rssi 必为 null
    /// （§9 红线：up=false 不许从死链读身份）、0.0.4 批D append 的 mode 随流
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

    /// wifi/connect 参数裁决（legacy 语义）：合法放行；缺 ssid / 空 ssid /
    /// 缺 pass / 密码短于下限 全部点名拒，"密码至少 8 位"是 legacy 原词。
    /// 等号下限是边界（§8 打折过的边界用例，本批补上）；下限数据化（批N4 件五
    /// N-11①）：传 8 = legacy 档，传 6 = 现场放宽档，文案数字随值走
    #[test]
    fn wifi_connect_args_parse_legacy_semantics() {
        let (ssid, pass) =
            rr3_wifi_connect_parse_args(&json!({"ssid": " home ", "pass": "password8"}), 8)
                .unwrap();
        assert_eq!(
            (ssid.as_str(), pass.as_str()),
            ("home", "password8"),
            "ssid 去首尾空白"
        );
        let (ssid, _) =
            rr3_wifi_connect_parse_args(&json!({"ssid": "home", "pass": "12345678"}), 8).unwrap();
        assert_eq!(ssid, "home", "恰好 8 位放行（等号边界）");
        let e = rr3_wifi_connect_parse_args(&json!({"pass": "password8"}), 8).unwrap_err();
        assert!(e.contains("ssid"), "缺 ssid 点名: {e}");
        let e =
            rr3_wifi_connect_parse_args(&json!({"ssid": "  ", "pass": "password8"}), 8)
                .unwrap_err();
        assert!(e.contains("ssid"), "空 ssid 点名: {e}");
        let e = rr3_wifi_connect_parse_args(&json!({"ssid": "home"}), 8).unwrap_err();
        assert!(e.contains("8 位"), "缺 pass 点名: {e}");
        let e =
            rr3_wifi_connect_parse_args(&json!({"ssid": "home", "pass": "short"}), 8).unwrap_err();
        assert!(
            e.contains("密码至少 8 位"),
            "短密码点名（legacy 原词）: {e}"
        );
        // 现场放宽档：下限 6 时 7 位密码放行、6 位等号放行、文案数字跟值
        let (ssid, _) =
            rr3_wifi_connect_parse_args(&json!({"ssid": "home", "pass": "1234567"}), 6).unwrap();
        assert_eq!(ssid, "home", "放宽档 7 位放行");
        let (ssid, _) =
            rr3_wifi_connect_parse_args(&json!({"ssid": "home", "pass": "123456"}), 6).unwrap();
        assert_eq!(ssid, "home", "放宽档等号 6 位放行");
        let e =
            rr3_wifi_connect_parse_args(&json!({"ssid": "home", "pass": "12345"}), 6).unwrap_err();
        assert!(e.contains("密码至少 6 位"), "文案数字随下限走: {e}");
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

    // ── MC-6 配网凭据持久化：handler 编排（sim 链路 + sim 存储）──

    /// connect 先落后端再转态、forget 真删后端——handler 编排两腿与状态机
    /// 同向；"重启仍在册"的承载半边在 rr-hal::provision_store 用例钉（本处只
    /// 钉编排顺序与状态机同向）。
    #[test]
    fn connect_and_forget_handlers_move_store_and_state_together() {
        let dev = rr_hal::sim::Rr3SimLink::rr3_sim_link_new(true, "rock-sim", -50);
        let store = rr_hal::provision_store::Rr3SimWifiStore::default();
        let mut prov = wifi::Rr3Provision::rr3_provision_new();

        let v = rr3_handle_wifi_connect(
            &dev,
            &store,
            &mut prov,
            &json!({"ssid": "home", "pass": "password8"}),
            8,
        )
        .unwrap();
        assert_eq!(v["mode"], "station");
        assert_eq!(prov.rr3_provision_mode(), "station", "状态机随存储同向转");
        assert_eq!(
            store
                .rr3_wifi_store_ssid()
                .unwrap()
                .map(|s| s.rr3_ssid_text_as_str().to_string()),
            Some("home".to_string()),
            "凭据已落后端"
        );

        let v = rr3_handle_wifi_forget(&store, &mut prov, &json!({})).unwrap();
        assert_eq!(v["mode"], "ap");
        assert_eq!(prov.rr3_provision_mode(), "ap");
        assert!(
            store.rr3_wifi_store_ssid().unwrap().is_none(),
            "forget 后端配置真删"
        );
    }

    /// fail-closed：存储失败时状态机**不动**、错误点名——不许"存储没成功却
    /// 当作配上网了"（存储是持久化真值，状态机跟着它走）。
    #[test]
    fn store_failure_leaves_provision_state_untouched() {
        struct Rr3FailingStore;
        impl Rr3WifiStore for Rr3FailingStore {
            fn rr3_wifi_store_connect(
                &self,
                _ssid: &str,
                _pass: &str,
            ) -> Result<(), rr_hal_api::Rr3HalError> {
                Err(rr_hal_api::Rr3HalError::Unsupported("test/failing-store"))
            }
            fn rr3_wifi_store_forget(&self) -> Result<(), rr_hal_api::Rr3HalError> {
                Err(rr_hal_api::Rr3HalError::Unsupported("test/failing-store"))
            }
            fn rr3_wifi_store_ssid(
                &self,
            ) -> Result<Option<rr_hal_api::Rr3SsidText>, rr_hal_api::Rr3HalError> {
                Err(rr_hal_api::Rr3HalError::Unsupported("test/failing-store"))
            }
        }
        let dev = rr_hal::sim::Rr3SimLink::rr3_sim_link_new(true, "rock-sim", -50);
        let store = Rr3FailingStore;
        let mut prov = wifi::Rr3Provision::rr3_provision_new();

        let e = rr3_handle_wifi_connect(
            &dev,
            &store,
            &mut prov,
            &json!({"ssid": "home", "pass": "password8"}),
            8,
        )
        .unwrap_err();
        assert!(e.contains("凭据持久化失败"), "存储失败点名: {e}");
        assert_eq!(
            prov.rr3_provision_mode(),
            "ap",
            "存储没成功，状态机不许转 station（fail-closed）"
        );

        let e = rr3_handle_wifi_forget(&store, &mut prov, &json!({})).unwrap_err();
        assert!(e.contains("凭据删除失败"), "删除失败点名: {e}");
    }
}
