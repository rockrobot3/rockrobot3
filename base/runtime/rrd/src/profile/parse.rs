//! 装配单文本解读（B9 结构收口）：`toml::Value` → 模型，形状错误全收集。
//!
//! 段序即错误追加序（[os] → [platform] → [runtime] → [supplies] → [slots] →
//! [bindings] → [enabled_slots] → [hardware] → [pins]）；一条形状问题一条点名，
//! 不静默放行。查册、缺格、档位一致性等**判据**不在这里（归 `profile_checks.rs`），
//! 本文件只回答"文本说了什么"。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::model::*;

/// 装配单路径解析成绝对路径：装载与横幅凭证共用同一个解析结果。
/// 装配单**必须已存在**才可装载，所以用 `fs::canonicalize`（与主题契约/叠加层同工具、
/// 同一条"致命文案不漏 `..`"的理据，见 `topics_load::rr3_contract_abs`）。
pub fn rr3_profile_abs(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map_err(|e| format!("路径 {} 无法解析为绝对路径: {e}", path.display()))
}

/// 解析装配单文本。形状错误全部收集完才返回（运维一次改对），逐条点名、不静默放行。
/// 各段解析下沉到 `rr3_*_parse`（错误追加顺序＝段序，文案逐字不变）。
pub fn rr3_profile_parse(text: &str) -> Result<Rr3Profile, Vec<String>> {
    let root: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("装配单 TOML 解析失败: {e}")]),
    };
    let mut errs = Vec::new();
    match root.get("schema_version").and_then(toml::Value::as_integer) {
        Some(v) if v == RR3_PROFILE_SCHEMA_VERSION => {}
        other => errs.push(format!(
            "schema_version 必须为 {RR3_PROFILE_SCHEMA_VERSION}，收到 {other:?}"
        )),
    }
    let bindings = rr3_bindings_parse(&root, &mut errs);
    let enabled_slots = rr3_enabled_parse(&root, &mut errs);
    let hardware = rr3_hardware_parse(&root, &mut errs);
    let pins = rr3_pins_parse(&root, &mut errs);
    let (platform_tier, platform_chip) = rr3_platform_parse(&root, &mut errs);
    let rt = rr3_runtime_parse(&root, &mut errs);
    let supplies = rr3_supplies_parse(&root, &platform_tier, &mut errs);
    let (slots_present, slots_device_absent) = rr3_slots_parse(&root, &mut errs);
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Rr3Profile {
        platform_tier,
        platform_chip,
        bindings,
        enabled_slots,
        slots_present,
        slots_device_absent,
        supplies,
        hardware,
        log_ring_cap: rt.log_ring_cap,
        hal_backend: rt.hal_backend,
        audio_backend: rt.audio_backend,
        servo_backend: rt.servo_backend,
        display_backend: rt.display_backend,
        pins,
    })
}

/// [bindings]：非空表、每格非空字符串值。[os] 段运行期不读，解析时无视（见文件头）。
fn rr3_bindings_parse(root: &toml::Value, errs: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut bindings = BTreeMap::new();
    match root.get("bindings").and_then(toml::Value::as_table) {
        None => errs
            .push("缺少 [bindings]（body/cognition 格位 → 模块；可为空表但段必须在）".to_string()),
        Some(t) => {
            for (slot, v) in t {
                match v.as_str() {
                    Some(m) if !m.is_empty() => {
                        bindings.insert(slot.clone(), m.to_string());
                    }
                    _ => errs.push(format!("bindings.{slot} 应为非空字符串（模块名）")),
                }
            }
        }
    }
    bindings
}

