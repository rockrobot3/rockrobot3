//! 播放期门控状态机（0.0.8 批 b2 订阅面，批D1 自 dispatch.rs 迁出）。
//!
//! 吃 brain 的 `tts_pcm` 帧流（帧体 `ms` = 块内样点时长），按"与上一帧间隔
//! 超过 gap 即新一轮"分段累计播放时长，外推门关闭时刻 = 本轮首帧 + 累计时长 + 尾窗。
//! 门开期 sense 弃 onset 发布（扬声器自声不再触发听声转头），状态机照喂不中断。
//!
//! 本文件只有判定与算术：gap/tail 两个策略量由调用方逐帧传入，时钟由调用方
//! 提供（`now` 入参），不读全局状态。

use std::time::{Duration, Instant};

/// TTS 回放的门控状态机：帧流（与上一帧间隔 > gap 判新一轮）累计 `ms`，
/// 外推线 = 本轮首帧时刻 + 累计时长 + 尾窗。状态只进不外泄，
/// [`Rr3TtsGate::rr3_tts_gate_open`] 是唯一读口。
#[derive(Debug, Default)]
pub(crate) struct Rr3TtsGate {
    /// 本轮首帧时刻（外推线的锚点）
    round_start: Option<Instant>,
    /// 上一帧到达时刻（轮分隔判据）
    last_frame_at: Option<Instant>,
    /// 本轮累计时长（帧体 ms 直加——brain 侧 ms 由样点数导出，含尾块实算）
    cum_ms: u64,
    /// 门外推线：now ≤ 外推线即门开
    deadline: Option<Instant>,
}

impl Rr3TtsGate {
    /// 喂一帧 tts_pcm：轮分隔（与上一帧间隔 > `gap_ms` 重置累计）→ 累计 → 外推。
    pub(crate) fn rr3_tts_gate_feed(&mut self, now: Instant, ms: u64, gap_ms: u64, tail_ms: u64) {
        if Self::rr3_round_break(self.last_frame_at, now, gap_ms) {
            self.round_start = Some(now);
            self.cum_ms = 0;
        }
        self.last_frame_at = Some(now);
        self.cum_ms = self.cum_ms.saturating_add(ms);
        if let Some(start) = self.round_start {
            // checked_add：Instant 端点溢出按"门长开"处理，不 panic（裸板口径）
            self.deadline = start.checked_add(Duration::from_millis(self.cum_ms + tail_ms));
        }
    }

    /// 轮分隔判据：从未收帧，或与上一帧间隔超 `gap_ms`，即新一轮。
    fn rr3_round_break(last: Option<Instant>, now: Instant, gap_ms: u64) -> bool {
        match last {
            None => true,
            Some(t) => now.duration_since(t) > Duration::from_millis(gap_ms),
        }
    }

    /// 门开着吗（now ≤ 外推线）。从未收到帧 = 关。
    pub(crate) fn rr3_tts_gate_open(&self, now: Instant) -> bool {
        match self.deadline {
            Some(d) => now <= d,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 门控三拍：帧到开门 → 尾窗外推到期关门 → 帧间隔超 gap 判新一轮重置累计。
    #[test]
    fn tts_gate_opens_expires_and_re_rounds_on_gap() {
        let t0 = Instant::now();
        let mut g = Rr3TtsGate::default();
        assert!(!g.rr3_tts_gate_open(t0), "从未收到帧 = 门关");
        g.rr3_tts_gate_feed(t0, 200, 500, 1000);
        // deadline = t0 + 200 + 1000
        assert!(g.rr3_tts_gate_open(t0));
        assert!(g.rr3_tts_gate_open(t0 + Duration::from_millis(1200)));
        assert!(!g.rr3_tts_gate_open(t0 + Duration::from_millis(1201)));
        // 间隔 600ms > gap 500ms：新一轮，累计重置——新 deadline = t0.6 + 100 + 1000。
        // t0+1.6s 在旧线（t0+1.2s）外本已关门，新线把它重新覆盖，即轮分隔生效的证据
        g.rr3_tts_gate_feed(t0 + Duration::from_millis(600), 100, 500, 1000);
        assert!(
            g.rr3_tts_gate_open(t0 + Duration::from_millis(1600)),
            "新轮外推线生效（旧线早已到期）"
        );
        assert!(g.rr3_tts_gate_open(t0 + Duration::from_millis(1700)));
        assert!(!g.rr3_tts_gate_open(t0 + Duration::from_millis(1701)));
    }

    /// 同轮多帧：ms 逐帧直加（brain 侧尾块按实算），deadline 随累计外推。
    #[test]
    fn tts_gate_accumulates_ms_within_round() {
        let t0 = Instant::now();
        let mut g = Rr3TtsGate::default();
        // 同轮三帧各 200ms、间隔 20ms（< gap 500）：deadline = t0 + 600 + 1000
        g.rr3_tts_gate_feed(t0, 200, 500, 1000);
        g.rr3_tts_gate_feed(t0 + Duration::from_millis(20), 200, 500, 1000);
        g.rr3_tts_gate_feed(t0 + Duration::from_millis(40), 200, 500, 1000);
        assert!(g.rr3_tts_gate_open(t0 + Duration::from_millis(1600)));
        assert!(!g.rr3_tts_gate_open(t0 + Duration::from_millis(1601)));
    }
}
