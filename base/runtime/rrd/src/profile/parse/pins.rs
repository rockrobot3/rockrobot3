//! [[pin]] 段解读：字段白名单、逐键类型与身份键 role/direction 的在场判据。

use super::super::model::*;

/// [[pin]]：可选段（schema 3，增补轮六 §4.2——纯 sim/无硬件格形态不给，此时校验②
/// 不判：无物理面可绑不等于角色缺失）。字段白名单 RR3_PIN_KEYS 十一键、类型逐键点名；
/// 身份键 role/direction 解析期即须在场（①缺失语义：role/direction 缺 → 红，判定
/// 不依赖角色表）；instance/controller 及各 requires 键的"在场与留空"在 boot 链按
/// 角色表逐条裁——本函数不带契约真值，不越权判角色语义。
pub(super) fn rr3_pins_parse(root: &toml::Value, errs: &mut Vec<String>) -> Vec<Rr3PinEntry> {
    let mut pins: Vec<Rr3PinEntry> = Vec::new();
    let Some(items) = root.get("pin").and_then(toml::Value::as_array) else {
        return pins;
    };
    for (i, item) in items.iter().enumerate() {
        let Some(t) = item.as_table() else {
            errs.push(format!("pin #{i} 须为 [[pin]] 表（数组内出现非表值）"));
            continue;
        };
        pins.push(rr3_pin_entry_parse(i, t, errs));
    }
    pins
}

/// 单条 [[pin]] 的逐键解析（含 role/direction 在场判据，顺序随键遍历）。
/// 键按类型分派：字符串面 / 数组面 / 数值面各一 helper，错误文案与逐键次序不变。
fn rr3_pin_entry_parse(i: usize, t: &toml::Table, errs: &mut Vec<String>) -> Rr3PinEntry {
    let mut pin = Rr3PinEntry::default();
    for (k, v) in t {
        match k.as_str() {
            "physical" | "mux_group" => rr3_pin_list(i, k, v, &mut pin, errs),
            "channel" => rr3_pin_num(i, v, &mut pin, errs),
            "role" | "instance" | "controller" | "direction" | "rail" | "addr" | "device"
            | "failsafe" => rr3_pin_str(i, k, v, &mut pin, errs),
            other => errs.push(format!(
                "pin #{i} 未知键 {other}（只认 {}）",
                RR3_PIN_KEYS.join("/")
            )),
        }
    }
    rr3_pin_required(i, t, errs);
    pin
}

/// 身份键在场判据：role / direction 缺一即红（顺序固定）。
fn rr3_pin_required(i: usize, t: &toml::Table, errs: &mut Vec<String>) {
    if !t.contains_key("role") {
        errs.push(format!(
            "pin #{i} 缺 role（角色外键——引脚侧唯一真值点必须挂角色）"
        ));
    }
    if !t.contains_key("direction") {
        errs.push(format!("pin #{i} 缺 direction（校验①缺失语义）"));
    }
}

/// 字符串类键（role/instance/controller/direction/rail/addr/device/failsafe）。
/// role/direction 空串与类型错同文案，其余键空串当值收（与拆分前一致）。
fn rr3_pin_str(i: usize, k: &str, v: &toml::Value, pin: &mut Rr3PinEntry, errs: &mut Vec<String>) {
    match k {
        "role" | "direction" => rr3_pin_str_req(i, k, v, pin, errs),
        "instance" | "controller" | "rail" | "addr" | "device" | "failsafe" => {
            rr3_pin_str_opt(i, k, v, pin, errs)
        }
        other => errs.push(format!(
            "pin #{i} 未知键 {other}（只认 {}）",
            RR3_PIN_KEYS.join("/")
        )),
    }
}

/// 必填非空字符串键（role/direction）：空串与类型错同文案。
fn rr3_pin_str_req(
    i: usize,
    k: &str,
    v: &toml::Value,
    pin: &mut Rr3PinEntry,
    errs: &mut Vec<String>,
) {
    match (k, v.as_str()) {
        ("role", Some(s)) if !s.is_empty() => pin.role = s.to_string(),
        ("role", _) => errs.push(format!("pin #{i}.role 缺失或为空（角色外键）")),
        ("direction", Some(s)) if !s.is_empty() => pin.direction = s.to_string(),
        ("direction", _) => errs.push(format!(
            "pin #{i}.direction 缺失或为空（校验①缺失语义：direction 缺 → 红）"
        )),
        _ => {}
    }
}

