// SPDX-License-Identifier: Apache-2.0
//! voice 判向策略：起音即时判向 + 语音态周期重估（置信度门槛 + 稳定共识 +
//! 防抖），中档回正 90。

use rr_audio_mech::rr3_doa_fix;

use crate::state::{lock_bus, now_ms, push_job, Job};

use super::*;

impl VoiceRun {
    /// 起音判向：首句不等共识（200 ms 内给出朝向）；低置信/方向不明只点名。
    pub(super) fn steer(&mut self, bus: &Arc<Mutex<Bus>>, why: &str) -> bool {
        let Some(fix) = rr3_doa_fix(&self.pre, 2, &self.doa_cfg) else {
            println!("[mic] DoA {why}：能量/几何不足，不判向");
            return false;
        };
        if fix.conf_q15 < DOA_CONF_MIN {
            println!(
                "[mic] DoA {why} bin={} deg={} conf={} < {DOA_CONF_MIN}，置信不足不转",
                fix.bin.rr3_doa_bin_as_str(),
                fix.deg,
                fix.conf_q15
            );
            return false;
        }
        self.apply_steer(bus, fix.bin, fix.deg, fix.conf_q15, why)
    }

    /// 语音态周期判向：置信度过门 → 同档连续共识 → 与上次不同档才下发。
    pub(super) fn doa_tick(&mut self, bus: &Arc<Mutex<Bus>>) {
        self.doa_blocks = self.doa_blocks.wrapping_add(1);
        if self.doa_blocks < DOA_EVAL_BLOCKS {
            return;
        }
        self.doa_blocks = 0;
        let Some(fix) = rr3_doa_fix(&self.pre, 2, &self.doa_cfg) else {
            return;
        };
        if fix.conf_q15 < DOA_CONF_MIN {
            return;
        }
        let Some(bin) = self.track.rr3_feed(fix.bin) else {
            return;
        };
        if self.last_bin == Some(bin) {
            return;
        }
        if !self.apply_steer(bus, bin, fix.deg, fix.conf_q15, "track") {
            // 防抖窗内被压住：清共识，让下一轮重新攒两次（否则该档只报一次会漏转）。
            self.track.rr3_reset();
        }
    }

    /// 档位落 pan：中档回正 90；同档或间隔不足不下发。
    fn apply_steer(
        &mut self,
        bus: &Arc<Mutex<Bus>>,
        bin: Rr3DoaBin,
        deg: i32,
        conf_q15: i32,
        why: &str,
    ) -> bool {
        if self.last_bin == Some(bin) {
            return false;
        }
        let pan = match bin {
            Rr3DoaBin::Left => PAN_LEFT_DEG,
            Rr3DoaBin::Center => PAN_CENTER_DEG,
            Rr3DoaBin::Right => PAN_RIGHT_DEG,
        };
        let now = now_ms();
        if self.last_bin.is_some() && now.saturating_sub(self.last_steer_ms) < STEER_GAP_MS {
            println!(
                "[mic] DoA {why} bin={} deg={deg} 距上次转向不足 {STEER_GAP_MS}ms，保持",
                bin.rr3_doa_bin_as_str()
            );
            return false;
        }
        self.last_bin = Some(bin);
        self.last_steer_ms = now;
        lock_bus(bus).st.wander_on = false; // 外部点名优先，自主扫摆让位
        push_job(
            bus,
            Job::Move {
                pan: Some(pan),
                tilt: None,
            },
        );
        println!(
            "[mic] DoA {why} bin={} deg={deg} conf={conf_q15} → pan={pan}",
            bin.rr3_doa_bin_as_str()
        );
        true
    }
}
