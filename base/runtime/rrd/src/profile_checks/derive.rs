//! 引脚派生机制：平台词形匹配、八物理键取值法与字段在场判定。

use rr_registry::Rr3PlatformTable;

use crate::profile::Rr3PinEntry;

/// 控制器词形解析（host 档派生规则的机制侧）：`<前缀><十进制编号>` 才算命中，返回
/// 编号串——roles.toml 注记的取值语义（pwm<N> 派生 pwm_chip=N）。前缀**住平台表**
/// `[controller_wordform]` 段（实-6 (a)，批N4 件七下移：rrd 零硬编码词形规则，
/// 换平台 = 改数据），本函数只做词形匹配机制。
pub(super) fn rr3_wordform_tail(controller: &str, prefix: &str) -> Option<String> {
    let rest = controller.strip_prefix(prefix)?;
    if !rr3_tail_digits(rest) {
        return None;
    }
    Some(rest.to_string())
}

fn rr3_tail_digits(rest: &str) -> bool {
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

/// 词形前缀查询 + fail-closed 守卫（实-6 (a)）：派生键查平台表词形前缀，查不到
/// 红——不许回退硬编码（回退 = 策略重新住回机制侧，下移作废）。
fn rr3_wordform_prefix(platform: &Rr3PlatformTable, key: &str) -> Result<String, String> {
    let Some(w) = platform.rr3_wordform(key) else {
        return Err(format!(
            "平台事实表 controller_wordform 无 {key} 词形（host 档派生前件缺失，fail-closed）"
        ));
    };
    Ok(w.to_string())
}

/// 单条 pin 的派生取值（机制侧，八物理键封闭）：role.derives（策略，contracts/
/// roles.toml）只声明"该角色供给哪些键"，取值法一律查本函数——键不在册/来源字段
/// 留空/控制器词形不合都红。gpio_led_line 与 gpio_relay_line 同取 physical[0]，由
/// 角色声明区分（两角色不共用同一派生键名，不歧义）；spi_device 由词形段前缀
/// （总线号）+ channel（片选号）合成 /dev/spidev<N>.<M>，与旧 [hardware] 直给值
/// （profiles/distro.toml 链④真件档）逐字节对齐。
pub(super) fn rr3_pin_derive_value(
    pin: &Rr3PinEntry,
    key: &str,
    platform: &Rr3PlatformTable,
) -> Result<String, String> {
    RR3_DERIVE_RULES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, derive)| derive(pin, key, platform))
        .unwrap_or_else(|| Err(format!("派生键 {key} 不在派生规则表（八物理键封闭）")))
}

/// 派生规则表：八物理键 → 取值法（同法可由多键共用）。
type Rr3DeriveFn = fn(&Rr3PinEntry, &str, &Rr3PlatformTable) -> Result<String, String>;
const RR3_DERIVE_RULES: [(&str, Rr3DeriveFn); 8] = [
    ("pwm_chip", rr3_derive_word),
    ("gpio_chip", rr3_derive_word),
    ("i2c_bus", rr3_derive_word),
    ("spi_device", rr3_derive_spi),
    ("pwm_channel", |p, _, _| rr3_derive_channel(p)),
    ("gpio_led_line", |p, _, _| rr3_derive_line(p)),
    ("gpio_relay_line", |p, _, _| rr3_derive_line(p)),
    ("ina219_addr", |p, _, _| rr3_derive_addr(p)),
];

/// 词形键派生（B9 自 pin_derive_value 抽出）：controller 必须合 `<prefix><N>`，
/// 派生值 = N 的字符串形式。
fn rr3_derive_word(
    pin: &Rr3PinEntry,
    key: &str,
    platform: &Rr3PlatformTable,
) -> Result<String, String> {
    let prefix = rr3_wordform_prefix(platform, key)?;
    rr3_wordform_tail(&pin.controller, &prefix).ok_or(format!(
        "controller 词形 {} 不合 {prefix}<N>",
        pin.controller
    ))
}

/// spi_device 派生（B9 自 pin_derive_value 抽出）：`/dev/spidev<bus>.<cs>`，
/// bus 自 controller 词形、cs 自 channel。
fn rr3_derive_spi(
    pin: &Rr3PinEntry,
    key: &str,
    platform: &Rr3PlatformTable,
) -> Result<String, String> {
    let prefix = rr3_wordform_prefix(platform, key)?;
    let Some(bus) = rr3_wordform_tail(&pin.controller, &prefix) else {
        return Err(format!(
            "controller 词形 {} 不合 {prefix}<N>",
            pin.controller
        ));
    };
    let Some(cs) = pin.channel else {
        return Err("channel 留空（SPI 片选号）".to_string());
    };
    Ok(format!("/dev/spidev{bus}.{cs}"))
}

fn rr3_derive_channel(pin: &Rr3PinEntry) -> Result<String, String> {
    let Some(ch) = pin.channel else {
        return Err("channel 留空".to_string());
    };
    Ok(ch.to_string())
}

fn rr3_derive_line(pin: &Rr3PinEntry) -> Result<String, String> {
    let Some(n) = pin.physical.first() else {
        return Err("physical 留空".to_string());
    };
    Ok(n.to_string())
}

fn rr3_derive_addr(pin: &Rr3PinEntry) -> Result<String, String> {
    if pin.addr.is_empty() {
        Err("addr 留空".to_string())
    } else {
        Ok(pin.addr.clone())
    }
}

/// 校验①-b 的"字段在场"判定（§5 缺失语义列）：String 空串 = 缺、channel 以 None =
/// 缺、physical 以空数组 = 缺。mux_group 不进 requires（互斥组由平台 conflicts 表
/// 反推，批N7 已立册）；词表外字段名由调用方点名（门 a 六表对账第 5 张的漂移）。
pub(super) fn rr3_pin_has(pin: &Rr3PinEntry, field: &str) -> bool {
    [
        ("role", !pin.role.is_empty()),
        ("instance", !pin.instance.is_empty()),
        ("controller", !pin.controller.is_empty()),
        ("channel", pin.channel.is_some()),
        ("physical", !pin.physical.is_empty()),
        ("direction", !pin.direction.is_empty()),
        ("rail", !pin.rail.is_empty()),
        ("addr", !pin.addr.is_empty()),
        ("device", !pin.device.is_empty()),
        ("failsafe", !pin.failsafe.is_empty()),
    ]
    .into_iter()
    .any(|(k, has)| k == field && has)
}
