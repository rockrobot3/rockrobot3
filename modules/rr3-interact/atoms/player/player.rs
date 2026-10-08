//! player 原子件入口（五件套②，0.0.5 批C③ 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `audio.playback`（atom.toml）。SURFACE 是启动自述（组合件 rr3_interact_run
//! 开头逐字打印，契约面零改动红线）。播放面经 rr-hal 播放 trait（`Rr3AudioSink`）
//! 由组合件注入，原子不直接碰声卡；M3 的 v0.0.1 播放面 = 正弦 burst 占位合成，
//! 走的是**真链路**——rr3.call 点名进 → sink 写块 → 应答 `{"played_ms"}`，
//! 真合成随里程碑换进 [`rr3_tts_play`] 内部，请求/应答形状不变。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/player.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &["player: rpc rr3-interact/tts/text (request)"];

/// 本模块 exposes 的 TTS 主题（module.toml [rpc].exposes 同源；原子层不重述 rpc 契约）
pub const RR3_TTS_TOPIC: &str = "rr3-interact/tts/text";

pub use mech::{rr3_tts_parse_args, rr3_tts_play, rr3_tts_result};
