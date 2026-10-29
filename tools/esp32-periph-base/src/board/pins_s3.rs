//! ESP32-S3 N16R8（xtensa-esp32s3-espidf）引脚装配对账锚。
//!
//! 与 classic 的关键差异：**GPIO26–37 被 16MB Flash / 8MB Octal PSRAM 占用**，
//! classic 的 26/27/32/33/34 全部不可用；另做一套杜邦线。避让区：
//! GPIO0/3/45/46（strapping）、19/20（原生 USB）、26–37（Flash/PSRAM）、
//! 43/44（UART0 控制台）。
//!
//! 真板接线（面包板档，待上板验收）：
//! - UART2 = GPIO41→TX / GPIO42←RX（921600 指令面；初版可降 460800）
//!   **2026-10-29 真板订正**：原定 17/18 在本板（KS56-S3 类）上双向不通——
//!   FT232 与 CP2102 两套适配器、TXD/RXD 两种对调、换 GND 点均零回包，而同排针的
//!   8/9（OLED）、4/5（LEDC）、6/7/15（I2S）全部正常，故排除排针/地/丝印错行；
//!   17 在板左下舵机座上同名丝印（疑经板内缓冲），不宜作 UART RX。改钉 41/42
//!   （JTAG MTDI/MTMS，J4 已引出、板上无外设占用）。
//! - I2C0  = GPIO8 SDA / GPIO9 SCL（SSD1306 0x3C）
//! - LEDC  = GPIO4 pan / GPIO5 tilt（50Hz 14bit）
//! - I2S0  = GPIO6 BCLK / GPIO15 SD / GPIO7 WS（INMP441 双麦）
//! - ADC   = GPIO1（ADC1_CH0，5V 轨测点；ADC2 与 Wi-Fi 冲突不可用）
//!
//! 常量只作装配对账与打印自证，不进消费路径。

/// I2C SDA 装配对账锚（= GPIO8，SDA 装配 `peripherals.pins.gpio8`）。
pub(crate) const RR3_BIND_I2C_SDA: u8 = 8;
/// I2C SCL 装配对账锚（= GPIO9，SCL 装配 `peripherals.pins.gpio9`）。
pub(crate) const RR3_BIND_I2C_SCL: u8 = 9;
/// 5V 轨测点装配对账锚（= GPIO1，ADC1_CH0，装配 `peripherals.pins.gpio1`）。
pub(crate) const RR3_BIND_ADC_RAIL: u8 = 1;
/// UART2 TX 装配对账锚（= GPIO41，UART2 装配 `peripherals.pins.gpio41`）。
pub(crate) const RR3_BIND_UART2_TX: u8 = 41;
/// UART2 RX 装配对账锚（= GPIO42，UART2 装配 `peripherals.pins.gpio42`）。
pub(crate) const RR3_BIND_UART2_RX: u8 = 42;
/// LEDC pan 轴装配对账锚（= GPIO4，pan 装配 `peripherals.pins.gpio4`）。
pub(crate) const RR3_BIND_LEDC_PAN: u8 = 4;
/// LEDC tilt 轴装配对账锚（= GPIO5，tilt 装配 `peripherals.pins.gpio5`）。
pub(crate) const RR3_BIND_LEDC_TILT: u8 = 5;
/// I2S BCLK 装配对账锚（= GPIO6，BCLK 装配 `peripherals.pins.gpio6`）。
pub(crate) const RR3_BIND_I2S_BCLK: u8 = 6;
/// I2S SD 装配对账锚（= GPIO15，SD 装配 `peripherals.pins.gpio15`）。
pub(crate) const RR3_BIND_I2S_SD: u8 = 15;
/// I2S WS 装配对账锚（= GPIO7，WS 装配 `peripherals.pins.gpio7`）。
pub(crate) const RR3_BIND_I2S_WS: u8 = 7;
