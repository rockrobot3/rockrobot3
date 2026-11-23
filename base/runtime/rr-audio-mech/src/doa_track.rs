// SPDX-License-Identifier: Apache-2.0
//! DoA 稳定滤波：同档连续 N 次观测才出口，抑制漫反射/底噪造成的单窗跳档。
//! 纯计数机制，时间节奏与置信度门槛由调用方（策略面）掌握。

use crate::Rr3DoaBin;

/// 稳定滤波器（每判向流一份，单线程喂）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3DoaTrack {
    need: u8,
    bin: Option<Rr3DoaBin>,
    count: u8,
    fired: bool,
}

impl Rr3DoaTrack {
    /// `need` = 连续同档次数（0 按 1 处理）。
    pub const fn rr3_new(need: u8) -> Self {
        Self {
            need: if need == 0 { 1 } else { need },
            bin: None,
            count: 0,
            fired: false,
        }
    }

    pub fn rr3_reset(&mut self) {
        self.bin = None;
        self.count = 0;
        self.fired = false;
    }

    /// 喂一档：连续 `need` 次同档时**只报一次** `Some`；档位变化重新计数。
    pub fn rr3_feed(&mut self, bin: Rr3DoaBin) -> Option<Rr3DoaBin> {
        if self.bin == Some(bin) {
            self.count = self.count.saturating_add(1);
        } else {
            self.bin = Some(bin);
            self.count = 1;
            self.fired = false;
        }
        if !self.fired && self.count >= self.need {
            self.fired = true;
            return Some(bin);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_once_after_needed_consecutive_observations() {
        let mut t = Rr3DoaTrack::rr3_new(2);
        assert_eq!(t.rr3_feed(Rr3DoaBin::Left), None);
        assert_eq!(t.rr3_feed(Rr3DoaBin::Left), Some(Rr3DoaBin::Left));
        assert_eq!(t.rr3_feed(Rr3DoaBin::Left), None, "同档不重复报");
    }

    #[test]
    fn bin_change_restarts_the_count() {
        let mut t = Rr3DoaTrack::rr3_new(3);
        assert_eq!(t.rr3_feed(Rr3DoaBin::Left), None);
        assert_eq!(t.rr3_feed(Rr3DoaBin::Left), None);
        assert_eq!(t.rr3_feed(Rr3DoaBin::Right), None, "换档重新计数");
        assert_eq!(t.rr3_feed(Rr3DoaBin::Right), None);
        assert_eq!(t.rr3_feed(Rr3DoaBin::Right), Some(Rr3DoaBin::Right));
    }

    #[test]
    fn reset_clears_partial_consensus() {
        let mut t = Rr3DoaTrack::rr3_new(2);
        t.rr3_feed(Rr3DoaBin::Left);
        t.rr3_reset();
        assert_eq!(t.rr3_feed(Rr3DoaBin::Left), None, "reset 后需重新攒两次");
        assert_eq!(t.rr3_feed(Rr3DoaBin::Left), Some(Rr3DoaBin::Left));
    }

    #[test]
    fn zero_need_behaves_as_one() {
        let mut t = Rr3DoaTrack::rr3_new(0);
        assert_eq!(t.rr3_feed(Rr3DoaBin::Center), Some(Rr3DoaBin::Center));
    }
}
