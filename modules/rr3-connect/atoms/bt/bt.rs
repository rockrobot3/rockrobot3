//! bt 原子件入口（五件套②，0.0.5 批C④ 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `net.bt`（atom.toml）。SURFACE 是启动自述（组合件 rr3_connect_run 开头
//! 逐字打印，契约面零改动红线）。**主题不登记**（方案 D5 显式裁定）：btctl 的
//! 日志回放/远程命令语义已被承接，蓝牙链路暂无可发布事件——按"不预先占位"
//! 纪律（conventions §3），启用事件面前须先改 contracts/topics.toml。BlueZ
//! 真后端属板上件（cfg 门），骨架不 claim 真射频链路。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（0.0.5 批I N-1/N-8 修订：env 注入退役改 config 数据化注入、
/// 栈名撤出由后端动态交——修订后的行重新冻结）
pub const SURFACE: &[&str] = &[
    "bt: 链路状态机骨架（0.0.4 批E）——Off/Discoverable/Connected 三态纯函数转移，真后端板上后置（cfg 门）",
    "bt: 开机初值经 config [rr3-connect.bt] up 注入（缺省关，出厂不自动开）；主题不登记（语义已被日志环+rr3-ctl 承接），启用前先改契约",
];

pub use mech::Rr3BtState;
