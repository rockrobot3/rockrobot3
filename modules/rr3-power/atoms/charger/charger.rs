//! charger 原子件入口（五件套②，0.0.5 批B③ 打样）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `power.charger`（atom.toml）。SURFACE 是启动自述（逐字冻结，契约面零改动）；
//! `Rr3ChargerCfg` 是装配注入的 config 数据形状（config.toml `[rr3-power.charger]`）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/charger.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &["charger: 充电状态 -> rr3-power/charger/state (event)"];

/// 发布面主题（atom.toml [events].publishes 同源；topics.toml owner=rr3-power）
pub const RR3_CHARGER_STATE_TOPIC: &str = "rr3-power/charger/state";

/// config `[rr3-power.charger]` 的形状
#[derive(Debug, Clone, Copy)]
pub struct Rr3ChargerCfg {
    pub state_period_ms: u64,
}