/// [enabled_slots]：非空表、每值非空字符串数组。enabled_slots 决定起谁（Q3 裁定），
/// 空表 = 无可起模块。
fn rr3_enabled_parse(root: &toml::Value, errs: &mut Vec<String>) -> BTreeMap<String, Vec<String>> {
    let mut enabled_slots = BTreeMap::new();
    match root.get("enabled_slots").and_then(toml::Value::as_table) {
        None => errs.push("缺少 [enabled_slots]（模块 → 本形态启用的格位集合）".to_string()),
        Some(t) => {
            if t.is_empty() {
                errs.push(
                    "[enabled_slots] 为空：enabled_slots 决定起谁，空装配单没有可起模块"
                        .to_string(),
                );
            }
            for (module, v) in t {
                match v.as_array() {
                    Some(items) => {
                        let slots = rr3_slot_items(module, items, errs);
                        enabled_slots.insert(module.clone(), slots);
                    }
                    None => errs.push(format!("enabled_slots.{module} 应为字符串数组")),
                }
            }
        }
    }
    enabled_slots
}

/// 单模块格位集：每项必须是非空字符串，逐条点名。
fn rr3_slot_items(module: &str, items: &[toml::Value], errs: &mut Vec<String>) -> Vec<String> {
    let mut slots = Vec::new();
    for item in items {
        match item.as_str() {
            Some(s) if !s.is_empty() => slots.push(s.to_string()),
            _ => errs.push(format!(
                "enabled_slots.{module} 的每一项都必须是非空字符串（格位名）"
            )),
        }
    }
    slots
}

/// [hardware]：可选段（纯 sim/无硬件格形态可以不给）。每键非空、值必须是
/// 字符串或整数标量（整数按十进制字符串化——rr-hal 描述符解析端全按字符串
/// 收）；布尔/浮点/数组/表逐条点名，一条不静默放行。
fn rr3_hardware_parse(root: &toml::Value, errs: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut hardware = BTreeMap::new();
    match root.get("hardware").and_then(toml::Value::as_table) {
        None => {}
        Some(t) => {
            for (k, v) in t {
                if k.is_empty() {
                    errs.push("[hardware] 键不得为空串".to_string());
                    continue;
                }
                match v {
                    toml::Value::String(s) => {
                        hardware.insert(k.clone(), s.clone());
                    }
                    toml::Value::Integer(i) => {
                        hardware.insert(k.clone(), i.to_string());
                    }
                    other => errs.push(format!(
                        "hardware.{k} 应为字符串或整数标量（物理绑定值），收到 {other:?}"
                    )),
                }
            }
        }
    }
    hardware
}

/// [[pin]]：可选段（schema 3，增补轮六 §4.2——纯 sim/无硬件格形态不给，此时校验②
/// 不判：无物理面可绑不等于角色缺失）。字段白名单 RR3_PIN_KEYS 十一键、类型逐键点名；
/// 身份键 role/direction 解析期即须在场（①缺失语义：role/direction 缺 → 红，判定
/// 不依赖角色表）；instance/controller 及各 requires 键的"在场与留空"在 boot 链按
/// 角色表逐条裁——本函数不带契约真值，不越权判角色语义。
fn rr3_pins_parse(root: &toml::Value, errs: &mut Vec<String>) -> Vec<Rr3PinEntry> {
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
    if !t.contains_key("role") {
        errs.push(format!(
            "pin #{i} 缺 role（角色外键——引脚侧唯一真值点必须挂角色）"
        ));
    }
    if !t.contains_key("direction") {
        errs.push(format!("pin #{i} 缺 direction（校验①缺失语义）"));
    }
    pin
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
        match x.as_integer() {
            Some(n) => pin.physical.push(n),
            None => errs.push(format!(
                "pin #{i}.physical 的每一项都必须是整数（物理引脚号）"
            )),
        }
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
        match x.as_str() {
            Some(s) if !s.is_empty() => pin.mux_group.push(s.to_string()),
            _ => errs.push(format!("pin #{i}.mux_group 的每一项都必须是非空字符串")),
        }
    }
}

