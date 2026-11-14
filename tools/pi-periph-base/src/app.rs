//! 板侧共享状态与装配体（五端口 + 策略量 + 运行期状态）。

use periph_mech::periph_core::Rr3ServoCore;
use periph_mech::rail_judge::{RailJudge, RailTier};
use rr_ipc::Rr3UartConn;
use rr_periph_proto::ports::Rr3ClockPort;

use crate::board::*;
use crate::face::{EyeStyle, EYE_IDLE};
use crate::ports;
use crate::servo::Axis;

/// 板侧共享状态（**不含硬件句柄**：控制面单线程串行改/读，硬件在 [`Board`]）。
pub(crate) struct State {
    pub(crate) servo: Rr3ServoCore,
    pub(crate) last_text: String,
    pub(crate) frame_mode: bool,
    pub(crate) expr_mode: bool,
    pub(crate) prev_expr: String,
    pub(crate) base: EyeStyle,
    pub(crate) rail_mv: u16,
    pub(crate) rail_raw: u16,
    pub(crate) rail_tier: RailTier,
    pub(crate) rail_valid: bool,
    pub(crate) rail_low_events: u32,
    pub(crate) binding_mismatch: u32,
    pub(crate) next_rail_ms: u64,
}

impl State {
    pub(crate) fn boot() -> Self {
        Self {
            servo: Rr3ServoCore::new(Axis::Pan.center_deg(), Axis::Tilt.center_deg()),
            last_text: String::from("(boot)"),
            frame_mode: false,
            expr_mode: false,
            prev_expr: String::from("none"),
            base: EYE_IDLE,
            rail_mv: 0,
            rail_raw: 0,
            rail_tier: RailTier::Ok,
            rail_valid: false,
            rail_low_events: 0,
            binding_mismatch: 0,
            next_rail_ms: 0,
        }
    }
}

/// 板侧装配体：五端口 + 策略量 + 共享状态。
pub(crate) struct Board {
    pub(crate) uart: Rr3UartConn,
    pub(crate) pwm: ports::PwmPort,
    pub(crate) oled: crate::oled::OledRenderer,
    pub(crate) rail: ports::RailSensor,
    pub(crate) clock: ports::MonotonicClock,
    pub(crate) judge: RailJudge,
    pub(crate) state: State,
    pub(crate) rail_err_logged: bool,
}

impl Board {
    pub(crate) fn new(uart: Rr3UartConn, i2c: ports::SharedI2c) -> Self {
        let oled = crate::oled::OledRenderer::new(i2c.clone());
        let rail = ports::RailSensor::probe(i2c, ADS1115_ADDR);
        Self {
            uart,
            pwm: ports::PwmPort::default_root(),
            oled,
            rail,
            clock: ports::MonotonicClock,
            judge: RailJudge::new(RAIL_MILD_MV, RAIL_SEVERE_MV),
            state: State::boot(),
            rail_err_logged: false,
        }
    }

    pub(crate) fn now_ms(&self) -> u64 {
        self.clock.rr3_now_ns() / 1_000_000
    }

    /// 上电面：OLED 初始化 + idle 表情；轨压首次采样（缺器件只点名不拦）。
    pub(crate) fn boot_surfaces(&mut self) {
        match self.oled.init() {
            Ok(()) => {
                if let Err(e) = self.render_expression("idle") {
                    eprintln!("[periph] OLED idle 渲染失败: {e}");
                }
            }
            Err(e) => eprintln!("[periph] OLED init 失败（面板命令将回 Err）: {e}"),
        }
        if let Err(e) = self.sample_rail() {
            eprintln!("[periph] 启动轨压采样失败（rail 请求将回 Err）: {e}");
            self.rail_err_logged = true;
        }
    }
}
