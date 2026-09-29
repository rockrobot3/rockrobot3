//! 传输层。`trait Transport` 的零拷贝签名（buffer 所有权移交语义）随 M2 一并设计，
//! 以免现在造一个 ADR-0007 §裁定 3 要推翻的接口。
//!
//! 规划（ADR-0006 §5）：
//! - `uds`：M2 唯一实现，路径取 `RR3_UDS_PATH_DEFAULT`，可被 `RR3_BUS_PATH_ENV` 覆盖
//! - `shm`：实时控制面（100Hz–1kHz 关节状态/力矩/PCM 流），M5
//! - `tcp`：多机协作，预留（标准端口 6060）
//!
//! 反射环（1k–10kHz 电机 FOC/PID）永远不进总线，见 ADR-0007 §裁定 1。

pub mod frame;

/// 默认 UDS 路径（ADR-0006 §6，自有路径避免与同机真实 Airymax 运行时冲突）
pub const RR3_UDS_PATH_DEFAULT: &str = "/var/run/rock3-ipc.sock";

/// 覆盖 UDS 路径的环境变量键（naming.md：`RR3_` 大写前缀）
pub const RR3_BUS_PATH_ENV: &str = "RR3_BUS_PATH";
