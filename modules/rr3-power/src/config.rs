//! rr3-power 装配数据形状（自 boot.rs 抽出，字段与语义逐字不变）。
//!
//! 独立成件让接入层（boot 解析/就绪编排）与机制层（dispatch 执行）共同依赖
//! 数据形状，而不是机制层反向引用接入层——模块依赖保持单向。

use crate::battery::Rr3BatteryCfg;
use crate::charger::Rr3ChargerCfg;

/// 本模块 config 的汇总形状（自原 composite.rs 迁入，字段形状零改动）
#[derive(Debug, Clone)]
pub(crate) struct Rr3PowerCfg {
    pub(crate) battery: Rr3BatteryCfg,
    pub(crate) charger: Rr3ChargerCfg,
    pub(crate) rail: Rr3RailCfg,
}

/// rail 段形状（批N2 件五）：段必填＋全部键必填——`up = false` 时其余键为惰性
/// 但显式写（F-6 精神：合法不适用也要写出来，统一解析路径无分支）；`max_age_ms
/// = 0` 合法（0 = 龄期恒超界 ⇒ 恒 Unknown，诚实语义）；`severe_mv < mild_mv`
/// 严格校验（相等也红——Mild 档不可达）。
#[derive(Debug, Clone)]
pub(crate) struct Rr3RailCfg {
    pub(crate) up: bool,
    pub(crate) domain: String,
    pub(crate) sample_period_ms: u64,
    pub(crate) mild_mv: u16,
    pub(crate) severe_mv: u16,
    pub(crate) max_age_ms: u32,
}
