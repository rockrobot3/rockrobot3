//! 执行面选择：舵机、数字输出与显示（linux / esp32 / sim）。

use crate::{Rr3GpioOut, Rr3PwmServo, Rr3UnavailableSource};

#[cfg(target_os = "linux")]
use super::selector::rr3_bindings_eff_log;
use super::*;

/// linux sysfs PWM 舵机面：生效绑定给芯片/通道，缺键返回 `None` 交调用方落占位。
#[cfg(target_os = "linux")]
fn rr3_servo_linux() -> Option<(Box<dyn Rr3PwmServo>, &'static str)> {
    rr3_bindings_eff_log("舵机").and_then(|b| {
        Some((
            Box::new(crate::linux_pwm::Rr3LinuxPwmServo::rr3_pwm_servo_new(
                std::path::Path::new(crate::linux_pwm::RR3_PWM_SYSFS_ROOT_DEFAULT),
                b.pwm_chip?,
                b.pwm_channel?,
            )) as Box<dyn Rr3PwmServo>,
            RR3_BACKEND_LINUX,
        ))
    })
}

/// ESP32 外设桥舵机面：设备路径/轴/波特取自生效绑定与应急 env，缺路径落占位。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_servo_esp32() -> (Box<dyn Rr3PwmServo>, &'static str) {
    match rr3_periph_uart_path() {
        Some(p) => {
            let axis = rr3_periph_axis();
            let baud = rr3_periph_baud();
            (
                Box::new(crate::esp32_uart::Rr3Esp32UartServo::rr3_esp32_servo_baud(
                    std::path::PathBuf::from(p),
                    axis,
                    baud,
                )) as Box<dyn Rr3PwmServo>,
                RR3_BACKEND_ESP32,
            )
        }
        None => {
            eprintln!(
                "rr-hal: esp32 档缺 UART 路径（装配单 [hardware].periph_uart 或 env {}），舵机面落占位（使用时以 Unsupported 回错）",
                crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV
            );
            (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
        }
    }
}

/// ESP32 外设桥 UART 设备路径解析（P0-3/P0-4）：优先从生效绑定
/// （描述符 [hardware].periph_uart + 应急 env RR3_HAL_PERIPH_UART 覆盖）取，
/// 回落直接读 env；都没有 → None（调用方 fail-closed 落 placeholder）。
/// 宿主档门（linux/mac 是桥的承载侧；espidf 档无桥面，调用点全部同门）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn rr3_periph_uart_path() -> Option<String> {
    if let Some(p) = rr3_periph_binding() {
        return Some(p);
    }
    std::env::var(rr_periph_proto::consts::RR3_HAL_PERIPH_UART_ENV)
        .map(|p| p.trim().to_string())
        .ok()
        .filter(|p| !p.is_empty())
}

/// 描述符里的桥面设备路径（[hardware].periph_uart；无绑定/缺键 = None）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_periph_binding() -> Option<String> {
    crate::descriptor::rr3_bindings_eff()
        .ok()
        .and_then(|b| b.periph_uart)
}

/// 舵机轴 env 裁决（只认 pan/tilt，缺省 [`rr_hal_api::RR3_PERIPH_AXIS_DEFAULT`]）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_periph_axis() -> &'static str {
    std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_AXIS_ENV)
        .ok()
        .and_then(|v| crate::esp32_uart::rr3_axis_parse(&v))
        .unwrap_or(rr_hal_api::RR3_PERIPH_AXIS_DEFAULT)
}

/// 桥面波特 env 裁决（白名单在 rr-ipc；缺省 = 板固件同档 RR3_BRIDGE_BAUD）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_periph_baud() -> u32 {
    std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_BAUD_ENV)
        .ok()
        .and_then(|v| crate::esp32_uart::rr3_baud_parse(&v))
        .unwrap_or(crate::esp32_uart::RR3_BRIDGE_BAUD)
}

