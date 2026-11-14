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
//!
//! 模块按能力面分件：`selector` 后端判定、`parse` 各 env 值裁决、`sink` 音频
//! 输入/输出、`acts` servo/gpio/display、`power` 电源与轨压、`link` 链路与配网存储。

mod acts;
mod link;
mod parse;
mod power;
mod selector;
mod sink;

#[cfg(test)]
mod tests;

pub use acts::{rr3_hal_display, rr3_hal_gpio_out, rr3_hal_servo};
pub use link::{rr3_hal_link, rr3_hal_wifi_store};
pub use parse::{
    rr3_gpio_line_key, rr3_ina219_parse, rr3_pwm_index_parse, rr3_sim_batt_parse,
    rr3_sim_chg_parse, rr3_sim_doa_parse, rr3_sim_rssi_parse, rr3_sim_up_parse,
};
pub use power::{rr3_hal_power, rr3_hal_rail};
pub use selector::{
    rr3_audio_backend, rr3_backend_default, rr3_backend_resolve, rr3_display_backend,
    rr3_servo_backend,
};
pub use sink::{rr3_audio_source, rr3_audio_source_cmd, rr3_hal_audio_sink};

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

/// 音频**采集面**的后端覆盖键（能力面级，2026-10-31）：组合形态（Pi 宿主 ×
/// ESP32 board）下，全局 `RR3_HAL_BACKEND=esp32` 会让同机的舵机/显示面也各自
/// 去开同一 UART，而 `Rr3UartConn` 是跨进程 flock 单消费者（status.md U 轮⑤
/// 结构冲突）——第二个 opener 必被拒、模块探活 exit(1)。音频面因此需要单独
/// 选档：本键只被 [`rr3_audio_source_cmd`] 读取（回放面仍走全局，
/// 板无扬声器 TTS 恒宿主），其余能力面保持全局/平台缺省。未设/空白 → 回落
/// 全局 [`rr3_backend_resolve`]，既有行为逐点不变。
pub const RR3_HAL_AUDIO_BACKEND_ENV: &str = "RR3_HAL_AUDIO_BACKEND";

/// 舵机面后端覆盖键（P0-4 组合档归属）：与 [`RR3_HAL_AUDIO_BACKEND_ENV`] 同语义——
/// 组合形态（Pi 宿主 × ESP32 board）下，全局 `RR3_HAL_BACKEND=linux` 让舵机面走
/// sysfs PWM，但舵机实际在 ESP32 板上。本键只被 [`rr3_hal_servo`] 读取，未设/
/// 空白 → 回落全局。
pub const RR3_HAL_SERVO_BACKEND_ENV: &str = "RR3_HAL_SERVO_BACKEND";

/// 显示面后端覆盖键（P0-4 组合档归属）：同舵机面——组合档显示（OLED/点阵）在
/// ESP32 板上，全局 linux 会让显示面走 spidev。本键只被 [`rr3_hal_display`] 读取。
pub const RR3_HAL_DISPLAY_BACKEND_ENV: &str = "RR3_HAL_DISPLAY_BACKEND";

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

/// linux I2C 电池读数的 INA219 分流电阻（毫欧正整数）。应急覆盖：描述符键
/// `ina219_shunt_mohm` 同域。与 LSB 键两件齐备才写校准（0x05）；缺一即电流面
/// 不可信（charging 恒 false，见 [`crate::linux_power`]）。
pub const RR3_HAL_INA219_SHUNT_ENV: &str = "RR3_HAL_INA219_SHUNT_MOHM";

/// linux I2C 电池读数的 INA219 电流 LSB（微安/位正整数）。应急覆盖：描述符键
/// `ina219_ilsb_ua` 同域（量程策略：LSB = 最大预期电流 / 32768）。
pub const RR3_HAL_INA219_ILSB_ENV: &str = "RR3_HAL_INA219_ILSB_UA";

/// linux（NetworkManager）链路读数的接口名键（任意非空串）。应急覆盖：描述符键
/// `link_ifname` 同域；都没给 → 链路面占位显式回错（fail-closed，runbook 件 4
/// 参考机 wlan0 在 profiles）。
pub const RR3_HAL_LINK_IFNAME_ENV: &str = "RR3_HAL_LINK_IFNAME";

/// linux spidev 显示面的设备路径键（如 `/dev/spidev0.0`）。应急覆盖：描述符键
/// `spi_device` 同域；都没给 → 显示面占位显式回错（fail-closed，链④真件档
/// 参考机 spidev0.0 在 profiles）。
pub const RR3_HAL_SPI_DEVICE_ENV: &str = "RR3_HAL_SPI_DEVICE";

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

/// 板外设命令帧承载符号经 backend 装配面 re-export（⑨-w 依赖面白名单：单元侧
/// `rr_hal::` 引用只许四类，命令旁路消费方走 `rr_hal::backend::` 同面引用——
/// backend 面就是板外设命令下行的职责面，见 [`rr3_audio_source_cmd`]）。
/// 宿主档门（批N8 件三）：桥旁路面是宿主档符号；espidf 档类型名经空枚举承载
/// （不可构造，`Option` 恒 `None`——装配面天然落 `None` 走 fail-closed，不静默）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use crate::esp32audio::{Rr3AudioCmdProducer, Rr3PeriphCmd};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub enum Rr3AudioCmdProducer {}
