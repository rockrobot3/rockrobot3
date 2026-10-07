//! 后端标识与选择入口（architecture.md §5、ADR-0007）。
//!
//! | 后端 | 落地 | 承载 |
//! |---|---|---|
//! | `linux` | M3（`linux.rs`，`alsa` feature + `target_os = "linux"` 双门） | Buildroot 根文件系统内的 ALSA 采集/回放 |
//! | `linux`（舵机） | 批F5a（`linux_pwm.rs`，仅 `target_os = "linux"` 单门，纯 std） | 树莓派 sysfs PWM 驱动 SG90（R11①/R22 定案档，GPIO12/13） |
//! | `linux`（GPIO） | 批F5a（`linux_gpio.rs`，仅 `target_os = "linux"` 单门，libc 声明壳） | 树莓派 chardev GPIO 数字输出：LED / 继电器（R11②，R8 预算表 GPIO4/17 软件路行） |
//! | `sim` | M3（`sim.rs`：正弦/噪声/静音三模式虚拟麦克风 + 尾窗虚拟扬声器） | `tools/sim`（bin `rr3-sim`）与 CI、无硬件开发 |
//! | `placeholder` | 恒回 `Unsupported` | 能力缺失显式回错的兜底（`Rr3UnavailableSource`） |
//! | `shm` 加速 | M5 | 实时控制面（100Hz–1kHz）零拷贝路径，配 `F_ZEROCOPY` |
//!
//! 反射环（1k–10kHz 电机 FOC/PID）不进这些 trait，留在 `rr3-act` 进程内。

use std::sync::Arc;

use crate::{
    Rr3AudioSink, Rr3AudioSource, Rr3GpioOut, Rr3Power, Rr3PwmServo, Rr3UnavailableSource,
};

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

/// sim 源的声源方位角键（度，-90..=90 的十进制小数，负=左/正=右；未设/取值不认 → 不注入）。
/// 双麦方位模型见 [`crate::sim::Rr3SimDoa`]：批F2 sim 判向的正门注入点，
/// 多通道采集 + 本键才生效（单声道链路无方向语义，注入被设备面忽略）。
pub const RR3_HAL_SIM_DOA_DEG_ENV: &str = "RR3_HAL_SIM_DOA_DEG";

/// linux（ALSA）后端的设备名键：如 `hw:0,0`、`plughw:1,0`；默认 `default`。
pub const RR3_HAL_ALSA_DEVICE_ENV: &str = "RR3_HAL_ALSA_DEVICE";

/// ALSA 采集/回放的默认设备名
pub const RR3_HAL_ALSA_DEVICE_DEFAULT: &str = "default";

/// linux sysfs PWM 的芯片号键（十进制非负整数；未设/取值不认 → 0）。
/// 树莓派 BCM2835 硬件 PWM：pwmchip0 的 pwm0=GPIO12、pwm1=GPIO13（R22 定案档，
/// `dtoverlay=pwm-2chan` 开双通道，方案 §5.5.2 接线 1）。
pub const RR3_HAL_PWM_CHIP_ENV: &str = "RR3_HAL_PWM_CHIP";

/// linux sysfs PWM 的通道号键（十进制非负整数；未设/取值不认 → 0 = GPIO12）。
pub const RR3_HAL_PWM_CHANNEL_ENV: &str = "RR3_HAL_PWM_CHANNEL";

/// linux chardev GPIO 的芯片号键（十进制非负整数；未设/取值不认 → 0）。
/// LED 与继电器两路共挂一个芯片（`/dev/gpiochipN`），线偏移按角色分键。
pub const RR3_HAL_GPIO_CHIP_ENV: &str = "RR3_HAL_GPIO_CHIP";

/// 角色对应的线偏移环境变量键（`led` → `RR3_HAL_GPIO_LED_LINE`、
/// `relay` → `RR3_HAL_GPIO_RELAY_LINE`）。取值十进制非负整数；role 仅认
/// 非空 ASCII 字母数字（防拼出怪键），不认返回 `None`。缺省值见
/// [`rr3_gpio_line_default`]。独立成纯函数：键拼装可单测，选择函数里只留
/// "读环境 + 裁决 + 缺省"。
pub fn rr3_gpio_line_env_key(role: &str) -> Option<String> {
    if role.is_empty() || !role.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(format!("RR3_HAL_GPIO_{}_LINE", role.to_ascii_uppercase()))
}

