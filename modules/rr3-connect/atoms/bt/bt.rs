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

/// 启动自述（字符串逐字冻结——原 src/bt.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &[
    "bt: 链路状态机骨架（0.0.4 批E）——Off/Discoverable/Connected 三态纯函数转移，BlueZ 真后端板上后置（cfg 门）",
    "bt: sim 后端 env 注入（RR3_HAL_SIM_BT_UP=1 开机即上电，缺省关）；主题不登记（语义已被日志环+rr3-ctl 承接），启用前先改契约",
];

/// sim 后端蓝牙开机初值键（`1` 上电进 Discoverable / `0` 或未设 → Off，
/// 出厂不自动开蓝牙）。取值不认一律按关——env 是 sim 注入口不是配置面，
/// 宽容解析反而藏错；`1`/`0` 词汇与 `RR3_HAL_SIM_LINK_UP` 同款。
pub const RR3_HAL_SIM_BT_UP_ENV: &str = "RR3_HAL_SIM_BT_UP";

pub use mech::{rr3_bt_backend_up_default, Rr3BtState};
