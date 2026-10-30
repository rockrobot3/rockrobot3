//! 后端标识与选择入口（architecture.md §5、ADR-0007）。
//!
//! | 后端 | 落地 | 承载 |
//! |---|---|---|
//! | `linux` | M3（`linux.rs`，`alsa` feature + `target_os = "linux"` 双门） | Buildroot 根文件系统内的 ALSA 采集/回放 |
//! | `linux`（舵机） | 批F5a（`linux_pwm.rs`，仅 `target_os = "linux"` 单门，纯 std） | 树莓派 sysfs PWM 驱动 SG90（R11①/R22 定案档，GPIO12/13） |
//! | `linux`（GPIO） | 批F5a（`linux_gpio.rs`，仅 `target_os = "linux"` 单门，libc 声明壳） | 树莓派 chardev GPIO 数字输出：LED / 继电器（R11②，R8 预算表 GPIO4/17 软件路行） |
//! | `linux`（显示） | 链④真件档（`linux_display.rs`，仅 `target_os = "linux"` 单门，libc 声明壳） | MAX7219 spidev 点阵帧渲染（R11③ trait 立面 + 链④真件到位实装；设备路径来自描述符 `spi_device`） |
//! | `linux`（电源） | 批F5a（`linux_power.rs`，仅 `target_os = "linux"` 单门，libc 声明壳） | INA219@0x40 I2C 总线电压读数（R18，判据 12；charging 判定随真件档校准） |
//! | `linux`（链路） | 批F5a（`linux_link.rs`，仅 `target_os = "linux"` 单门，nmcli 子进程） | NetworkManager 链路状态读数（R18，判据 13；配网/断链重连是 NM 自身行为，HAL 只如实报数） |
//! | `sim` | M3（`sim.rs`：正弦/噪声/静音三模式虚拟麦克风 + 尾窗虚拟扬声器） | `tools/sim`（bin `rr3-sim`）与 CI、无硬件开发 |
//! | `mac` | 宿主真件（`macaudio.rs`，仅 `target_os = "macos"` 单门，手写 AudioQueue 绑定） | macOS 宿主真麦拾音/扬声器回放——显式 `RR3_HAL_BACKEND=mac` 才启用，平台缺省仍 sim（CI 纪律） |
//! | `esp32` | 批C（`esp32_uart.rs`，`RR3_HAL_PERIPH_UART` 指定设备路径）；A 批 c3 音频源（`esp32audio.rs`，板面双麦流） | ESP32 外设桥：舵机/显示两能力面的 UART 能力帧承载 + 音频上行——板不进城，宿主直连；跨进程 flock 排他即单消费者 |
//! | `placeholder` | 恒回 `Unsupported` | 能力缺失显式回错的兜底（`Rr3UnavailableSource`） |
//!
//! 反射环（1k–10kHz 电机 FOC/PID）不进这些 trait，留在 `rr3-act` 进程内。

use crate::{
    provision_store::Rr3WifiStore, Rr3AudioSink, Rr3AudioSource, Rr3GpioOut, Rr3Power, Rr3PwmServo,
    Rr3Rail, Rr3UnavailableSource,
};

/// 后端标识（写入日志与诊断主题 `rr3-runtime/module/state`）
pub const RR3_BACKEND_LINUX: &str = "linux";
pub const RR3_BACKEND_SIM: &str = "sim";
pub const RR3_BACKEND_ESP32: &str = "esp32";
/// macOS CoreAudio 音频后端（macaudio.rs，宿主真麦/扬声器）：仅显式启用——
/// macOS 平台缺省仍 sim（CI/e2e 真开麦是纪律红线），与 linux"显式启用真件"同裁定。
pub const RR3_BACKEND_MAC: &str = "mac";
pub const RR3_BACKEND_PLACEHOLDER: &str = "placeholder";

/// 后端选择的环境变量键（naming.md：`RR3_` 大写前缀）。
/// 取值：`sim` / `linux` / `esp32` / `mac` / `placeholder`；未设或取值不认 → 走平台默认。
pub const RR3_HAL_BACKEND_ENV: &str = "RR3_HAL_BACKEND";

// —— 以下 `RR3_HAL_*` 键全部是**应急覆盖**（批D④ 收口，ADR-0018 裁定 4）：
// 物理绑定的真值来自描述符（`profiles/<机>.toml` [hardware] → `RR3_HAL_BINDINGS`
// 注入，见 [`crate::descriptor`]）；本组 env 同键覆盖描述符，供现场救场。描述符
// 与 env 都没给的键，对应能力面落占位后端显式回错——代码缺省 fail-closed，不回落
// 任何一块具体板的引脚。键位语义/取值域的裁决纯函数留在本模块（compose 共用）。

/// sim 源的波形选择键：`sine` / `noise` / `silence` / `burst`（默认 `sine`，必过 VAD 起音阈值；
/// `burst` = 1s 正弦 + 1s 静音循环，VAD 周期性发 onset/offset，多轮对话闭环用）。
pub const RR3_HAL_SIM_WAVE_ENV: &str = "RR3_HAL_SIM_WAVE";

