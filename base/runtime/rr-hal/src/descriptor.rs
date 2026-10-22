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
//! 本处、`rr3_hal_env_override_map` 与 profiles 键位。
//! 键集冻结裁定（0.0.5 方案 §13.1 C4，评审已批J 在册）：[hardware] **不扩键**，
//! 九键维持现状——后续扩键/扩 role 同走契约级评审，届时本处与
//! rr-registry `RR3_ASSEMBLE_DESC_KEYS` 在册表**两处同批**（副本滞后 = fail-closed
//! 拒绝，见 rr-registry assemble 模块契约测试）。
//! 十键批（0.0.6 链④真件档）：MAX7219 显示面随真件落地按裁定流程扩 `spi_device`
//! 键（spidev 设备路径），rr-registry 在册表与 face 原子 assemble.toml 句柄同批。
//! 板侧配置三键批（批N4 件七(b) 基-24 桥上配置腿）：`i2c_sda`/`i2c_scl`/
//! `adc_rail_pin` 进场——板侧裸寄存器自愈路径（I2C unstick）与打印面的引脚真值，
//! rtos 档 `[hardware]` 原样直给、经桥 `rr3.set_bindings` 注入板侧（载荷键域/取值域
//! 真值单点 = rr-ipc bindings 模块，本表解析共用其 `rr3_gpio_number_parse`）。
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

use std::collections::BTreeMap;

use rr_ipc::{rr3_cred_store_parse, rr3_gpio_number_parse};

use crate::backend::{rr3_ina219_addr_parse, rr3_pwm_index_parse};

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
    /// NetworkManager 链路读数的接口名（参考机 runbook 件 4 = wlan0）
    pub link_ifname: Option<String>,
    /// spidev 设备路径（显示面 MAX7219；链④真件档，参考机 = /dev/spidev0.0）
    pub spi_device: Option<String>,
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

impl Rr3HalBindings {
    /// 单键写入：key 必须在册，raw 按该键取值域裁决。`from_text` 与
    /// [`rr3_hal_bindings_compose`] 共用——同一套键位真值，两处不各说一套。
    fn rr3_binding_set(&mut self, key: &str, raw: &str) -> Result<(), String> {
        let bad = || format!("描述符键 {key} 取值不认: {raw:?}");
        match key {
            "alsa_device" => self.alsa_device = Some(rr3_non_empty(raw).ok_or_else(bad)?),
            "link_ifname" => self.link_ifname = Some(rr3_non_empty(raw).ok_or_else(bad)?),
            "spi_device" => self.spi_device = Some(rr3_non_empty(raw).ok_or_else(bad)?),
            "pwm_chip" => self.pwm_chip = Some(rr3_pwm_index_parse(raw).ok_or_else(bad)?),
            "pwm_channel" => self.pwm_channel = Some(rr3_pwm_index_parse(raw).ok_or_else(bad)?),
            "gpio_chip" => self.gpio_chip = Some(rr3_pwm_index_parse(raw).ok_or_else(bad)?),
            "gpio_led_line" => self.gpio_led_line = Some(rr3_pwm_index_parse(raw).ok_or_else(bad)?),
            "gpio_relay_line" => {
                self.gpio_relay_line = Some(rr3_pwm_index_parse(raw).ok_or_else(bad)?)
            }
            "i2c_bus" => self.i2c_bus = Some(rr3_pwm_index_parse(raw).ok_or_else(bad)?),
            "i2c_sda" => self.i2c_sda = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "i2c_scl" => self.i2c_scl = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "adc_rail_pin" => self.adc_rail_pin = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "uart2_tx" => self.uart2_tx = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "uart2_rx" => self.uart2_rx = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "ledc_pan" => self.ledc_pan = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "ledc_tilt" => self.ledc_tilt = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "i2s_bclk" => self.i2s_bclk = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "i2s_sd" => self.i2s_sd = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "i2s_ws" => self.i2s_ws = Some(rr3_gpio_number_parse(raw).ok_or_else(bad)?),
            "cred_store" => self.cred_store = Some(rr3_cred_store_parse(raw).ok_or_else(bad)?),
            "ina219_addr" => self.ina219_addr = Some(rr3_ina219_addr_parse(raw).ok_or_else(bad)?),
            other => {
                return Err(format!(
                    "描述符键 {other} 未在册（[hardware] 不得自造键位，在册键集见 descriptor.rs）"
                ))
            }
        }
        Ok(())
    }
}

/// 描述符文本解析（fail-closed：坏值/未知键/非字符串逐条点名，一条不静默放行）。
/// 输入形状 = rrd 注入器产物（TOML 平表、值全字符串）；空文本 = 空绑定（合法，
/// 对应装配单未给 `[hardware]` 段）。
pub fn rr3_hal_bindings_from_text(text: &str) -> Result<Rr3HalBindings, Vec<String>> {
    let v: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("描述符 TOML 解析失败: {e}")]),
    };
    let Some(t) = v.as_table() else {
        return Err(vec![format!("描述符顶层必须是 TOML 表，收到 {v:?}")]);
    };
    let mut b = Rr3HalBindings::default();
    let mut errs = Vec::new();
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
    if errs.is_empty() {
        Ok(b)
    } else {
        Err(errs)
    }
}

