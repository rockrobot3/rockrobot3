//! Web 面板：设备端只读状态页 + 调试操作。
//!
//! 面板**不是**第二条控制面：它经 rr-bus 收发，与模块同权，不得绕过 rrd 的注册与
//! 前缀归属检查（ADR-0006 §裁定 3）。静态资源在 `www/`。

pub mod server;

pub const SURFACE: &[&str] = &["panel: www 静态页（经总线，随 M5）"];

pub fn surface() -> impl Iterator<Item = &'static str> {
    SURFACE.iter().chain(server::SURFACE.iter()).copied()
}