/// sim 源的声源方位角键（度，-90..=90 的十进制小数，负=左/正=右；未设/取值不认 → 不注入）。
/// 双麦方位模型见 [`crate::sim::Rr3SimDoa`]：批F2 sim 判向的正门注入点，
/// 多通道采集 + 本键才生效（单声道链路无方向语义，注入被设备面忽略）。
pub const RR3_HAL_SIM_DOA_DEG_ENV: &str = "RR3_HAL_SIM_DOA_DEG";

/// linux（ALSA）后端的设备名键：如 `hw:0,0`、`plughw:1,0`。应急覆盖：描述符键
/// `alsa_device` 同域；都没给 → 音频面占位显式回错（fail-closed）。
pub const RR3_HAL_ALSA_DEVICE_ENV: &str = "RR3_HAL_ALSA_DEVICE";

/// linux sysfs PWM 的芯片号键（十进制非负整数）。应急覆盖：描述符键 `pwm_chip`
/// 同域；都没给 → 舵机面占位显式回错（fail-closed，参考机真值 pwmchip0 在
/// profiles [hardware]，R22 定案档）。
pub const RR3_HAL_PWM_CHIP_ENV: &str = "RR3_HAL_PWM_CHIP";

/// linux sysfs PWM 的通道号键（十进制非负整数）。应急覆盖：描述符键 `pwm_channel`
/// 同域；都没给 → 舵机面占位显式回错（fail-closed，参考机 pwm0 = GPIO12）。
pub const RR3_HAL_PWM_CHANNEL_ENV: &str = "RR3_HAL_PWM_CHANNEL";

/// linux chardev GPIO 的芯片号键（十进制非负整数）。应急覆盖：描述符键
/// `gpio_chip` 同域；都没给 → 数字输出面占位显式回错（fail-closed）。
pub const RR3_HAL_GPIO_CHIP_ENV: &str = "RR3_HAL_GPIO_CHIP";

/// linux I2C 电池读数的总线号键（十进制非负整数）。应急覆盖：描述符键 `i2c_bus`
/// 同域；都没给 → 电源面占位显式回错（fail-closed，参考机 i2c1 在 profiles）。
pub const RR3_HAL_I2C_BUS_ENV: &str = "RR3_HAL_I2C_BUS";

/// linux I2C 电池读数的 INA219 从地址键（`0x` 前缀或纯十六进制）。应急覆盖：
/// 描述符键 `ina219_addr` 同域；都没给 → 电源面占位显式回错（fail-closed，
/// BOM #5 定案档 0x40 在 profiles）。
pub const RR3_HAL_INA219_ADDR_ENV: &str = "RR3_HAL_INA219_ADDR";

/// linux（NetworkManager）链路读数的接口名键（任意非空串）。应急覆盖：描述符键
/// `link_ifname` 同域；都没给 → 链路面占位显式回错（fail-closed，runbook 件 4
/// 参考机 wlan0 在 profiles）。
pub const RR3_HAL_LINK_IFNAME_ENV: &str = "RR3_HAL_LINK_IFNAME";

/// linux spidev 显示面的设备路径键（如 `/dev/spidev0.0`）。应急覆盖：描述符键
/// `spi_device` 同域；都没给 → 显示面占位显式回错（fail-closed，链④真件档
/// 参考机 spidev0.0 在 profiles）。
pub const RR3_HAL_SPI_DEVICE_ENV: &str = "RR3_HAL_SPI_DEVICE";

/// INA219 从地址键的取值裁决（只认 `0x` 前缀十六进制——裸写有十进制歧义
/// （"40" 是 0x28 还是 40？），不收；7-bit 可寻址域 0x08..=0x77 外不认）。
pub fn rr3_ina219_addr_parse(s: &str) -> Option<u8> {
    let s = s.trim();
    let v = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    u8::from_str_radix(v, 16)
        .ok()
        .filter(|a| (0x08..=0x77).contains(a))
}

