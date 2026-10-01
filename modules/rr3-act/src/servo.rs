//! 舵机控制：接收 `rr3-act/servo/command`（角度/时限，<100Hz 事务面），
//! 摘要发布 `rr3-act/servo/state`。驱动经 rr-hal trait，不直接碰 /sys。

pub const SURFACE: &[&str] =
    &["servo: rpc rr3-act/servo/command (request) | publish rr3-act/servo/state (stream)"];