/// 数值类键：channel（整数通道号）。
fn rr3_pin_num(i: usize, v: &toml::Value, pin: &mut Rr3PinEntry, errs: &mut Vec<String>) {
    match v.as_integer() {
        Some(n) => pin.channel = Some(n),
        None => errs.push(format!("pin #{i}.channel 应为整数")),
    }
}

/// [platform]：必填段、单键 tier（ADR-0018 修订 1 ④ 七段之一）。tier 取值域
/// host/rtos/bare 三档；chip = 平台事实表外键（§4.6，校验③④⑤⑧的判据前件选册
/// ——contracts/platform/<chip>.toml，批N1 基-22），非空必填：芯片是装机事实，
/// 留空 = ⑤⑧ 判不了 = fail-closed（同"留空即红"纪律），未知键逐条点名。
fn rr3_platform_parse(root: &toml::Value, errs: &mut Vec<String>) -> (String, String) {
    let Some(t) = root.get("platform").and_then(toml::Value::as_table) else {
        errs.push("缺少 [platform]（tier = host/rtos/bare；schema_version=2 起必填）".to_string());
        return (String::new(), String::new());
    };
    let tier = rr3_plat_tier(t, errs);
    let chip = rr3_plat_chip(t, errs);
    for k in t.keys() {
        if k != "tier" && k != "chip" {
            errs.push(format!("[platform] 未知键 {k}（只认 tier / chip）"));
        }
    }
    (tier, chip)
}

/// [platform].tier：封闭三档（host/rtos/bare）；缺键或出域逐条点名。
fn rr3_plat_tier(t: &toml::Table, errs: &mut Vec<String>) -> String {
    match t.get("tier") {
        None => {
            errs.push("[platform] 缺 tier（host/rtos/bare）".to_string());
            String::new()
        }
        Some(v) => match v.as_str() {
            Some(s) if RR3_PLATFORM_TIERS.contains(&s) => s.to_string(),
            other => {
                errs.push(format!(
                    "platform.tier 必须是 host/rtos/bare 之一，收到 {other:?}"
                ));
                String::new()
            }
        },
    }
}

/// [platform].chip：平台事实表外键，非空必填（留空 = ③④⑤⑧ 判不了，fail-closed）。
fn rr3_plat_chip(t: &toml::Table, errs: &mut Vec<String>) -> String {
    match t.get("chip") {
        Some(toml::Value::String(s)) if !s.trim().is_empty() => s.clone(),
        Some(toml::Value::String(_)) => {
            errs.push(
                "[platform].chip 不得为空串（平台事实表外键，contracts/platform/<chip>.toml 选册）"
                    .to_string(),
            );
            String::new()
        }
        Some(other) => {
            errs.push(format!("[platform].chip 应为字符串，收到 {other:?}"));
            String::new()
        }
        None => {
            errs.push(
                "[platform] 缺 chip（平台事实表外键——校验③④⑤⑧按它选 contracts/platform/<chip>.toml，留空即红）"
                    .to_string(),
            );
            String::new()
        }
    }
}

/// [runtime] 段解析产物（五键）
struct Rr3RuntimeParsed {
    log_ring_cap: usize,
    hal_backend: String,
    audio_backend: Option<String>,
    servo_backend: Option<String>,
    display_backend: Option<String>,
}

/// 后端名可选覆盖键：取值域同 hal_backend，不认即点名（组合档归属键）
fn rr3_backend_opt(t: &toml::Table, key: &str, errs: &mut Vec<String>) -> Option<String> {
    let v = t.get(key).and_then(toml::Value::as_str)?;
    if RR3_HAL_BACKEND_LEGAL.contains(&v) {
        Some(v.to_string())
    } else {
        errs.push(format!(
            "runtime.{key} 取值不认: {v}（允许 {}）",
            RR3_HAL_BACKEND_LEGAL.join("/")
        ));
        None
    }
}

