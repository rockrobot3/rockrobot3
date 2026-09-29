//! 电池采样与低压兜底。
//!
//! 关断决策在本地完成，不等总线、不等 brain（architecture §7.3 安全兜底）；
//! `rr3-power/battery/state` 只是摘要上报。

pub const SURFACE: &[&str] =
    &["battery: 电压/电量/温度摘要 -> rr3-power/battery/state (event)，关断为本地决策"];