/// 可空字符串键（instance/controller/rail/addr/device/failsafe）：空串当值收。
fn rr3_pin_str_opt(
    i: usize,
    k: &str,
    v: &toml::Value,
    pin: &mut Rr3PinEntry,
    errs: &mut Vec<String>,
) {
    let Some(s) = v.as_str() else {
        errs.push(rr3_pin_str_miss(i, k));
        return;
    };
    rr3_pin_str_set(pin, k, s);
}

/// 可空字符串键的类型错文案（逐键点名，与拆分前逐字一致）。
fn rr3_pin_str_miss(i: usize, k: &str) -> String {
    match k {
        "instance" => format!("pin #{i}.instance 应为字符串"),
        "controller" => format!("pin #{i}.controller 应为字符串"),
        "rail" => format!("pin #{i}.rail 应为字符串（供电轨名；信号侧控制线显式写 \"none\"）"),
        "addr" => {
            format!("pin #{i}.addr 应为字符串（0x 前缀十六进制；\"合法不适用\"显式写 \"none\"）")
        }
        "device" => format!(
            "pin #{i}.device 应为字符串（器件型号词，外键 → contracts/devices.toml——批N3 件八）"
        ),
        _ => format!("pin #{i}.failsafe 应为字符串（release/hold/driven_low/driven_high）"),
    }
}

/// 字面值落位（可空字符串键）。
fn rr3_pin_str_set(pin: &mut Rr3PinEntry, k: &str, s: &str) {
    match k {
        "instance" => pin.instance = s.to_string(),
        "controller" => pin.controller = s.to_string(),
        "rail" => pin.rail = s.to_string(),
        "addr" => pin.addr = s.to_string(),
        "device" => pin.device = s.to_string(),
        _ => pin.failsafe = s.to_string(),
    }
}

/// 数组类键：physical（整数数组）与 mux_group（非空字符串数组）。
fn rr3_pin_list(i: usize, k: &str, v: &toml::Value, pin: &mut Rr3PinEntry, errs: &mut Vec<String>) {
    match k {
        "physical" => rr3_pin_phys(i, v, pin, errs),
        _ => rr3_pin_mux(i, v, pin, errs),
    }
}

/// `physical`：整数数组（逐项判型）。
fn rr3_pin_phys(i: usize, v: &toml::Value, pin: &mut Rr3PinEntry, errs: &mut Vec<String>) {
    let Some(a) = v.as_array() else {
        errs.push(format!("pin #{i}.physical 应为整数数组"));
        return;
    };
    for x in a {
        rr3_phys_item(i, x, pin, errs);
    }
}

/// 单个物理引脚号：整数收册，其余逐项点名。
fn rr3_phys_item(i: usize, x: &toml::Value, pin: &mut Rr3PinEntry, errs: &mut Vec<String>) {
    match x.as_integer() {
        Some(n) => pin.physical.push(n),
        None => errs.push(format!(
            "pin #{i}.physical 的每一项都必须是整数（物理引脚号）"
        )),
    }
}

/// `mux_group`：非空字符串数组（显式 [] 合法）。
fn rr3_pin_mux(i: usize, v: &toml::Value, pin: &mut Rr3PinEntry, errs: &mut Vec<String>) {
    let Some(a) = v.as_array() else {
        errs.push(format!(
            "pin #{i}.mux_group 应为字符串数组（显式 [] 合法，互斥组归平台 conflicts 表，批N7 已立册）"
        ));
        return;
    };
    for x in a {
        rr3_mux_item(i, x, pin, errs);
    }
}

/// 单个 mux_group 元素：非空字符串收册，其余逐项点名。
fn rr3_mux_item(i: usize, x: &toml::Value, pin: &mut Rr3PinEntry, errs: &mut Vec<String>) {
    match x.as_str() {
        Some(s) if !s.is_empty() => pin.mux_group.push(s.to_string()),
        _ => errs.push(format!("pin #{i}.mux_group 的每一项都必须是非空字符串")),
    }
}

/// 数值类键：channel（整数通道号）。
fn rr3_pin_num(i: usize, v: &toml::Value, pin: &mut Rr3PinEntry, errs: &mut Vec<String>) {
    match v.as_integer() {
        Some(n) => pin.channel = Some(n),
        None => errs.push(format!("pin #{i}.channel 应为整数")),
    }
}
