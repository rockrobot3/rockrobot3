//! 采集器装配构造与点头手势步伐推进。

use std::time::{Duration, Instant};

use rr3mod::BusHandle;
use rr_hal_api::Rr3AudioCfg;
use rr_hal_api::Rr3AudioSource;

use crate::boot::Rr3SenseConfig;
use crate::capture;
use crate::doa_ring::Rr3DoaRing;
use crate::tts_gate::Rr3TtsGate;
use crate::vad::Rr3VadState;

use super::nod::rr3_nod_advance;
use super::pcm::Rr3FrameBuf;
use super::Rr3SenseDispatcher;

impl Rr3SenseDispatcher {
    /// 装配构造（main.rs 装配段调用）：探活句柄/命令旁路/后端名/开源形状由
    /// boot.rs 编排产出；frames_per_tick 预计算、VAD 状态机、判向窗策略收口、
    /// ring（channels≥2 才分配）构造即初始化，与原主循环语义一致。
    pub(crate) fn new(
        source: Box<dyn Rr3AudioSource>,
        cmd_tx: Option<rr_hal::backend::Rr3AudioCmdProducer>,
        backend: &'static str,
        open_cfg: Rr3AudioCfg,
        cfg: Rr3SenseConfig,
    ) -> Self {
        let frames_per_tick =
            capture::rr3_frames_per_tick(cfg.audio.sample_rate, cfg.audio.frame_ms);
        let vad = Rr3VadState::rr3_vad_new(cfg.vad, cfg.audio.frame_ms);
        let doa_cfg = capture::Rr3DoaCfg {
            sample_rate_hz: cfg.audio.sample_rate,
            frame_ms: cfg.audio.frame_ms,
            mic_baseline_mm: cfg.doa.mic_baseline_mm,
            window_rms_min: cfg.doa.window_rms_min,
            bin_threshold_deg: cfg.doa.bin_threshold_deg,
        };
        let doa_ring = if cfg.audio.channels >= 2 {
            Some(Rr3DoaRing::rr3_ring_new(
                (cfg.audio.sample_rate as u64 * cfg.doa.window_ms / 1000) as usize,
                cfg.audio.channels as usize,
            ))
        } else {
            None
        };
        Self {
            source,
            backend,
            open_cfg,
            cfg,
            frames_per_tick,
            vad,
            tick: Duration::from_millis(cfg.audio.frame_ms),
            seq: 0,
            cmd_tx,
            tts_gate: Rr3TtsGate::default(),
            pending_pan: None,
            pending_nod: None,
            doa_cfg,
            doa_ring,
            read_pool: Rr3FrameBuf::default(),
        }
    }

    /// 点头手势推进（宿主桥档专用）：蓄势段（回中位）到点下发后，按
    /// `nod_hold_ms` 再下发上仰段并清账。两段各自计数（`reflex_tilt_cmds`），
    /// 板侧应答失败不回插——手势是示意动作，丢一次可接受，重试会拉长手势。
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn rr3_nod_step(&mut self, now: Instant, bus: &BusHandle) {
        let Some(st) = self.pending_nod else {
            return;
        };
        let Some(tx) = self.cmd_tx.as_ref() else {
            self.pending_nod = None;
            return;
        };
        self.pending_nod = rr3_nod_advance(tx, bus, st, now);
    }
}
