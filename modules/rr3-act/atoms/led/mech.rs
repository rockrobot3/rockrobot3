//! led 原子件机制主体（五件套③，批I F3/A-1 重构：去线程去 mpsc → 纯状态机）。
//!
//! 状态机不持 gpio、不自持节拍：[`rr3_led_tick`]（自由函数，`now_ms` 注入）
//! 由组合件每圈驱动（节拍归组合件，A-1），返回 `Some(level)` 表示目标电平
//! 变化，组合件落写 gpio。非 blink 态显式表示在 `mode` 字段（不再用"长眠
//! 超时"兜）；`Off` 缺省留在件内（执行器安全态，属安全核同族，不许注入）。
//! 写失败折日志不退——LED 是妆面件（A-12 分类），落写失败由组合件 `eprintln`。
//!
//! 参数下钻（缺/非串文案的 Value 面）已上移承载层（0.0.5 批I A-8，ADR-0019
//! 修订 4）：白名单判定留件（`Rr3LedEffect::rr3_led_effect_parse`），件面
//! serde_json 清零。

use super::Rr3LedEffect;

/// led 状态机（mode/level/next_toggle_ms 三状态字段 + 装配期定死的周期）。
/// `current` 记账随 `rr3_led_apply` 推进（应答 previous 用，主循环单线程无竞争）。
pub struct Rr3Led {
    mode: Rr3LedEffect,
    level: bool,
    next_toggle_ms: u64,
    blink_period_ms: u64,
}

impl Rr3Led {
    /// 建状态机：Off / 灭 / 翻转点 0（执行器安全态缺省，不许注入）
    pub fn rr3_led_new(blink_period_ms: u64) -> Self {
        Self {
            mode: Rr3LedEffect::Off,
            level: false,
            next_toggle_ms: 0,
            blink_period_ms,
        }
    }

    /// 半周期推导（机制，留件）：blink 亮/灭各占周期一半，至少 1ms
    fn rr3_led_half_ms(&self) -> u64 {
        (self.blink_period_ms / 2).max(1)
    }

    /// 应用效果：立即落目标电平（blink 从**亮相**起步，不等下一半拍），
    /// 返回 (生效, 前一, 目标电平)。目标电平由组合件落写 gpio。
    pub fn rr3_led_apply(
        &mut self,
        now_ms: u64,
        e: Rr3LedEffect,
    ) -> (Rr3LedEffect, Rr3LedEffect, bool) {
        let previous = self.mode;
        self.mode = e;
        self.level = e != Rr3LedEffect::Off;
        if e == Rr3LedEffect::Blink {
            self.next_toggle_ms = now_ms + self.rr3_led_half_ms();
        }
        (e, previous, self.level)
    }
}

/// 节拍推进（A-1，组合件每圈调用）：blink 到点半周期翻转一次并按当前时刻
/// 重锚；非 blink 恒 `None`（不空转）。返回 `Some(level)` = 目标电平，组合件
/// 落写 gpio。
pub fn rr3_led_tick(now_ms: u64, led: &mut Rr3Led) -> Option<bool> {
    if led.mode != Rr3LedEffect::Blink || now_ms < led.next_toggle_ms {
        return None;
    }
    led.level = !led.level;
    led.next_toggle_ms = now_ms + led.rr3_led_half_ms();
    Some(led.level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::Rr3SimGpioOut;
    use rr_hal::Rr3GpioOut as _;

    #[test]
    fn effect_parse_covers_whitelist_and_rejects_garbage() {
        for (s, want) in [
            ("on", Rr3LedEffect::On),
            ("off", Rr3LedEffect::Off),
            ("blink", Rr3LedEffect::Blink),
        ] {
            assert_eq!(Rr3LedEffect::rr3_led_effect_parse(s), Some(want));
            assert_eq!(want.rr3_led_effect_as_str(), s);
        }
        for bad in ["", "ON", "pulse", "呼吸"] {
            assert_eq!(Rr3LedEffect::rr3_led_effect_parse(bad), None, "{bad}");
        }
    }

    /// apply 的记账面：current 随切换推进，previous 是切换前的值（初始 off 由
    /// 首次 apply 的 previous 断言钉住）；目标电平同步返回（合成时钟，零等待）
    #[test]
    fn apply_tracks_current_and_previous() {
        let mut led = Rr3Led::rr3_led_new(500);
        assert!(!led.level, "初始安全态=灭");
        let (applied, previous, level) = led.rr3_led_apply(1000, Rr3LedEffect::On);
        assert_eq!((applied, previous), (Rr3LedEffect::On, Rr3LedEffect::Off));
        assert!(level, "on 立即出目标电平（亮相不等拍）");
        let (applied, previous, level) = led.rr3_led_apply(1000, Rr3LedEffect::Blink);
        assert_eq!((applied, previous), (Rr3LedEffect::Blink, Rr3LedEffect::On));
        assert!(level, "blink 从亮相起步");
    }

    /// blink 真的在翻电平（合成时钟，A-1/R-4：真等半周期 → 注入 now_ms 逐拍
    /// 推）；切回 off 后电平归零并稳定（半周期×3 内 tick 不再翻转）。
    /// 行为序列与原真等用例逐字同构，只是时钟由注入给定。
    #[test]
    fn blink_toggles_level_and_off_stops_it() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut led = Rr3Led::rr3_led_new(40); // 半周期 20ms
        let (_, _, level) = led.rr3_led_apply(1000, Rr3LedEffect::Blink);
        gpio.set(level).unwrap();
        assert!(gpio.rr3_sim_gpio_level(), "blink 亮相即亮");

        // 逐拍推 10 个半周期：必翻出两种电平
        let mut seen_low = false;
        let mut t = 1000;
        for _ in 1..=10 {
            t += 20;
            if let Some(level) = rr3_led_tick(t, &mut led) {
                gpio.set(level).unwrap();
            }
            if !gpio.rr3_sim_gpio_level() {
                seen_low = true;
            }
        }
        assert!(seen_low, "10 个半周期内 blink 必须翻出灭态");

        // 半周期中点不翻（早于到点 tick 恒 None）
        t += 10;
        assert!(rr3_led_tick(t, &mut led).is_none(), "未到半周期不翻");

        let (_, _, level) = led.rr3_led_apply(t, Rr3LedEffect::Off);
        gpio.set(level).unwrap();
        assert!(!gpio.rr3_sim_gpio_level(), "切 off 立即归零");
        for _ in 0..3 {
            t += 20;
            assert!(rr3_led_tick(t, &mut led).is_none(), "off 后 tick 不翻");
            assert!(!gpio.rr3_sim_gpio_level(), "切 off 后电平稳定");
        }
    }

    /// on/off 落电平：apply 同步返回目标电平，组合件落写后 gpio 即见——
    /// 判据是"成真"，不再是时延（原真等轮询改合成时钟同构）
    #[test]
    fn on_and_off_take_effect_immediately() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut led = Rr3Led::rr3_led_new(500);
        let (_, _, level) = led.rr3_led_apply(0, Rr3LedEffect::On);
        gpio.set(level).unwrap();
        assert!(gpio.rr3_sim_gpio_level(), "on 必须落亮");
        let (_, _, level) = led.rr3_led_apply(0, Rr3LedEffect::Off);
        gpio.set(level).unwrap();
        assert!(!gpio.rr3_sim_gpio_level(), "off 必须落灭");
    }
}
