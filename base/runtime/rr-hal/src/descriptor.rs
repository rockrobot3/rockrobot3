// SPDX-License-Identifier: Apache-2.0
//! 物理绑定描述符（ADR-0018 裁定 4、批D④ 收口）：**这块板/平台是什么的唯一数据
//! 事实源**。引脚/控制器编号/总线/设备名收进声明式描述符（`profiles/<机型>.toml`
//! 的 `[hardware]` 段），由 rrd 装配链序列化注入 `RR3_HAL_BINDINGS`；`RR3_HAL_*`
//! 环境变量降为**应急覆盖**（现场救场用，同键 env 赢）；代码缺省一律 **fail-closed**
//! ——描述符与应急 env 都没给的键，对应能力面落占位后端显式回错，不回落任何一块
//! 具体板的引脚（"板 = 数据"从此句成立，architecture §12.3）。
//!
//! 分层与真值：`profiles/<机>.toml` 是人写的数据事实源（rrd 解析并点名形状错误）；
//! 本模块是运行期消费端——解析注入文本、合成生效绑定，键位语义与取值域在这里裁。
//! 键集与白名单闭合（R11）：`led`/`relay` 两角色，扩 role 走契约级评审，届时同步
//! 本处、`rr3_env_override_map` 与 profiles 键位。
//! 键集冻结裁定（0.0.5 方案 §13.1 C4，评审已批J 在册）：[hardware] **不扩键**，
//! 九键维持现状——后续扩键/扩 role 同走契约级评审，届时本处与
//! rr-registry `RR3_ASSEMBLE_DESC_KEYS` 在册表**两处同批**（副本滞后 = fail-closed
//! 拒绝，见 rr-registry assemble 模块契约测试）。
//! 十键批（0.0.6 链④真件档）：MAX7219 显示面随真件落地按裁定流程扩 `spi_device`
//! 键（spidev 设备路径），rr-registry 在册表与 face 原子 assemble.toml 句柄同批。
//! 板侧配置三键批（批N4 件七(b) 基-24 桥上配置腿）：`i2c_sda`/`i2c_scl`/
//! `adc_rail_pin` 进场——板侧裸寄存器自愈路径（I2C unstick）与打印面的引脚真值，
//! rtos 档 `[hardware]` 原样直给、经桥 `rr3.set_bindings` 注入板侧（载荷键域/取值域
//! 真值单点 = rr-ipc bindings 模块，本表解析共用其 `rr3_gpio_num_parse`）。
//! 三键不在 [pins] 派生键集（无 role 对应——自愈路径没有能力面角色，rrd 门 n 不收）；
//! 扩键裁定与批N3 件六 `spi_device` 同款：契约级评审、rr-registry 在册表两处同批、
//! contract.rs 机判钉死。三键不设 `RR3_HAL_*` 应急 env 覆盖位（板侧配置随描述符
//! 走单一路径，不设第二注入面——现场救场改装配单数据 + 重启）。
//! 板侧主驱动七键批（批N9 件五，MC-7 缺③ [hardware] 进位补齐）：`uart2_tx`/
//! `uart2_rx`/`ledc_pan`/`ledc_tilt`/`i2s_bclk`/`i2s_sd`/`i2s_ws` 进场——消费语义 =
//! 板侧主驱动**装配对账锚**（esp-idf-hal 需具名引脚类型，驱动仍具名装配，注入值作
//! 对账真值，不齐板侧 ERROR 点名＋计数；批N4 件七(c) 消费面纪律同款）。同上款：
//! 不在 [pins] 派生键集、不设应急 env 覆盖位、rr-ipc/rr-registry/contract.rs
//! 三处同批。`ledc_pan`/`ledc_tilt` 与 esp32-mini [[pin]] physical 同源同值
//! （rtos 档 [pins] 派生为在册余量归 0.0.7 批G/D1——派生落位后本两键退役）。
//! 凭据存储源键批（批N17 件二，N-10 ② 键半边契约级扩批）：`cred_store` 进场——
//! **非 GPIO** 存储源标记 `nvs:<namespace>`（全案 §3.6 凭据行），取值域裁决
//! `rr3_cred_store_parse` 真值单点在 rr-ipc（键集/取值域纪律不变）；消费语义 =
//! 板侧读数面可见（GET /api/board/bindings 随键），boot 读链不消费＝批N14 架构
//! 事实（注入晚于 WiFi 起栈）、无装配对账语义（非引脚不进对账锚）。同上款：不在
//! [pins] 派生键集、不设应急 env 覆盖位、rr-ipc/rr-registry/contract.rs 三处同批。
//! OLED 键批（W5-3，Pi 显示面换装）：`oled_addr` 进场——SSD1306 的 7-bit 从地址，
//! 与既有 `i2c_bus` 两键齐备即显示面走 OLED 档；本键走 [pins] 派生轨
//! （`ui.face.i2c` 的 addr → oled_addr），取值域与 `ina219_addr` 同一裁决函数
//! （`rr3_i2c_addr_parse`，两器件共用 7-bit 地址域，不写第二套解析）。