/// 生效绑定合成（ADR-0018 裁定 4 的字面分层）：描述符为底，`overrides` 应急覆盖
/// （键 = 描述符键名、值 = 原始文本，同键覆盖赢）。覆盖键未在册或取值不认 →
/// `Err` 点名——应急写错必须当场听见，绝不静默落描述符值。描述符解析失败同败。
pub fn rr3_hal_bindings_compose(
    descriptor: Option<&str>,
    overrides: &BTreeMap<String, String>,
) -> Result<Rr3HalBindings, String> {
    let mut b = match descriptor {
        None | Some("") => Rr3HalBindings::default(),
        Some(text) => rr3_hal_bindings_from_text(text)
            .map_err(|errs| format!("描述符解析失败: {}", errs.join("; ")))?,
    };
    for (k, raw) in overrides {
        b.rr3_binding_set(k, raw)
            .map_err(|e| format!("应急覆盖无效: {e}"))?;
    }
    Ok(b)
}

/// 读进程环境的 `RR3_HAL_*` 应急键，映射成描述符键名（compose 的 overrides 供数）。
/// role 线偏移只认白名单 `led`/`relay`（与 `[hardware]` 键集一致，R11 闭合）。
pub fn rr3_hal_env_override_map() -> BTreeMap<String, String> {
    fn put(m: &mut BTreeMap<String, String>, key: &str, env: &str) {
        if let Some(v) = std::env::var_os(env) {
            m.insert(key.to_string(), v.to_string_lossy().into_owned());
        }
    }
    let mut m = BTreeMap::new();
    put(
        &mut m,
        "alsa_device",
        crate::backend::RR3_HAL_ALSA_DEVICE_ENV,
    );
    put(&mut m, "pwm_chip", crate::backend::RR3_HAL_PWM_CHIP_ENV);
    put(
        &mut m,
        "pwm_channel",
        crate::backend::RR3_HAL_PWM_CHANNEL_ENV,
    );
    put(&mut m, "gpio_chip", crate::backend::RR3_HAL_GPIO_CHIP_ENV);
    put(&mut m, "gpio_led_line", "RR3_HAL_GPIO_LED_LINE");
    put(&mut m, "gpio_relay_line", "RR3_HAL_GPIO_RELAY_LINE");
    put(&mut m, "i2c_bus", crate::backend::RR3_HAL_I2C_BUS_ENV);
    put(
        &mut m,
        "ina219_addr",
        crate::backend::RR3_HAL_INA219_ADDR_ENV,
    );
    put(
        &mut m,
        "link_ifname",
        crate::backend::RR3_HAL_LINK_IFNAME_ENV,
    );
    put(&mut m, "spi_device", crate::backend::RR3_HAL_SPI_DEVICE_ENV);
    m
}

