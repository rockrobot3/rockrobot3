//! 占位后端：所有能力显式回 `Unsupported`，不静默退化。

use crate::act::{Rr3Actuator, Rr3Display, Rr3FaceFrame, Rr3GpioOut, Rr3Position, Rr3PwmServo};
use crate::audio::{Rr3AudioSink, Rr3AudioSource};
use crate::cfg::Rr3AudioCfg;
use crate::err::Rr3HalError;
use crate::link::{Rr3Link, Rr3LinkState};
use crate::power::{Rr3BatteryState, Rr3Power, Rr3Rail, Rr3RailState};
use crate::profile::Rr3Store;

/// 占位后端：所有能力返回 `Unsupported`。它承载的是"能力缺失必须显式回错，不许
/// 静默退化"的设计裁定，不是一时代替后端的临时件。音频两路（source/sink）由
/// `rr-hal` 的 `backend::sim` / `backend::linux` 承担；gpio/pwm 已有 linux 后端；
/// power/link/display 仍由本占位回错，直到对应 linux 后端实装（display 真件档
/// MAX7219 SPI，R11③）。no_std 可达，rtos/bare 档直接复用。
pub struct Rr3UnavailableSource;

impl Rr3GpioOut for Rr3UnavailableSource {
    fn set(&mut self, _level: bool) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/gpio"))
    }
    fn level(&self) -> Result<bool, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/gpio"))
    }
}

impl Rr3PwmServo for Rr3UnavailableSource {
    fn set_angle(&mut self, _deg: u16) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/pwm"))
    }
    fn angle(&self) -> Result<u16, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/pwm"))
    }
}

impl Rr3AudioSource for Rr3UnavailableSource {
    fn open(&mut self, _cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
    fn read(&mut self, _buf: &mut [i16]) -> Result<usize, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
}

impl Rr3AudioSink for Rr3UnavailableSource {
    fn open(&mut self, _cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
    fn write(&mut self, _samples: &[i16]) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
}

impl Rr3Power for Rr3UnavailableSource {
    fn battery(&self) -> Result<Rr3BatteryState, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/power"))
    }
}

impl Rr3Rail for Rr3UnavailableSource {
    fn rr3_read(&self, _domain: &str) -> Result<Rr3RailState, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    }
    fn rr3_current_ma(&self, _domain: &str) -> Result<u16, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    }
    fn rr3_switch(&mut self, _domain: &str, _on: bool) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    }
}

impl Rr3Link for Rr3UnavailableSource {
    fn state(&self) -> Result<Rr3LinkState, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/link"))
    }
}

impl Rr3Display for Rr3UnavailableSource {
    fn draw(&mut self, _frame: &Rr3FaceFrame) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/display"))
    }
    fn frame(&self) -> Result<Rr3FaceFrame, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/display"))
    }
}

impl Rr3Actuator for Rr3UnavailableSource {
    fn rr3_jog(&mut self, _deg: u16, _deadline_ms: u32) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    }
    fn rr3_goto(&mut self, _deg: u16, _deadline_ms: u32) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    }
    fn rr3_position(&self) -> Result<Rr3Position, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    }
    fn rr3_hold(&mut self, _hold: bool) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    }
}

impl Rr3Store for Rr3UnavailableSource {
    fn rr3_store_read(&self, _key: &str, _buf: &mut [u8]) -> Result<usize, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/store"))
    }
    fn rr3_store_write(&mut self, _key: &str, _data: &[u8]) -> Result<(), Rr3HalError> {
        // S-14 载体性质实测前写侧恒拒（fail-closed）——rtos 档 calibrated 只许
        // 内存态，冒充持久化即违 §6 复位语义④
        Err(Rr3HalError::Unsupported("placeholder/store"))
    }
}
