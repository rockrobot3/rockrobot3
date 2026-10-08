//! battery 原子件入口（五件套②，0.0.5 批B③ 打样）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `power.battery`（atom.toml）。SURFACE 是启动自述（组合件 rr3_power_run 开头
//! 逐字打印，契约面零改动红线）；`Rr3BatteryCfg` 是装配注入的 config 数据形状
//! （config.toml `[rr3-power.battery]`，阈值等策略数据化注入，机制件零策略载荷，
//! ADR-0019 裁定 5 L3）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/battery.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &[
    "battery: 电压/电量/温度摘要 -> rr3-power/battery/state (event)，关断为本地决策",
    "battery: 低电越阈 -> rr3-power/battery/low (event)，同一次低电段恰发一次",
];

/// 发布面两主题（atom.toml [events].publishes 同源；topics.toml owner=rr3-power）
pub const RR3_BATTERY_STATE_TOPIC: &str = "rr3-power/battery/state";
pub const RR3_BATTERY_LOW_TOPIC: &str = "rr3-power/battery/low";

/// config `[rr3-power.battery]` 的形状（采样周期 + 两级阈值）
#[derive(Debug, Clone, Copy)]
pub struct Rr3BatteryCfg {
    pub sample_period_ms: u64,
    pub low_voltage_mv: u16,
    pub cutoff_voltage_mv: u16,
}

pub use mech::{rr3_battery_low_body, rr3_battery_state_body, Rr3LowEdge};
