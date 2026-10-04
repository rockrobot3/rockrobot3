//! LED / 灯效执行（`rr3-act/led/effect`）。表情渲染属 rr3-interact，不在此处。
//!
//! 灯效经一路 `Rr3GpioOut`：on/off 即写电平；blink 由 worker 线程按周期翻转
//! （亮/灭各占周期一半）。效果白名单：`on` / `off` / `blink`——效果清单是
//! 模块面的策略，不进 HAL trait；表情到灯效的映射属 rr3-interact。

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use rr_hal::{rr3_block_on, Rr3GpioOut};
use serde_json::Value;

pub const RR3_LED_EFFECT_TOPIC: &str = "rr3-act/led/effect";

/// 启动自述：本子模块的总线面一行说明
pub const SURFACE: &[&str] = &["led: rpc rr3-act/led/effect (request，白名单 on/off/blink)"];

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

/// led/effect 参数裁决：缺/非串/不认一律 `Err` 点名白名单
pub fn rr3_led_parse_args(args: &Value) -> Result<Rr3LedEffect, String> {
    let Some(e) = args.get("effect") else {
        return Err("缺 effect（on/off/blink）".into());
    };
    let Some(s) = e.as_str() else {
        return Err(format!("effect 必须是字符串，收到 {e}"));
    };
    Rr3LedEffect::rr3_led_effect_parse(s)
        .ok_or_else(|| format!("不认的灯效 {s:?}（白名单 on/off/blink）"))
}

/// 灯效执行体：worker 线程独占 gpio 写面，主循环经 channel 下发效果、
/// 本地记账当前效果（应答 previous 用，单线程主循环内无竞争）。
pub struct Rr3Led {
    tx: mpsc::Sender<Rr3LedEffect>,
    current: Rr3LedEffect,
}

impl Rr3Led {
    /// 起 worker：收效果命令并落电平；blink 模式下每半周期翻转一次。
    /// gpio 失败只 `eprintln` 不退出——LED 是妆面件，写失败不当致命
    /// （与总线传输性失败的处置不同层）。
    pub fn rr3_led_new(gpio: Arc<dyn Rr3GpioOut>, blink_period_ms: u64) -> Self {
        let (tx, rx) = mpsc::channel();
        let half = Duration::from_millis((blink_period_ms / 2).max(1));
        std::thread::Builder::new()
            .name("rr3-act-led".into())
            .spawn(move || rr3_led_worker(gpio, rx, half))
            .expect("起灯效 worker 线程失败");
        Self {
            tx,
            current: Rr3LedEffect::Off,
        }
    }

    /// 应用效果：命令交 worker，返回 (生效效果, 前一效果)。
    /// worker 已死（send 失败）按业务性失败折 error——进程本身没坏。
    pub fn rr3_led_apply(&mut self, e: Rr3LedEffect) -> Result<(Rr3LedEffect, Rr3LedEffect), String> {
        self.tx.send(e).map_err(|_| "灯效 worker 已退出".to_string())?;
        let previous = self.current;
        self.current = e;
        Ok((e, previous))
    }
}

