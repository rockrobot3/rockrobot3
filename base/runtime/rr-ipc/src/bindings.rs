//! 板侧绑定载荷真值（批N4 件七(b) 基-24 桥上配置腿）。
//!
//! 桥上配置的机制/策略分离：**键集与取值域的真值单点在本模块**（no_std 纯 core，
//! 宿主 rr-hal 下发腿与板侧 esp32-periph-base 接收腿两侧消费——板侧不依赖
//! rr-hal/rr-bus/rrd（门 ⑨-w），宿主不依赖板侧 crate，rr-ipc 是两者共同依赖的
//! 汇合点）；**载荷的 JSON 遍历与 method 分发留在两侧各自消费面**（本 crate 不引
//! serde_json——no_std 纪律，JSON 机制不是协议层语义）。载荷形状（两侧同守，
//! 漂移即 fail-closed 点名）：
//!
//! ```json
//! {"i2c_sda": "21", "i2c_scl": "22", "adc_rail_pin": "34", "uart2_tx": "18", ...}
//! ```
//!
//! - method 面（不走 topic 面——门 ⑨-d topics 生成物约束）：[`RR3_METHOD_SET_BINDINGS`]，
//!   载荷住 `params.args`（无主题语义，topic 缺省空串——宿主 `rr3.call` 形状的
//!   method 面推广）。
//! - 值全字符串标量（描述符同形纪律：注入文本 TOML 平表 `键 = "值"`，JSON 侧不换形）。
//! - 键出域/值不认 = fail-closed 点名（板侧拒收折 `result.error`，不静默丢弃）。
//! - 注入快照 [`Rr3BoardBindings`] 的 `None` = 未注入：**消费面 fail-closed 拒**
//!   （板侧自愈路径读不到即拒，不回退任何硬编码——"板 = 数据"的桥上腿）。
//!
//! 键位批史：批N4 件七(b) 三键（自愈/观测面）；批N9 件五七键（板侧主驱动引脚
//! 真值——MC-7 缺③ [hardware] 进位补齐，消费语义 = 板侧主驱动**装配对账锚**：
//! esp-idf-hal 需具名引脚类型，驱动仍具名装配，注入值作对账真值，不齐板侧
//! ERROR 点名＋计数）；批N17 件一 `cred_store` 键（**非 GPIO**——存储源标记
//! `nvs:<namespace>`，全案 §3.6 凭据行；消费语义 = 板侧读数面可见，boot 读链
//! 不消费＝批N14 架构事实注入晚于 WiFi 起栈，无装配对账语义＝非引脚不进对账
//! 锚）。十键是 GPIO 编号（取值域共用 [`rr3_gpio_number_parse`]），cred_store
//! 取值域独立裁决 [`rr3_cred_store_parse`]（同在本模块——键集与取值域真值单点
//! 纪律不变）。

// no_std + alloc（crate 根已 extern crate alloc）：String/format 走 alloc 面
//（std 目标上与 std::string::String 同一实现，零行为差——lib.rs G1 注记同径）。
use alloc::format;
use alloc::string::String;

/// `rr3.set_bindings` 的 method 名（method 面，不走 topic 面——门 ⑨-d topics
/// 生成物约束；宿主 esp32_uart 下发与板侧 frame 分发共用本真值，漂移即协议断路）。
pub const RR3_METHOD_SET_BINDINGS: &str = "rr3.set_bindings";

/// I2C 数据线 GPIO 编号键（板侧裸寄存器自愈路径消费）
pub const RR3_BOARD_BINDING_I2C_SDA: &str = "i2c_sda";

/// I2C 时钟线 GPIO 编号键（板侧裸寄存器自愈路径消费）
pub const RR3_BOARD_BINDING_I2C_SCL: &str = "i2c_scl";

/// 电池分压 ADC 输入脚 GPIO 编号键（板侧打印面/轨监视消费）
pub const RR3_BOARD_BINDING_ADC_RAIL_PIN: &str = "adc_rail_pin";

/// UART2 发送脚 GPIO 编号键（桥链路面驱动装配对账；批N9 件五）
pub const RR3_BOARD_BINDING_UART2_TX: &str = "uart2_tx";

/// UART2 接收脚 GPIO 编号键（桥链路面驱动装配对账；批N9 件五）
pub const RR3_BOARD_BINDING_UART2_RX: &str = "uart2_rx";

/// LEDC pan（水平）轴信号脚 GPIO 编号键（舵机驱动装配对账；批N9 件五——与
/// profiles/esp32-mini.toml [[pin]] physical 同源同值，rtos 档 [pins] 派生落位后退役）
pub const RR3_BOARD_BINDING_LEDC_PAN: &str = "ledc_pan";