/// `[runtime].log_ring_cap`：正整数且有上界（环按行常驻内存，超界随日志量吃光
/// 板载内存）；缺/非整数/非正/超界逐条点名，坏值一律留 0（装载红在前，值不外泄）。
fn rr3_rt_cap(t: &toml::Table, errs: &mut Vec<String>) -> usize {
    match t.get("log_ring_cap").and_then(toml::Value::as_integer) {
        Some(v) if v > 0 && v <= RR3_LOG_CAP_MAX => v as usize,
        Some(v) if v > 0 => {
            errs.push(format!(
                "runtime.log_ring_cap = {v} 超上界 {RR3_LOG_CAP_MAX}（日志环按行常驻内存，超界会随日志量吃光板载内存）"
            ));
            0
        }
        Some(v) => {
            errs.push(format!(
                "runtime.log_ring_cap = {v} 须为正整数（日志环容量，0 = 运维留痕无环）"
            ));
            0
        }
        None => {
            errs.push(
                "runtime.log_ring_cap 缺失或不是整数（日志环容量，行数；批N5 件二）".to_string(),
            );
            0
        }
    }
}

/// `[runtime].hal_backend`：P0-3 根因门——必填且须在白名单内。后端选择是部署
/// 事实，必须进装配单显式数据，不靠隐式缺省（漏 env 静默落 sim = 生产假验证）。
fn rr3_rt_hal(t: &toml::Table, errs: &mut Vec<String>) -> String {
    match t.get("hal_backend").and_then(toml::Value::as_str) {
        Some(v) if RR3_HAL_BACKEND_LEGAL.contains(&v) => v.to_string(),
        Some(v) => {
            errs.push(format!(
                "runtime.hal_backend 取值不认: {v}（允许 {}）",
                RR3_HAL_BACKEND_LEGAL.join("/")
            ));
            String::new()
        }
        None => {
            errs.push(
                "runtime.hal_backend 缺失（P0-3：后端选择必须进装配单显式数据，不靠隐式缺省）"
                    .to_string(),
            );
            String::new()
        }
    }
}

/// `[runtime]` 未知键点名（只认 [`RR3_RUNTIME_KEYS`]，拼错键不静默当缺省）。
fn rr3_rt_unknown(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        if !RR3_RUNTIME_KEYS.contains(&k.as_str()) {
            errs.push(format!(
                "[runtime] 未知键 {k}（只认 {}）",
                RR3_RUNTIME_KEYS.join("/")
            ));
        }
    }
}

/// [runtime]：必填段（schema 5 起必填，批N5 件二 F-30——"环容量 ∈ 装配单键集，
/// 装载期缺即红"与 F-6 缺失语义同一列）。键序：log_ring_cap → hal_backend →
/// 三个可选覆盖键 → 未知键；错误追加序即此序（运维逐条改）。
fn rr3_runtime_parse(root: &toml::Value, errs: &mut Vec<String>) -> Rr3RuntimeParsed {
    let mut out = Rr3RuntimeParsed {
        log_ring_cap: 0,
        hal_backend: String::new(),
        audio_backend: None,
        servo_backend: None,
        display_backend: None,
    };
    let Some(t) = root.get("runtime").and_then(toml::Value::as_table) else {
        errs.push("缺少 [runtime]（rrd 运行参数；schema_version=5 起必填，批N5 件二）".to_string());
        return out;
    };
    out.log_ring_cap = rr3_rt_cap(t, errs);
    out.hal_backend = rr3_rt_hal(t, errs);
    // 三个可选覆盖键：不设 → 对应能力面回落全局 hal_backend
    out.audio_backend = rr3_backend_opt(t, "audio_backend", errs);
    out.servo_backend = rr3_backend_opt(t, "servo_backend", errs);
    out.display_backend = rr3_backend_opt(t, "display_backend", errs);
    rr3_rt_unknown(t, errs);
    out
}

