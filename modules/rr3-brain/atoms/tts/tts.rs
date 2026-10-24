//! tts 原子件入口（五件套②，批N32 件四 Rust 拆分收敛，ADR-0016）：只声明
//! 公开面，机制在 mech.rs。占格 `cognition.tts`（atom.toml）。
//! brain 启动自述是组合件级逐字面（main.rs 自印 publish/subscribe/rpc——Python
//! composite.py 打印序列契约零改动），不走 atom SURFACE 链；装配 = src/main.rs
//! `#[path]` 原位挂载（rr3-sense 同形）。本件只装**纯机制**（voice 命名/mock
//! 正弦 PCM，⑧-b 依赖闭包纪律：无 IO 无环境读取无 JSON）；HTTP 传输与合成客户端
//! 在组合件级 [`crate::cloud`]。M4 的 PCM 是验证性消费（非空 + 时长换算入日志）；
//! 真播放随 M5 板上 ALSA sink（ADR-0014 裁定 3）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 公开面：voice 命名与 mock 正弦 PCM（确定性 440Hz；参数化底座
/// rr3_sine_pcm_ms 与后缀常量由 rr3_sine_pcm/rr3_tts_voice 内部构成——mech 内
/// pub fn 留给件级 parity 腿，bin 面无件外读者不进入口 re-export）
pub use mech::{rr3_sine_pcm, rr3_tts_voice};