use std::collections::BTreeMap;

use rr_ipc::{rr3_cred_store_parse, rr3_gpio_num_parse};

use crate::backend::{rr3_i2c_addr_parse, rr3_pwm_index_parse};

/// 描述符 env 键：rrd spawn 注入（装配单 `[hardware]` 的序列化文本，每行
/// `键 = "值"`）。未设 = 无描述符（`--manifest` 调试后门或纯 sim 形态）。
pub const RR3_HAL_BINDINGS_ENV: &str = "RR3_HAL_BINDINGS";

/// 一块板/形态的物理绑定（不可变快照）。`None` = 该键未给：对应能力面在选择时
/// 落占位后端（使用时以 `Unsupported` 显式回错），**没有**任何键的代码缺省。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3HalBindings {
    /// ALSA 设备名（音频采集/回放；如 `default`、`hw:0,0`）
    pub alsa_device: Option<String>,
    /// sysfs PWM 芯片号（舵机；R22 定案档参考机 = 0）
    pub pwm_chip: Option<u32>,
    /// sysfs PWM 通道号（参考机 pwm0 = GPIO12）
    pub pwm_channel: Option<u32>,
    /// chardev GPIO 芯片号（LED/继电器共挂 `/dev/gpiochipN`）
    pub gpio_chip: Option<u32>,
    /// LED 线偏移（BCM 编号；R8 预算表参考机 = 4）
    pub gpio_led_line: Option<u32>,
    /// 继电器线偏移（参考机 = 17）
    pub gpio_relay_line: Option<u32>,
    /// I2C 总线号（电池读数；参考机接线 3 = i2c1）
    pub i2c_bus: Option<u32>,
    /// INA219 从地址（7-bit，`0x` 前缀十六进制；BOM #5 参考机 = 0x40）
    pub ina219_addr: Option<u8>,
    /// INA219 分流电阻（毫欧，装配事实；缺 = 不校准、电流面不可信）
    pub ina219_shunt_mohm: Option<u32>,
    /// INA219 电流 LSB（微安/位，量程策略；与分流电阻两键齐备才校准）
    pub ina219_ilsb_ua: Option<u32>,
    /// NetworkManager 链路读数的接口名（参考机 runbook 件 4 = wlan0）
    pub link_ifname: Option<String>,
    /// spidev 设备路径（显示面 MAX7219；链④真件档，参考机 = /dev/spidev0.0）
    pub spi_device: Option<String>,
    /// I2C 显示面 SSD1306 从地址（7-bit，`0x` 前缀十六进制；Pi 真机 = 0x3C）。
    /// 与 `i2c_bus` 两键齐备 = 显示面走 OLED 档；缺一回落 `spi_device`。
    pub oled_addr: Option<u8>,
    /// I2C 数据线 GPIO 编号（板侧自愈路径；esp32-mini = 21，真值源
    /// profiles/esp32-mini.toml [hardware]，经桥 rr3.set_bindings 注入板侧）
    pub i2c_sda: Option<u8>,
    /// I2C 时钟线 GPIO 编号（板侧自愈路径；esp32-mini = 22）
    pub i2c_scl: Option<u8>,
    /// 电池分压 ADC 输入脚 GPIO 编号（板侧打印面；esp32-mini = 34）
    pub adc_rail_pin: Option<u8>,
    /// UART2 发送脚 GPIO 编号（板侧桥链路装配对账；esp32-mini = 18，批N9 件五）
    pub uart2_tx: Option<u8>,
    /// UART2 接收脚 GPIO 编号（板侧桥链路装配对账；esp32-mini = 19，批N9 件五）
    pub uart2_rx: Option<u8>,
    /// LEDC pan 轴信号脚 GPIO 编号（板侧舵机装配对账；esp32-mini = 32，批N9 件五）
    pub ledc_pan: Option<u8>,
    /// LEDC tilt 轴信号脚 GPIO 编号（板侧舵机装配对账；esp32-mini = 33，批N9 件五）
    pub ledc_tilt: Option<u8>,
    /// I2S BCLK 脚 GPIO 编号（板侧双麦装配对账；esp32-mini = 4，批N9 件五）
    pub i2s_bclk: Option<u8>,
    /// I2S SD 脚 GPIO 编号（板侧双麦装配对账；esp32-mini = 27，批N9 件五）
    pub i2s_sd: Option<u8>,
    /// I2S WS 脚 GPIO 编号（板侧双麦装配对账；esp32-mini = 26，批N9 件五）
    pub i2s_ws: Option<u8>,
    /// 凭据存储源标记 `nvs:<namespace>`（板侧读数面可见；esp32-mini = `nvs:rr3`，
    /// 批N17 件二——非 GPIO 字符串键，取值域裁决真值单点在 rr-ipc）
    pub cred_store: Option<String>,
    /// ESP32 外设桥 UART 设备路径（组合档 Pi×ESP32 宿主侧真值；如 `/dev/ttyAMA0`）。
    /// P0-3/P0-4：后端选择是部署事实，必须进装配单显式数据，不靠隐式 env。
    /// 舵机/显示/音频采集面的 esp32 后端共用此路径（Rr3UartConn flock 单消费者
    /// 纪律下由音频面独占持有，其余面走桥命令帧）。None → 应急 env
    /// `RR3_HAL_PERIPH_UART` 仍可救场（fail-closed，不猜设备）。
    pub periph_uart: Option<String>,
}

