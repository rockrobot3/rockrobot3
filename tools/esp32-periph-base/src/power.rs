//! 5V 轨电压采样机制（方案A Q1）——纯机制：读 ADC 一次 + 按分压比换算回轨压 mV，
//! **不含任何阈值判定**（欠压档位/迟滞判定核见同批步3）。
//!
//! 机制与策略分离：策略量（引脚/分压比/采样节拍/ADC 档位/标定）全在 [`crate::board`]，
//! 本模块只做"读一次 → 换算 → 回 mV"的机械动作，换板只动 board.rs（本文件零板名）。

use esp_idf_hal::adc::oneshot::{config::AdcChannelConfig, AdcChannelDriver, AdcDriver};
use esp_idf_hal::adc::ADC1;
use esp_idf_hal::gpio::Gpio34;
use esp_idf_sys::EspError;

use crate::board::{
    ADC_ATTEN, ADC_CALIBRATION, ADC_RAIL_DIVIDER_DEN, ADC_RAIL_DIVIDER_NUM, ADC_RESOLUTION,
};

/// 5V 轨电压采样器（运行面独占 ADC1_CH6 @ GPIO34）。
///
/// 通道驱动器**拥有** AdcDriver（`AdcChannelDriver<'static, Gpio34, AdcDriver<..>>`）——
/// 避开"通道借驱动"的自引用（两者都是 'static 具名外设 token，move 入线程后永不 drop）。
/// `read_raw` + `raw_to_cal` 一对调用给出**同一拍**一致的 (引脚 raw, 引脚 mV)——比
/// 分别调 `read()`/`read_raw()`（两次转换、两拍不同值）更适合对拍标定。校准由 ESP-IDF
/// line-fitting 提供（esp32 面），故 `raw_to_cal` 直接回 mV。两驱动上游均标 `Send`，
/// 本结构自动 Send（随 [`crate::periph::RtPeriph`] 入运行面线程）。
pub(crate) struct RailMonitor {
    ch: AdcChannelDriver<'static, Gpio34, AdcDriver<'static, ADC1>>,
}

impl RailMonitor {
    /// 装配 ADC1 单元 + GPIO34 通道（衰减/分辨率/标定全取策略面）。失败即 Err——调用方
    /// 按 OLED init 纪律"打印不 panic"（缺测点/ADC 组件未启用不拖垮其余外设面）。
    pub(crate) fn new(adc: ADC1, pin: Gpio34) -> Result<Self, EspError> {
        let driver = AdcDriver::new(adc)?;
        let cfg = AdcChannelConfig {
            attenuation: ADC_ATTEN,
            resolution: ADC_RESOLUTION,
            calibration: ADC_CALIBRATION,
        };
        let ch = AdcChannelDriver::new(driver, pin, &cfg)?;
        Ok(Self { ch })
    }

    /// 采一次：读引脚 raw + 标定 mV，换算回 5V 轨 mV。返回 `(轨压 mV, 引脚 raw)`。
    /// **纯机制**——不判阈值、不决策，跑一次即返回（调用方在运行面自主节拍/点名作业里用）。
    pub(crate) fn sample(&mut self) -> Result<(u16, u16), EspError> {
        let raw = self.ch.read_raw()?;
        let pin_mv = self.ch.raw_to_cal(raw)?;
        Ok((rail_mv_from_pin(pin_mv), raw))
    }
}

/// 分压换算纯函数：轨压 mV = 引脚 mV × NUM / DEN（零板依赖，只算不碰硬件）。占位
/// 1:1 下即恒等；真板标定回填 [`ADC_RAIL_DIVIDER_NUM`]/[`ADC_RAIL_DIVIDER_DEN`] 生效。
pub(crate) fn rail_mv_from_pin(pin_mv: u16) -> u16 {
    ((pin_mv as u32) * ADC_RAIL_DIVIDER_NUM / ADC_RAIL_DIVIDER_DEN) as u16
}