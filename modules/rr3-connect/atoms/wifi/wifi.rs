//! wifi 原子件入口（五件套②，0.0.5 批C④ 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `net.wifi`（atom.toml）。SURFACE 是启动自述（组合件 rr3_connect_run 开头
//! 逐字打印，契约面零改动红线）；`Rr3LinkCfg` 是 config `[rr3-connect.wifi]` 的
//! 形状（上报周期 + 漫游探针开关，config 数据化注入，机制件零策略载荷）；链路
//! 读数经 rr-hal trait（`Rr3Link`）由组合件注入，原子不直接碰 NetworkManager。
//! 配网凭据只存内存（conventions §3），真持久化（wpa_supplicant/NVS）板上后置。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/wifi.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &[
    "wifi: NetworkManager/D-Bus（板上件）-> rr3-connect/net/link (event)；sim 后端 env 注入",
    "wifi: 状态翻转必发 + 周期保底重发（流是摘要不是逐拍镜像）；mode=ap|station 随流发布",
    "wifi: rpc wifi/scan|wifi/connect|wifi/forget（0.0.4 批D）；凭据内存不落盘重启即忘，真持久化板上后置",
];

/// 发布面唯一主题（module.toml [events].publishes 同源）
pub const RR3_LINK_TOPIC: &str = "rr3-connect/net/link";

/// 配网面三主题（0.0.4 批D：topics.toml/manifest/pin 三处同批改；rpc 面在 module.toml [rpc].exposes，原子层不重述）
pub const RR3_WIFI_SCAN_TOPIC: &str = "rr3-connect/wifi/scan";
pub const RR3_WIFI_CONNECT_TOPIC: &str = "rr3-connect/wifi/connect";
pub const RR3_WIFI_FORGET_TOPIC: &str = "rr3-connect/wifi/forget";

/// config `[rr3-connect.wifi]` 的形状（上报周期 + 漫游探针开关）
#[derive(Debug, Clone, Copy)]
pub struct Rr3LinkCfg {
    pub link_report_period_ms: u64,
    pub roam_probe_enabled: bool,
}

pub use mech::{
    rr3_link_body, rr3_wifi_connect_parse_args, rr3_wifi_connect_result,
    rr3_wifi_forget_parse_args, rr3_wifi_forget_result, rr3_wifi_scan_parse_args,
    rr3_wifi_scan_result, Rr3LinkEdge, Rr3Provision,
};