/// LEDC tilt（俯仰）轴信号脚 GPIO 编号键（同 [`RR3_BOARD_BINDING_LEDC_PAN`]）
pub const RR3_BOARD_BINDING_LEDC_TILT: &str = "ledc_tilt";

/// I2S BCLK 位时钟脚 GPIO 编号键（双麦采集驱动装配对账；批N9 件五）
pub const RR3_BOARD_BINDING_I2S_BCLK: &str = "i2s_bclk";

/// I2S SD 数据脚 GPIO 编号键（双麦采集驱动装配对账；批N9 件五）
pub const RR3_BOARD_BINDING_I2S_SD: &str = "i2s_sd";

/// I2S WS 字选择脚 GPIO 编号键（双麦采集驱动装配对账；批N9 件五）
pub const RR3_BOARD_BINDING_I2S_WS: &str = "i2s_ws";

/// 凭据存储源标记键（**非 GPIO**——全案 §3.6 凭据行，批N17 件一）：值形
/// `nvs:<namespace>`（如 `nvs:rr3`），声明"凭据在哪存"而非凭据值本身——SSID/
/// 密码零回显契约不涉。消费语义 = 板侧读数面可见（GET /api/board/bindings
/// 随键）；boot 读链不消费（批N14 架构事实：桥上注入晚于 WiFi 起栈）；无装配
/// 对账语义（非引脚，不进板侧对账锚）。
pub const RR3_BOARD_BINDING_CRED_STORE: &str = "cred_store";

/// 板侧绑定键在册表（载荷键域封闭判据；扩键走契约级评审并三处同批——rr-hal
/// `Rr3HalBindings` 字段集与 rr-registry `RR3_ASSEMBLE_DESC_KEYS` 由
/// contract.rs 机判钉死，本表是桥上载荷键域的第三面真值，三处同批）。
pub const RR3_BOARD_BINDING_KEYS: [&str; 11] = [
    RR3_BOARD_BINDING_I2C_SDA,
    RR3_BOARD_BINDING_I2C_SCL,
    RR3_BOARD_BINDING_ADC_RAIL_PIN,
    RR3_BOARD_BINDING_UART2_TX,
    RR3_BOARD_BINDING_UART2_RX,
    RR3_BOARD_BINDING_LEDC_PAN,
    RR3_BOARD_BINDING_LEDC_TILT,
    RR3_BOARD_BINDING_I2S_BCLK,
    RR3_BOARD_BINDING_I2S_SD,
    RR3_BOARD_BINDING_I2S_WS,
    RR3_BOARD_BINDING_CRED_STORE,
];

/// 板侧注入绑定快照（不可变数据）：`None` = 该键未注入，消费面 fail-closed 拒。
/// 无任何硬编码缺省——板侧引脚真值唯一来源 = 装配单 `[hardware]` 经桥注入。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3BoardBindings {
    /// I2C 数据线 GPIO 编号（esp32-mini = 21）
    pub i2c_sda: Option<u8>,
    /// I2C 时钟线 GPIO 编号（esp32-mini = 22）
    pub i2c_scl: Option<u8>,
    /// 电池分压 ADC 输入脚 GPIO 编号（esp32-mini = 34）
    pub adc_rail_pin: Option<u8>,
    /// UART2 发送脚 GPIO 编号（esp32-mini = 18；批N9 件五）
    pub uart2_tx: Option<u8>,
    /// UART2 接收脚 GPIO 编号（esp32-mini = 19；批N9 件五）
    pub uart2_rx: Option<u8>,
    /// LEDC pan 轴信号脚 GPIO 编号（esp32-mini = 32；批N9 件五）
    pub ledc_pan: Option<u8>,
    /// LEDC tilt 轴信号脚 GPIO 编号（esp32-mini = 33；批N9 件五）
    pub ledc_tilt: Option<u8>,
    /// I2S BCLK 脚 GPIO 编号（esp32-mini = 4；批N9 件五）
    pub i2s_bclk: Option<u8>,
    /// I2S SD 脚 GPIO 编号（esp32-mini = 27；批N9 件五）
    pub i2s_sd: Option<u8>,
    /// I2S WS 脚 GPIO 编号（esp32-mini = 26；批N9 件五）
    pub i2s_ws: Option<u8>,
    /// 凭据存储源标记 `nvs:<namespace>`（esp32-mini = `nvs:rr3`；批N17 件一——
    /// 非 GPIO 字符串键，取值域见 [`rr3_cred_store_parse`]）
    pub cred_store: Option<String>,
}

