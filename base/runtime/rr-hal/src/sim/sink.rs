//! sim 音频播放面（虚拟扬声器）与尾窗裁剪。

use std::sync::Mutex;

use crate::{Rr3AudioCfg, Rr3AudioSink, Rr3ErrText, Rr3HalError};

/// sim 音频播放面（虚拟扬声器）：write 进来的 PCM 落进有界环形尾窗，
/// 测试与 e2e 用 [`Rr3SimAudioSink::rr3_sim_sink_played`] 回读。
pub struct Rr3SimAudioSink {
    open: Mutex<Option<Rr3AudioCfg>>,
    played: Mutex<Vec<i16>>,
    /// 尾窗上限（采样点数）：e2e 长跑不吃内存
    cap: usize,
}

impl Rr3SimAudioSink {
    pub fn rr3_sim_sink_new(cap_samples: usize) -> Self {
        Self {
            open: Mutex::new(None),
            played: Mutex::new(Vec::new()),
            cap: cap_samples,
        }
    }

    /// 回读已"播放"的采样（尾窗内）
    pub fn rr3_sim_sink_played(&self) -> Vec<i16> {
        self.played
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn rr3_sim_sink_count(&self) -> usize {
        self.played.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

impl Rr3AudioSink for Rr3SimAudioSink {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        if ![
            cfg.sample_rate_hz != 0,
            cfg.channels != 0,
            cfg.bit_depth == 16,
        ]
        .iter()
        .all(|ok| *ok)
        {
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                &format!("sim 扬声器要求 16bit 采样且 rate/channels 非零，收到 {cfg:?}"),
            )));
        }
        *self.open.lock().unwrap_or_else(|p| p.into_inner()) = Some(cfg);
        Ok(())
    }

    fn write(&mut self, samples: &[i16]) -> Result<(), Rr3HalError> {
        let open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        if open.is_none() {
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                "sim 扬声器 write 前必须 open",
            )));
        }
        let mut played = self.played.lock().unwrap_or_else(|p| p.into_inner());
        played.extend_from_slice(samples);
        rr3_sim_trim(&mut played, self.cap);
        Ok(())
    }
}

/// 尾窗上限：超出部分从最旧端丢弃（e2e 长跑不吃内存）。
fn rr3_sim_trim(played: &mut Vec<i16>, cap: usize) {
    let over = played.len().saturating_sub(cap);
    if over > 0 {
        played.drain(..over);
    }
}
