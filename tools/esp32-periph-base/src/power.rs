//! 5V 轨电压采样（平台适配薄壳）——判定机制已迁 `periph-mech`（P0-A A3）：
//! 分压换算、双阈值电平分类与迟滞边沿全在机制核，本文件只留 ADC 通道装配与
//! 一次采样（读 raw + 标定 mV 同拍一致）。
//!
//! 机制与策略分离：策略量（引脚/分压比/ADC 档位/阈值）全在 [`crate::board`]；
//! [`RailJudge`]/[`RailTier`] 自机制核回流，保持既有 `crate::power::*` 消费路径。

use esp_idf_hal::adc::oneshot::{config::AdcChannelConfig, AdcChannelDriver, AdcDriver};
use esp_idf_hal::adc::ADC1;
#[cfg(feature = "esp32s3")]
use esp_idf_hal::gpio::Gpio1;
#[cfg(feature = "esp32")]
use esp_idf_hal::gpio::Gpio34;
use esp_idf_sys::EspError;
use periph_mech::rail_judge::rr3_rail_mv_from_pin;

use crate::board::{
    ADC_ATTEN, ADC_CALIBRATION, ADC_RAIL_DIVIDER_DEN, ADC_RAIL_DIVIDER_NUM, ADC_RESOLUTION,
};

pub(crate) use periph_mech::rail_judge::{RailJudge, RailTier};

/// 芯片档测点引脚类型别名（W1/T2）：classic = GPIO34（ADC1_CH6），S3 = GPIO1（ADC1_CH0）。
/// 具名装配仍由 [`crate::main`] 按档给 `peripherals.pins.gpioNN`，本别名只解耦
/// 类型面——`Gpio34` 在 S3 上不满足 `ADCPin`（且被 Flash/PSRAM 占用）。
#[cfg(feature = "esp32")]
pub(crate) type Rr3RailPin = Gpio34;
#[cfg(feature = "esp32s3")]
pub(crate) type Rr3RailPin = Gpio1;

/// 5V 轨电压采样器（运行面独占 ADC1 测点）。
///
/// 通道驱动器**拥有** AdcDriver（`AdcChannelDriver<'static, Rr3RailPin, AdcDriver<..>>`）——
/// 避开"通道借驱动"的自引用（两者都是 'static 具名外设 token，move 入线程后永不 drop）。
/// `read_raw` + `raw_to_cal` 一对调用给出**同一拍**一致的 (引脚 raw, 引脚 mV)——比
/// 分别调 `read()`/`read_raw()`（两次转换、两拍不同值）更适合对拍标定。校准由
/// 平台 line-fitting 提供，故 `raw_to_cal` 直接回 mV。两驱动上游均标 `Send`，
/// 本结构自动 Send（随 [`crate::periph::RtPeriph`] 入运行面线程）。
pub(crate) struct RailMonitor {
    ch: AdcChannelDriver<'static, Rr3RailPin, AdcDriver<'static, ADC1>>,
}

impl RailMonitor {
    /// 装配 ADC1 单元 + 测点通道（衰减/分辨率/标定全取策略面）。失败即 Err——调用方
    /// 按 OLED init 纪律"打印不 panic"（缺测点/ADC 组件未启用不拖垮其余外设面）。
    pub(crate) fn new(adc: ADC1, pin: Rr3RailPin) -> Result<Self, EspError> {
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
    /// **纯采样**——不判阈值、不决策（判定在机制核 [`RailJudge`]）。
    pub(crate) fn sample(&mut self) -> Result<(u16, u16), EspError> {
        let raw = self.ch.read_raw()?;
        let pin_mv = self.ch.raw_to_cal(raw)?;
        Ok((
            rr3_rail_mv_from_pin(pin_mv, ADC_RAIL_DIVIDER_NUM, ADC_RAIL_DIVIDER_DEN),
            raw,
        ))
    }
}