/// 非空串裁决（trim 后空 = 不认）。
fn rr3_non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 描述符键 → 落位函数（键位真值单点；`false` = 取值不认）。
type Rr3BindSet = fn(&mut Rr3HalBindings, &str) -> bool;

/// 在册键表：非空串域 4 + 索引数域 6 + GPIO 号域 10 + 两种特殊值域
/// + INA219 分流两键（正整数域）+ OLED 地址一键（7-bit 地址域）。
const RR3_BIND_SETS: [(&str, Rr3BindSet); 25] = [
    ("alsa_device", |b, s| {
        rr3_non_empty(s).map(|v| b.alsa_device = Some(v)).is_some()
    }),
    ("link_ifname", |b, s| {
        rr3_non_empty(s).map(|v| b.link_ifname = Some(v)).is_some()
    }),
    ("spi_device", |b, s| {
        rr3_non_empty(s).map(|v| b.spi_device = Some(v)).is_some()
    }),
    ("periph_uart", |b, s| {
        rr3_non_empty(s).map(|v| b.periph_uart = Some(v)).is_some()
    }),
    ("pwm_chip", |b, s| {
        rr3_pwm_index_parse(s)
            .map(|v| b.pwm_chip = Some(v))
            .is_some()
    }),
    ("pwm_channel", |b, s| {
        rr3_pwm_index_parse(s)
            .map(|v| b.pwm_channel = Some(v))
            .is_some()
    }),
    ("gpio_chip", |b, s| {
        rr3_pwm_index_parse(s)
            .map(|v| b.gpio_chip = Some(v))
            .is_some()
    }),
    ("gpio_led_line", |b, s| {
        rr3_pwm_index_parse(s)
            .map(|v| b.gpio_led_line = Some(v))
            .is_some()
    }),
    ("gpio_relay_line", |b, s| {
        rr3_pwm_index_parse(s)
            .map(|v| b.gpio_relay_line = Some(v))
            .is_some()
    }),
    ("i2c_bus", |b, s| {
        rr3_pwm_index_parse(s)
            .map(|v| b.i2c_bus = Some(v))
            .is_some()
    }),
    ("i2c_sda", |b, s| {
        rr3_gpio_num_parse(s).map(|v| b.i2c_sda = Some(v)).is_some()
    }),
    ("i2c_scl", |b, s| {
        rr3_gpio_num_parse(s).map(|v| b.i2c_scl = Some(v)).is_some()
    }),
    ("adc_rail_pin", |b, s| {
        rr3_gpio_num_parse(s)
            .map(|v| b.adc_rail_pin = Some(v))
            .is_some()
    }),
    ("uart2_tx", |b, s| {
        rr3_gpio_num_parse(s)
            .map(|v| b.uart2_tx = Some(v))
            .is_some()
    }),
    ("uart2_rx", |b, s| {
        rr3_gpio_num_parse(s)
            .map(|v| b.uart2_rx = Some(v))
            .is_some()
    }),
    ("ledc_pan", |b, s| {
        rr3_gpio_num_parse(s)
            .map(|v| b.ledc_pan = Some(v))
            .is_some()
    }),
    ("ledc_tilt", |b, s| {
        rr3_gpio_num_parse(s)
            .map(|v| b.ledc_tilt = Some(v))
            .is_some()
    }),
    ("i2s_bclk", |b, s| {
        rr3_gpio_num_parse(s)
            .map(|v| b.i2s_bclk = Some(v))
            .is_some()
    }),
    ("i2s_sd", |b, s| {
        rr3_gpio_num_parse(s).map(|v| b.i2s_sd = Some(v)).is_some()
    }),
    ("i2s_ws", |b, s| {
        rr3_gpio_num_parse(s).map(|v| b.i2s_ws = Some(v)).is_some()
    }),
    ("cred_store", |b, s| {
        let v = rr3_cred_store_parse(s);
        let ok = v.is_some();
        b.cred_store = v;
        ok
    }),
    ("ina219_addr", |b, s| {
        let v = rr3_i2c_addr_parse(s);
        let ok = v.is_some();
        b.ina219_addr = v;
        ok
    }),
    ("oled_addr", |b, s| {
        let v = rr3_i2c_addr_parse(s);
        let ok = v.is_some();
        b.oled_addr = v;
        ok
    }),
    ("ina219_shunt_mohm", |b, s| {
        rr3_pwm_index_parse(s)
            .filter(|v| *v > 0)
            .map(|v| b.ina219_shunt_mohm = Some(v))
            .is_some()
    }),
    ("ina219_ilsb_ua", |b, s| {
        rr3_pwm_index_parse(s)
            .filter(|v| *v > 0)
            .map(|v| b.ina219_ilsb_ua = Some(v))
            .is_some()
    }),
];

