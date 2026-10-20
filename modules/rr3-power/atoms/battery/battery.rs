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

/// 启动自述（批B③ 逐字冻结面；批N5 件二笔3 随 battery/ok 新发布面 append 一行——
/// 自述与发布面同账，防 ⑨-l 那类"publishes 有主题自述不认"的漂移）
pub const SURFACE: &[&str] = &[
    "battery: 电压/电量/温度摘要 -> rr3-power/battery/state (event)，关断为本地决策",
    "battery: 低电越阈 -> rr3-power/battery/low (event)，同一次低电段恰发一次",
    "battery: 回阈恢复 -> rr3-power/battery/ok (event)，同一次低电段恢复沿恰发一次（批N5 件二笔3）",
];

/// 发布面三主题（atom.toml [events].publishes 同源；topics.toml owner=rr3-power）
pub const RR3_BATTERY_STATE_TOPIC: &str = "rr3-power/battery/state";
pub const RR3_BATTERY_LOW_TOPIC: &str = "rr3-power/battery/low";
/// 低电恢复事件（批N5 件二笔3，§17 运维留痕）：回阈首拍恰一次，rrd 据此清
/// 全局低电旗、恢复新 spawn。与 low 同 owner 同 L1，常量放邻位（两个沿一个对）。
pub const RR3_BATTERY_OK_TOPIC: &str = "rr3-power/battery/ok";

/// config `[rr3-power.battery]` 的形状（采样周期 + 两级阈值）
#[derive(Debug, Clone, Copy)]
pub struct Rr3BatteryCfg {
    pub sample_period_ms: u64,
    /// 低电阈值：本轮唯一有后果的阈值——越过即发 `battery/low`，rrd 置低电旗
    /// → MC-5 低电禁动（执行面拒动）。
    pub low_voltage_mv: u16,
    /// 关断阈值：**声明未生效**（0.0.5 批M MC-8① 裁定 (c)）——0.0.5 **不关断**
    /// （关断动作归 M6 关机链，需 T1/T2 物理断电硬件）。本字段保留为策略数据，
    /// 机器读者 = 注入门（`assemble.toml` ops 槽，⑧-c 门强制）；不构成"已实现关断"。
    pub cutoff_voltage_mv: u16,
}

pub use mech::Rr3LowEdge;
