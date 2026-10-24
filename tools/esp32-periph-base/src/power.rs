//! 5V 轨电压采样与欠压判定机制（方案A Q1/Q2）——纯机制：读 ADC 一次 + 按分压比换算
//! 回轨压 mV + 双阈值迟滞判定，**不含任何策略量**（阈值/引脚/分压比全在 [`crate::board`]）。
//!
//! 机制与策略分离：策略量（引脚/分压比/采样节拍/ADC 档位/标定/阈值/接闸）全在
//! [`crate::board`]，本模块只做"读一次 → 换算 → 电平分类 + 越阈边沿"的机械动作，
//! 换板只动 board.rs（本文件零板名）。

use esp_idf_hal::adc::oneshot::{config::AdcChannelConfig, AdcChannelDriver, AdcDriver};
use esp_idf_hal::adc::ADC1;
#[cfg(feature = "esp32s3")]
use esp_idf_hal::gpio::Gpio1;
#[cfg(feature = "esp32")]
use esp_idf_hal::gpio::Gpio34;
use esp_idf_sys::EspError;

use rr_mech::control::Rr3LowEdge;

use crate::board::{
    ADC_ATTEN, ADC_CALIBRATION, ADC_RAIL_DIVIDER_DEN, ADC_RAIL_DIVIDER_NUM, ADC_RESOLUTION,
    RAIL_MILD_MV, RAIL_SEVERE_MV,
};

/// 芯片档测点引脚类型别名（W1/T2）：classic = GPIO34（ADC1_CH6），S3 = GPIO1（ADC1_CH0）。
/// 具名装配仍由 [`crate::main`] 按档给 `peripherals.pins.gpioNN`，本别名只解耦
/// 类型面——`Gpio34` 在 S3 上不满足 `ADCPin`（且被 Flash/PSRAM 占用）。
#[cfg(feature = "esp32")]
pub(crate) type Rr3RailPin = Gpio34;
#[cfg(feature = "esp32s3")]
pub(crate) type Rr3RailPin = Gpio1;

/// 5V 轨电压采样器（运行面独占 ADC1 测点，classic CH6 / S3 CH0）。
///
/// 通道驱动器**拥有** AdcDriver（`AdcChannelDriver<'static, Rr3RailPin, AdcDriver<..>>`）——
/// 避开"通道借驱动"的自引用（两者都是 'static 具名外设 token，move 入线程后永不 drop）。
/// `read_raw` + `raw_to_cal` 一对调用给出**同一拍**一致的 (引脚 raw, 引脚 mV)——比
/// 分别调 `read()`/`read_raw()`（两次转换、两拍不同值）更适合对拍标定。校准由 ESP-IDF
/// line-fitting 提供（esp32 面），故 `raw_to_cal` 直接回 mV。两驱动上游均标 `Send`，
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

// ---------------------------------------------------------------------------
// 欠压判定核（方案A Q2 步3）：双阈值电平分类（钳制用）+ 迟滞边沿（事件用）
// ---------------------------------------------------------------------------

/// 5V 轨欠压档位：正常 / 轻度（串行降级）/ 重度（拒动）。**电平分类**——每拍由
/// [`tier_of`] 从轨压算出，驱动面按档钳制（见 [`crate::periph::RtPeriph::servo_set`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RailTier {
    Ok,
    Mild,
    Severe,
}

impl RailTier {
    /// 档名（`INFO rail_tier=` 与越阈打站文案同串）。
    pub(crate) fn name(self) -> &'static str {
        match self {
            RailTier::Ok => "ok",
            RailTier::Mild => "mild",
            RailTier::Severe => "severe",
        }
    }
}

/// 电平分类（**纯函数**，机制面）：轨压 mV → 档位。阈值取策略面 [`crate::board`]，
/// 本函数只做比较、不含迟滞/事件语义（越阈事件走 [`RailJudge`] 的迟滞边沿）。
pub(crate) fn tier_of(mv: u16) -> RailTier {
    if mv <= RAIL_SEVERE_MV {
        RailTier::Severe
    } else if mv <= RAIL_MILD_MV {
        RailTier::Mild
    } else {
        RailTier::Ok
    }
}

/// 欠压判定核（**机制面**，方案A Q2）：**一核两条线**——轻度/重度两条迟滞边沿各由
/// rr-mech 安全核的 [`Rr3LowEdge`] 一实例承载，**不另写第二个边沿点**（"越阈一次一段、
/// 严格回阈才重新武装"的语义钉死在安全核内，调用方只能喂电压不能改语义——裁定 5）。
///
/// 职责二分：**边沿管事件**（每低电段首拍报一次 → 打站/计数），**电平管钳制**（[`tier_of`]
/// 每拍给档 → 驱动面拒动/降级）。二者共用同一对阈值，故"向下档变"与"边沿事件"逐拍等价：
/// `Ok→Mild`（轻度线报）、`Ok→Severe` / `Mild→Severe`（重度线报）——回阈（向上档变）不报，
/// 正是"越阈"语义。入参电压由运行面采样注入（不读墙上时钟、不睡眠）。
pub(crate) struct RailJudge {
    mild: Rr3LowEdge,
    severe: Rr3LowEdge,
}

impl RailJudge {
    /// 装配：两线阈值取策略面 [`RAIL_MILD_MV`] / [`RAIL_SEVERE_MV`]（重度 < 轻度）。
    pub(crate) fn new() -> Self {
        Self {
            mild: Rr3LowEdge::rr3_low_edge_new(RAIL_MILD_MV),
            severe: Rr3LowEdge::rr3_low_edge_new(RAIL_SEVERE_MV),
        }
    }

    /// 喂一拍轨压 mV：返回 `(本拍电平档位, 是否越阈首发事件)`。事件 = 两线任一"低电段
    /// 首拍"，逐拍等价于**向下档变**（详见类型注）。
    pub(crate) fn feed(&mut self, mv: u16) -> (RailTier, bool) {
        let mild_hit = self.mild.rr3_low_edge_cross(mv);
        let severe_hit = self.severe.rr3_low_edge_cross(mv);
        (tier_of(mv), mild_hit || severe_hit)
    }
}