/// 角色 → 线偏移缺省值（BCM 编号）：`led` → 4、`relay` → 17——R8 引脚预算表
/// "GPIO4/17… 软件路可用"行（与 I2S/I2C 不冲突）；接线手册未单列 LED/继电器
/// 行，真件档装配单如改脚位走 env 覆写，代码缺省不动。白名单外的 role 无
/// 缺省（`None`）：env 也没给时落必败线偏移，ensure 时显式回错，不静默选脚。
pub fn rr3_gpio_line_default(role: &str) -> Option<u32> {
    match role {
        "led" => Some(4),
        "relay" => Some(17),
        _ => None,
    }
}

/// sim 电池的初始电量键（0–100 的十进制整数；未设/取值不认 → 80）。
/// e2e 注入低电量的正门：真 rr3-power 进程从 rrd 继承环境，按此值起表。
pub const RR3_HAL_SIM_BATTERY_PERCENT_ENV: &str = "RR3_HAL_SIM_BATTERY_PERCENT";

/// sim 电池的充电态键（`1` 充电 / `0` 未充电；未设/取值不认 → 未充电）。
pub const RR3_HAL_SIM_BATTERY_CHARGING_ENV: &str = "RR3_HAL_SIM_BATTERY_CHARGING";

/// sim 链路的在线态键（`1` 在线 / `0` 断链；未设/取值不认 → 在线）。
/// e2e 注入断链场景的正门：真 rr3-connect 进程从 rrd 继承环境，按此值起表。
pub const RR3_HAL_SIM_LINK_UP_ENV: &str = "RR3_HAL_SIM_LINK_UP";

/// sim 链路的 SSID 键（任意字符串；未设 → `rock-sim`）。断链时设备面抹掉，不在此裁。
pub const RR3_HAL_SIM_LINK_SSID_ENV: &str = "RR3_HAL_SIM_LINK_SSID";

/// sim 链路的信号强度键（-100..=-1 的十进制整数 dBm；未设/取值不认 → -50）。
pub const RR3_HAL_SIM_LINK_RSSI_ENV: &str = "RR3_HAL_SIM_LINK_RSSI";

/// sim 链路 SSID 的缺省值（backend.rs 缺省语义，e2e 断言与实现引用同一处）
pub const RR3_HAL_SIM_LINK_SSID_DEFAULT: &str = "rock-sim";

/// sim 链路信号强度的缺省值（dBm）
pub const RR3_HAL_SIM_LINK_RSSI_DEFAULT: i8 = -50;

/// 平台默认后端：linux + `alsa` feature 编进来了就是 `linux`，否则 `sim`。
pub const fn rr3_hal_backend_default() -> &'static str {
    if cfg!(all(target_os = "linux", feature = "alsa")) {
        RR3_BACKEND_LINUX
    } else {
        RR3_BACKEND_SIM
    }
}

/// 解析生效的后端名：`RR3_HAL_BACKEND` 显式取值优先（取值不认则回落平台默认并返回
/// `Err` 点名），否则平台默认。`linux` 表示"linux 平台在位"（批F5a 订正：原按 alsa
/// feature 门判可用性，与舵机/GPIO 等无 alsa 依赖的 linux 后端错位——真件档板上
/// 显式 `RR3_HAL_BACKEND=linux` 会被误落 placeholder，act 探活 exit(1) 起不来）；
/// 各能力面是否编进了对应 linux 后端由各选择函数自判（音频按 `alsa` feature，
/// 舵机按 sysfs PWM 模块编译门），选了 `linux` 但该能力后端没编进 → 选择函数落
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
            if cfg!(target_os = "linux") {
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

/// sim 方位角键的取值裁决（-90..=90 的有限十进制小数）。独立成纯函数：裁决可单测，
/// 选择函数里只留"读环境 + 裁决 + 缺省"。
pub fn rr3_sim_doa_deg_parse(s: &str) -> Option<f64> {
    s.trim()
        .parse::<f64>()
        .ok()
        .filter(|d| d.is_finite() && (-90.0..=90.0).contains(d))
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
        // 显式 linux 但 alsa 后端没编进（无 alsa 的板上构建）：占位显式回错，
        // 不静默落 sim（resolve 订正为平台门后此分支结构上可达）
        RR3_BACKEND_LINUX => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        RR3_BACKEND_PLACEHOLDER => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        _ => {
            let doa = std::env::var(RR3_HAL_SIM_DOA_DEG_ENV)
                .ok()
                .and_then(|v| rr3_sim_doa_deg_parse(&v))
                .and_then(crate::sim::Rr3SimDoa::rr3_sim_doa_new);
            (
                Box::new(crate::sim::Rr3SimAudioSource::rr3_sim_source_new_with_doa(
                    rr3_hal_sim_wave(),
                    doa,
                )),
                RR3_BACKEND_SIM,
            )
        }
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
        // 同 audio_source：显式 linux 但 alsa 没编进 → 占位显式回错，不静默落 sim
        RR3_BACKEND_LINUX => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        RR3_BACKEND_PLACEHOLDER => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        _ => (
            Box::new(crate::sim::Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10)),
            RR3_BACKEND_SIM,
        ),
    }
}

