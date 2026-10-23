//! relay 原子件入口（五件套②，0.0.5 批C② 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `act.relay`（atom.toml）。SURFACE 是启动自述（组合件 rr3_act_run 开头
//! 逐字打印，契约面零改动红线）。最小面：`{"on": bool}` 写一路 `Rr3GpioOut`；
//! 通断状态摘要 `rr3-act/relay/state` 变化即发（0.0.3 T3 补裁定 append，
//! RPC 与 intent 两条驱动路共用同一条状态流）。通断面经 rr-hal trait 由组合件注入。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/relay.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &[
    "relay: rpc rr3-act/relay/set (request)",
    "relay: pub rr3-act/relay/state (event, 变化即发)",
];

/// 指令面主题（module.toml [rpc].exposes 同源；原子层不重述 rpc 契约）
pub const RR3_RELAY_SET_TOPIC: &str = "rr3-act/relay/set";

/// 通断状态摘要主题（atom.toml [events].publishes 同源；contracts/topics.toml 25 条之一）
pub const RR3_RELAY_STATE_TOPIC: &str = "rr3-act/relay/state";

pub use mech::{rr3_relay_apply, rr3_relay_counter_key, rr3_relay_state_due, Rr3RelayApplyError};