/// 角色对应的线偏移应急 env 键（`led` → `RR3_HAL_GPIO_LED_LINE`、
/// `relay` → `RR3_HAL_GPIO_RELAY_LINE`）。取值十进制非负整数；role 仅认
/// 非空 ASCII 字母数字（防拼出怪键），不认返回 `None`。线偏移的**真值**在描述符
/// （`profiles/<机>.toml` [hardware] 的 `gpio_led_line`/`gpio_relay_line`，
/// R8 预算表参考机 GPIO4/17）——本组 env 只是应急覆盖，两处都没给时数字输出面
/// 落占位显式回错（fail-closed，批D④ 收口，不再有代码缺省）。白名单外的 role
/// 无键无绑定（`None`）。独立成纯函数：键拼装可单测，选择函数里只留合成与选路。
pub fn rr3_gpio_line_env_key(role: &str) -> Option<String> {
    if role.is_empty() || !role.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(format!("RR3_HAL_GPIO_{}_LINE", role.to_ascii_uppercase()))
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
/// macOS 平台缺省同为 `sim`（真麦走显式 `mac`——CI/无头不真开设备，见 [`RR3_BACKEND_MAC`]）。
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
        RR3_BACKEND_ESP32 => Ok(RR3_BACKEND_ESP32),
        RR3_BACKEND_LINUX => {
            if cfg!(target_os = "linux") {
                Ok(RR3_BACKEND_LINUX)
            } else {
                Ok(RR3_BACKEND_PLACEHOLDER) // 使用时回 Unsupported，不静默降级
            }
        }
        RR3_BACKEND_MAC => {
            if cfg!(target_os = "macos") {
                Ok(RR3_BACKEND_MAC)
            } else {
                Ok(RR3_BACKEND_PLACEHOLDER) // 使用时回 Unsupported，不静默降级
            }
        }
        other => Err(format!(
            "环境变量 {RR3_HAL_BACKEND_ENV} 取值不认: {other}（允许 sim/linux/esp32/mac/placeholder）"
        )),
    }
}

/// 描述符失败点名文案（R3-D3/0.0.5 批J G5）：裁决纯函数独立可单测。
/// 生产消费方只有 cfg(linux) 的 [`rr3_hal_bindings_effective_named`]，故生产门
/// 收 linux；test 门留给文案形状单测（macOS 主机也要跑）。
#[cfg(any(target_os = "linux", test))]
fn rr3_hal_bindings_failure_note(facet: &str, err: &str) -> String {
    format!("rr-hal: 描述符解析失败，{facet}落占位（使用时以 Unsupported 回错）: {err}")
}

