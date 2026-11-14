//! 采样配置：`Rr3AudioCfg` 与 legacy 16k/单声道/16bit 缺省档。

/// HAL 采样配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3AudioCfg {
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub bit_depth: u8,
}

/// 与 legacy ESP32 链路一致的默认拾音格式（16k/单声道/16bit）
pub const RR3_AUDIO_CFG_16K_MONO16: Rr3AudioCfg = Rr3AudioCfg {
    sample_rate_hz: 16000,
    channels: 1,
    bit_depth: 16,
};
