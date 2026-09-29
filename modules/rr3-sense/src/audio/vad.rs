//! 本地 VAD：静音/起音**边沿**检测。
//!
//! 只发边沿 + 置信度摘要，不发闭环数据（ADR-0007 §决策一）。判停阈值与 legacy/main.c
//! 的对话策略一致，参数落 config.toml，硬编码视为回归。

pub const SURFACE: &[&str] = &["audio/vad: 起音/静音边沿 -> rr3-sense/audio/vad_start (event)"];