impl Rr3BoardBindings {
    /// I2C 自愈成对引脚：sda/scl 都注入才成对（缺一即 `None`——半份配置不构成
    /// 可执行的自愈前件，fail-closed 交给消费面点名，不猜不造）。
    pub fn rr3_board_bindings_i2c(&self) -> Option<(u8, u8)> {
        Some((self.i2c_sda?, self.i2c_scl?))
    }
}

/// GPIO 编号取值域裁决：十进制 u8（trim 宽容）。真值单点——rr-hal 描述符解析
/// （`i2c_sda` 等三键）与板侧载荷解析共用，两侧取值域漂移即此处单改。
pub fn rr3_gpio_number_parse(s: &str) -> Option<u8> {
    s.trim().parse::<u8>().ok()
}

/// 凭据存储源取值域裁决（批N17 件一）：`nvs:` 前缀＋非空 namespace（trim 宽容
/// 整体，前缀后 namespace 原样不 trim——namespace 含空白不认，fail-closed）。
/// 真值单点——rr-hal 描述符臂、桥上载荷臂与板侧消费面共用，漂移即此处单改。
/// 返回 Some 归一形 `nvs:<namespace>`（前缀大小写敏感，只认小写——TOML 字面
/// 纪律同描述符全字符串值不换形）。
pub fn rr3_cred_store_parse(s: &str) -> Option<String> {
    let s = s.trim();
    let ns = s.strip_prefix("nvs:")?;
    if ns.is_empty() {
        return None;
    }
    Some(format!("nvs:{ns}"))
}