/// [supplies]：必填段（看门狗三键 + 时序预算位三键，六键全必填、类型逐键检查、
/// 未知键点名）——这是 G6 档位一致性校验与 0.0.6 点验矩阵的输入事实。
fn rr3_supplies_parse(root: &toml::Value, tier: &str, errs: &mut Vec<String>) -> Rr3Supplies {
    let mut supplies = Rr3Supplies::default();
    match root.get("supplies").and_then(toml::Value::as_table) {
        None => errs.push(
            "缺少 [supplies]（平台看门狗与时序预算声明；schema_version=2 起必填）".to_string(),
        ),
        Some(t) => {
            rr3_supplies_core(t, &mut supplies, errs);
            rr3_supplies_sub(t, &mut supplies, tier, errs);
        }
    }
    supplies
}

/// `[supplies]` 标量位：看门狗声明/时序预算/喂狗者 + 未知键点名。
fn rr3_supplies_core(t: &toml::Table, supplies: &mut Rr3Supplies, errs: &mut Vec<String>) {
    rr3_supplies_flags(t, supplies, errs);
    rr3_int_into(
        t,
        "watchdog_timeout_ms",
        &mut supplies.watchdog_timeout_ms,
        errs,
    );
    rr3_int_into(t, "tick_period_ms", &mut supplies.tick_period_ms, errs);
    rr3_int_into(t, "tick_jitter_ms", &mut supplies.tick_jitter_ms, errs);
    rr3_int_into(
        t,
        "safety_core_interval_ms",
        &mut supplies.safety_core_interval_ms,
        errs,
    );
    // domain / budget 是子段键，各自成块解析（第七供给位 / 校验⑩ 分母）
    for k in t.keys() {
        if k != "domain" && k != "budget" && !RR3_SUPPLIES_KEYS.contains(&k.as_str()) {
            errs.push(format!(
                "[supplies] 未知键 {k}（只认 {}）",
                RR3_SUPPLIES_KEYS.join("/")
            ));
        }
    }
}

/// 看门狗有无与非空喂者两键（B9 自 supplies_core 抽出）：缺/型错各自点名。
fn rr3_supplies_flags(t: &toml::Table, supplies: &mut Rr3Supplies, errs: &mut Vec<String>) {
    match t.get("watchdog_present").and_then(toml::Value::as_bool) {
        Some(b) => supplies.watchdog_present = b,
        None => errs.push("supplies.watchdog_present 缺失或不是布尔".to_string()),
    }
    match t.get("watchdog_feeder").and_then(toml::Value::as_str) {
        Some(s) if !s.is_empty() => supplies.watchdog_feeder = s.to_string(),
        _ => errs.push("supplies.watchdog_feeder 缺失或不是非空字符串".to_string()),
    }
}

/// 整数键落位（B9 自 supplies_core 抽出）：一条键一条点名，字段引用直给。
fn rr3_int_into(t: &toml::Table, key: &str, slot: &mut i64, errs: &mut Vec<String>) {
    match supplies_int(t.get(key), key) {
        Ok(v) => *slot = v,
        Err(e) => errs.push(e),
    }
}

fn rr3_supplies_sub(
    t: &toml::Table,
    supplies: &mut Rr3Supplies,
    tier: &str,
    errs: &mut Vec<String>,
) {
    // 缺数组 = 红——schema 3 装载期校验⑥-a 需要名单源。
    match t.get("domain").and_then(toml::Value::as_array) {
        Some(items) => supplies.domains = rr3_domains_parse(items, errs),
        None => errs.push(
            "缺少 [[supplies.domain]]（电力网络声明，第七供给位；schema_version=3 起必填，至少声明一域）"
                .to_string(),
        ),
    }
    match t.get("budget").and_then(toml::Value::as_table) {
        None => errs.push(
            "缺少 [supplies.budget]（内存预算声明，校验⑩ 分母；schema_version=4 起必填——host 档显式写 not_applicable = true，不许留空）"
                .to_string(),
        ),
        Some(bt) => supplies.budget = rr3_budget_parse(bt, tier, errs),
    }
}

