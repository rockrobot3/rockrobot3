//! 舵机命令解析与缓速移动。

use std::time::Duration;

use periph_mech::periph_core::{rr3_servo_gate, rr3_slew_steps};
use rr_periph_proto::ports::Rr3PwmPort;
use serde_json::{json, Value};

use crate::app::Board;
use crate::args::{channel_of, parse_axis, parse_deg};
use crate::board::*;
use crate::servo::{axis_check, deg_to_duty, Axis};

impl Board {
    pub(crate) fn handle_servo(&mut self, args: &Value) -> Result<Value, String> {
        let axis = parse_axis(args.get("axis"))?;
        match args.get("angle_deg") {
            None | Some(Value::Null) => {
                let deg = self.state.servo.deg(channel_of(axis));
                Ok(json!({ "angle_deg": deg, "move_ms": 0 }))
            }
            Some(v) => {
                let deg = v
                    .as_u64()
                    .ok_or_else(|| format!("angle_deg 非法: {v}（须 0-180 整数）"))?
                    as u32;
                let d = self.servo_move(axis, deg)?;
                Ok(json!({ "angle_deg": d, "move_ms": 0 }))
            }
        }
    }

    /// 舵机移动（机制核：行程校验 + 轨压闸 + 缓速步进；本面只管写端口与等待）。
    fn servo_move(&mut self, axis: Axis, deg: u32) -> Result<u32, String> {
        let d = axis_check(axis, deg)?;
        rr3_servo_gate(RAIL_GATE_ARMED, self.state.rail_tier, axis.name(), d)?;
        let ch = channel_of(axis);
        for step in rr3_slew_steps(self.state.servo.deg(ch), d, SERVO_SLEW_STEP_DEG) {
            let ticks = deg_to_duty(step, DUTY_MIN_TICKS, DUTY_MAX_TICKS);
            self.pwm.rr3_pwm_duty(ch, ticks)?;
            if step != d {
                std::thread::sleep(Duration::from_millis(SERVO_SLEW_STEP_MS));
            }
        }
        self.state.servo.commit(ch, d);
        self.redraw_text();
        Ok(d)
    }

    /// 单轴定位（PAN/TILT 共用）：角度解析 → 缓速移动 → 回执带实际角度。
    pub(crate) fn axis_deg(
        &mut self,
        axis: Axis,
        tag: &str,
        arg: Option<&str>,
    ) -> Result<String, String> {
        parse_deg(arg).and_then(|deg| self.servo_move(axis, deg).map(|d| format!("OK {tag}={d}")))
    }

    /// SERVO 双轴臂：两角度同序解析（任一非法即拒），pan 先动 tilt 后动。
    pub(crate) fn servo_pair(
        &mut self,
        p: Result<u32, String>,
        t: Result<u32, String>,
    ) -> Result<String, String> {
        match (p, t) {
            (Ok(p), Ok(t)) => self
                .servo_move(Axis::Pan, p)
                .and_then(|p| self.servo_move(Axis::Tilt, t).map(|t| (p, t)))
                .map(|(p, t)| format!("OK servo pan={p} tilt={t}")),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }
}
