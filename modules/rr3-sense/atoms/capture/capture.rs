//! capture 原子件入口（五件套②，0.0.5 批C① 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `audio.capture`（atom.toml）。SURFACE 是启动自述（组合件 rr3_sense_run 开头
//! 逐字打印，契约面零改动红线）；`Rr3AudioConfig` 是装配注入的 config 数据形状
//! （config.toml `[rr3-sense.audio]`，机制件零策略载荷，ADR-0019 裁定 5 L3）。
//! DoA 粗判向随本原子落档（B批 b3b 起 TDOA：判向窗快照在 onset 时由组合件从
//! ring 线性化后传入，机制件纯函数零状态；0.0.5 批I S-8 整型化：Q7 定标 +
//! 256 格 asin 查表延续）；vad_start onset 的 `doa` 字段体组装（rr3_doa_field）
//! 随 0.0.5 批I S-7 上移组合件——判向机制仍在件内，组合件只组装字节（跨原子
//! 零类型依赖，只过 serde_json::Value）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/audio/capture.rs 与 src/audio/doa.rs 的
/// SURFACE 常量，契约面零改动；打印序 = 原 surface() 链序 capture → doa）
pub const SURFACE: &[&str] = &[
    "audio/capture: 16k/mono/S16_LE -> rr3-sense/audio/pcm (stream)",
    "audio/doa: 双麦 TDOA 时域互相关粗判向（±90° 分档）→ vad_start onset 的 doa 字段（sense 内部落档，零新契约）",
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

/// DoA 判向窗配置形状（B批 b3b，config `[rr3-sense.doa]`）：TDOA 机制面所需
/// 的全部策略量一次注入（机制件零策略载荷，ADR-0019 裁定 5 L3）。
/// - `sample_rate_hz`/`frame_ms`：能量门按采集帧评的定标参数（与 audio 段同源）；
/// - `mic_baseline_mm`：双麦间距（物理 lag 上限与 sinθ 反演的几何量）；
/// - `window_rms_min`：窗能量门（任一采集帧 rms ≥ 此值才算有声窗，0 = 门全开）；
/// - `bin_threshold_deg`：三档分档边界（S-5 策略数据化延续）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3DoaCfg {
    pub sample_rate_hz: u32,
    pub frame_ms: u64,
    pub mic_baseline_mm: u32,
    pub window_rms_min: i64,
    pub bin_threshold_deg: i32,
}

pub(crate) use mech::rr3_pcm_best_channel;
/// `Rr3DoaBin` 仅被 composite 测试按名构造（bin 目标面无读者，pub use 在 bin 编译
/// 单元会吃 unused_imports）——测试面专用导出。
#[cfg(test)]
pub use mech::Rr3DoaBin;
/// 公开面（0.0.5 批I S-6 收口；深挖收口再修）：节拍换算/判向与判向结果是件契约面；
/// `rr3_pcm_event_body`/`rr3_doa_field` 已上移组合件（S-7），stream 级下混亦随
/// 深挖收口上移组合件（`Rr3PcmStream` 是承载层数据类型，C7 白名单外）——
/// `rr3_pcm_downmix_mono` pub(crate)（唯一读者是组合件承载层）；DoA 全部策略量
/// （能量门/基线/档界）随 B批 b3b 收进 [`Rr3DoaCfg`] 整体注入（件内无一常量）。
pub use mech::{rr3_doa_fix, rr3_frames_per_tick, Rr3DoaFix};
