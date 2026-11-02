//! 外设端口契约（ADR-0024 裁定，P0-A A1 迁入）——**平台无关的可移植接缝**：
//! 所有"碰硬件"的动作收拢到五条 trait，机制核与协议面只依赖 trait、不依赖
//! 任何平台的驱动类型。
//!
//! ## 为什么住这里
//!
//! 协议层（本 crate）与端口契约是同一层真值：换平台（ESP32 classic / S3 / 树莓派 /
//! 仿真）只需另写一份五端口实现，帧面、舵机机制、OLED 编码、轨压判定零改动。
//! 依赖方向与 `rr-periph-proto` 同一条纪律：**零 esp-idf 依赖**——宿主 `rr-hal`
//! 与 xtensa 固件双向引用本 crate，任何一侧的平台依赖都会污染另一侧。
//!
//! ## 边界纪律
//!
//! - 端口只露**事务级**动作（一笔写 / 一次占空 / 一次采样 / 一次读时钟），
//!   重试、自愈、降亮阶梯等策略全留上层机制件——端口不做策略。
//! - I2C 总线自愈（GPIO 位打）是平台恢复机制，不进写端口（它操作引脚而非事务）。
//! - I2S/麦克风本轮不抽象（登记为后续 audio port）：采集面与音频策略强耦合，
//!   先让已被真板踩过的四类外设（UART/I2C/PWM/ADC）稳定。
//!
//! ## 实现面归各平台
//!
//! 本文件只有**契约**；各平台实现（如 esp32 档的 `I2cDriver`/`RailMonitor`/
//! 板载时钟）留在各自的适配 crate，不进这里。

use rr_ipc::Rr3ByteStream;

/// UART 字节流端口：帧面的全部 io 需求就是读/写两条方法——**不复制签名**，直接
/// 继承 rr-ipc [`Rr3ByteStream`] 契约本体（`rr3_read_into` / `rr3_write_all_bytes`，
/// 即"阻塞读 / 整段写"语义；收帧 `rr3_recv_buf`、发帧 `rr3_send_buf` 是其 blanket
/// 派生，端口实现者自动获得整帧语义）。任何平台只要能把硬件读写映射到这两条方法，
/// 就能复用本固件的帧面。`Send` 是 rr-ipc 传输契约（`rr_ipc::Rr3TransportConn`）
/// 的前置，端口一并钉住。写侧另给一个语义别名 `rr3_uart_write_all`（上层读起来
/// 就是"整段写出"），实现委托字节契约。
pub trait Rr3UartPort: Rr3ByteStream + Send {
    /// 写满 `buf`（返回实际写入字节数；短写 = io 失败，由上层记账）。
    fn rr3_uart_write_all(&mut self, buf: &[u8]) -> usize {
        self.rr3_write_all_bytes(buf)
    }
}

// blanket：rr-ipc 字节契约的实现者即是 UART 端口（固件 UART 句柄、宿主桥、
// 测试假件同一口径），端口无需逐类型再写一遍转发。
impl<T: Rr3ByteStream + Send + ?Sized> Rr3UartPort for T {}

/// I2C 端口：**单笔写事务**（START-addr-bytes-STOP）。重试/总线自愈/整页重写
/// 全在上层，端口只回这一笔的成败。
pub trait Rr3I2cPort {
    fn rr3_i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), String>;
}

/// PWM 通道（端口级轴名，不绑引脚号——引脚真值在装配面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rr3PwmChannel {
    Pan,
    Tilt,
}

/// PWM 端口：设一路占空（ticks 口径由平台实现换算；0 = 释放/不激磁）。
pub trait Rr3PwmPort {
    fn rr3_pwm_duty(&mut self, channel: Rr3PwmChannel, duty: u32) -> Result<(), String>;
}

/// ADC 端口：读一次测点，回 `(轨压 mV, 引脚 raw)`。`pin` 是**装配真值对账参数**
/// （宿主注入的测点号），实现须拒收与已装配通道不符的引脚——不冒领读数。
pub trait Rr3AdcPort {
    fn rr3_adc_read_mv(&mut self, pin: u8) -> Result<(u16, u16), String>;
}

/// 单调时钟端口（帧头 timestamp 用；epoch 无关，只作诊断参考）。
pub trait Rr3ClockPort {
    fn rr3_now_ns(&self) -> u64;
}