/// [[supplies.domain]] 逐域解析（段名沿用 supplies.domains 文案口径）
fn rr3_domains_parse(items: &[toml::Value], errs: &mut Vec<String>) -> Vec<Rr3Domain> {
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let label = format!("supplies.domains[{i}]");
        let Some(dt) = item.as_table() else {
            errs.push(format!("{label} 必须是 [[supplies.domain]] 表"));
            continue;
        };
        out.push(rr3_domain_one(dt, &label, errs));
    }
    out
}

/// 单域解析：键表驱动（[`RR3_DOMAIN_KEYS`]）逐键取数，未知键点名。
fn rr3_domain_one(dt: &toml::Table, label: &str, errs: &mut Vec<String>) -> Rr3Domain {
    let mut d = Rr3Domain::default();
    for key in RR3_DOMAIN_KEYS {
        let path = format!("{label}.{key}");
        match key {
            "name" | "sense" | "current_sense" | "switch" | "evidence" => {
                rr3_domain_str(&mut d, key, dt.get(key), &path, errs)
            }
            "switchable" | "armed" => rr3_domain_bool(&mut d, key, dt.get(key), &path, errs),
            _ => rr3_domain_int(&mut d, key, dt.get(key), &path, errs),
        }
    }
    for k in dt.keys() {
        if !RR3_DOMAIN_KEYS.contains(&k.as_str()) {
            errs.push(format!(
                "{label} 未知键 {k}（只认 {}）",
                RR3_DOMAIN_KEYS.join("/")
            ));
        }
    }
    d
}

/// 字符串类域键：非空即收；缺失/空串同文案。
fn rr3_domain_str(
    d: &mut Rr3Domain,
    key: &str,
    v: Option<&toml::Value>,
    path: &str,
    errs: &mut Vec<String>,
) {
    match v.and_then(toml::Value::as_str) {
        Some(s) if !s.is_empty() => {
            let v = s.to_string();
            match key {
                "name" => d.name = v,
                "sense" => d.sense = v,
                "current_sense" => d.current_sense = v,
                "evidence" => d.evidence = v,
                _ => d.switch = v,
            }
        }
        _ => errs.push(format!("{path} 缺失或不是非空字符串")),
    }
}

/// 布尔类域键：缺失/非布尔同文案。
fn rr3_domain_bool(
    d: &mut Rr3Domain,
    key: &str,
    v: Option<&toml::Value>,
    path: &str,
    errs: &mut Vec<String>,
) {
    match (key, v.and_then(toml::Value::as_bool)) {
        ("switchable", Some(b)) => d.switchable = b,
        ("armed", Some(b)) => d.armed = b,
        _ => errs.push(format!("{path} 缺失或不是布尔")),
    }
}

/// 数值类域键：经 [`supplies_int`] 校验，按键落位。
fn rr3_domain_int(
    d: &mut Rr3Domain,
    key: &str,
    v: Option<&toml::Value>,
    path: &str,
    errs: &mut Vec<String>,
) {
    match supplies_int(v, path) {
        Ok(n) => match key {
            "nominal_mv" => d.nominal_mv = n,
            "mild_mv" => d.mild_mv = n,
            "severe_mv" => d.severe_mv = n,
            "current_cap_ma" => d.current_cap_ma = n,
            _ => d.max_age_ms = n,
        },
        Err(e) => errs.push(e),
    }
}

fn rr3_budget_parse(bt: &toml::Table, tier: &str, errs: &mut Vec<String>) -> Rr3Budget {
    let mut budget = Rr3Budget::default();
    rr3_budget_keys(bt, errs);
    match bt.get("not_applicable").and_then(toml::Value::as_bool) {
        Some(b) => budget.not_applicable = b,
        None => errs.push(
            "supplies.budget.not_applicable 缺失或不是布尔（校验⑩ 不适用必须显式声明，不许留空——F-6）"
                .to_string(),
        ),
    }
    let nums = rr3_budget_nums(bt, &mut budget, errs);
    if !tier.is_empty() {
        if tier == "host" {
            rr3_budget_host(&budget, &nums, errs);
        } else {
            rr3_budget_rtos(&budget, &nums, errs);
        }
    }
    budget
}