/// 按环境选舵机（与 [`rr3_audio_source`] 同一套规则）。linux 平台走 sysfs PWM
/// 后端（[`crate::linux_pwm`]，R11①/R22 定案档），芯片/通道来自生效绑定
/// （描述符 `pwm_chip`/`pwm_channel`，`RR3_HAL_PWM_*` env 应急覆盖）；键缺失或
/// 合成失败 → 占位件使用时以 `Unsupported` 回错（fail-closed，不回落参考机引脚），
/// 非 linux 平台显式要 `linux` 时同落占位件，不静默换 sim。
pub fn rr3_hal_servo() -> (Box<dyn Rr3PwmServo>, &'static str) {
    // 舵机面走能力面覆盖键（[`RR3_HAL_SERVO_BACKEND_ENV`]）：组合档舵机在 ESP32
    // 板上时设 esp32，全局 hal_backend 仍 linux（其余能力面走宿主）。
    match rr3_servo_backend().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        // linux 平台走 sysfs PWM；显式 linux 但非 linux 平台同落占位，不静默换 sim。
        RR3_BACKEND_LINUX => {
            #[cfg(target_os = "linux")]
            {
                rr3_servo_linux()
                    .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER))
            }
            #[cfg(not(target_os = "linux"))]
            {
                (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
            }
        }
        // ESP32 外设桥（批C）：设备路径来自生效绑定（描述符 [hardware].periph_uart
        // + 应急 env RR3_HAL_PERIPH_UART 覆盖）；键缺失 → 占位显式回错
        // （fail-closed，不猜设备）。
        // 宿主档门（批N8 件三）：espidf 档无桥面，本臂剔除后显式要 esp32 落
        // 下方 `_` 兜底臂占位回错（resolve 白名单单点不裁）。
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        RR3_BACKEND_ESP32 => rr3_servo_esp32(),
        RR3_BACKEND_SIM => (
            Box::new(crate::sim::Rr3SimServo::rr3_sim_servo_new(90)),
            RR3_BACKEND_SIM,
        ),
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// 按环境选一路数字输出（`role` 白名单 `led` / `relay`——各调一次、各传各的
/// 角色，各得一路独立线偏移；带参而不靠调用顺序配键是刻意的：顺序耦合在
/// 半夜改代码时是隐形地雷）。linux 档的芯片号/线偏移来自生效绑定（描述符
/// `gpio_chip` + `gpio_<role>_line`，`RR3_HAL_*` env 应急覆盖）；role 白名单外
/// 或键缺失/合成失败 → 占位件使用时以 `Unsupported` 回错（fail-closed，批D④
/// 收口——不再有"必败线偏移 ensure 时才炸"的代码缺省，缺数据在选择面就点名）。
///
/// 返回 [`Box`]：批I F3 拆掉灯效 worker 线程后无跨线程共享（blink 节拍上收
/// 组合件单线程驱动），写面 `&mut self` 单一所有者即可，Arc 形状随之退役。
pub fn rr3_hal_gpio_out(
    // 非 linux 平台 linux 分支被剔除、sim 分支不按角色分键——参数在那些平台
    // 编译面上未使用是结构性的，不是疏漏（linux 上它决定线偏移绑定键）
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] role: &str,
) -> (Box<dyn Rr3GpioOut>, &'static str) {
    match rr3_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => rr3_gpio_linux(role),
        RR3_BACKEND_SIM => (
            Box::new(crate::sim::Rr3SimGpioOut::rr3_sim_gpio_new(false)),
            RR3_BACKEND_SIM,
        ),
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// linux 档数字输出：芯片号/线偏移取自生效绑定，role 白名单外或键缺失落占位。
fn rr3_gpio_linux(role: &str) -> (Box<dyn Rr3GpioOut>, &'static str) {
    #[cfg(target_os = "linux")]
    {
        rr3_bindings_eff_log("数字输出")
            .and_then(|b| {
                Some((
                    Box::new(crate::linux_gpio::Rr3LinuxGpioOut::rr3_gpio_out_new(
                        std::path::Path::new(crate::linux_gpio::RR3_GPIO_DEV_ROOT_DEFAULT),
                        b.gpio_chip?,
                        rr3_gpio_line(&b, role)?,
                        role,
                    )) as Box<dyn Rr3GpioOut>,
                    RR3_BACKEND_LINUX,
                ))
            })
            .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = role;
        unreachable!("上面 guard 已保证只在本分支编译")
    }
}

/// role 白名单到线偏移：白名单外 = 无键无绑定（fail-closed）。
#[cfg(target_os = "linux")]
fn rr3_gpio_line(b: &crate::descriptor::Rr3HalBindings, role: &str) -> Option<u32> {
    let lines: [(&str, Option<u32>); 2] = [("led", b.gpio_led_line), ("relay", b.gpio_relay_line)];
    // 白名单外：无键无绑定，fail-closed
    lines
        .iter()
        .find(|(r, _)| *r == role)
        .and_then(|(_, line)| *line)
}

/// 按环境选显示面（与 [`rr3_audio_source`] 同一套规则）。linux 档走
/// MAX7219 spidev 后端（[`crate::linux_display`]，链④真件档实装），设备路径
/// 来自生效绑定（描述符 `spi_device`，`RR3_HAL_SPI_DEVICE` env 应急覆盖）；
/// 键缺失、合成失败或开口失败 → 占位件使用时以 `Unsupported` 回错
/// （fail-closed）——rr3-interact 开机探活会按"能力缺失显式回错"退出，不
/// 静默换 sim。mac 宿主档（C4 修正）：Mac 天然无 LED 矩阵，显示面落 sim 同
/// 款虚拟点阵（内存记录，机制同源）——宿主开发档不因显示件缺席整体判死。
pub fn rr3_hal_display() -> (Box<dyn crate::Rr3Display>, &'static str) {
    // 显示面走能力面覆盖键（[`RR3_HAL_DISPLAY_BACKEND_ENV`]）：组合档 OLED/点阵在
    // ESP32 板上时设 esp32，全局 hal_backend 仍 linux（其余能力面走宿主）。
    match rr3_display_backend().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => rr3_display_linux(),
        RR3_BACKEND_SIM => (
            Box::new(crate::sim::Rr3SimDisplay::rr3_sim_display_new([0; 8])),
            RR3_BACKEND_SIM,
        ),
        // mac 宿主档：真麦真扬声器 + 虚拟点阵（无显示硬件是平台事实，非能力缺失）
        #[cfg(target_os = "macos")]
        RR3_BACKEND_MAC => (
            Box::new(crate::sim::Rr3SimDisplay::rr3_sim_display_new([0; 8])),
            RR3_BACKEND_MAC,
        ),
        // ESP32 外设桥（批C）：OLED 出帧链路的宿主承载（帧面 rr3.call，板侧
        // 8x8→64x64 放大渲染）。设备路径来自生效绑定（描述符 [hardware].periph_uart
        // + 应急 env RR3_HAL_PERIPH_UART 覆盖）；缺 → 占位显式回错。
        // 宿主档门（批N8 件三）：同 [`rr3_hal_servo`] 的 esp32 臂。
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        RR3_BACKEND_ESP32 => rr3_display_esp32(),
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// linux 档显示：MAX7219 spidev 后端，设备路径取自生效绑定（缺键落占位回错）。
fn rr3_display_linux() -> (Box<dyn crate::Rr3Display>, &'static str) {
    #[cfg(target_os = "linux")]
    {
        rr3_bindings_eff_log("显示")
            .and_then(|b| {
                Some((
                    Box::new(crate::linux_display::Rr3Max7219SpiDisplay::rr3_max7219_new(
                        std::path::Path::new(b.spi_device.as_deref()?),
                    )) as Box<dyn crate::Rr3Display>,
                    RR3_BACKEND_LINUX,
                ))
            })
            .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER))
    }
    #[cfg(not(target_os = "linux"))]
    {
        unreachable!("上面 guard 已保证只在本分支编译")
    }
}

/// esp32 档显示：UART 帧面承载，UART 路径缺失即点名落占位（fail-closed）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_display_esp32() -> (Box<dyn crate::Rr3Display>, &'static str) {
    match rr3_periph_uart_path() {
        Some(p) => {
            let baud = rr3_periph_baud();
            (
                Box::new(crate::esp32_uart::Rr3Esp32UartDisplay::rr3_esp32_disp_baud(
                    std::path::PathBuf::from(p),
                    baud,
                )) as Box<dyn crate::Rr3Display>,
                RR3_BACKEND_ESP32,
            )
        }
        None => {
            eprintln!(
                "rr-hal: esp32 档缺 UART 路径（装配单 [hardware].periph_uart 或 env {}），显示面落占位（使用时以 Unsupported 回错）",
                crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV
            );
            (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
        }
    }
}
