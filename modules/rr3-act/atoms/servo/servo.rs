//! servo 原子件入口（五件套②，0.0.5 批C② 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `act.servo`（atom.toml）。SURFACE 是启动自述（组合件 rr3_act_run 开头
//! 逐字打印，契约面零改动红线）；`Rr3ServoCfg` 是装配注入的 config 数据形状
//! （config.toml `[rr3-act.servo]` 只持时限/限速——行程真值 = 执行轴状态注入的
//! lo/hi（批N3 件五门 g 收敛，档案是唯一真值点），组合件 rr3_act_run 覆填本
//! 形状，机制件零策略载荷，ADR-0019 裁定 5 L3）。执行面经 rr-hal trait
//! （`Rr3PwmServo`）由组合件注入，原子不直接碰 /sys；HAL 只执行，行程与时限
//! 裁决在这里（限值来源见 `angle_min_deg`/`angle_max_deg` 字段注释）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/servo.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] =
    &["servo: rpc rr3-act/servo/command (request) | publish rr3-act/servo/state (stream，限速)"];

/// 指令面主题（module.toml [rpc].exposes 同源；原子层不重述 rpc 契约）
pub const RR3_SERVO_COMMAND_TOPIC: &str = "rr3-act/servo/command";

/// 状态流主题（atom.toml [events].publishes 同源；topics.toml owner=rr3-act）
pub const RR3_SERVO_STATE_TOPIC: &str = "rr3-act/servo/state";

/// config `[rr3-act.servo]` 的形状（解析与裁决的唯一依据）。行程两键的填值
/// 来源 = 执行轴状态 RR3_ACT_AXIS_STATE 的 lo/hi（批N3 件五门 g 收敛，档案
/// 是唯一真值点）；config 解析面占位 0/180，由组合件 rr3_act_run 覆填。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ServoCfg {
    /// 行程下限（度）——档案 lo_deg（经执行轴状态注入）
    pub angle_min_deg: u16,
    /// 行程上限（度）——档案 hi_deg（经执行轴状态注入）
    pub angle_max_deg: u16,
    /// 指令时限上限：超过它拒收——"超时即停，不给堵转留机会"
    pub move_timeout_ms: u64,
    /// servo/state 两次发布的最小间隔（限速流的"限速"）
    pub state_min_interval_ms: u64,
}

pub use mech::{
    rr3_servo_check_motion, rr3_servo_check_timeout, rr3_servo_check_travel, Rr3RateGate,
};