/// 单条目写入（载荷遍历侧逐条调用）：键必须在册、值过取值域裁决；坏条目**不
/// 半写入**（先裁决后赋值）。独立纯函数：键域/值域可单测。
pub fn rr3_board_binding_entry(b: &mut Rr3BoardBindings, k: &str, v: &str) -> Result<(), String> {
    let bad = || format!("板侧绑定键 {k} 取值不认: {v:?}");
    match k {
        RR3_BOARD_BINDING_I2C_SDA => b.i2c_sda = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?),
        RR3_BOARD_BINDING_I2C_SCL => b.i2c_scl = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?),
        RR3_BOARD_BINDING_ADC_RAIL_PIN => {
            b.adc_rail_pin = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?)
        }
        RR3_BOARD_BINDING_UART2_TX => b.uart2_tx = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?),
        RR3_BOARD_BINDING_UART2_RX => b.uart2_rx = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?),
        RR3_BOARD_BINDING_LEDC_PAN => b.ledc_pan = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?),
        RR3_BOARD_BINDING_LEDC_TILT => {
            b.ledc_tilt = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?)
        }
        RR3_BOARD_BINDING_I2S_BCLK => {
            b.i2s_bclk = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?)
        }
        RR3_BOARD_BINDING_I2S_SD => b.i2s_sd = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?),
        RR3_BOARD_BINDING_I2S_WS => b.i2s_ws = Some(rr3_gpio_number_parse(v).ok_or_else(bad)?),
        RR3_BOARD_BINDING_CRED_STORE => {
            b.cred_store = Some(rr3_cred_store_parse(v).ok_or_else(bad)?)
        }
        other => {
            return Err(format!(
                "板侧绑定键 {other} 未在册（载荷键域见 bindings.rs 在册表）"
            ))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GPIO 编号取值域：十进制 u8、trim 宽容；负数/越域/非十进制全拒。
    #[test]
    fn gpio_number_parse_domain() {
        assert_eq!(rr3_gpio_number_parse("21"), Some(21));
        assert_eq!(rr3_gpio_number_parse(" 34 "), Some(34));
        assert_eq!(rr3_gpio_number_parse("0"), Some(0));
        assert_eq!(rr3_gpio_number_parse("255"), Some(255));
        assert_eq!(
            rr3_gpio_number_parse("+2"),
            Some(2),
            "u8 parse 标准语义收前导加号"
        );
        for bad in ["", " ", "-1", "256", "0x15", "21x", "1.5"] {
            assert_eq!(rr3_gpio_number_parse(bad), None, "{bad}");
        }
    }

    /// 单条目写入：十一键逐个入位（批N4 三键 + 批N9 件五七键 + 批N17 件一
    /// cred_store）；键出域/坏值点名即红，且红条目不半写入。
    #[test]
    fn entry_fills_known_keys_and_names_unknown() {
        let mut b = Rr3BoardBindings::default();
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_I2C_SDA, "21").expect("sda 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_I2C_SCL, " 22 ")
            .expect("trim 宽容必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_ADC_RAIL_PIN, "34")
            .expect("rail 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_UART2_TX, "18").expect("tx 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_UART2_RX, "19").expect("rx 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_LEDC_PAN, "32").expect("pan 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_LEDC_TILT, "33").expect("tilt 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_I2S_BCLK, "4").expect("bclk 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_I2S_SD, "27").expect("sd 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_I2S_WS, "26").expect("ws 必须入位");
        rr3_board_binding_entry(&mut b, RR3_BOARD_BINDING_CRED_STORE, "nvs:rr3")
            .expect("cred_store 必须入位");
        assert_eq!(b.rr3_board_bindings_i2c(), Some((21, 22)));
        assert_eq!(b.adc_rail_pin, Some(34));
        assert_eq!(b.uart2_tx, Some(18));
        assert_eq!(b.uart2_rx, Some(19));
        assert_eq!(b.ledc_pan, Some(32));
        assert_eq!(b.ledc_tilt, Some(33));
        assert_eq!(b.i2s_bclk, Some(4));
        assert_eq!(b.i2s_sd, Some(27));
        assert_eq!(b.i2s_ws, Some(26));
        assert_eq!(b.cred_store.as_deref(), Some("nvs:rr3"));

        let mut b2 = Rr3BoardBindings::default();
        let e = rr3_board_binding_entry(&mut b2, "gpio_line", "4").expect_err("键出域必须红");
        assert!(e.contains("gpio_line 未在册"), "{e}");
        let e = rr3_board_binding_entry(&mut b2, RR3_BOARD_BINDING_I2C_SDA, "0x15")
            .expect_err("坏值必须红");
        assert!(e.contains("i2c_sda 取值不认"), "{e}");
        let e = rr3_board_binding_entry(&mut b2, RR3_BOARD_BINDING_I2C_SCL, "256")
            .expect_err("越 u8 必须红");
        assert!(e.contains("i2c_scl 取值不认"), "{e}");
        let e = rr3_board_binding_entry(&mut b2, RR3_BOARD_BINDING_CRED_STORE, "flash:boot")
            .expect_err("非 nvs: 前缀必须红");
        assert!(e.contains("cred_store 取值不认"), "{e}");
        let e = rr3_board_binding_entry(&mut b2, RR3_BOARD_BINDING_CRED_STORE, "nvs:")
            .expect_err("空 namespace 必须红");
        assert!(e.contains("cred_store 取值不认"), "{e}");
        assert_eq!(b2, Rr3BoardBindings::default(), "红条目不得半写入");
    }

    /// 凭据存储源取值域（批N17 件一）：`nvs:` 前缀＋非空 namespace；整体 trim
    /// 宽容，namespace 原样不 trim；前缀大小写敏感、空 namespace/无前缀/空值
    /// 全拒（fail-closed）。
    #[test]
    fn cred_store_parse_domain() {
        assert_eq!(
            rr3_cred_store_parse("nvs:rr3").as_deref(),
            Some("nvs:rr3")
        );
        assert_eq!(
            rr3_cred_store_parse(" nvs:rr3 ").as_deref(),
            Some("nvs:rr3"),
            "整体 trim 宽容"
        );
        assert_eq!(
            rr3_cred_store_parse("nvs:rr3-cred").as_deref(),
            Some("nvs:rr3-cred"),
            "namespace 含连字符合法"
        );
        for bad in ["", " ", "nvs:", "nvs: ", "NVS:rr3", "flash:boot", "rr3", "nvs"] {
            assert_eq!(rr3_cred_store_parse(bad), None, "{bad}");
        }
    }

    /// I2C 成对语义：sda/scl 缺一即 None（半份配置不成对——消费面 fail-closed 的
    /// 前件裁决），两侧齐才 Some。
    #[test]
    fn i2c_pair_requires_both_sides() {
        let mut b = Rr3BoardBindings::default();
        assert_eq!(b.rr3_board_bindings_i2c(), None);
        b.i2c_sda = Some(21);
        assert_eq!(b.rr3_board_bindings_i2c(), None, "半份配置不成对");
        b.i2c_scl = Some(22);
        assert_eq!(b.rr3_board_bindings_i2c(), Some((21, 22)));
    }

    /// 在册表覆盖十一常量且恰十一键（键域封闭的表级锚——扩键先扩本表再扩消费面；
    /// 批N4 件七(b) 三键 + 批N9 件五七键 + 批N17 件一 cred_store，次序 = 批史）。
    #[test]
    fn keys_table_is_exactly_the_eleven_consts() {
        assert_eq!(
            RR3_BOARD_BINDING_KEYS,
            [
                "i2c_sda",
                "i2c_scl",
                "adc_rail_pin",
                "uart2_tx",
                "uart2_rx",
                "ledc_pan",
                "ledc_tilt",
                "i2s_bclk",
                "i2s_sd",
                "i2s_ws",
                "cred_store",
            ]
        );
    }
}