/// 未知键点名：预算段只认 [`RR3_BUDGET_KEYS`] 四键（三数 + 不适用位）。
fn rr3_budget_keys(bt: &toml::Table, errs: &mut Vec<String>) {
    for k in bt.keys() {
        if !RR3_BUDGET_KEYS.contains(&k.as_str()) {
            errs.push(format!(
                "[supplies.budget] 未知键 {k}（只认 {}）",
                RR3_BUDGET_KEYS.join("/")
            ));
        }
    }
}

/// 三数键收集（类型错点名）：返回"本次出现的键→值"，分档自洽判据共用。
fn rr3_budget_nums(
    bt: &toml::Table,
    budget: &mut Rr3Budget,
    errs: &mut Vec<String>,
) -> BTreeMap<&'static str, i64> {
    let mut nums: BTreeMap<&'static str, i64> = BTreeMap::new();
    for key in [
        "task_stack_budget_bytes",
        "heap_total_bytes",
        "heap_reserved_bytes",
    ] {
        match bt.get(key).and_then(toml::Value::as_integer) {
            Some(v) => {
                nums.insert(key, v);
                match key {
                    "task_stack_budget_bytes" => budget.task_stack_budget_bytes = v,
                    "heap_total_bytes" => budget.heap_total_bytes = v,
                    _ => budget.heap_reserved_bytes = v,
                }
            }
            None => {
                if bt.get(key).is_some() {
                    errs.push(format!("supplies.budget.{key} 应为整数"));
                }
            }
        }
    }
    nums
}

/// host 档自洽：不适用必须显式为 true，且三数键不得并存（F-6 / ADR-0021 修订 3）。
fn rr3_budget_host(budget: &Rr3Budget, nums: &BTreeMap<&'static str, i64>, errs: &mut Vec<String>) {
    if !budget.not_applicable {
        errs.push(
            "分档自洽校验失败（校验⑩）：tier = host 但 supplies.budget.not_applicable = false——校验⑩ 对 host 档不适用，合法不适用必须显式写 true，不许留空（F-6 / ADR-0021 修订 3）"
                .to_string(),
        );
    }
    if !nums.is_empty() {
        errs.push(format!(
            "分档自洽校验失败（校验⑩）：tier = host 但 [supplies.budget] 并存数字预算键 {}（与 not_applicable = true 自相矛盾——栈/堆预算只属于 rtos/bare 档，ADR-0021 修订 3）",
            nums.keys().copied().collect::<Vec<_>>().join("/")
        ));
    }
}

/// rtos/bare 档自洽：三数必填、正负域与"可用堆 > 0"物理判据。
fn rr3_budget_rtos(budget: &Rr3Budget, nums: &BTreeMap<&'static str, i64>, errs: &mut Vec<String>) {
    if budget.not_applicable {
        errs.push(
            "分档自洽校验失败（校验⑩）：tier = rtos/bare 但 supplies.budget.not_applicable = true（rtos/bare 档有真栈/堆预算，写不适用是自相矛盾——ADR-0021 修订 3）"
                .to_string(),
        );
    }
    for key in [
        "task_stack_budget_bytes",
        "heap_total_bytes",
        "heap_reserved_bytes",
    ] {
        if !nums.contains_key(key) {
            errs.push(format!(
                "supplies.budget.{key} 缺失（rtos/bare 档必填——校验⑩ 分母，合法不适用才走 not_applicable，不许留空——F-6）"
            ));
        }
    }
    if let (Some(&st), Some(&tot), Some(&res)) = (
        nums.get("task_stack_budget_bytes"),
        nums.get("heap_total_bytes"),
        nums.get("heap_reserved_bytes"),
    ) {
        if st <= 0 {
            errs.push(format!(
                "supplies.budget.task_stack_budget_bytes = {st} 须为正整数（⑩-a 分母，0 预算 = 栈账无分母）"
            ));
        }
        if tot <= 0 {
            errs.push(format!(
                "supplies.budget.heap_total_bytes = {tot} 须为正整数（⑩-b 分母基数）"
            ));
        }
        if res < 0 {
            errs.push(format!(
                "supplies.budget.heap_reserved_bytes = {res} 须为非负整数（平台常驻占用，负数是物理矛盾）"
            ));
        }
        if tot > 0 && res >= 0 && tot <= res {
            errs.push(format!(
                "supplies.budget.heap_total_bytes = {tot} ≤ heap_reserved_bytes = {res}（可用堆 = total − reserved ≤ 0，⑩-b 无分母——物理矛盾）"
            ));
        }
    }
}

