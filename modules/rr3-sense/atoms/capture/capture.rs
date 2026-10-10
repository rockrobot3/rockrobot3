//! capture 原子件入口（五件套②，0.0.5 批C① 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `audio.capture`（atom.toml）。SURFACE 是启动自述（组合件 rr3_sense_run 开头
//! 逐字打印，契约面零改动红线）；`Rr3AudioConfig` 是装配注入的 config 数据形状
//! （config.toml `[rr3-sense.audio]`，机制件零策略载荷，ADR-0019 裁定 5 L3）。
//! DoA 粗判向随本原子落档（多通道原始帧只在此一用，下混前判向；0.0.5 批I S-8
//! 整型化：isqrt + Q15 定点 + 256 格 asin 查表）；vad_start onset 的 `doa` 字段体
//! 组装（rr3_doa_field）随 0.0.5 批I S-7 上移组合件——判向机制仍在件内，组合件
//! 只组装字节（跨原子零类型依赖，只过 serde_json::Value）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/audio/capture.rs 与 src/audio/doa.rs 的
/// SURFACE 常量，契约面零改动；打印序 = 原 surface() 链序 capture → doa）
pub const SURFACE: &[&str] = &[
    "audio/capture: 16k/mono/S16_LE -> rr3-sense/audio/pcm (stream)",
    "audio/doa: 双麦 ILD 粗判向（±90° 分档）→ vad_start onset 的 doa 字段（sense 内部落档，零新契约）",
];

/// 发布面主题（atom.toml [events].publishes 同源；topics.toml owner=rr3-sense）
pub const RR3_PCM_TOPIC: &str = "rr3-sense/audio/pcm";

/// config `[rr3-sense.audio]` 的形状（采样率/声道/节拍）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3AudioConfig {
    pub sample_rate: u32,
    pub channels: u8,
    pub frame_ms: u64,
}

pub(crate) use mech::rr3_pcm_downmix_mono;
/// `Rr3DoaBin` 仅被 composite 测试按名构造（bin 目标面无读者，pub use 在 bin 编译
/// 单元会吃 unused_imports）——测试面专用导出。
#[cfg(test)]
pub use mech::Rr3DoaBin;
/// 公开面（0.0.5 批I S-6 收口；深挖收口再修）：节拍换算/判向与判向结果是件契约面；
/// `rr3_pcm_event_body`/`rr3_doa_field` 已上移组合件（S-7），stream 级下混亦随
/// 深挖收口上移组合件（`Rr3PcmStream` 是承载层数据类型，C7 白名单外）——
/// `rr3_pcm_downmix_mono`/阈值常数 pub(crate)（唯一读者是组合件承载层）
pub use mech::{rr3_doa_fix, rr3_frames_per_tick, Rr3DoaFix};
