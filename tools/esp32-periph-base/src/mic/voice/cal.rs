// SPDX-License-Identifier: Apache-2.0
//! voice 开机底噪标定策略：满窗中位数 → 阈值剖面 → VAD/DoA 门限同步。

use rr_audio_mech::{rr3_vad_tune, Rr3NoiseEst};

use super::*;

impl VoiceRun {
    /// 标定一拍：未满窗静默累积；满窗后整定并在下一块生效。
    pub(super) fn cal_tick(&mut self, rms: i64) {
        self.cal.rr3_feed(rms);
        if !self.cal.rr3_ready() {
            return;
        }
        let Some(med) = self.cal.rr3_median() else {
            return;
        };
        if med < CAL_MIN_MEDIAN {
            self.rr3_cal_retry(med);
            return;
        }
        let p90 = self.cal.rr3_pct(90).unwrap_or(med);
        let cfg = rr3_vad_tune(med, &VAD_TUNE, VAD_BASE);
        self.vad.rr3_vad_retune(cfg);
        self.vad_cfg = cfg;
        self.doa_cfg.window_rms_min = cfg.rms_start;
        self.calibrated = true;
        println!(
            "[mic] VAD 标定 median={med} p90={p90} → start={} tail={} silence_tail={}ms min_speech={}ms max_utter={}ms",
            cfg.rms_start, cfg.rms_tail, cfg.silence_tail_ms, cfg.min_speech_ms, MAX_UTTER_MS
        );
    }

    /// 底噪中位过低（无有效拾音）：重开标定窗，期间继续不落段并抽样点名。
    fn rr3_cal_retry(&mut self, med: i64) {
        self.cal_tries = self.cal_tries.wrapping_add(1);
        if self.cal_tries <= 3 || self.cal_tries % 10 == 0 {
            println!(
                "[mic] VAD 标定底噪过低 median={med}（第 {} 次重采样；查麦克风/供电）",
                self.cal_tries
            );
        }
        self.cal = Rr3NoiseEst::rr3_new();
    }
}
