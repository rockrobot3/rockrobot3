//! 后端标识与选择入口（architecture.md §5、ADR-0007）。
//!
//! | 后端 | 落地 | 承载 |
//! |---|---|---|
//! | `linux` | M3（`linux.rs`，`alsa` feature + `target_os = "linux"` 双门） | Buildroot 根文件系统内的 ALSA 采集/回放 |
//! | `sim` | M3（`sim.rs`：正弦/噪声/静音三模式虚拟麦克风 + 尾窗虚拟扬声器） | `tools/sim`（bin `rr3-sim`）与 CI、无硬件开发 |
//! | `placeholder` | 恒回 `Unsupported` | 能力缺失显式回错的兜底（`Rr3UnavailableSource`） |
//! | `shm` 加速 | M5 | 实时控制面（100Hz–1kHz）零拷贝路径，配 `F_ZEROCOPY` |
//!
//! 反射环（1k–10kHz 电机 FOC/PID）不进这些 trait，留在 `rr3-act` 进程内。

use std::sync::Arc;

use crate::{Rr3AudioSink, Rr3AudioSource, Rr3GpioOut, Rr3PwmServo, Rr3UnavailableSource};

/// 后端标识（写入日志与诊断主题 `rr3-runtime/module/state`）
pub const RR3_BACKEND_LINUX: &str = "linux";
pub const RR3_BACKEND_SIM: &str = "sim";
pub const RR3_BACKEND_PLACEHOLDER: &str = "placeholder";

/// 后端选择的环境变量键（naming.md：`RR3_` 大写前缀）。
/// 取值：`sim` / `linux` / `placeholder`；未设或取值不认 → 走平台默认。
pub const RR3_HAL_BACKEND_ENV: &str = "RR3_HAL_BACKEND";

/// sim 源的波形选择键：`sine` / `noise` / `silence` / `burst`（默认 `sine`，必过 VAD 起音阈值；
/// `burst` = 1s 正弦 + 1s 静音循环，VAD 周期性发 onset/offset，多轮对话闭环用）。
pub const RR3_HAL_SIM_WAVE_ENV: &str = "RR3_HAL_SIM_WAVE";

/// linux（ALSA）后端的设备名键：如 `hw:0,0`、`plughw:1,0`；默认 `default`。
pub const RR3_HAL_ALSA_DEVICE_ENV: &str = "RR3_HAL_ALSA_DEVICE";

/// ALSA 采集/回放的默认设备名
pub const RR3_HAL_ALSA_DEVICE_DEFAULT: &str = "default";

/// 平台默认后端：linux + `alsa` feature 编进来了就是 `linux`，否则 `sim`。
pub const fn rr3_hal_backend_default() -> &'static str {
    if cfg!(all(target_os = "linux", feature = "alsa")) {
        RR3_BACKEND_LINUX
    } else {
        RR3_BACKEND_SIM
    }
}

/// 解析生效的后端名：`RR3_HAL_BACKEND` 显式取值优先（取值不认则回落平台默认并返回
/// `Err` 点名），否则平台默认。`linux` 只在编译进 alsa 后端时可用，否则回落
/// `placeholder`（使用时以 `Unsupported` 显式回错，不静默换 sim）。
pub fn rr3_hal_backend_resolve() -> Result<&'static str, String> {
    let Some(v) = std::env::var_os(RR3_HAL_BACKEND_ENV) else {
        return Ok(rr3_hal_backend_default());
    };
    let v = v.to_string_lossy().into_owned();
    match v.as_str() {
        RR3_BACKEND_SIM => Ok(RR3_BACKEND_SIM),
        RR3_BACKEND_PLACEHOLDER => Ok(RR3_BACKEND_PLACEHOLDER),
        RR3_BACKEND_LINUX => {
            if cfg!(all(target_os = "linux", feature = "alsa")) {
                Ok(RR3_BACKEND_LINUX)
            } else {
                Ok(RR3_BACKEND_PLACEHOLDER) // 使用时回 Unsupported，不静默降级
            }
        }
        other => Err(format!(
            "环境变量 {RR3_HAL_BACKEND_ENV} 取值不认: {other}（允许 sim/linux/placeholder）"
        )),
    }
}

fn rr3_hal_sim_wave() -> crate::sim::Rr3SimWave {
    std::env::var(RR3_HAL_SIM_WAVE_ENV)
        .ok()
        .and_then(|v| crate::sim::Rr3SimWave::rr3_sim_wave_parse(&v))
        .unwrap_or(crate::sim::Rr3SimWave::Sine)
}

