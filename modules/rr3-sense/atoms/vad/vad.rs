//! vad 原子件入口（五件套②，0.0.5 批C① 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `audio.vad`（atom.toml）。SURFACE 是启动自述（组合件 rr3_sense_run 开头
//! 逐字打印，契约面零改动红线）；`Rr3VadConfig` 是装配注入的 config 数据形状
//! （config.toml `[rr3-sense.vad]`，阈值等策略数据化注入，机制件零策略载荷，
//! ADR-0019 裁定 5 L3）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/audio/vad.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &["audio/vad: 起音/静音边沿 -> rr3-sense/audio/vad_start (event)"];

/// 发布面主题（atom.toml [events].publishes 同源；topics.toml owner=rr3-sense）
pub const RR3_VAD_START_TOPIC: &str = "rr3-sense/audio/vad_start";

/// VAD 参数（config.toml `[rr3-sense.vad]`；缺省值 = config.toml 的登记值）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3VadConfig {
    /// 起音阈值（帧 RMS ≥ 它才算"有声"）
    pub rms_start: i64,
    /// 判停尾静音：在语音中连续静音达到此时长 → Offset
    pub silence_tail_ms: u64,
    /// 最短语音：起音须连续保持此时长才认（防瞬态 blip 误触发）
    pub min_speech_ms: u64,
}

impl Default for Rr3VadConfig {
    fn default() -> Self {
        Self {
            rms_start: 1500,
            silence_tail_ms: 700,
            min_speech_ms: 200,
        }
    }
}

/// 公开面（0.0.5 批I S-6 收口）：状态机与边沿类型是件契约面；
/// 事件体构造已上移组合件（S-7），`rr3_frame_rms` 降 pub(crate)（无件外读者）
pub use mech::{Rr3VadEdge, Rr3VadState};
