//! Web 面板：设备端状态页 + 调试操作（0.0.4 批C，D3/Q3 裁定落地）。
//!
//! 面板**不是**第二条控制面：硬件控制只走 `rr3.call` 经 rrd 归属门（server.rs 的
//! [`server::Rr3PanelDispatch`] 无 Publish 变体，越权在类型面就造不出来）；ctl 查询
//! 走进程内裸 UDS 短连，与外部运维探针同权（ADR-0010）。监听面缺省回环
//! `127.0.0.1:8080`，`RR3_PANEL_ADDR` 可改（局域网可信面声明落 README）。
//! 静态资源单文件内嵌于 `www/index.html`，无运行时文件依赖。

pub mod server;

/// 单文件内嵌静态页（`GET /` 的 body）——编译期打进二进制
pub const RR3_PANEL_INDEX_HTML: &str = include_str!("www/index.html");

pub const SURFACE: &[&str] = &["panel: www 静态页（经总线，随 M5）"];

pub fn surface() -> impl Iterator<Item = &'static str> {
    SURFACE.iter().chain(server::SURFACE.iter()).copied()
}
