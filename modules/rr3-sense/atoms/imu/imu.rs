//! imu 原子件入口（五件套②，0.0.5 批C① 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `sensor.imu`（atom.toml）。**占位件**（方案 §12.1）：SURFACE 摘要落位，
//! 真件后端随采购批填实——融合与闭环在 rr3-act 的反射环里完成，此处只做摘要上报。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/imu.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &["imu: 姿态摘要 -> rr3-sense/imu/state (stream)"];
