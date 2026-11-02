//! ESP32 端口实现（薄壳）——契约本体在 [`rr_periph_proto::ports`]（P0-A A1 迁入），
//! 本文件只留"把 esp-idf / 板载驱动包成端口"的实现侧，不含任何策略量。
//!
//! 边界纪律（与 rr-hal 的宿主接缝同款）：端口只露事务级动作，重试/自愈/降亮阶梯
//! 全在上层机制件；I2C 总线自愈是平台恢复机制（操作引脚而非事务），不进写端口。

use esp_idf_hal::i2c::I2cDriver;
use rr_periph_proto::ports::{Rr3AdcPort, Rr3ClockPort, Rr3I2cPort};

use crate::board::{I2C_TIMEOUT, RR3_BIND_ADC_RAIL};
use crate::power::RailMonitor;

/// esp-idf I2C 驱动的新 type 适配器：trait 在 `rr-periph-proto`、`I2cDriver` 在
/// `esp-idf-hal`——本 crate 对两者皆外来，孤儿规则禁止直接 impl（E0117）。故用
/// **借用型新 type**做一层纯转发，不含任何策略；调用点写 `&mut rr3_esp_i2c(&mut i2c)`
/// （临时值在语句内有效，不引入生命周期噪音）。
pub(crate) struct Rr3EspI2c<'a, 'd>(pub(crate) &'a mut I2cDriver<'d>);

/// 适配器构造单点（模块外不直接写结构体字面量，便于后续换实现只改一处）。
pub(crate) fn rr3_esp_i2c<'a, 'd>(drv: &'a mut I2cDriver<'d>) -> Rr3EspI2c<'a, 'd> {
    Rr3EspI2c(drv)
}

impl<'d> Rr3I2cPort for Rr3EspI2c<'_, 'd> {
    fn rr3_i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), String> {
        self.0
            .write(addr, bytes, I2C_TIMEOUT)
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
