//! led 原子件入口（五件套②，0.0.5 批C② 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `act.led`（atom.toml）。SURFACE 是启动自述（组合件 rr3_act_run 开头
//! 逐字打印，契约面零改动红线）；效果白名单 `Rr3LedEffect` 是请求体的取值集
//! （效果清单是模块面的策略，不进 HAL trait；表情到灯效的映射属 rr3-interact）。
//! 灯面经 rr-hal trait（`Rr3GpioOut`）由组合件注入，原子不直接碰 /sys。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/led.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &["led: rpc rr3-act/led/effect (request，白名单 on/off/blink)"];

/// 指令面主题（module.toml [rpc].exposes 同源；原子层不重述 rpc 契约）
pub const RR3_LED_EFFECT_TOPIC: &str = "rr3-act/led/effect";

/// 灯效白名单（请求体 `{"effect": ...}` 的取值集）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3LedEffect {
    On,
    Off,
    Blink,
}

impl Rr3LedEffect {
    pub fn rr3_led_effect_parse(s: &str) -> Option<Self> {
        match s {
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            "blink" => Some(Self::Blink),
            _ => None,
        }
    }

    pub fn rr3_led_effect_as_str(&self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Blink => "blink",
        }
    }
}

pub use mech::{rr3_led_parse_args, Rr3Led};
