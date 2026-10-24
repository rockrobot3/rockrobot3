//! 外设端口层（ADR-0024）——本固件的**可移植接缝**：所有"碰硬件"的动作收拢到
//! 五条 trait，机制核（原子件）与协议面只依赖 trait、不依赖 esp-idf 驱动类型。
//!
//! 分工：
//! - **trait 面**（本文件上半）：平台无关的端口契约——UART 字节流 / I2C 单笔写 /
//!   PWM 占空 / ADC 毫伏读 / 单调时钟。换板（树莓派、S3、仿真）只需另写一份实现，
//!   上层帧面、舵机机制、OLED 渲染、轨压判定零改动。
//! - **ESP32 实现面**（本文件下半 + [`crate::periph`] 的 PWM 实现）：把 esp-idf-hal
//!   驱动包成端口——薄壳，只做错误文案与阻塞语义的对齐，不含策略量。
//!
//! 边界纪律（与 rr-hal 的宿主接缝同款）：
//! - 端口只露**事务级**动作（一笔写/一次占空/一次采样），重试、自愈、降亮阶梯等
//!   全部留在上层机制件（[`crate::oled`]/[`crate::periph`]）——端口不做策略。
//! - I2C 总线自愈（GPIO 位打）是**平台恢复机制**，不进写端口（它操作的是引脚而非
//!   事务）；见 [`crate::oled::i2c_bus_recover`]。
//! - I2S/麦克风本轮不抽象（登记为后续 audio port）：采集面与音频策略强耦合，先让
//!   已被真板踩过的四类外设（UART/I2C/PWM/ADC）稳定下来。

use esp_idf_hal::i2c::I2cDriver;

use rr_ipc::Rr3ByteStream;

use crate::board::{I2C_TIMEOUT, RR3_BIND_ADC_RAIL};
use crate::power::RailMonitor;

// ---------------------------------------------------------------------------
// 端口契约（平台无关）
// ---------------------------------------------------------------------------

/// UART 字节流端口：帧面的全部 io 需求就是读/写两条方法——**不复制签名**，直接
/// 继承 rr-ipc [`Rr3ByteStream`] 契约本体（`rr3_read_into` / `rr3_write_all_bytes`，
/// 即"阻塞读 / 整段写"语义；收帧 `rr3_recv_buf`、发帧 `rr3_send_buf` 是其 blanket
/// 派生，端口实现者自动获得整帧语义）。任何平台只要能把硬件读写映射到这两条方法，
/// 就能复用本固件的帧面。`Send` 是 rr-ipc 传输契约（[`rr_ipc::Rr3TransportConn`]）
/// 的前置，端口一并钉住。写侧另给一个语义别名 `rr3_uart_write_all`（上层读起来
/// 就是"整段写出"），实现委托字节契约。
pub(crate) trait Rr3UartPort: Rr3ByteStream + Send {
    /// 写满 `buf`（返回实际写入字节数；短写 = io 失败，由上层记账）。
    fn rr3_uart_write_all(&mut self, buf: &[u8]) -> usize {
        self.rr3_write_all_bytes(buf)
    }
}

// blanket：rr-ipc 字节契约的实现者即是 UART 端口（固件 [`crate::link::UartIo`]、
// 宿主桥、测试假件同一口径），端口无需逐类型再写一遍转发。
impl<T: Rr3ByteStream + Send + ?Sized> Rr3UartPort for T {}

/// I2C 端口：**单笔写事务**（START-addr-bytes-STOP）。重试/总线自愈/整页重写
/// 全在上层（[`crate::oled`]），端口只回这一笔的成败。
pub(crate) trait Rr3I2cPort {
    fn rr3_i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), String>;
}

/// PWM 通道（端口级轴名，不绑引脚号——引脚真值在装配面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rr3PwmChannel {
    Pan,
    Tilt,
}

/// PWM 端口：设一路占空（ticks 口径由平台实现换算；0 = 释放/不激磁）。
pub(crate) trait Rr3PwmPort {
    fn rr3_pwm_duty(&mut self, channel: Rr3PwmChannel, duty: u32) -> Result<(), String>;
}

/// ADC 端口：读一次测点，回 `(轨压 mV, 引脚 raw)`。`pin` 是**装配真值对账参数**
/// （宿主注入的测点号），实现须拒收与已装配通道不符的引脚——不冒领读数。
pub(crate) trait Rr3AdcPort {
    fn rr3_adc_read_mv(&mut self, pin: u8) -> Result<(u16, u16), String>;
}

/// 单调时钟端口（帧头 timestamp 用；epoch 无关，只作诊断参考）。
pub(crate) trait Rr3ClockPort {
    fn rr3_now_ns(&self) -> u64;
}

// ---------------------------------------------------------------------------
// ESP32 实现（薄壳）
// ---------------------------------------------------------------------------

/// esp-idf I2C 驱动即 I2C 端口：一笔写、超时取板侧策略常量（阻塞至完成）。
impl<'d> Rr3I2cPort for I2cDriver<'d> {
    fn rr3_i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), String> {
        self.write(addr, bytes, I2C_TIMEOUT)
            .map_err(|e| format!("{e:?}"))
    }
}

/// 5V 轨采样器即 ADC 端口：`pin` 必须等于本板装配的测点锚
/// （[`RR3_BIND_ADC_RAIL`]），否则点名拒——ADC1_CH6 是具名装配，换脚即换装配。
impl Rr3AdcPort for RailMonitor {
    fn rr3_adc_read_mv(&mut self, pin: u8) -> Result<(u16, u16), String> {
        if pin != RR3_BIND_ADC_RAIL {
            return Err(format!(
                "ADC 端口仅装配测点 GPIO{RR3_BIND_ADC_RAIL}，拒绝读 GPIO{pin}（换测点须改具名装配）"
            ));
        }
        self.sample().map_err(|e| format!("ADC 读失败: {e:?}"))
    }
}

/// 板载 esp_timer 时钟（微秒 → 纳秒）。实现委托 [`crate::link::rr3_frame_now_ns`]
/// ——同一时基单点，音频上行等未迁移面继续直取，两路不产生第二份换算。
pub(crate) struct Rr3BoardClock;

impl Rr3ClockPort for Rr3BoardClock {
    fn rr3_now_ns(&self) -> u64 {
        crate::link::rr3_frame_now_ns()
    }
}
