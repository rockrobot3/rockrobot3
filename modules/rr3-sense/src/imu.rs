//! IMU 姿态读取。融合与闭环在 rr3-act 的反射环里完成，此处只做摘要上报。

pub const SURFACE: &[&str] = &["imu: 姿态摘要 -> rr3-sense/imu/state (stream)"];
