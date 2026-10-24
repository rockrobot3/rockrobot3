//! 经典 ESP32（xtensa-esp32-espidf）引脚装配对账锚。
//!
//! 真板接线（2026-10 验收档）：
//! - UART2 = GPIO18→TX / GPIO19←RX（921600 指令面）
//! - I2C0  = GPIO21 SDA / GPIO22 SCL（SSD1306 0x3C）
//! - LEDC  = GPIO32 pan / GPIO33 tilt（50Hz 14bit）
//! - I2S0  = GPIO4 BCLK / GPIO27 SD / GPIO26 WS（INMP441 双麦）
//! - ADC   = GPIO34（ADC1_CH6，5V 轨测点，input-only）
//!
//! 这些常量只作**装配对账与打印自证**，不进消费路径；具名装配仍在 main.rs
//! （esp-idf-hal 要求 `peripherals.pins.gpioNN` 具体类型）。

/// I2C SDA 装配对账锚（= GPIO21，SDA 装配 `peripherals.pins.gpio21`）。
pub(crate) const RR3_BIND_I2C_SDA: u8 = 21;
/// I2C SCL 装配对账锚（= GPIO22，SCL 装配 `peripherals.pins.gpio22`）。
pub(crate) const RR3_BIND_I2C_SCL: u8 = 22;
/// 5V 轨测点装配对账锚（= GPIO34，ADC 装配 `peripherals.pins.gpio34`）。
pub(crate) const RR3_BIND_ADC_RAIL: u8 = 34;
/// UART2 TX 装配对账锚（= GPIO18，UART2 装配 `peripherals.pins.gpio18`）。
pub(crate) const RR3_BIND_UART2_TX: u8 = 18;
/// UART2 RX 装配对账锚（= GPIO19，UART2 装配 `peripherals.pins.gpio19`）。
pub(crate) const RR3_BIND_UART2_RX: u8 = 19;
/// LEDC pan 轴装配对账锚（= GPIO32，pan 装配 `peripherals.pins.gpio32`）。
pub(crate) const RR3_BIND_LEDC_PAN: u8 = 32;
/// LEDC tilt 轴装配对账锚（= GPIO33，tilt 装配 `peripherals.pins.gpio33`）。
pub(crate) const RR3_BIND_LEDC_TILT: u8 = 33;
/// I2S BCLK 装配对账锚（= GPIO4，BCLK 装配 `peripherals.pins.gpio4`）。
pub(crate) const RR3_BIND_I2S_BCLK: u8 = 4;
/// I2S SD 装配对账锚（= GPIO27，SD 装配 `peripherals.pins.gpio27`）。
pub(crate) const RR3_BIND_I2S_SD: u8 = 27;
/// I2S WS 装配对账锚（= GPIO26，WS 装配 `peripherals.pins.gpio26`）。
pub(crate) const RR3_BIND_I2S_WS: u8 = 26;