/// 按环境选舵机（与 [`rr3_hal_audio_source`] 同一套规则）。linux 平台走 sysfs PWM
/// 后端（[`crate::linux_pwm`]，R11①/R22 定案档：pwmchip0/pwm0 = GPIO12 硬件 PWM，
/// 芯片/通道可经 `RR3_HAL_PWM_CHIP` / `RR3_HAL_PWM_CHANNEL` 选）；非 linux 平台
/// 显式要 `linux` 时返回占位件，使用时以 `Unsupported` 回错，不静默换 sim。
pub fn rr3_hal_servo() -> (Box<dyn Rr3PwmServo>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return (
                Box::new(crate::linux_pwm::Rr3LinuxPwmServo::rr3_linux_pwm_servo_default()),
                RR3_BACKEND_LINUX,
            );
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_LINUX => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        RR3_BACKEND_SIM => (
            Box::new(crate::sim::Rr3SimServo::rr3_sim_servo_new(90)),
            RR3_BACKEND_SIM,
        ),
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// 按环境选一路数字输出（`role` 白名单 `led` / `relay`——各调一次、各传各的
/// 角色，各得一路独立线偏移；带参而不靠调用顺序配键是刻意的：顺序耦合在
/// 半夜改代码时是隐形地雷）。
///
/// 返回 [`Arc`] 而不是 [`Box`]：灯效的 blink 翻转在 worker 线程里写同一通道，
/// 测试也要从另一线程观察电平——共享属这类设备的常态形状，不是偶发需求。
pub fn rr3_hal_gpio_out(
    // 非 linux 平台 linux 分支被剔除、sim 分支不按角色分键——参数在那些平台
    // 编译面上未使用是结构性的，不是疏漏（linux 上它决定线偏移 env 键）
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] role: &str,
) -> (Arc<dyn Rr3GpioOut>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return (
                Arc::new(crate::linux_gpio::Rr3LinuxGpioOut::rr3_linux_gpio_out_default(role)),
                RR3_BACKEND_LINUX,
            );
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_SIM => (
            Arc::new(crate::sim::Rr3SimGpioOut::rr3_sim_gpio_new(false)),
            RR3_BACKEND_SIM,
        ),
        _ => (Arc::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// sim 电量键的取值裁决（0–100 十进制整数）。独立成纯函数：裁决可单测，
/// 选择函数里只留"读环境 + 裁决 + 缺省"。
pub fn rr3_sim_battery_percent_parse(s: &str) -> Option<u8> {
    s.trim().parse::<u8>().ok().filter(|p| *p <= 100)
}

/// sim 充电态键的取值裁决（只认 `1` / `0`）。
pub fn rr3_sim_battery_charging_parse(s: &str) -> Option<bool> {
    match s.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// sim 链路在线态键的取值裁决（只认 `1` / `0`）。
pub fn rr3_sim_link_up_parse(s: &str) -> Option<bool> {
    match s.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// sim 链路信号强度键的取值裁决（-100..=-1 的十进制整数 dBm；0 与正值、
/// 越界、垃圾一律不认，落缺省 -50）。
pub fn rr3_sim_link_rssi_parse(s: &str) -> Option<i8> {
    s.trim()
        .parse::<i8>()
        .ok()
        .filter(|r| (-100..=-1).contains(r))
}

/// linux PWM 芯片/通道号键的取值裁决（十进制非负整数，trim 后解析）。
pub fn rr3_pwm_index_parse(s: &str) -> Option<u32> {
    s.trim().parse::<u32>().ok()
}

/// 按环境选电源读数面（与 [`rr3_hal_audio_source`] 同一套规则）。linux 尚无
/// 电量计后端（板上件）：显式要 `linux` 时返回占位件，使用时以 `Unsupported`
/// 回错，不静默换 sim——rr3-power 开机探活会按"能力缺失显式回错"退出。
pub fn rr3_hal_power() -> (Box<dyn Rr3Power>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_SIM => {
            let percent = std::env::var(RR3_HAL_SIM_BATTERY_PERCENT_ENV)
                .ok()
                .and_then(|v| rr3_sim_battery_percent_parse(&v))
                .unwrap_or(80);
            let charging = std::env::var(RR3_HAL_SIM_BATTERY_CHARGING_ENV)
                .ok()
                .and_then(|v| rr3_sim_battery_charging_parse(&v))
                .unwrap_or(false);
            let dev = crate::sim::Rr3SimBattery::rr3_sim_battery_new(percent, charging);
            (Box::new(dev), RR3_BACKEND_SIM)
        }
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// 按环境选网络链路读数面（与 [`rr3_hal_audio_source`] 同一套规则）。linux 尚无
/// NetworkManager 后端（板上件，0.0.2 方案 D7 cfg 门骨架归属）：显式要 `linux`
/// 时返回占位件，使用时以 `Unsupported` 回错，不静默换 sim——rr3-connect 开机
/// 探活会按"能力缺失显式回错"退出。
pub fn rr3_hal_link() -> (Box<dyn crate::Rr3Link>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_SIM => {
            let up = std::env::var(RR3_HAL_SIM_LINK_UP_ENV)
                .ok()
                .and_then(|v| rr3_sim_link_up_parse(&v))
                .unwrap_or(true);
            let ssid = std::env::var(RR3_HAL_SIM_LINK_SSID_ENV)
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| RR3_HAL_SIM_LINK_SSID_DEFAULT.to_string());
            let rssi = std::env::var(RR3_HAL_SIM_LINK_RSSI_ENV)
                .ok()
                .and_then(|v| rr3_sim_link_rssi_parse(&v))
                .unwrap_or(RR3_HAL_SIM_LINK_RSSI_DEFAULT);
            let dev = crate::sim::Rr3SimLink::rr3_sim_link_new(up, &ssid, rssi);
            (Box::new(dev), RR3_BACKEND_SIM)
        }
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
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
        // linux 平台默认档：无 alsa feature 构建落 sim，带 alsa 构建落 linux
        // （舵机面在 linux 平台也可得 sysfs PWM 后端——构造不触设备，此处不探活）
        let mut legal = vec![RR3_BACKEND_SIM, RR3_BACKEND_PLACEHOLDER];
        if cfg!(target_os = "linux") {
            legal.push(RR3_BACKEND_LINUX);
        }
        let (servo, servo_backend) = rr3_hal_servo();
        assert!(
            legal.contains(&servo_backend),
            "舵机后端名合法: {servo_backend}"
        );
        let (gpio, gpio_backend) = rr3_hal_gpio_out("led");
        assert!(
            legal.contains(&gpio_backend),
            "数字输出后端名合法: {gpio_backend}"
        );
        // sim 档（本机 macOS 默认）下探活必须成功——执行件从断电态起步
        if servo_backend == RR3_BACKEND_SIM {
            assert_eq!(rr3_block_on(servo.angle()).unwrap(), 90, "sim 舵机初始中位");
            assert!(!rr3_block_on(gpio.level()).unwrap(), "sim 通道初始断电");
        }
    }

    /// GPIO 角色 → env 键拼装与缺省线偏移（R8 预算表 GPIO4/17 软件路行）：
    /// 白名单两角色键/缺省逐字钉死，空串/怪字符不拼键，白名单外无缺省。
    #[test]
    fn gpio_role_env_key_and_line_default_match_whitelist() {
        assert_eq!(
            rr3_gpio_line_env_key("led").as_deref(),
            Some("RR3_HAL_GPIO_LED_LINE")
        );
        assert_eq!(
            rr3_gpio_line_env_key("relay").as_deref(),
            Some("RR3_HAL_GPIO_RELAY_LINE")
        );
        assert_eq!(rr3_gpio_line_env_key(""), None, "空串不拼键");
        assert_eq!(rr3_gpio_line_env_key("led-1"), None, "怪字符不拼键");
        assert_eq!(rr3_gpio_line_default("led"), Some(4));
        assert_eq!(rr3_gpio_line_default("relay"), Some(17));
        assert_eq!(rr3_gpio_line_default("fan"), None, "白名单外无缺省，必败线偏移显式回错");
    }

    /// linux PWM 芯片/通道号键的取值裁决：十进制非负整数才收，负数/垃圾/空不认
    /// （落缺省 pwmchip0/pwm0 = GPIO12）。
    #[test]
    fn pwm_index_env_values_parse_or_default() {
        assert_eq!(rr3_pwm_index_parse("0"), Some(0));
        assert_eq!(rr3_pwm_index_parse("1"), Some(1));
        assert_eq!(rr3_pwm_index_parse(" 2 "), Some(2));
        for bad in ["", "-1", "3.5", "abc"] {
            assert_eq!(rr3_pwm_index_parse(bad), None, "{bad}");
        }
    }

    /// sim 电池两个 env 键的取值裁决：合法档、越界/垃圾全点名。e2e 从 rrd 继承
    /// 环境注入低电量的正门靠这两个裁决，取值不认一律落缺省（80% / 未充电）。
    #[test]
    fn sim_battery_env_values_parse_or_default() {
        assert_eq!(rr3_sim_battery_percent_parse("0"), Some(0));
        assert_eq!(rr3_sim_battery_percent_parse("100"), Some(100));
        assert_eq!(rr3_sim_battery_percent_parse(" 5 "), Some(5));
        assert_eq!(rr3_sim_battery_percent_parse("101"), None, "越界不认");
        assert_eq!(rr3_sim_battery_percent_parse("-1"), None, "u8 负数不认");
        assert_eq!(rr3_sim_battery_percent_parse("abc"), None, "垃圾不认");
        assert_eq!(rr3_sim_battery_charging_parse("1"), Some(true));
        assert_eq!(rr3_sim_battery_charging_parse("0"), Some(false));
        for bad in ["", "true", "2", "01x"] {
            assert_eq!(rr3_sim_battery_charging_parse(bad), None, "{bad}");
        }
    }

    /// sim 链路两个 env 键的取值裁决：在线态只认 1/0，rssi 只认 -100..=-1；
    /// 0 与正值、越界、垃圾全点名。e2e 注入断链场景（RR3_HAL_SIM_LINK_UP=0）
    /// 的正门靠这两个裁决，取值不认一律落缺省（在线 / -50）。
    #[test]
    fn sim_link_env_values_parse_or_default() {
        assert_eq!(rr3_sim_link_up_parse("1"), Some(true));
        assert_eq!(rr3_sim_link_up_parse(" 0 "), Some(false));
        for bad in ["", "true", "2", "on"] {
            assert_eq!(rr3_sim_link_up_parse(bad), None, "{bad}");
        }
        assert_eq!(rr3_sim_link_rssi_parse("-50"), Some(-50));
        assert_eq!(rr3_sim_link_rssi_parse("-100"), Some(-100), "下界在域内");
        assert_eq!(rr3_sim_link_rssi_parse("-1"), Some(-1), "上界在域内");
        assert_eq!(rr3_sim_link_rssi_parse("0"), None, "0 dBm 不是合法读数");
        assert_eq!(rr3_sim_link_rssi_parse("-101"), None, "越下界不认");
        assert_eq!(rr3_sim_link_rssi_parse("abc"), None, "垃圾不认");
    }

    /// sim 方位角 env 键的取值裁决：±90 闭区间内的有限小数才收，越界/垃圾不认
    /// （落"不注入"）。批F2 判向 e2e（RR3_HAL_SIM_DOA_DEG=-60）的正门靠它。
    #[test]
    fn sim_doa_deg_env_values_parse_or_default() {
        assert_eq!(rr3_sim_doa_deg_parse("-60"), Some(-60.0));
        assert_eq!(rr3_sim_doa_deg_parse(" 45.5 "), Some(45.5));
        assert_eq!(rr3_sim_doa_deg_parse("-90"), Some(-90.0), "下界在域内");
        assert_eq!(rr3_sim_doa_deg_parse("90"), Some(90.0), "上界在域内");
        assert_eq!(rr3_sim_doa_deg_parse("0"), Some(0.0));
        assert_eq!(rr3_sim_doa_deg_parse("90.1"), None, "越上界不认");
        assert_eq!(rr3_sim_doa_deg_parse("-90.5"), None, "越下界不认");
        assert_eq!(rr3_sim_doa_deg_parse("abc"), None, "垃圾不认");
        assert_eq!(rr3_sim_doa_deg_parse(""), None, "空串不认");
        assert_eq!(rr3_sim_doa_deg_parse("nan"), None, "NaN 不认");
        assert_eq!(rr3_sim_doa_deg_parse("inf"), None, "inf 不认");
    }
}
