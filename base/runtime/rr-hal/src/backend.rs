//! 后端规划（architecture.md §5、ADR-0007）。
//!
//! | 后端 | 落地 | 承载 |
//! |---|---|---|
//! | `linux` | M2 | Buildroot 根文件系统内的 `/dev`、ALSA、iio、gpio-sysfs |
//! | `sim` | M2 | `tools/rr3-sim` 使用的虚拟设备，供 CI 与无硬件开发 |
//! | `shm` 加速 | M5 | 实时控制面（100Hz–1kHz）零拷贝路径，配 `F_ZEROCOPY` |
//!
//! 反射环（1k–10kHz 电机 FOC/PID）不进这些 trait，留在 `rr3-act` 进程内。

/// 后端标识（写入日志与诊断主题 `rr3-runtime/module/state`）
pub const RR3_BACKEND_LINUX: &str = "linux";
pub const RR3_BACKEND_SIM: &str = "sim";
pub const RR3_BACKEND_PLACEHOLDER: &str = "placeholder";
