//! 电源面选择：INA219 电池读数与轨压/域压读数。

use crate::{Rr3Power, Rr3Rail, Rr3UnavailableSource};

#[cfg(target_os = "linux")]
use super::selector::rr3_bindings_eff_log;
use super::*;

/// 电源面分流校准参数：两键齐备才给 `Some`；只给一半时点名（电流面保持
/// 不可信——不拿半个配置去校准，"没校过"与"没在充"不许混）。
#[cfg(target_os = "linux")]
fn rr3_ina_cal(b: &crate::descriptor::Rr3HalBindings) -> Option<crate::linux_power::Rr3InaCal> {
    match (b.ina219_shunt_mohm, b.ina219_ilsb_ua) {
        (Some(r_mohm), Some(i_lsb_ua)) => Some(crate::linux_power::Rr3InaCal { r_mohm, i_lsb_ua }),
        (None, None) => None,
        (shunt, ilsb) => {
            eprintln!(
                "rr-hal: INA219 分流校准参数只给了一半（shunt_mohm={shunt:?}, ilsb_ua={ilsb:?}）——电流面不校准，charging 恒 false"
            );
            None
        }
    }
}

/// 按环境选电源读数面（与 [`rr3_audio_source`] 同一套规则）。linux 走
/// INA219 I2C 后端（[`crate::linux_power`]，R18），总线/地址来自生效绑定
/// （描述符 `i2c_bus`/`ina219_addr`，`RR3_HAL_*` env 应急覆盖）；键缺失或合成
/// 失败 → 占位件使用时以 `Unsupported` 回错（fail-closed）——rr3-power 开机
/// 探活会按"能力缺失显式回错"退出，不静默换 sim。
pub fn rr3_hal_power() -> (Box<dyn Rr3Power>, &'static str) {
    match rr3_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return rr3_bindings_eff_log("电源读数")
                .and_then(|b| {
                    let cal = rr3_ina_cal(&b);
                    b.i2c_bus.zip(b.ina219_addr).map(|(bus, addr)| {
                        (
                            Box::new(crate::linux_power::Rr3LinuxIna219::rr3_linux_power_new(
                                std::path::Path::new(crate::linux_power::RR3_I2C_DEV_ROOT_DEFAULT),
                                bus,
                                addr,
                                cal,
                            )) as Box<dyn Rr3Power>,
                            RR3_BACKEND_LINUX,
                        )
                    })
                })
                .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER));
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_SIM => {
            let percent = std::env::var(RR3_HAL_SIM_BATTERY_PERCENT_ENV)
                .ok()
                .and_then(|v| rr3_sim_batt_parse(&v))
                .unwrap_or(80);
            let charging = std::env::var(RR3_HAL_SIM_BATTERY_CHARGING_ENV)
                .ok()
                .and_then(|v| rr3_sim_chg_parse(&v))
                .unwrap_or(false);
            let dev = crate::sim::Rr3SimBattery::rr3_sim_battery_new(percent, charging);
            (Box::new(dev), RR3_BACKEND_SIM)
        }
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// 按环境选轨压/域压读数面（批N2 件五，P-9 归属搬家——机制不动、归属搬家：
/// 轨压采样与双阈值判定核的机制本体已在板 crate 真跑（`tools/esp32-periph-base`
/// 的 `RailMonitor`/`RailJudge`），宿主侧今日只立选择入口）。今日全后端落占位
/// （使用时以 `Unsupported` 回错，fail-closed）：宿主真件（INA219 电压/电流面）
/// 归 0.0.7 真件批（status.md 敞口表 12 行），届时照 [`rr3_hal_power`] 模板接
/// linux 分支与生效绑定。消费方（rr3-power 件五）`rail.up = true` 时开机探活，
/// 占位回错即 `exit(1)`——能力缺失显式回错，不静默带病常驻。
pub fn rr3_hal_rail() -> (Box<dyn Rr3Rail>, &'static str) {
    (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
}