impl Rr3HalBindings {
    /// 单键写入：key 必须在册，raw 按该键取值域裁决。`from_text` 与
    /// [`rr3_bindings_compose`] 共用——同一套键位真值，两处不各说一套。
    fn rr3_binding_set(&mut self, key: &str, raw: &str) -> Result<(), String> {
        let Some((_, set)) = RR3_BIND_SETS.iter().find(|(k, _)| *k == key) else {
            return Err(format!(
                "描述符键 {key} 未在册（[hardware] 不得自造键位，在册键集见 descriptor.rs）"
            ));
        };
        if set(self, raw) {
            Ok(())
        } else {
            Err(format!("描述符键 {key} 取值不认: {raw:?}"))
        }
    }
}

/// 描述符文本解析（fail-closed：坏值/未知键/非字符串逐条点名，一条不静默放行）。
/// 输入形状 = rrd 注入器产物（TOML 平表、值全字符串）；空文本 = 空绑定（合法，
/// 对应装配单未给 `[hardware]` 段）。
pub fn rr3_bindings_parse(text: &str) -> Result<Rr3HalBindings, Vec<String>> {
    let v: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("描述符 TOML 解析失败: {e}")]),
    };
    let Some(t) = v.as_table() else {
        return Err(vec![format!("描述符顶层必须是 TOML 表，收到 {v:?}")]);
    };
    let mut b = Rr3HalBindings::default();
    let mut errs = Vec::new();
    rr3_bindings_each(t, &mut b, &mut errs);
    rr3_bindings_finish(b, errs)
}

/// 收尾：错误集空即交绑定，否则整册拒（收集式校验，文案次序不变）。
fn rr3_bindings_finish(
    b: Rr3HalBindings,
    errs: Vec<String>,
) -> Result<Rr3HalBindings, Vec<String>> {
    if errs.is_empty() {
        Ok(b)
    } else {
        Err(errs)
    }
}

/// 逐键收集：非字符串标量点名，键值合法性下沉到 `rr3_binding_set`。
fn rr3_bindings_each(t: &toml::Table, b: &mut Rr3HalBindings, errs: &mut Vec<String>) {
    for (k, val) in t {
        let Some(s) = val.as_str() else {
            errs.push(format!(
                "描述符键 {k} 应为字符串标量（注入器已字符串化），收到 {val:?}"
            ));
            continue;
        };
        if let Err(e) = b.rr3_binding_set(k, s) {
            errs.push(e);
        }
    }
}