/// 生效绑定获取（R3-D3/批J G5 收口）：各选择函数 linux 分支此前以 `.ok()` 取
/// 绑定，描述符解析失败（[hardware] 键名拼错/取值不认/应急 env 无效）被静默
/// 吞掉、只落占位，板上排障无从下手。改为失败当场 `eprintln!` 点名（rrd 系统
/// 实例 stderr 进 journal），随后照旧落占位——失败语义不变，只补"错误当场可见"。
/// 仅 linux 分支调用（非 linux 平台无人消费），故 cfg 门收进本平台。
#[cfg(target_os = "linux")]
fn rr3_hal_bindings_effective_named(facet: &str) -> Option<crate::descriptor::Rr3HalBindings> {
    match crate::descriptor::rr3_hal_bindings_effective() {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("{}", rr3_hal_bindings_failure_note(facet, &e));
            None
        }
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
/// linux 档的设备名来自生效绑定（[`crate::descriptor::rr3_hal_bindings_effective`]：
/// 描述符为底、env 应急覆盖）；键缺失/合成失败 → 占位显式回错（fail-closed，批D④）。
pub fn rr3_hal_audio_source() -> (Box<dyn Rr3AudioSource>, &'static str) {
    let (src, _, backend) = rr3_hal_audio_source_with_cmd();
    (src, backend)
}

/// 板外设命令帧承载符号经 backend 装配面 re-export（⑨-w 依赖面白名单：单元侧
/// `rr_hal::` 引用只许四类，命令旁路消费方走 `rr_hal::backend::` 同面引用——
/// backend 面就是板外设命令下行的职责面，见 [`rr3_hal_audio_source_with_cmd`]）。
/// 宿主档门（批N8 件三）：桥旁路面是宿主档符号；espidf 档类型名经空枚举承载
/// （不可构造，`Option` 恒 `None`——装配面天然落 `None` 走 fail-closed，不静默）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use crate::esp32audio::{Rr3AudioCmdProducer, Rr3PeriphCmd};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub enum Rr3AudioCmdProducer {}

/// 音频源 + 板外设命令旁路产面（B 批 b1）：esp32 档返 `Some`——flock 单消费者
/// 纪律下板外设命令下行唯一通路 = 持 conn 的音频源旁路（esp32audio.rs）；
/// 其余档 `None`（无旁路面，消费方按 None fail-closed 关闭反射环）。
pub fn rr3_hal_audio_source_with_cmd() -> (
    Box<dyn Rr3AudioSource>,
    Option<Rr3AudioCmdProducer>,
    &'static str,
) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(all(target_os = "linux", feature = "alsa")) => {
            #[cfg(all(target_os = "linux", feature = "alsa"))]
            return rr3_hal_bindings_effective_named("音频采集")
                .and_then(|b| {
                    b.alsa_device.map(|dev| {
                        (
                            Box::new(crate::linux::Rr3AlsaAudioSource::rr3_alsa_source_new(&dev))
                                as Box<dyn Rr3AudioSource>,
                            None,
                            RR3_BACKEND_LINUX,
                        )
                    })
                })
                .unwrap_or((
                    Box::new(Rr3UnavailableSource),
                    None,
                    RR3_BACKEND_PLACEHOLDER,
                ));
            #[cfg(not(all(target_os = "linux", feature = "alsa")))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        // 显式 linux 但 alsa 后端没编进（无 alsa 的板上构建）：占位显式回错，
        // 不静默落 sim（resolve 订正为平台门后此分支结构上可达）
        RR3_BACKEND_LINUX => (
            Box::new(Rr3UnavailableSource),
            None,
            RR3_BACKEND_PLACEHOLDER,
        ),
        // macOS 真麦（显式 mac 才进；缺省仍 sim——CI/无头不真开设备）
        RR3_BACKEND_MAC if cfg!(target_os = "macos") => {
            #[cfg(target_os = "macos")]
            return (
                Box::new(crate::macaudio::Rr3MacAudioSource::rr3_mac_source_new())
                    as Box<dyn Rr3AudioSource>,
                None,
                RR3_BACKEND_MAC,
            );
            #[cfg(not(target_os = "macos"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        // 显式 mac 但平台不是 macOS：占位显式回错，不静默落 sim
        RR3_BACKEND_MAC => (
            Box::new(Rr3UnavailableSource),
            None,
            RR3_BACKEND_PLACEHOLDER,
        ),
        // ESP32 板面双麦流（A 批 c3，esp32audio.rs）：设备/波特来自应急 env
        // （与舵机/显示面同键）；缺 env → 点名 + placeholder（fail-closed，
        // 不猜设备不静默换 sim）。
        // 宿主档门（批N8 件三）：同 [`rr3_hal_servo`] 的 esp32 臂。
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        RR3_BACKEND_ESP32 => {
            match std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV)
                .map(|p| p.trim().to_string())
            {
                Ok(p) if !p.is_empty() => {
                    let baud = std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_BAUD_ENV)
                        .ok()
                        .and_then(|v| crate::esp32_uart::rr3_periph_baud_parse(&v))
                        .unwrap_or(crate::esp32_uart::RR3_BRIDGE_BAUD);
                    let (src, producer) =
                        crate::esp32audio::Rr3Esp32UartAudioSource::rr3_esp32_audio_source_new_with_cmd(
                            std::path::PathBuf::from(p),
                            baud,
                        );
                    (
                        Box::new(src) as Box<dyn Rr3AudioSource>,
                        Some(producer),
                        RR3_BACKEND_ESP32,
                    )
                }
                _ => {
                    eprintln!(
                        "rr-hal: esp32 档缺环境变量 {}，音频采集面落占位（使用时以 Unsupported 回错）",
                        crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV
                    );
                    (
                        Box::new(Rr3UnavailableSource),
                        None,
                        RR3_BACKEND_PLACEHOLDER,
                    )
                }
            }
        }
        RR3_BACKEND_PLACEHOLDER => (
            Box::new(Rr3UnavailableSource),
            None,
            RR3_BACKEND_PLACEHOLDER,
        ),
        RR3_BACKEND_SIM => {
            let doa = std::env::var(RR3_HAL_SIM_DOA_DEG_ENV)
                .ok()
                .and_then(|v| rr3_sim_doa_deg_parse(&v))
                .and_then(crate::sim::Rr3SimDoa::rr3_sim_doa_new);
            (
                Box::new(crate::sim::Rr3SimAudioSource::rr3_sim_source_new_with_doa(
                    rr3_hal_sim_wave(),
                    doa,
                )),
                None,
                RR3_BACKEND_SIM,
            )
        }
        // `&str` 模式编译器裁定无法穷举（E0004）：白名单收敛在
        // [`rr3_hal_backend_resolve`] 单点，此臂不可达；防御性落占位
        // （使用时 Unsupported 显式回错），不静默换 sim。
        _ => (
            Box::new(Rr3UnavailableSource),
            None,
            RR3_BACKEND_PLACEHOLDER,
        ),
    }
}

