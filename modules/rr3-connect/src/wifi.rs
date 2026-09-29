//! WiFi 链路：经 NetworkManager/D-Bus 管理与取状态，发布 `rr3-connect/net/link`。
//!
//! 云端凭据不落盘（conventions §3），HTTPS 会话参数随 M5 与 rr3-brain 对齐。

pub const SURFACE: &[&str] =
    &["wifi: NetworkManager/D-Bus -> rr3-connect/net/link (event)"];