/// 生效绑定合成（ADR-0018 裁定 4 的字面分层）：描述符为底，`overrides` 应急覆盖
/// （键 = 描述符键名、值 = 原始文本，同键覆盖赢）。覆盖键未在册或取值不认 →
/// `Err` 点名——应急写错必须当场听见，绝不静默落描述符值。描述符解析失败同败。
pub fn rr3_bindings_compose(
    descriptor: Option<&str>,
    overrides: &BTreeMap<String, String>,
) -> Result<Rr3HalBindings, String> {
    let mut b = rr3_desc_base(descriptor)?;
    rr3_overrides_apply(&mut b, overrides)?;
    Ok(b)
}

/// 应急覆盖逐键落到描述符底上（同键覆盖赢，写错当场点名）。
fn rr3_overrides_apply(
    b: &mut Rr3HalBindings,
    overrides: &BTreeMap<String, String>,
) -> Result<(), String> {
    for (k, raw) in overrides {
        b.rr3_binding_set(k, raw)
            .map_err(|e| format!("应急覆盖无效: {e}"))?;
    }
    Ok(())
}

/// 描述符为底：缺省/空文本即空表，解析失败整册拒。
fn rr3_desc_base(descriptor: Option<&str>) -> Result<Rr3HalBindings, String> {
    match descriptor {
        None | Some("") => Ok(Rr3HalBindings::default()),
        Some(text) => {
            rr3_bindings_parse(text).map_err(|errs| format!("描述符解析失败: {}", errs.join("; ")))
        }
    }
}

/// 读进程环境的 `RR3_HAL_*` 应急键，映射成描述符键名（compose 的 overrides 供数）。
/// role 线偏移只认白名单 `led`/`relay`（与 `[hardware]` 键集一致，R11 闭合）。
pub fn rr3_env_override_map() -> BTreeMap<String, String> {
    // 描述符为底、env 应急：表 = 绑定键 → 环境变量名（顺序只影响可读性，结果入 BTreeMap）。
    let vars = [
        ("alsa_device", crate::backend::RR3_HAL_ALSA_DEVICE_ENV),
        ("pwm_chip", crate::backend::RR3_HAL_PWM_CHIP_ENV),
        ("pwm_channel", crate::backend::RR3_HAL_PWM_CHANNEL_ENV),
        ("gpio_chip", crate::backend::RR3_HAL_GPIO_CHIP_ENV),
        ("gpio_led_line", "RR3_HAL_GPIO_LED_LINE"),
        ("gpio_relay_line", "RR3_HAL_GPIO_RELAY_LINE"),
        ("i2c_bus", crate::backend::RR3_HAL_I2C_BUS_ENV),
        ("ina219_addr", crate::backend::RR3_HAL_INA219_ADDR_ENV),
        (
            "ina219_shunt_mohm",
            crate::backend::RR3_HAL_INA219_SHUNT_ENV,
        ),
        ("ina219_ilsb_ua", crate::backend::RR3_HAL_INA219_ILSB_ENV),
        ("link_ifname", crate::backend::RR3_HAL_LINK_IFNAME_ENV),
        ("spi_device", crate::backend::RR3_HAL_SPI_DEVICE_ENV),
        ("oled_addr", crate::backend::RR3_HAL_OLED_ADDR_ENV),
        // ESP32 外设桥 UART 路径：常量直引（不经过 cfg 门控的 esp32_uart 模块，
        // 保证 Windows 等非宿主平台也能编译描述符）。
        (
            "periph_uart",
            rr_periph_proto::consts::RR3_HAL_PERIPH_UART_ENV,
        ),
    ];
    let mut m = BTreeMap::new();
    for (key, env) in vars {
        if let Some(v) = std::env::var_os(env) {
            m.insert(key.to_string(), v.to_string_lossy().into_owned());
        }
    }
    m
}

/// 进程外壳：`RR3_HAL_BINDINGS` 描述符 + `RR3_HAL_*` 应急 env 一次合成生效绑定。
/// 选择函数在 linux 分支调用它；`Err` 或键缺失 → 落占位后端显式回错（fail-closed）。
pub fn rr3_bindings_eff() -> Result<Rr3HalBindings, String> {
    let descriptor = std::env::var(RR3_HAL_BINDINGS_ENV).ok();
    rr3_bindings_compose(descriptor.as_deref(), &rr3_env_override_map())
}

#[cfg(test)]
mod tests;
