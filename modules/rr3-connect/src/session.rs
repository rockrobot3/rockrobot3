//! 云端会话：`rr3-connect/cloud/session`（request 事务面）。
//!
//! 会话建立/续期由 brain 侧发起，connect 只提供链路（骨架自述的原语义）。
//! sim 语义：收到 `action="open"` → 链路 up 则回 mock 会话应答（真 TLS 会话
//! 属云侧凭据联调，0.0.2 范围外，`RR3_BRAIN_CLOUD=mock` 注入点维持），链路
//! down 则业务性失败折 error 应答回话——请求-应答语义，不当致命错误。
//! 凭据不经过本模块持久化（conventions §3）。

use rr_hal::Rr3LinkState;
use serde_json::{json, Value};

/// 会话点名（module.toml [rpc].exposes 同源；topics.toml owner=rr3-connect）
pub const RR3_SESSION_TOPIC: &str = "rr3-connect/cloud/session";

pub const SURFACE: &[&str] = &[
    "session: rpc rr3-connect/cloud/session (request)；action=open，链路就绪才应答",
    "session: sim 回 mock 会话（真 TLS 属云侧联调范围外）；凭据不落盘",
];

/// session 点名参数裁决：只认 `action="open"`。缺参/类型错/未知值点名拒。
pub fn rr3_session_parse_args(args: &Value) -> Result<(), String> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺 action（必须取 \"open\"）".to_string())?;
    if action != "open" {
        return Err(format!("未知 action: {action}（只认 \"open\"）"));
    }
    Ok(())
}

/// 会话结果裁决：链路 up → mock 会话应答（回显当前链路 ssid）；down → 业务性
/// 失败（折 result.error，不断链）。"链路"是判据词，e2e 靠它点名。
/// ssid 序列化面：契约值类型 `Rr3SsidText` 出定长缓冲取 &str（serde_json 不
/// 进契约类型，F1 定形）。
pub fn rr3_session_result(st: &Rr3LinkState) -> Result<Value, String> {
    if !st.up {
        return Err("链路未就绪（net/link up=false），云端会话拒绝建立".into());
    }
    Ok(json!({
        "session": "opened",
        "endpoint": "mock",
        "ssid": st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::Rr3SsidText;

    fn st(up: bool) -> Rr3LinkState {
        Rr3LinkState {
            up,
            ssid: up.then(|| Rr3SsidText::rr3_ssid_text_new("rock-sim")),
            rssi_dbm: up.then_some(-50),
        }
    }

    /// 参数裁决：合法值放行，缺参 / 类型错 / 未知值全部点名——业务性失败折
    /// result.error 的入参一半钉在这里。
    #[test]
    fn session_args_parse_open_only() {
        assert_eq!(rr3_session_parse_args(&json!({"action": "open"})), Ok(()));
        let e = rr3_session_parse_args(&json!({})).unwrap_err();
        assert!(e.contains("action"), "缺参点名: {e}");
        let e = rr3_session_parse_args(&json!({"action": 1})).unwrap_err();
        assert!(e.contains("action"), "类型错点名: {e}");
        let e = rr3_session_parse_args(&json!({"action": "close"})).unwrap_err();
        assert!(e.contains("close") && e.contains("open"), "未知值点名: {e}");
    }

    /// 会话裁决：链路 up 回 mock 会话（回显 ssid），down 折 error 点名"链路"。
    #[test]
    fn session_result_follows_link_up() {
        let v = rr3_session_result(&st(true)).unwrap();
        assert_eq!(v["session"], "opened");
        assert_eq!(v["endpoint"], "mock");
        assert_eq!(v["ssid"], "rock-sim");
        let e = rr3_session_result(&st(false)).unwrap_err();
        assert!(e.contains("链路"), "断链要点名判据词: {e}");
    }
}