/// 按环境选音频播放面（与 [`rr3_hal_audio_source`] 同一套规则）
pub fn rr3_hal_audio_sink() -> (Box<dyn Rr3AudioSink>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(all(target_os = "linux", feature = "alsa")) => {
            #[cfg(all(target_os = "linux", feature = "alsa"))]
            return rr3_hal_bindings_effective_named("音频回放")
                .and_then(|b| {
                    b.alsa_device.map(|dev| {
                        (
                            Box::new(crate::linux::Rr3AlsaAudioSink::rr3_alsa_sink_new(&dev))
                                as Box<dyn Rr3AudioSink>,
                            RR3_BACKEND_LINUX,
                        )
                    })
                })
                .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER));
            #[cfg(not(all(target_os = "linux", feature = "alsa")))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        // 同 audio_source：显式 linux 但 alsa 没编进 → 占位显式回错，不静默落 sim
        RR3_BACKEND_LINUX => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        // macOS 真扬声器（显式 mac 才进；缺省仍 sim——CI/无头不真开设备）
        RR3_BACKEND_MAC if cfg!(target_os = "macos") => {
            #[cfg(target_os = "macos")]
            return (
                Box::new(crate::macaudio::Rr3MacAudioSink::rr3_mac_sink_new())
                    as Box<dyn Rr3AudioSink>,
                RR3_BACKEND_MAC,
            );
            #[cfg(not(target_os = "macos"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        // 显式 mac 但平台不是 macOS：占位显式回错，不静默落 sim
        RR3_BACKEND_MAC => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        // 板无扬声器（A 批 c3）：esp32 档回放面显式落占位——TTS 回放恒宿主
        // （mac AudioQueue / sim 尾窗），fail-closed 不静默换。
        RR3_BACKEND_ESP32 => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        RR3_BACKEND_PLACEHOLDER => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        RR3_BACKEND_SIM => (
            Box::new(crate::sim::Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10)),
            RR3_BACKEND_SIM,
        ),
        // 同 audio_source：白名单收敛在 resolve 单点，此臂不可达；防御性落占位
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// 按环境选舵机（与 [`rr3_hal_audio_source`] 同一套规则）。linux 平台走 sysfs PWM
/// 后端（[`crate::linux_pwm`]，R11①/R22 定案档），芯片/通道来自生效绑定
/// （描述符 `pwm_chip`/`pwm_channel`，`RR3_HAL_PWM_*` env 应急覆盖）；键缺失或
/// 合成失败 → 占位件使用时以 `Unsupported` 回错（fail-closed，不回落参考机引脚），
/// 非 linux 平台显式要 `linux` 时同落占位件，不静默换 sim。
pub fn rr3_hal_servo() -> (Box<dyn Rr3PwmServo>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return rr3_hal_bindings_effective_named("舵机")
                .and_then(|b| {
                    Some((
                        Box::new(crate::linux_pwm::Rr3LinuxPwmServo::rr3_linux_pwm_servo_new(
                            std::path::Path::new(crate::linux_pwm::RR3_PWM_SYSFS_ROOT_DEFAULT),
                            b.pwm_chip?,
                            b.pwm_channel?,
                        )) as Box<dyn Rr3PwmServo>,
                        RR3_BACKEND_LINUX,
                    ))
                })
                .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER));
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_LINUX => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
        // ESP32 外设桥（批C）：设备路径来自应急 env（描述符键 periph_uart 随真件
        // 装配批补入 profiles）；键缺失 → 占位显式回错（fail-closed，不猜设备）。
        // 宿主档门（批N8 件三）：espidf 档无桥面，本臂剔除后显式要 esp32 落
        // 下方 `_` 兜底臂占位回错（resolve 白名单单点不裁）。
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        RR3_BACKEND_ESP32 => {
            match std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV)
                .map(|p| p.trim().to_string())
            {
                Ok(p) if !p.is_empty() => {
                    let axis = std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_AXIS_ENV)
                        .ok()
                        .and_then(|v| crate::esp32_uart::rr3_periph_axis_parse(&v))
                        .unwrap_or(rr_hal_api::RR3_PERIPH_AXIS_DEFAULT);
                    // 批D②：波特率 env（白名单裁决，缺省 115200 不变——板固件
                    // 已钉 `UART2_BAUD_HZ`（2026-10-30 起 2M），会话须显式同档）
                    let baud = std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_BAUD_ENV)
                        .ok()
                        .and_then(|v| crate::esp32_uart::rr3_periph_baud_parse(&v))
                        .unwrap_or(crate::esp32_uart::RR3_BRIDGE_BAUD);
                    (
                        Box::new(
                            crate::esp32_uart::Rr3Esp32UartServo::rr3_esp32_uart_servo_new_with_baud(
                                std::path::PathBuf::from(p),
                                axis,
                                baud,
                            ),
                        ) as Box<dyn Rr3PwmServo>,
                        RR3_BACKEND_ESP32,
                    )
                }
                _ => {
                    eprintln!(
                        "rr-hal: esp32 档缺环境变量 {}，舵机面落占位（使用时以 Unsupported 回错）",
                        crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV
                    );
                    (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
                }
            }
        }
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
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return rr3_hal_bindings_effective_named("数字输出")
                .and_then(|b| {
                    let line = match role {
                        "led" => b.gpio_led_line,
                        "relay" => b.gpio_relay_line,
                        _ => None, // 白名单外：无键无绑定，fail-closed
                    };
                    Some((
                        Box::new(crate::linux_gpio::Rr3LinuxGpioOut::rr3_linux_gpio_out_new(
                            std::path::Path::new(crate::linux_gpio::RR3_GPIO_DEV_ROOT_DEFAULT),
                            b.gpio_chip?,
                            line?,
                            role,
                        )) as Box<dyn Rr3GpioOut>,
                        RR3_BACKEND_LINUX,
                    ))
                })
                .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER));
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_SIM => (
            Box::new(crate::sim::Rr3SimGpioOut::rr3_sim_gpio_new(false)),
            RR3_BACKEND_SIM,
        ),
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
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