/// 进程外壳：`RR3_HAL_BINDINGS` 描述符 + `RR3_HAL_*` 应急 env 一次合成生效绑定。
/// 选择函数在 linux 分支调用它；`Err` 或键缺失 → 落占位后端显式回错（fail-closed）。
pub fn rr3_hal_bindings_effective() -> Result<Rr3HalBindings, String> {
    let descriptor = std::env::var(RR3_HAL_BINDINGS_ENV).ok();
    rr3_hal_bindings_compose(descriptor.as_deref(), &rr3_hal_env_override_map())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// rrd 注入器产物形状的全键正例：十四键逐字段落位（值全字符串——注入器已
    /// 字符串化；原十三键 = 十 host 键 + 批N4 三键，批N17 件二随键集补 cred_store；
    /// 批N9 七键不在本测试覆盖面，键域由 entry/rr-ipc 侧单测钉）。
    #[test]
    fn full_descriptor_parses_all_fourteen_keys() {
        let text = concat!(
            "alsa_device = \"default\"\n",
            "pwm_chip = \"0\"\n",
            "pwm_channel = \"0\"\n",
            "gpio_chip = \"0\"\n",
            "gpio_led_line = \"4\"\n",
            "gpio_relay_line = \"17\"\n",
            "i2c_bus = \"1\"\n",
            "ina219_addr = \"0x40\"\n",
            "link_ifname = \"wlan0\"\n",
            "spi_device = \"/dev/spidev0.0\"\n",
            "i2c_sda = \"21\"\n",
            "i2c_scl = \"22\"\n",
            "adc_rail_pin = \"34\"\n",
            "cred_store = \"nvs:rr3\"\n",
        );
        let b = rr3_hal_bindings_from_text(text).expect("全键正例必须解析");
        assert_eq!(b.alsa_device.as_deref(), Some("default"));
        assert_eq!(b.pwm_chip, Some(0));
        assert_eq!(b.pwm_channel, Some(0));
        assert_eq!(b.gpio_chip, Some(0));
        assert_eq!(b.gpio_led_line, Some(4));
        assert_eq!(b.gpio_relay_line, Some(17));
        assert_eq!(b.i2c_bus, Some(1));
        assert_eq!(b.ina219_addr, Some(0x40));
        assert_eq!(b.link_ifname.as_deref(), Some("wlan0"));
        assert_eq!(b.spi_device.as_deref(), Some("/dev/spidev0.0"));
        assert_eq!(b.i2c_sda, Some(21));
        assert_eq!(b.i2c_scl, Some(22));
        assert_eq!(b.adc_rail_pin, Some(34));
        assert_eq!(b.cred_store.as_deref(), Some("nvs:rr3"));
    }

    /// 空文本 = 空绑定（合法，装配单未给 [hardware]；纯 sim 形态照常起）。
    #[test]
    fn empty_text_is_empty_bindings() {
        assert_eq!(
            rr3_hal_bindings_from_text("").expect("空文本必须合法"),
            Rr3HalBindings::default()
        );
    }

    /// fail-closed 点名：未知键、坏值（十进制域/裸十六进制/空串/越 u8/非 nvs:
    /// 前缀）、非字符串值，一次全收集不静默放行。
    #[test]
    fn unknown_keys_and_bad_values_are_all_named() {
        let text = concat!(
            "pwm_chip = \"abc\"\n",
            "i2c_bus = \"-1\"\n",
            "ina219_addr = \"40\"\n",    // 裸写有十进制歧义，只认 0x 前缀
            "alsa_device = \"  \"\n",    // trim 后空 = 不认
            "mystery_key = \"x\"\n",
            "link_ifname = 3\n",         // 非字符串标量
            "i2c_sda = \"256\"\n",       // 板侧键取值域 = u8，越域即红
            "cred_store = \"flash:boot\"\n", // 非 nvs: 前缀，批N17 件二
        );
        let errs = rr3_hal_bindings_from_text(text).expect_err("坏值必须报错");
        assert_eq!(errs.len(), 8, "{errs:?}");
        assert!(
            errs.iter().any(|e| e.contains("pwm_chip 取值不认")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("i2c_bus 取值不认")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("ina219_addr 取值不认")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("alsa_device 取值不认")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("mystery_key 未在册")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("link_ifname 应为字符串标量")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("i2c_sda 取值不认")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("cred_store 取值不认")),
            "{errs:?}"
        );
    }

    /// compose 分层：描述符为底、overrides 同键覆盖赢、overrides 也能补描述符没给的键。
    #[test]
    fn compose_overrides_win_over_descriptor() {
        let desc = "pwm_chip = \"0\"\ngpio_led_line = \"4\"\n";
        let mut ov = BTreeMap::new();
        ov.insert("pwm_chip".to_string(), "1".to_string());
        ov.insert("link_ifname".to_string(), "mlan0".to_string());
        let b = rr3_hal_bindings_compose(Some(desc), &ov).expect("合成必须成功");
        assert_eq!(b.pwm_chip, Some(1), "同键应急覆盖赢");
        assert_eq!(b.gpio_led_line, Some(4), "描述符未被覆盖的键原样保留");
        assert_eq!(
            b.link_ifname.as_deref(),
            Some("mlan0"),
            "覆盖可补描述符缺键"
        );
    }

    /// compose fail-closed：坏描述符合并成一条 Err；overrides 未知键/坏值点名；
    /// 无描述符 + 空 overrides = 空绑定（不是某板缺省）。
    #[test]
    fn compose_fails_closed_and_names_the_cause() {
        let e = rr3_hal_bindings_compose(Some("pwm_chip = \"x\"\n"), &BTreeMap::new())
            .expect_err("坏描述符必须败");
        assert!(
            e.contains("描述符解析失败") && e.contains("pwm_chip"),
            "{e}"
        );

        let mut ov = BTreeMap::new();
        ov.insert("nope".to_string(), "1".to_string());
        let e = rr3_hal_bindings_compose(None, &ov).expect_err("未知覆盖键必须败");
        assert!(
            e.contains("应急覆盖无效") && e.contains("nope 未在册"),
            "{e}"
        );

        let mut ov = BTreeMap::new();
        ov.insert("pwm_chip".to_string(), "1.5".to_string());
        let e = rr3_hal_bindings_compose(None, &ov).expect_err("坏覆盖值必须败");
        assert!(
            e.contains("应急覆盖无效") && e.contains("pwm_chip 取值不认"),
            "{e}"
        );

        let b = rr3_hal_bindings_compose(None, &BTreeMap::new()).expect("空合成必须合法");
        assert_eq!(
            b,
            Rr3HalBindings::default(),
            "无描述符无覆盖 = 空绑定（fail-closed 不回落某板）"
        );
    }
}
