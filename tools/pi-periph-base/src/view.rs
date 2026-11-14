//! 轨压周期采样与 INFO 应答。

use rr_periph_proto::ports::Rr3PwmChannel;

use crate::app::{Board, State};
use crate::board::RAIL_SAMPLE_PERIOD_MS;
use crate::servo::Axis;

impl Board {
    pub(crate) fn maybe_sample_rail(&mut self) {
        let now = self.now_ms();
        if now < self.state.next_rail_ms {
            return;
        }
        self.state.next_rail_ms = now.saturating_add(RAIL_SAMPLE_PERIOD_MS);
        if let Err(e) = self.sample_rail() {
            if !self.rail_err_logged {
                eprintln!("[periph] rail 周期采样失败（后续静默，RAIL 请求仍回 Err）: {e}");
                self.rail_err_logged = true;
            }
        }
    }

    pub(crate) fn sample_rail(&mut self) -> Result<(u16, u16), String> {
        let (mv, raw) = self.rail.sample()?;
        let (tier, edge) = self.judge.feed(mv);
        if edge {
            self.state.rail_low_events = self.state.rail_low_events.saturating_add(1);
            eprintln!("[periph] 5V 轨越阈: {mv}mV tier={}", tier.name());
        }
        self.state.rail_mv = mv;
        self.state.rail_raw = raw;
        self.state.rail_tier = tier;
        self.state.rail_valid = true;
        self.rail_err_logged = false;
        Ok((mv, raw))
    }

    pub(crate) fn info_line(&self) -> String {
        info_of(&self.state)
    }
}

pub(crate) fn info_of(state: &State) -> String {
    let pan = state.servo.deg(Rr3PwmChannel::Pan);
    let tilt = state.servo.deg(Rr3PwmChannel::Tilt);
    let (plo, phi) = Axis::Pan.range();
    let (tlo, thi) = Axis::Tilt.range();
    let rail = if state.rail_valid {
        format!(
            "rail={}mV raw={} rail_tier={}",
            state.rail_mv,
            state.rail_raw,
            state.rail_tier.name()
        )
    } else {
        String::from("rail=absent")
    };
    format!(
        "INFO pan={pan:03} tilt={tilt:03} lim=pan[{plo},{phi}]/tilt[{tlo},{thi}] \
         wander=off phase=0 {rail} low_events={} binding_mismatch={} oled_last=\"{}\"",
        state.rail_low_events, state.binding_mismatch, state.last_text
    )
}