/// 按环境选显示面（与 [`rr3_hal_audio_source`] 同一套规则）。linux 档走
/// MAX7219 spidev 后端（[`crate::linux_display`]，链④真件档实装），设备路径
/// 来自生效绑定（描述符 `spi_device`，`RR3_HAL_SPI_DEVICE` env 应急覆盖）；
/// 键缺失、合成失败或开口失败 → 占位件使用时以 `Unsupported` 回错
/// （fail-closed）——rr3-interact 开机探活会按"能力缺失显式回错"退出，不
/// 静默换 sim。mac 宿主档（C4 修正）：Mac 天然无 LED 矩阵，显示面落 sim 同
/// 款虚拟点阵（内存记录，机制同源）——宿主开发档不因显示件缺席整体判死。
pub fn rr3_hal_display() -> (Box<dyn crate::Rr3Display>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return rr3_hal_bindings_effective_named("显示")
                .and_then(|b| {
                    Some((
                        Box::new(
                            crate::linux_display::Rr3Max7219SpiDisplay::rr3_max7219_display_new(
                                std::path::Path::new(b.spi_device.as_deref()?),
                            ),
                        ) as Box<dyn crate::Rr3Display>,
                        RR3_BACKEND_LINUX,
                    ))
                })
                .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER));
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
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
        // 8x8→64x64 放大渲染）。设备路径同舵机面的应急 env；缺 → 占位显式回错。
        // 宿主档门（批N8 件三）：同 [`rr3_hal_servo`] 的 esp32 臂。
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        RR3_BACKEND_ESP32 => {
            match std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV)
                .map(|p| p.trim().to_string())
            {
                Ok(p) if !p.is_empty() => {
                    // 批D②：波特率 env（白名单裁决，缺省 115200 不变——板固件
                    // 已钉 `UART2_BAUD_HZ`（2026-10-30 起 2M），会话须显式同档）
                    let baud = std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_BAUD_ENV)
                        .ok()
                        .and_then(|v| crate::esp32_uart::rr3_periph_baud_parse(&v))
                        .unwrap_or(crate::esp32_uart::RR3_BRIDGE_BAUD);
                    (
                        Box::new(
                            crate::esp32_uart::Rr3Esp32UartDisplay::rr3_esp32_uart_display_new_with_baud(
                                std::path::PathBuf::from(p),
                                baud,
                            ),
                        ) as Box<dyn crate::Rr3Display>,
                        RR3_BACKEND_ESP32,
                    )
                }
                _ => {
                    eprintln!(
                        "rr-hal: esp32 档缺环境变量 {}，显示面落占位（使用时以 Unsupported 回错）",
                        crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV
                    );
                    (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
                }
            }
        }
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// 按环境选电源读数面（与 [`rr3_hal_audio_source`] 同一套规则）。linux 走
/// INA219 I2C 后端（[`crate::linux_power`]，R18），总线/地址来自生效绑定
/// （描述符 `i2c_bus`/`ina219_addr`，`RR3_HAL_*` env 应急覆盖）；键缺失或合成
/// 失败 → 占位件使用时以 `Unsupported` 回错（fail-closed）——rr3-power 开机
/// 探活会按"能力缺失显式回错"退出，不静默换 sim。
pub fn rr3_hal_power() -> (Box<dyn Rr3Power>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return rr3_hal_bindings_effective_named("电源读数")
                .and_then(|b| {
                    Some((
                        Box::new(crate::linux_power::Rr3LinuxIna219::rr3_linux_power_new(
                            std::path::Path::new(crate::linux_power::RR3_I2C_DEV_ROOT_DEFAULT),
                            b.i2c_bus?,
                            b.ina219_addr?,
                        )) as Box<dyn Rr3Power>,
                        RR3_BACKEND_LINUX,
                    ))
                })
                .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER));
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
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

/// 按环境选网络链路读数面（与 [`rr3_hal_audio_source`] 同一套规则）。linux
/// 分支走 NetworkManager 后端（批F5a⑤，`linux_link.rs` nmcli 子进程承载；
/// 配网/断链重连是 NM 自身行为，HAL 只如实报数），接口名来自生效绑定（描述符
/// `link_ifname`，`RR3_HAL_LINK_IFNAME` env 应急覆盖）；键缺失或合成失败 →
/// 占位件使用时以 `Unsupported` 回错（fail-closed）；显式 `linux` 但非 linux
/// 平台时同落占位件，不静默换 sim。
///
/// `nmcli_timeout`（0.0.5 批I N-2）：nmcli 子进程超时，由模块 config 的
/// `timeout_ms` 传入——没有代码缺省（同 `link_ifname` 的 fail-closed 口径），
/// sim/placeholder 分支忽略该值。最坏翻转延迟自此由配置管住，不再由子进程
/// 自由决定。
pub fn rr3_hal_link(
    // 非 linux 平台 linux 分支被剔除、sim/placeholder 分支不消费超时——参数在
    // 那些平台编译面上未使用是结构性的，不是疏漏（linux 上它进子进程守护）
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    nmcli_timeout: std::time::Duration,
) -> (Box<dyn crate::Rr3Link>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return rr3_hal_bindings_effective_named("链路读数")
                .and_then(|b| {
                    Some((
                        Box::new(crate::linux_link::Rr3LinuxNmLink::rr3_linux_link_new(
                            b.link_ifname?,
                            "nmcli",
                            nmcli_timeout,
                        )) as Box<dyn crate::Rr3Link>,
                        RR3_BACKEND_LINUX,
                    ))
                })
                .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER));
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
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

