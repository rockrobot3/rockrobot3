//! led 原子件入口（五件套②，0.0.5 批C② 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `act.led`（atom.toml）。SURFACE 是启动自述（组合件 rr3_act_run 开头
//! 逐字打印，契约面零改动红线）；效果枚举 `Rr3LedEffect` 是请求体的取值域，
//! 本形态接受的**子集**由 config `[rr3-act.led] effects` 注入（0.0.5 批M MC-10
//! A-9：值本身是契约字面量不许改、集合可改——某形态不开 blink 就删数据不改码；
//! 表情到灯效的映射属 rr3-interact）。灯面经 rr-hal trait（`Rr3GpioOut`）由
//! 组合件注入，原子不直接碰 /sys。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/led.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &["led: rpc rr3-act/led/effect (request，白名单 on/off/blink)"];

/// 指令面主题（module.toml [rpc].exposes 同源；原子层不重述 rpc 契约）
pub const RR3_LED_EFFECT_TOPIC: &str = "rr3-act/led/effect";

/// 灯效**契约值集**（值本身是契约字面量，不许改；0.0.5 批M MC-10 A-9）：
/// 可注入的是"本形态接受哪个子集"（config `[rr3-act.led] effects`），本常量只
/// 供 boot 校验与错误文案点名在册值；运行期白名单是注入子集。扩值须同批改此表
/// 与 [`Rr3LedEffect::rr3_led_effect_parse`]（用例钉死两处一致）。
pub const RR3_LED_EFFECT_VALUES: [&str; 3] = ["on", "off", "blink"];

/// 灯效枚举（请求体 `{"effect": ...}` 的取值域；本形态接受的子集由 config 注入）
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

/// 灯效集合的展示串（`on/off/blink`；错误文案与日志共用的唯一渲染点）。
/// 空集显示为 `(空)`——"某形态一条灯效都不开"是合法数据选择，不是错。
pub fn rr3_led_effects_join(effects: &[Rr3LedEffect]) -> String {
    if effects.is_empty() {
        return "(空)".to_string();
    }
    effects
        .iter()
        .map(Rr3LedEffect::rr3_led_effect_as_str)
        .collect::<Vec<_>>()
        .join("/")
}

pub use mech::{rr3_led_tick, Rr3Led};

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约值集常量与 parse/as_str 表两处一致（A-9：值不许改，扩值须同批两处）
    #[test]
    fn effect_value_set_pins_parse_table() {
        for s in RR3_LED_EFFECT_VALUES {
            let e = Rr3LedEffect::rr3_led_effect_parse(s)
                .unwrap_or_else(|| panic!("在册值必须可解析: {s}"));
            assert_eq!(e.rr3_led_effect_as_str(), s, "值↔枚举↔串三相一致");
        }
        let listed =
            [Rr3LedEffect::On, Rr3LedEffect::Off, Rr3LedEffect::Blink].map(|e| e.rr3_led_effect_as_str());
        assert_eq!(listed, RR3_LED_EFFECT_VALUES, "枚举全集与值集常量漂移");
    }

    /// 集合展示串：常规集 / 单元素 / 空集（空集是合法数据选择，非错）
    #[test]
    fn effects_join_renders_set_and_empty() {
        assert_eq!(
            rr3_led_effects_join(&[Rr3LedEffect::On, Rr3LedEffect::Off, Rr3LedEffect::Blink]),
            "on/off/blink"
        );
        assert_eq!(rr3_led_effects_join(&[Rr3LedEffect::On]), "on");
        assert_eq!(rr3_led_effects_join(&[]), "(空)");
    }
}
