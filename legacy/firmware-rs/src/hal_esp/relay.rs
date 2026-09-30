//! relay：继电器控制（移植自 legacy/core/relay）
//!
//! 硬件：板载继电器 REL 丝印针 → GPIO21。
//! 状态必须软件变量保存——REL 针悬空/未接继电器时回读引脚电平不可信
//! （实测 GPIO21 写 1 读回 0），与 C 版 s_state 同理。

use esp_idf_svc::hal::gpio::{Output, PinDriver};
use esp_idf_svc::sys::EspError;

pub struct Relay<'d> {
    pin: PinDriver<'d, Output>,
    state: bool,
}

impl<'d> Relay<'d> {
    pub fn new(mut pin: PinDriver<'d, Output>) -> Self {
        let _ = pin.set_low();
        Self { pin, state: false }
    }

    /// 设置继电器开/关（写引脚 + 更新软件状态）
    pub fn set(&mut self, on: bool) -> Result<(), EspError> {
        if on {
            self.pin.set_high()?;
        } else {
            self.pin.set_low()?;
        }
        self.state = on;
        Ok(())
    }

    /// 读取软件保存的状态（不回读引脚）
    pub fn get(&self) -> bool {
        self.state
    }
}