/// 按环境选配网凭据存储面（0.0.5 批M M-γ，MC-6：凭据由后端承载持久化）。
/// linux 分支走 NetworkManager 连接配置后端（[`crate::provision_store::
/// Rr3NmWifiStore`]，nmcli 子进程承载），子进程超时由模块 config 的
/// `timeout_ms` 传入（与链路读数后端同一档，无代码缺省）；sim 分支内存档
/// （进程内存态即"在册"，sim 无"重启仍在"语义）；其余落占位（使用时以
/// `Unsupported` 回错，fail-closed 不静默换）。
///
/// 构造不触设备（选择函数口径）；凭据落盘策略归 NM 本身，件内不自写文件
/// ——`Rr3Provision` 只管态不管存（工单 MC-6 红线）。
pub fn rr3_hal_wifi_store(
    // 非 linux 平台 linux 分支被剔除、sim/placeholder 分支不消费超时——参数在
    // 那些平台编译面上未使用是结构性的，不是疏漏（linux 上它进子进程守护）
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    store_timeout: std::time::Duration,
) -> (Box<dyn Rr3WifiStore>, &'static str) {
    match rr3_hal_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return (
                Box::new(
                    crate::provision_store::Rr3NmWifiStore::rr3_nm_wifi_store_new(
                        "nmcli",
                        store_timeout,
                    ),
                ),
                RR3_BACKEND_LINUX,
            );
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_SIM => (
            Box::new(crate::provision_store::Rr3SimWifiStore::default()),
            RR3_BACKEND_SIM,
        ),
        _ => (
            Box::new(crate::provision_store::Rr3WifiStoreUnavailable),
            RR3_BACKEND_PLACEHOLDER,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析函数的平台默认分支：未设环境变量时回落平台默认（本机 macOS = sim）。
    /// 环境变量支无法在本测试里安全置位（并发测试读同一进程环境，std::env::set_var
    /// 是 unsafe 且有竞态），故三态自洽：未设 → 钉平台缺省；显式合法值 → 白名单；
    /// 显式非法值 → Err 点名（fail-closed）。显式取值路径另由模块侧 e2e 以真实
    /// 进程环境覆盖（如 `--audio-backend mac` 起的 rr3-sim 全环）。
    #[test]
    fn default_backend_is_sim_off_linux_alsa() {
        let env_set = std::env::var_os(RR3_HAL_BACKEND_ENV).is_some();
        match rr3_hal_backend_resolve() {
            Ok(resolved) => {
                assert!(
                    [
                        RR3_BACKEND_SIM,
                        RR3_BACKEND_LINUX,
                        RR3_BACKEND_MAC,
                        RR3_BACKEND_PLACEHOLDER,
                    ]
                    .contains(&resolved),
                    "解析结果必须是合法后端名: {resolved}"
                );
                if !env_set {
                    if cfg!(all(target_os = "linux", feature = "alsa")) {
                        assert_eq!(resolved, RR3_BACKEND_LINUX);
                    } else {
                        assert_eq!(resolved, RR3_BACKEND_SIM);
                    }
                }
            }
            Err(e) => {
                assert!(env_set, "未设环境变量必须解析成功: {e}");
            }
        }
    }

    /// G5 收口（R3-D3）：描述符失败点名文案含面名与原因原文——"错误当场可见"
    /// 的形状钉死（文案改动此处即红，防止退回无声吞错）。
    #[test]
    fn descriptor_failure_note_names_facet_and_reason() {
        let note = rr3_hal_bindings_failure_note(
            "音频采集",
            "描述符解析失败: 描述符键 gpio_led_line 取值不认: \"four\"",
        );
        assert!(note.contains("rr-hal:"), "{note}");
        assert!(note.contains("音频采集"), "{note}");
        assert!(note.contains("gpio_led_line"), "{note}");
        assert!(note.contains("Unsupported"), "{note}");
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
        if cfg!(target_os = "macos") {
            // mac 宿主档显示面 = sim 同款虚拟点阵（宿主无 LED 矩阵是平台事实）
            legal.push(RR3_BACKEND_MAC);
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
        let (display, display_backend) = rr3_hal_display();
        assert!(
            legal.contains(&display_backend),
            "显示后端名合法: {display_backend}"
        );
        let (link, link_backend) = rr3_hal_link(std::time::Duration::from_secs(5));
        assert!(
            legal.contains(&link_backend),
            "链路后端名合法: {link_backend}"
        );
        // sim 档（本机 macOS 默认）下探活必须成功——执行件从断电态起步
        // （探活全是读面 &self，无需可变绑定；写面 &mut self 的使用在模块侧）
        if servo_backend == RR3_BACKEND_SIM {
            assert_eq!(servo.angle().unwrap(), 90, "sim 舵机初始中位");
            assert!(!gpio.level().unwrap(), "sim 通道初始断电");
        }
        if display_backend == RR3_BACKEND_SIM || display_backend == RR3_BACKEND_MAC {
            assert_eq!(display.frame().unwrap(), [0; 8], "宿主虚拟点阵初始黑屏");
        }
        if link_backend == RR3_BACKEND_SIM {
            let st = link.state().unwrap();
            if st.up {
                assert!(
                    st.ssid.is_some() && st.rssi_dbm.is_some(),
                    "sim 在链必带 SSID/RSSI"
                );
            } else {
                assert!(
                    st.ssid.is_none() && st.rssi_dbm.is_none(),
                    "sim 断链必无 SSID/RSSI"
                );
            }
        }
    }

    /// 音频面选择（A 批 c3）：源/播两选择函数拿到的后端名必须合法；**播面
    /// 永不落 esp32**（板无扬声器，fail-closed 显式档）。构造不触设备
    /// （esp32 源 open 才真开 UART），环境变量缺席时本机缺省档合法即可。
    #[test]
    fn audio_selectors_return_legal_backend_objects() {
        let (src, src_backend) = rr3_hal_audio_source();
        assert!(
            [
                RR3_BACKEND_SIM,
                RR3_BACKEND_LINUX,
                RR3_BACKEND_MAC,
                RR3_BACKEND_ESP32,
                RR3_BACKEND_PLACEHOLDER,
            ]
            .contains(&src_backend),
            "音频源后端名合法: {src_backend}"
        );
        let (sink, sink_backend) = rr3_hal_audio_sink();
        assert!(
            [
                RR3_BACKEND_SIM,
                RR3_BACKEND_LINUX,
                RR3_BACKEND_MAC,
                RR3_BACKEND_PLACEHOLDER,
            ]
            .contains(&sink_backend),
            "音频播后端名合法: {sink_backend}"
        );
        assert_ne!(
            sink_backend, RR3_BACKEND_ESP32,
            "板无扬声器，播面永不落 esp32"
        );
        let _ = (src, sink); // 对象可用性归使用面（探活），选择面不触设备
    }

    /// GPIO 角色 → 应急 env 键拼装（R8 预算表 GPIO4/17 真值已迁 profiles
    /// [hardware]，批D④ 后无代码缺省）：键拼装是通用机制（任意字母数字 role
    /// 都拼得出键），**绑定白名单在选择面**（rr3_hal_gpio_out 只认 led/relay，
    /// 白名单外 fail-closed 显式回错）；空串/怪字符不拼键。
    #[test]
    fn gpio_role_env_key_matches_whitelist() {
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
        assert_eq!(
            rr3_gpio_line_env_key("fan").as_deref(),
            Some("RR3_HAL_GPIO_FAN_LINE"),
            "键拼装通用；fan 不在选择面白名单，无绑定 fail-closed"
        );
    }

    /// linux PWM 芯片/通道号键的取值裁决（应急覆盖与描述符共用）：十进制非负
    /// 整数才收，负数/垃圾/空不认（真值 pwmchip0/pwm0 = GPIO12 在 profiles
    /// [hardware]，批D④ 后代码不再持有缺省）。
    #[test]
    fn pwm_index_env_values_parse_or_default() {
        assert_eq!(rr3_pwm_index_parse("0"), Some(0));
        assert_eq!(rr3_pwm_index_parse("1"), Some(1));
        assert_eq!(rr3_pwm_index_parse(" 2 "), Some(2));
        for bad in ["", "-1", "3.5", "abc"] {
            assert_eq!(rr3_pwm_index_parse(bad), None, "{bad}");
        }
    }

    /// INA219 从地址键的取值裁决：只认 `0x` 前缀十六进制（裸写有十进制歧义
    /// 不收），7-bit 可寻址域 0x08..=0x77 外（含 0x00 广播、0x78+ 保留段）与
    /// 垃圾不认。
    #[test]
    fn ina219_addr_parse_accepts_hex_within_7bit() {
        assert_eq!(rr3_ina219_addr_parse("0x40"), Some(0x40), "BOM #5 定案档");
        assert_eq!(rr3_ina219_addr_parse("0X48"), Some(0x48), "大写前缀");
        assert_eq!(rr3_ina219_addr_parse(" 0x48 "), Some(0x48), "trim");
        for bad in ["0x00", "0x07", "0x78", "0xFF", "16", "40", "", "zz"] {
            assert_eq!(rr3_ina219_addr_parse(bad), None, "{bad}");
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