/// [slots]：必填段、present = 本板物理在位格位声明（缺格三源合一第一源）。
/// 每项非空字符串；未知键点名。空数组合法——板可以什么都不在位（全 Degraded）。
/// device_absent = base 层格的器件缺席登记（D3 裁定 5）：表键为格位名、值为
/// 非空理由；留空即红（no_injection 范式——缺席不写理由 = 未申报的静默退化）。
fn rr3_slots_parse(
    root: &toml::Value,
    errs: &mut Vec<String>,
) -> (Vec<String>, BTreeMap<String, String>) {
    let mut slots_present: Vec<String> = Vec::new();
    let mut slots_device_absent: BTreeMap<String, String> = BTreeMap::new();
    let Some(t) = root.get("slots").and_then(toml::Value::as_table) else {
        errs.push(
            "缺少 [slots]（present = 本板物理在位格位声明；schema_version=2 起必填）".to_string(),
        );
        return (slots_present, slots_device_absent);
    };
    rr3_slots_present(t, &mut slots_present, errs);
    rr3_slots_absent(t, &mut slots_device_absent, errs);
    for k in t.keys() {
        if k != "present" && k != "device_absent" {
            errs.push(format!(
                "[slots] 未知键 {k}（只认 present / device_absent）"
            ));
        }
    }
    (slots_present, slots_device_absent)
}

/// [slots].present（B9 自 slots_parse 抽出）：本板物理在位格位数组，逐项非空。
fn rr3_slots_present(t: &toml::Table, out: &mut Vec<String>, errs: &mut Vec<String>) {
    match t.get("present").and_then(toml::Value::as_array) {
        Some(items) => {
            for item in items {
                match item.as_str() {
                    Some(s) if !s.is_empty() => out.push(s.to_string()),
                    _ => errs
                        .push("[slots].present 的每一项都必须是非空字符串（格位名）".to_string()),
                }
            }
        }
        None => errs.push("[slots] 缺 present（本板物理在位格位数组）".to_string()),
    }
}

/// [slots].device_absent（B9 自 slots_parse 抽出）：缺席须显式申报非空理由。
fn rr3_slots_absent(t: &toml::Table, out: &mut BTreeMap<String, String>, errs: &mut Vec<String>) {
    let Some(absent) = t.get("device_absent").and_then(toml::Value::as_table) else {
        return;
    };
    for (slot, reason) in absent {
        match reason.as_str() {
            Some(r) if !r.trim().is_empty() => {
                out.insert(slot.clone(), r.to_string());
            }
            _ => errs.push(format!(
                "[slots].device_absent 格位 {slot} 的理由必须是非空字符串（缺席须显式申报理由，留空即红）"
            )),
        }
    }
}

/// [supplies] 的整数键读取：缺失或类型不合同一条错误点名（运维一次改对）
fn supplies_int(v: Option<&toml::Value>, key: &str) -> Result<i64, String> {
    v.and_then(toml::Value::as_integer)
        .ok_or_else(|| format!("supplies.{key} 缺失或不是整数"))
}