/// worker 主循环：非 blink 长眠等命令；blink 每半周期醒一次翻电平。
/// 切效果即落目标电平（blink 从亮相起步），不等下一个半拍。
fn rr3_led_worker(gpio: Arc<dyn Rr3GpioOut>, rx: mpsc::Receiver<Rr3LedEffect>, half: Duration) {
    let mut mode = Rr3LedEffect::Off;
    let mut level = false;
    rr3_block_on(gpio.set(level)).unwrap_or_else(|e| eprintln!("rr3-act-led: 初始写电平失败: {e}"));
    loop {
        let wait = match mode {
            Rr3LedEffect::Blink => half,
            _ => Duration::from_secs(3600),
        };
        match rx.recv_timeout(wait) {
            Ok(new) => {
                mode = new;
                level = new != Rr3LedEffect::Off;
                rr3_block_on(gpio.set(level))
                    .unwrap_or_else(|e| eprintln!("rr3-act-led: 写电平失败: {e}"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                level = !level;
                rr3_block_on(gpio.set(level))
                    .unwrap_or_else(|e| eprintln!("rr3-act-led: 翻转写电平失败: {e}"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::Rr3SimGpioOut;
    use serde_json::json;
    use std::time::Instant;

    #[test]
    fn effect_parse_covers_whitelist_and_rejects_garbage() {
        for (s, want) in [("on", Rr3LedEffect::On), ("off", Rr3LedEffect::Off), ("blink", Rr3LedEffect::Blink)] {
            assert_eq!(Rr3LedEffect::rr3_led_effect_parse(s), Some(want));
            assert_eq!(want.rr3_led_effect_as_str(), s);
        }
        for bad in ["", "ON", "pulse", "呼吸"] {
            assert_eq!(Rr3LedEffect::rr3_led_effect_parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn parse_args_rejects_missing_non_string_and_unknown() {
        let e = rr3_led_parse_args(&json!({})).unwrap_err();
        assert!(e.contains("effect"), "缺参数要点名: {e}");
        let e = rr3_led_parse_args(&json!({"effect": 3})).unwrap_err();
        assert!(e.contains("字符串"), "类型不符要点名: {e}");
        let e = rr3_led_parse_args(&json!({"effect": "strobe"})).unwrap_err();
        assert!(e.contains("白名单"), "不认的值点名白名单: {e}");
    }

    /// apply 的记账面：current 随切换推进，previous 是切换前的值（初始 off 由
    /// 首次 apply 的 previous 断言钉住）
    #[test]
    fn apply_tracks_current_and_previous() {
        let mut led = Rr3Led::rr3_led_new(
            Arc::new(Rr3SimGpioOut::rr3_sim_gpio_new(false)),
            500,
        );
        let (applied, previous) = led.rr3_led_apply(Rr3LedEffect::On).unwrap();
        assert_eq!((applied, previous), (Rr3LedEffect::On, Rr3LedEffect::Off));
        let (applied, previous) = led.rr3_led_apply(Rr3LedEffect::Blink).unwrap();
        assert_eq!((applied, previous), (Rr3LedEffect::Blink, Rr3LedEffect::On));
    }

    /// blink 真的在翻电平：短周期下采样窗口内必须见过两种电平；
    /// 切回 off 后电平归零并稳定。观察走 Arc 的另一份克隆（worker 独占写面）。
    #[test]
    fn blink_toggles_level_and_off_stops_it() {
        let gpio: Arc<dyn Rr3GpioOut> = Arc::new(Rr3SimGpioOut::rr3_sim_gpio_new(false));
        let mut led = Rr3Led::rr3_led_new(gpio.clone(), 40);
        led.rr3_led_apply(Rr3LedEffect::Blink).unwrap();
        let started = Instant::now();
        let mut seen = (false, false);
        while started.elapsed() < Duration::from_millis(600) {
            let lv = rr3_block_on(gpio.level()).unwrap();
            if lv {
                seen.0 = true;
            } else {
                seen.1 = true;
            }
            if seen.0 && seen.1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(seen.0 && seen.1, "600ms 内 blink 必须翻出两种电平: {seen:?}");

        led.rr3_led_apply(Rr3LedEffect::Off).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert!(
            !rr3_block_on(gpio.level()).unwrap(),
            "切 off 后电平归零且稳定（半周期×3 内不再翻转）"
        );
    }

    /// on/off 落电平：worker 异步生效，断言走有界轮询（判据是"成真"，不是时延）
    #[test]
    fn on_and_off_take_effect_immediately() {
        let gpio: Arc<dyn Rr3GpioOut> = Arc::new(Rr3SimGpioOut::rr3_sim_gpio_new(false));
        let mut led = Rr3Led::rr3_led_new(gpio.clone(), 500);
        led.rr3_led_apply(Rr3LedEffect::On).unwrap();
        assert!(
            rr3_wait_until(|| rr3_block_on(gpio.level()).unwrap(), Duration::from_millis(500)),
            "on 必须落亮"
        );
        led.rr3_led_apply(Rr3LedEffect::Off).unwrap();
        assert!(
            rr3_wait_until(|| !rr3_block_on(gpio.level()).unwrap(), Duration::from_millis(500)),
            "off 必须落灭"
        );
    }

    /// 有界轮询：每 5ms 求值一次，期限内成真即过——与 e2e 侧 rr3_wait_until 同一口径
    fn rr3_wait_until<F: FnMut() -> bool>(mut pred: F, dur: Duration) -> bool {
        let started = Instant::now();
        loop {
            if pred() {
                return true;
            }
            if started.elapsed() >= dur {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
