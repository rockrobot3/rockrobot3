//! 各 `RR3_HAL_*` 环境变量取值的纯函数裁决。

/// INA219 从地址键的取值裁决（只认 `0x` 前缀十六进制——裸写有十进制歧义
/// （"40" 是 0x28 还是 40？），不收；7-bit 可寻址域 0x08..=0x77 外不认）。
pub fn rr3_ina219_parse(s: &str) -> Option<u8> {
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
pub fn rr3_gpio_line_key(role: &str) -> Option<String> {
    if role.is_empty() || !role.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(format!("RR3_HAL_GPIO_{}_LINE", role.to_ascii_uppercase()))
}

/// sim 方位角键的取值裁决（-90..=90 的有限十进制小数）。独立成纯函数：裁决可单测，
/// 选择函数里只留"读环境 + 裁决 + 缺省"。
pub fn rr3_sim_doa_parse(s: &str) -> Option<f64> {
    s.trim()
        .parse::<f64>()
        .ok()
        .filter(|d| d.is_finite() && (-90.0..=90.0).contains(d))
}

/// sim 电量键的取值裁决（0–100 十进制整数）。独立成纯函数：裁决可单测，
/// 选择函数里只留"读环境 + 裁决 + 缺省"。
pub fn rr3_sim_batt_parse(s: &str) -> Option<u8> {
    s.trim().parse::<u8>().ok().filter(|p| *p <= 100)
}

/// sim 充电态键的取值裁决（只认 `1` / `0`）。
pub fn rr3_sim_chg_parse(s: &str) -> Option<bool> {
    let known = [("1", true), ("0", false)];
    known.iter().find(|(k, _)| *k == s.trim()).map(|(_, v)| *v)
}

/// sim 链路在线态键的取值裁决（只认 `1` / `0`）。
pub fn rr3_sim_up_parse(s: &str) -> Option<bool> {
    let known = [("1", true), ("0", false)];
    known.iter().find(|(k, _)| *k == s.trim()).map(|(_, v)| *v)
}

/// sim 链路信号强度键的取值裁决（-100..=-1 的十进制整数 dBm；0 与正值、
/// 越界、垃圾一律不认，落缺省 -50）。
pub fn rr3_sim_rssi_parse(s: &str) -> Option<i8> {
    s.trim()
        .parse::<i8>()
        .ok()
        .filter(|r| (-100..=-1).contains(r))
}

/// linux PWM 芯片/通道号键的取值裁决（十进制非负整数，trim 后解析）。
pub fn rr3_pwm_index_parse(s: &str) -> Option<u32> {
    s.trim().parse::<u32>().ok()
}
