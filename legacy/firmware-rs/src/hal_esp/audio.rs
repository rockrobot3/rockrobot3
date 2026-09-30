//! audio：I2S 采音/播放（占位，语音链路阶段实现）
//!
//! 迁移契约：audio_start(16000Hz)/mic_enable()/speaker 交付 PCM。
//! INMP441 + MAX98357A；Linux 侧对应 ALSA 重写。