/// 按环境选音频源。解析失败（环境变量取值不认）或显式 `placeholder` 时返回
/// 占位后端——它会在使用处以 `Unsupported` 显式回错，**不静默换后端**。
pub fn rr3_hal_audio_source() -> (Box<dyn Rr3AudioSource>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(all(target_os = "linux", feature = "alsa")) => {
            #[cfg(all(target_os = "linux", feature = "alsa"))]
            return (
                Box::new(crate::linux::Rr3AlsaAudioSource::rr3_alsa_source_default()),
                RR3_BACKEND_LINUX,
            );
            #[cfg(not(all(target_os = "linux", feature = "alsa")))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_PLACEHOLDER => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        _ => (
            Box::new(crate::sim::Rr3SimAudioSource::rr3_sim_source_new(
                rr3_hal_sim_wave(),
            )),
            RR3_BACKEND_SIM,
        ),
    }
}

/// 按环境选音频播放面（与 [`rr3_hal_audio_source`] 同一套规则）
pub fn rr3_hal_audio_sink() -> (Box<dyn Rr3AudioSink>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(all(target_os = "linux", feature = "alsa")) => {
            #[cfg(all(target_os = "linux", feature = "alsa"))]
            return (
                Box::new(crate::linux::Rr3AlsaAudioSink::rr3_alsa_sink_default()),
                RR3_BACKEND_LINUX,
            );
            #[cfg(not(all(target_os = "linux", feature = "alsa")))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_PLACEHOLDER => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        _ => (
            Box::new(crate::sim::Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10)),
            RR3_BACKEND_SIM,
        ),
    }
}

/// 按环境选舵机（与 [`rr3_hal_audio_source`] 同一套规则）。linux 尚无 PWM 后端：
/// 显式要 `linux` 时返回占位件，使用时以 `Unsupported` 回错，不静默换 sim。
pub fn rr3_hal_servo() -> (Box<dyn Rr3PwmServo>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_SIM => (
            Box::new(crate::sim::Rr3SimServo::rr3_sim_servo_new(90)),
            RR3_BACKEND_SIM,
        ),
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// 按环境选一路数字输出（LED / 继电器各调一次，各得一路独立通道）。
///
/// 返回 [`Arc`] 而不是 [`Box`]：灯效的 blink 翻转在 worker 线程里写同一通道，
/// 测试也要从另一线程观察电平——共享属这类设备的常态形状，不是偶发需求。
pub fn rr3_hal_gpio_out() -> (Arc<dyn Rr3GpioOut>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_SIM => (
            Arc::new(crate::sim::Rr3SimGpioOut::rr3_sim_gpio_new(false)),
            RR3_BACKEND_SIM,
        ),
        _ => (Arc::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rr3_block_on;

    /// 解析函数的平台默认分支：未设环境变量时回落平台默认（本机 macOS = sim）。
    /// 环境变量支无法在本测试里安全置位（并发测试读同一进程环境，std::env::set_var
    /// 是 unsafe 且有竞态），显式取值路径由模块侧 e2e 以真实进程环境覆盖。
    #[test]
    fn default_backend_is_sim_off_linux_alsa() {
        // 不动环境：只钉"解析不 panic 且返回合法档位"
        let resolved = rr3_hal_backend_resolve().expect("未设环境变量必须解析成功");
        assert!(
            [RR3_BACKEND_SIM, RR3_BACKEND_LINUX, RR3_BACKEND_PLACEHOLDER].contains(&resolved),
            "解析结果必须是合法后端名: {resolved}"
        );
        if cfg!(all(target_os = "linux", feature = "alsa")) {
            assert_eq!(resolved, RR3_BACKEND_LINUX);
        } else {
            assert_eq!(resolved, RR3_BACKEND_SIM);
        }
    }

    /// 执行面（舵机/数字输出）选择：平台默认下拿到的后端名必须合法，且对象可用
    /// （sim 上探活读回成功；占位件留给使用时的 Unsupported——选择函数本身不触设备）。
    #[test]
    fn act_device_selectors_return_legal_backend_objects() {
        let (servo, servo_backend) = rr3_hal_servo();
        assert!(
            [RR3_BACKEND_SIM, RR3_BACKEND_PLACEHOLDER].contains(&servo_backend),
            "舵机后端名合法: {servo_backend}"
        );
        let (gpio, gpio_backend) = rr3_hal_gpio_out();
        assert!(
            [RR3_BACKEND_SIM, RR3_BACKEND_PLACEHOLDER].contains(&gpio_backend),
            "数字输出后端名合法: {gpio_backend}"
        );
        // sim 档（本机 macOS 默认）下探活必须成功——执行件从断电态起步
        if servo_backend == RR3_BACKEND_SIM {
            assert_eq!(rr3_block_on(servo.angle()).unwrap(), 90, "sim 舵机初始中位");
            assert_eq!(rr3_block_on(gpio.level()).unwrap(), false, "sim 通道初始断电");
        }
    }
}
