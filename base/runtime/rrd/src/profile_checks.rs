//! 装载期校验族（校验①②⑤⑥⑦⑨⑩ 判据 + 装配扫描 + 缺格探针 + 轴档案装载）。
//!
//! 自 `profile.rs` 拆出（批N45 第三刀）：本模块只持**判定与探针**，输入全部是
//! 已装载的装配单实体（[`Rr3Profile`]）与契约表（格位/角色/平台）——除
//! [`rr3_scan_modules`]（modules-root 扫盘）与 [`rr3_axis_load`]（轴档案读盘）
//! 外不碰文件系统。编排住 [`crate::profile::rr3_boot_with_probe`]（校验次序
//! 与错误聚合在那边，本模块不排总）；引脚能力/占用面住 [`crate::profile_pins`]。
//! 校验项与错误文案逐字保持原样（门禁与文档按文案点名消费，改文案即改契约）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rr_registry::{Rr3AssembleFacts, Rr3PlatformTable, Rr3RoleTable, Rr3SlotTable};

use crate::profile::{
    rr3_assemble_scan, rr3_axis_files_load, rr3_axis_state_text, rr3_bindings_text,
    rr3_demand_budget, rr3_demand_expand, rr3_memory_budget, rr3_power_gaps, Rr3Domain,
    Rr3PinEntry, Rr3Profile, RR3_PIN_DERIVED_KEYS, RR3_PIN_KEYS,
};

/// 控制器词形解析（host 档派生规则的机制侧）：`<前缀><十进制编号>` 才算命中，返回
/// 编号串——roles.toml 注记的取值语义（pwm<N> 派生 pwm_chip=N）。前缀**住平台表**
/// `[controller_wordform]` 段（实-6 (a)，批N4 件七下移：rrd 零硬编码词形规则，
/// 换平台 = 改数据），本函数只做词形匹配机制。
fn rr3_wordform_tail(controller: &str, prefix: &str) -> Option<String> {
    let rest = controller.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(rest.to_string())
}

/// 词形前缀查询 + fail-closed 守卫（实-6 (a)）：派生键查平台表词形前缀，查不到
/// 红——不许回退硬编码（回退 = 策略重新住回机制侧，下移作废）。
fn rr3_wordform_prefix(platform: &Rr3PlatformTable, key: &str) -> Result<String, String> {
    platform
        .rr3_wordform(key)
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "平台事实表 controller_wordform 无 {key} 词形（host 档派生前件缺失，fail-closed）"
            )
        })
}

/// 单条 pin 的派生取值（机制侧，八物理键封闭）：role.derives（策略，contracts/
/// roles.toml）只声明"该角色供给哪些键"，取值法一律查本函数——键不在册/来源字段
/// 留空/控制器词形不合都红。gpio_led_line 与 gpio_relay_line 同取 physical[0]，由
/// 角色声明区分（两角色不共用同一派生键名，不歧义）；spi_device 由词形段前缀
/// （总线号）+ channel（片选号）合成 /dev/spidev<N>.<M>，与旧 [hardware] 直给值
/// （profiles/distro.toml 链④真件档）逐字节对齐。
fn rr3_pin_derive_value(
    pin: &Rr3PinEntry,
    key: &str,
    platform: &Rr3PlatformTable,
) -> Result<String, String> {
    match key {
        "pwm_chip" | "gpio_chip" | "i2c_bus" => rr3_derive_word(pin, key, platform),
        "spi_device" => rr3_derive_spi(pin, key, platform),
        "pwm_channel" => rr3_derive_channel(pin),
        "gpio_led_line" | "gpio_relay_line" => rr3_derive_line(pin),
        "ina219_addr" => rr3_derive_addr(pin),
        other => Err(format!("派生键 {other} 不在派生规则表（八物理键封闭）")),
    }
}

/// 词形键派生（B9 自 pin_derive_value 抽出）：controller 必须合 `<prefix><N>`，
/// 派生值 = N 的字符串形式。
fn rr3_derive_word(
    pin: &Rr3PinEntry,
    key: &str,
    platform: &Rr3PlatformTable,
) -> Result<String, String> {
    let prefix = rr3_wordform_prefix(platform, key)?;
    rr3_wordform_tail(&pin.controller, &prefix)
        .ok_or_else(|| format!("controller 词形 {} 不合 {prefix}<N>", pin.controller))
}

/// spi_device 派生（B9 自 pin_derive_value 抽出）：`/dev/spidev<bus>.<cs>`，
/// bus 自 controller 词形、cs 自 channel。
fn rr3_derive_spi(
    pin: &Rr3PinEntry,
    key: &str,
    platform: &Rr3PlatformTable,
) -> Result<String, String> {
    let prefix = rr3_wordform_prefix(platform, key)?;
    let bus = rr3_wordform_tail(&pin.controller, &prefix)
        .ok_or_else(|| format!("controller 词形 {} 不合 {prefix}<N>", pin.controller))?;
    let cs = pin
        .channel
        .map(|c| c.to_string())
        .ok_or_else(|| "channel 留空（SPI 片选号）".to_string())?;
    Ok(format!("/dev/spidev{bus}.{cs}"))
}

fn rr3_derive_channel(pin: &Rr3PinEntry) -> Result<String, String> {
    pin.channel
        .map(|c| c.to_string())
        .ok_or_else(|| "channel 留空".to_string())
}

fn rr3_derive_line(pin: &Rr3PinEntry) -> Result<String, String> {
    pin.physical
        .first()
        .map(|n| n.to_string())
        .ok_or_else(|| "physical 留空".to_string())
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
fn rr3_pin_has(pin: &Rr3PinEntry, field: &str) -> bool {
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

/// 格位引用查册门三扇合一：bindings/enabled_slots、[slots].present、
/// [slots].device_absent（判定口径见各子函数）。
pub(crate) fn rr3_check_slot_refs(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = rr3_check_bind_slots(profile, table);
    errs.extend(rr3_check_present(profile, table));
    errs.extend(rr3_check_absent(profile, table));
    errs
}

/// bindings/enabled_slots 的查册门（bindings 另拒基础层格位——基础层恒在位）
fn rr3_check_bind_slots(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();
    for slot in profile.bindings.keys() {
        if !table.rr3_slot_contains(slot) {
            errs.push(format!(
                "bindings 格位 {slot} 未在册（contracts/slots.toml 无此格，装配单不得自造格位）"
            ));
        } else if table.rr3_slot_layer(slot).as_deref() == Some("base") {
            errs.push(format!(
                "bindings 不得绑定基础层格位 {slot}（基础层由基座内建承载，恒在位）"
            ));
        }
    }
    // enabled_slots 的格位必须查册命中（基础层允许——恒在位，只记账不绑定）
    for (module, slots) in &profile.enabled_slots {
        for slot in slots {
            if !table.rr3_slot_contains(slot) {
                errs.push(format!(
                    "enabled_slots 模块 {module} 的格位 {slot} 未在册（contracts/slots.toml 无此格）"
                ));
            }
        }
    }
    errs
}

/// [slots].present 查册门：声明集只认 rr-registry 那一册；基础层进 present 属
/// 自相矛盾（恒在位），点名拒。
fn rr3_check_present(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();
    for slot in &profile.slots_present {
        if !table.rr3_slot_contains(slot) {
            errs.push(format!(
                "[slots].present 格位 {slot} 未在册（contracts/slots.toml 无此格）"
            ));
        } else if table.rr3_slot_layer(slot).as_deref() == Some("base") {
            errs.push(format!(
                "[slots].present 不得声明基础层格位 {slot}（基础层由基座内建承载，恒在位）"
            ));
        }
    }
    errs
}

/// [slots].device_absent 查册门（D3 裁定 5）：格名在册 + 只登记 base 层格
/// （非 base 格的缺席表达 = 不进 present，写进本表属第二套语法即红）。
fn rr3_check_absent(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();
    for slot in profile.slots_device_absent.keys() {
        if !table.rr3_slot_contains(slot) {
            errs.push(format!(
                "[slots].device_absent 格位 {slot} 未在册（contracts/slots.toml 无此格）"
            ));
        } else if table.rr3_slot_layer(slot).as_deref() != Some("base") {
            errs.push(format!(
                "[slots].device_absent 只登记基础层格位，{slot} 非 base 层（非基础层格的缺席表达 = 不进 [slots].present，写进本表属第二套语法）"
            ));
        }
    }
    errs
}

/// 档位一致性（G6：bare 必须有看门狗）+ tier↔tier_scope 交叉校验（L-10）+
/// 占位/退役格禁入生产装配面（P0-2）。三处引用合并去重逐格查，未在册格由
/// 上面查册门点名，此处只对在册格判 scope，不重复报。
pub(crate) fn rr3_check_tier(profile: &Rr3Profile, table: &Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();
    rr3_tier_bare(profile, &mut errs);
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    referenced.extend(profile.bindings.keys().cloned());
    for slots in profile.enabled_slots.values() {
        referenced.extend(slots.iter().cloned());
    }
    referenced.extend(profile.slots_present.iter().cloned());
    for slot in &referenced {
        if !table.rr3_slot_contains(slot) {
            continue;
        }
        rr3_slot_retired(slot, table, &mut errs);
        rr3_slot_scope(&profile.platform_tier, slot, table, &mut errs);
    }
    errs
}

/// G6：无看门狗的板不允许归 bare 档（ADR-0018 修订 1）。
fn rr3_tier_bare(profile: &Rr3Profile, errs: &mut Vec<String>) {
    if profile.platform_tier == "bare" && !profile.supplies.watchdog_present {
        errs.push(
            "档位一致性校验失败：tier = bare 但 supplies.watchdog_present = false——无看门狗的板不允许归 bare 档（ADR-0018 修订 1），拒绝启动"
                .to_string(),
        );
    }
}

/// P0-2 根因门：占位/退役格不得进生产装配面（真件落地后改 status=normal）。
fn rr3_slot_retired(slot: &str, table: &Rr3SlotTable, errs: &mut Vec<String>) {
    if let Some(status) = table.rr3_slot_status(slot) {
        if status == "placeholder" || status == "retired" {
            errs.push(format!(
                "装配面引用 {status} 格 {slot}（占位/退役格不得进生产装配面；真件落地后改 contracts/slots.toml status=normal 再绑定）"
            ));
        }
    }
}

/// L-10：tier↔tier_scope 交叉校验（该档不适用此格）。
fn rr3_slot_scope(tier: &str, slot: &str, table: &Rr3SlotTable, errs: &mut Vec<String>) {
    let scope = table.rr3_slot_tier_scope(slot).unwrap_or_default();
    if !scope.iter().any(|t| t == tier) {
        errs.push(format!(
            "tier↔tier_scope 交叉校验失败：装配档 {tier} 引用格 {slot}，其 tier_scope = {scope:?} 不含 {tier}（该档不适用此格，ADR-0012 增补注记分族），拒绝启动"
        ));
    }
}

/// 结构自洽：每条 binding 都必须被 enabled_slots 认领（否则是死数据）
pub(crate) fn rr3_check_claims(profile: &Rr3Profile) -> Vec<String> {
    let mut errs = Vec::new();
    for (slot, module) in &profile.bindings {
        let claimed = profile
            .enabled_slots
            .get(module)
            .is_some_and(|slots| slots.contains(slot));
        if !claimed {
            errs.push(format!(
                "bindings 条目 {slot}={module} 未被 enabled_slots 认领（模块 {module} 不在 enabled_slots，或其 enabled_slots 不含 {slot}）"
            ));
        }
    }
    errs
}

/// 装载期校验①（[[pin]] 逐条）+ host 档词形派生；返回 (错误, 派生视图)。
/// rtos/bare 档不派生（板侧描述符消费归 0.0.7 批G/批D1），pins 校验照跑。
pub(crate) fn rr3_check_pins(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    table: &Rr3SlotTable,
    platform: &Rr3PlatformTable,
) -> (Vec<String>, BTreeMap<String, String>) {
    let mut errs = Vec::new();
    let mut derived = BTreeMap::new();
    // host 档词形前件守卫（实-6 (a)）：词形表空 = 平台没提供派生前件，fail-closed
    if profile.platform_tier == "host" && platform.rr3_wordform_map().is_empty() {
        errs.push(
            "host 档平台事实表 controller_wordform 为空（词形派生前件缺失，host 档必须选带词形表的主机平台册，fail-closed）"
                .into(),
        );
    }
    for pin in &profile.pins {
        let (pin_errs, derives) = rr3_check_pin(pin, roles, table);
        errs.extend(pin_errs);
        let Some(derives) = derives else { continue };
        if profile.platform_tier != "host" {
            continue;
        }
        errs.extend(rr3_derive_pin(pin, &derives, platform, &mut derived));
    }
    (errs, derived)
}

/// 单条 pin 的静态校验（角色查册 / per_instance / requires / direction /
/// role.slot）；角色未在册时直接返回单条点名（不级联刷屏），Second 返回 derived
/// 声明供 host 档派生。四类判据各自具名，追加次序不变。
fn rr3_check_pin(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    table: &Rr3SlotTable,
) -> (Vec<String>, Option<Vec<String>>) {
    let mut errs = Vec::new();
    let label = format!("pins[{}]", pin.rr3_pin_id());
    let Some(derives) = rr3_pin_role_gate(pin, roles, &label, &mut errs) else {
        return (errs, None);
    };
    rr3_pin_inst_check(pin, roles, &label, &mut errs);
    rr3_pin_req_check(pin, roles, &label, &mut errs);
    rr3_pin_dir_check(pin, roles, &label, &mut errs);
    rr3_pin_slot_check(pin, roles, table, &mut errs);
    (errs, Some(derives))
}

/// 角色在册门：未在册即点名并中止本条（不级联刷屏）。
fn rr3_pin_role_gate(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    label: &str,
    errs: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Some(derives) = roles.rr3_role_derives(&pin.role) else {
        errs.push(format!(
            "{label} 角色 {} 未在册（contracts/roles.toml 无此角色，装配单不得自造角色）",
            pin.role
        ));
        return None;
    };
    Some(derives)
}

/// per_instance 规则：true ⇒ instance 必填；false ⇒ instance 禁填。
fn rr3_pin_inst_check(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    label: &str,
    errs: &mut Vec<String>,
) {
    match roles.rr3_role_per_inst(&pin.role) {
        Some(true) if pin.instance.is_empty() => errs.push(format!(
            "{label} 角色 {} per_instance = true，instance 必填（多实例角色的身份一半在 instance）",
            pin.role
        )),
        Some(false) if !pin.instance.is_empty() => errs.push(format!(
            "{label} 角色 {} per_instance = false，instance 禁填（填了 pin_id 就有两个真值点）",
            pin.role
        )),
        _ => {}
    }
}

/// requires 逐项在场（§5 缺失语义：空串/None/空数组 = 缺；"none" 算在场）。
fn rr3_pin_req_check(pin: &Rr3PinEntry, roles: &Rr3RoleTable, label: &str, errs: &mut Vec<String>) {
    for f in roles.rr3_role_requires(&pin.role).unwrap_or_default() {
        if !RR3_PIN_KEYS.contains(&f.as_str()) {
            errs.push(format!(
                "角色 {} 的 requires 引用未知字段 {f}（[pins] 字段词表外——门 a 六表对账第 5 张的漂移）",
                pin.role
            ));
        } else if !rr3_pin_has(pin, &f) {
            errs.push(format!(
                "{label} 缺 {f}（角色 {} requires 必填，留空即红——§5 缺失语义；\"合法不适用\"须显式写 none）",
                pin.role
            ));
        }
    }
}

/// direction 相等（pin 的物理方向与角色声明各说一套 = 数据矛盾）。
fn rr3_pin_dir_check(pin: &Rr3PinEntry, roles: &Rr3RoleTable, label: &str, errs: &mut Vec<String>) {
    if let Some(d) = roles.rr3_role_direction(&pin.role) {
        if pin.direction != d {
            errs.push(format!(
                "{label} direction = {} 与角色 {} direction = {d} 不一致（校验①）",
                pin.direction, pin.role
            ));
        }
    }
}

/// role.slot ∈ slots（roles/slots 两表对账的装载侧执法）。
fn rr3_pin_slot_check(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    table: &Rr3SlotTable,
    errs: &mut Vec<String>,
) {
    if let Some(slot) = roles.rr3_role_slot(&pin.role) {
        if !table.rr3_slot_contains(&slot) {
            errs.push(format!(
                "角色 {} 的 slot {slot} 未在格位契约在册（roles/slots 两表对账）",
                pin.role
            ));
        }
    }
}

/// host 档派生：derives 声明逐键取值（多实例共键须同值）+ 词形漂移检查。
/// 词形清单住平台表——表里有什么判什么，rrd 零硬编码词形清单。
fn rr3_derive_pin(
    pin: &Rr3PinEntry,
    derives: &[String],
    platform: &Rr3PlatformTable,
    derived: &mut BTreeMap<String, String>,
) -> Vec<String> {
    let mut errs = Vec::new();
    let label = format!("pins[{}]", pin.rr3_pin_id());
    for d in derives {
        rr3_derive_one(pin, d, &label, platform, derived, &mut errs);
    }
    rr3_drift_check(pin, derives, &label, platform, &mut errs);
    errs
}

/// 单条派生键取值并落账：多实例共键须同值（同一资源维）。
fn rr3_derive_one(
    pin: &Rr3PinEntry,
    d: &str,
    label: &str,
    platform: &Rr3PlatformTable,
    derived: &mut BTreeMap<String, String>,
    errs: &mut Vec<String>,
) {
    match rr3_pin_derive_value(pin, d, platform) {
        Ok(v) => match derived.get(d) {
            None => {
                derived.insert(d.to_string(), v);
            }
            Some(prev) if prev != &v => errs.push(format!(
                "{label} 派生键 {d} 与他条取值不同（{prev} vs {v}）——多实例共键须同值（同一资源维）"
            )),
            _ => {}
        },
        Err(e) => errs.push(format!("{label} 派生 {d} 失败: {e}")),
    }
}

/// 词形漂移（①-f）：控制器词形指明总线族，该族派生键必须被角色声明。
/// derives 显式空的角色跳过（"不派生"是声明不是漏声明）。
fn rr3_drift_check(
    pin: &Rr3PinEntry,
    derives: &[String],
    label: &str,
    platform: &Rr3PlatformTable,
    errs: &mut Vec<String>,
) {
    if derives.is_empty() {
        return;
    }
    for (key, prefix) in platform.rr3_wordform_map() {
        if rr3_wordform_tail(&pin.controller, prefix).is_some() && !derives.iter().any(|d| d == key)
        {
            errs.push(format!(
                "{label} controller 词形 {} 指明 {key}，但角色 {} derives 未声明（派生漂移）",
                pin.controller, pin.role
            ));
        }
    }
}

/// 门 n（§8 注入路径唯一）：[hardware] 出现引脚侧派生键一律红——pin 实际派生
/// 该键时点名两份取值，否则按稳态判据点名。
pub(crate) fn rr3_check_hw_keys(
    profile: &Rr3Profile,
    derived: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut errs = Vec::new();
    for k in RR3_PIN_DERIVED_KEYS {
        if let Some(hv) = profile.hardware.get(k) {
            match derived.get(k) {
                Some(dv) => errs.push(format!(
                    "hardware.{k} = \"{hv}\" 与 [pins] 派生值 \"{dv}\" 两处并存（门 n：注入路径唯一，[pins] 派生或 [hardware] 直给二者取一）"
                )),
                None => errs.push(format!(
                    "hardware.{k} 是引脚侧派生键（schema 3 起唯一来源 = [pins] 派生，[hardware] 不得直给——门 n 稳态判据）"
                )),
            }
        }
    }
    errs
}

/// 装载期校验⑥-a/⑥-b：功率面角色必须挂真实供电轨，轨名必须在册；
/// signal 面缺 rail 合法，"none" 是显式不适值。
pub(crate) fn rr3_check_rails(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    domain_names: &BTreeSet<String>,
) -> Vec<String> {
    let mut errs = Vec::new();
    for pin in &profile.pins {
        let kind = roles.rr3_role_kind(&pin.role);
        let rail_unlisted = pin.rail.is_empty() || pin.rail == "none";
        if rail_unlisted {
            if kind.as_deref() == Some("power") {
                errs.push(format!(
                    "pins[{}] rail {} 但角色 {} kind = power（校验⑥-b：功率面必须挂真实供电轨，缺 rail 或 none 即红）",
                    pin.rr3_pin_id(),
                    if pin.rail.is_empty() { "缺失" } else { "= none" },
                    pin.role
                ));
            }
        } else if !domain_names.contains(pin.rail.as_str()) {
            errs.push(format!(
                "pins[{}] rail = {} 未在 [[supplies.domain]] 在册（校验⑥-a；域名单源在该形态的域声明，§18 F-5）",
                pin.rr3_pin_id(),
                pin.rail
            ));
        }
    }
    errs
}

/// 装载期校验⑥-c/⑥-d/⑥-e 的域面：武装需测点与龄期上界、evidence 与 armed
/// 成对且形制合法、三个绑定引用位必须是 [pins] 的 pin_id、可切域必有开关位。
pub(crate) fn rr3_check_domains(profile: &Rr3Profile) -> Vec<String> {
    let mut errs = Vec::new();
    for d in &profile.supplies.domains {
        errs.extend(rr3_check_domain(d, &profile.pins));
    }
    errs
}

/// 单个电力域的五项判据（文案与批N21 件二同源），按武装/绑定/开关三面分立。
fn rr3_check_domain(d: &Rr3Domain, pins: &[Rr3PinEntry]) -> Vec<String> {
    let mut errs = Vec::new();
    let label = format!("supplies.domains[{}]", d.name);
    rr3_arm_check(d, &label, &mut errs);
    rr3_bind_check(d, pins, &label, &mut errs);
    if d.switchable && d.switch == "none" {
        errs.push(format!(
            "{label} switchable = true 但 switch = none（可切域必有开关控制位，两键成对）"
        ));
    }
    errs
}

/// ⑥-c/⑥-e 武装面：武装需测点与龄期上界，evidence 与 armed 成对且形制合法。
fn rr3_arm_check(d: &Rr3Domain, label: &str, errs: &mut Vec<String>) {
    if d.armed {
        rr3_arm_sense(d, label, errs);
        rr3_arm_ev(d, label, errs);
    } else if d.evidence != "none" {
        errs.push(format!(
            "{label} armed = false 但 evidence = {}（校验⑥-e：未武装不携证据，evidence 显式写 \"none\"——证据位与武装位成对，不出现过期引用）",
            if d.evidence.is_empty() { "缺失" } else { &d.evidence }
        ));
    }
}

/// ⑥-c：无测点/无龄期上界的域不得武装拒动。
fn rr3_arm_sense(d: &Rr3Domain, label: &str, errs: &mut Vec<String>) {
    if d.sense == "none" || d.max_age_ms <= 0 {
        errs.push(format!(
            "{label} armed = true 但 sense = none 或 max_age_ms ≤ 0（校验⑥-c：无测点/无龄期上界的域不得武装拒动——未标定即 armed=false，§3.1 降级语义）"
        ));
    }
}

/// ⑥-e：武装拒动必须引用一条形制合法的真板实测证据。
fn rr3_arm_ev(d: &Rr3Domain, label: &str, errs: &mut Vec<String>) {
    if d.evidence == "none" || d.evidence.is_empty() {
        errs.push(format!(
            "{label} armed = true 但 evidence = none（校验⑥-e：武装拒动必须引用一条真板实测证据，规则住 conventions §6.1；未标定即 armed = false）"
        ));
    } else if !rr_ipc::evidence::rr3_evidence_ref_ok(&d.evidence) {
        errs.push(format!(
            "{label}.evidence = {} 形制不合法（校验⑥-e：应为 <形态>/<四位编号>，如 实测/0001）",
            d.evidence
        ));
    }
}

/// ⑥-d 绑定位：sense/current_sense/switch 一律写 pin_id（"none" 是合法显式值）。
fn rr3_bind_check(d: &Rr3Domain, pins: &[Rr3PinEntry], label: &str, errs: &mut Vec<String>) {
    for (key, val) in [
        ("sense", &d.sense),
        ("current_sense", &d.current_sense),
        ("switch", &d.switch),
    ] {
        if val == "none" {
            continue;
        }
        if !pins.iter().any(|p| p.rr3_pin_id() == *val) {
            errs.push(format!(
                "{label}.{key} = {val} 不在 [pins] 的 pin_id 集（校验⑥-d：绑定位一律写 pin_id，\"none\" 是合法显式值——第二套语法如 adc:gp34 一律红，F-2）"
            ));
        }
    }
}

/// 装配扫描产物：模块名 → [(件名, facts)]（预算判据分子；坏形进 broken 旁路）
type Rr3Scan = Vec<(String, Vec<(String, Rr3AssembleFacts)>)>;

/// 装配扫描：逐模块读 atoms/*/assemble.toml；坏形/注入缺进 broken（该模块
/// Degraded，不 fatal），合法 facts 进 scanned（预算判据分子）。
pub(crate) fn rr3_scan_modules(
    root: &Path,
    profile: &Rr3Profile,
) -> (Rr3Scan, Vec<(String, String)>) {
    let mut broken = Vec::new();
    let mut scanned = Vec::new();
    for m in profile.enabled_slots.keys() {
        let (atoms, msgs) = rr3_assemble_scan(&root.join("modules").join(m));
        scanned.push((m.clone(), atoms));
        for msg in msgs {
            broken.push((m.clone(), msg));
        }
    }
    (scanned, broken)
}

/// 校验⑨（电流预算/覆盖）与校验⑩（内存预算）：分子来自装配扫描 facts，
/// 分母来自装配单；坏形件不进加总（可见面 = assemble_broken 点名）。
pub(crate) fn rr3_check_budgets(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    scanned: &Rr3Scan,
) -> Vec<String> {
    let mut errs = Vec::new();
    let declared = rr3_demand_expand(profile, roles, scanned);
    errs.extend(rr3_demand_budget(&declared, &profile.supplies.domains));
    errs.extend(rr3_power_gaps(profile, roles, scanned));
    let stack_sum: i64 = scanned
        .iter()
        .flat_map(|(_, atoms)| atoms.iter())
        .map(|(_, f)| f.task_stack_bytes)
        .sum();
    let heap_sum: i64 = scanned
        .iter()
        .flat_map(|(_, atoms)| atoms.iter())
        .map(|(_, f)| f.heap_peak_bytes)
        .sum();
    errs.extend(rr3_memory_budget(
        stack_sum,
        heap_sum,
        &profile.supplies.budget,
        &profile.platform_tier,
    ));
    errs
}

/// 校验②（后果 = 降不拒）：present ∩ enabled_slots 的缺角色清单
pub(crate) fn rr3_role_gaps(profile: &Rr3Profile, roles: &Rr3RoleTable) -> Vec<(String, String)> {
    let mut gaps = Vec::new();
    let enabled_all: BTreeSet<String> = profile.enabled_slots.values().flatten().cloned().collect();
    for slot in &enabled_all {
        if !profile.slots_present.contains(slot) {
            continue;
        }
        for role in roles.rr3_role_slot_names(slot) {
            // kind=logic 角色按定义无 [pins] 物理绑定（roles.toml 头注：logic =
            // 无物理绑定，不进 [pins]；链路/音频四格的真实载体是 [hardware]
            // link_ifname/alsa_device 与 board 具名装配）——它们不在缺角色点名面，
            // 否则"能力面已角色化"反会把 boot 判降（P1 件四）。
            if roles.rr3_role_kind(&role).as_deref() == Some("logic") {
                continue;
            }
            if !profile.pins.iter().any(|p| p.role == role) {
                gaps.push((slot.clone(), role));
            }
        }
    }
    gaps.sort();
    gaps
}

/// 缺格三源合一（方案 §15）：实测缺（探测）+ 声明缺（present）+ 注入缺
/// （enabled_slots 声明但无指向本模块的 binding）；基础层恒在位跳过。
pub(crate) fn rr3_slot_gaps(
    profile: &Rr3Profile,
    table: &Rr3SlotTable,
    roles: &Rr3RoleTable,
    probe: &dyn crate::probe::Rr3Probe,
) -> Vec<(String, String)> {
    let i2c_bind = rr3_i2c_bind(profile, roles);
    let mut missing = rr3_probe_gaps(profile, &i2c_bind, probe);
    rr3_declared_gaps(profile, table, &mut missing);
    missing.sort();
    missing
}

/// L-14 器件级探测的 (bus, addr) 组装：格位 → 该格角色在 [pins] 的 I2C 绑定
/// （controller 词形 i2c<N> → N，addr 非 none 非空才有效）。无绑定 → None →
/// 探针回落总线级五面判据（无 [pins] 形态兼容面）。
fn rr3_i2c_bind(profile: &Rr3Profile, roles: &Rr3RoleTable) -> BTreeMap<String, (u32, String)> {
    let mut i2c_bind = BTreeMap::new();
    let enabled_all: BTreeSet<String> = profile.enabled_slots.values().flatten().cloned().collect();
    for slot in &enabled_all {
        for role in roles.rr3_role_slot_names(slot) {
            for pin in profile.pins.iter().filter(|p| p.role == role) {
                let Some(n) = pin
                    .controller
                    .strip_prefix("i2c")
                    .and_then(|rest| rest.parse::<u32>().ok())
                else {
                    continue;
                };
                if !pin.addr.is_empty() && pin.addr != "none" {
                    i2c_bind
                        .entry(slot.clone())
                        .or_insert_with(|| (n, pin.addr.clone()));
                }
            }
        }
    }
    i2c_bind
}

/// 探测先行（MC-4）：bindings 声明的格位逐格探测；探测自身故障按"探测不到"
/// 处理（fail-closed，方案 §4）——缺格三源合一的"实测缺"即出自本环。
fn rr3_probe_gaps(
    profile: &Rr3Profile,
    i2c_bind: &BTreeMap<String, (u32, String)>,
    probe: &dyn crate::probe::Rr3Probe,
) -> Vec<(String, String)> {
    let mut missing = Vec::new();
    for (slot, module) in &profile.bindings {
        let (bus, addr) = match i2c_bind.get(slot) {
            Some((n, a)) => (Some(*n), Some(a.as_str())),
            None => (None, None),
        };
        if !probe.rr3_probe_slot_bound(slot, bus, addr) {
            missing.push((module.clone(), slot.clone()));
        }
    }
    missing
}

/// 声明缺/注入缺逐格补齐（去重；板说没有的格不再看 binding——绑了也白绑）
fn rr3_declared_gaps(
    profile: &Rr3Profile,
    table: &Rr3SlotTable,
    missing: &mut Vec<(String, String)>,
) {
    for (module, slots) in &profile.enabled_slots {
        for slot in slots {
            if table.rr3_slot_layer(slot).as_deref() == Some("base") {
                continue;
            }
            let absent = !profile.slots_present.contains(slot);
            if (absent || profile.bindings.get(slot) != Some(module))
                && !missing.contains(&(module.clone(), slot.clone()))
            {
                missing.push((module.clone(), slot.clone()));
            }
        }
    }
}

/// manifest 路径（模块名升序：BTreeMap 键序，确定性）
pub(crate) fn rr3_manifests(profile: &Rr3Profile, root: &Path) -> Vec<PathBuf> {
    profile
        .enabled_slots
        .keys()
        .map(|m| root.join("modules").join(m).join("module.toml"))
        .collect()
}

/// hal_bindings 注入文本：host 档派生视图 ∪ [hardware] 非引脚键（门 n 已保证
/// 物理键不双占，合并无覆盖歧义）；rtos/bare 档 derived 为空，[hardware] 原样。
pub(crate) fn rr3_hal_bindings(
    derived: BTreeMap<String, String>,
    hardware: &BTreeMap<String, String>,
) -> String {
    let mut merged = derived;
    for (k, v) in hardware {
        merged.insert(k.clone(), v.clone());
    }
    rr3_bindings_text(&merged)
}

/// 执行轴装载与投影：目录 = 接线档同级 axis/，pin_ids = [[pin]] 全集；
/// 装载一次，清单同时进装配快照（面板出图要看见全部轴），投影只吃这份清单。
pub(crate) fn rr3_axis_load(
    wiring_abs: &Path,
    pin_ids: &BTreeSet<String>,
) -> (Vec<crate::axis::Rr3AxisFile>, String, Vec<String>) {
    let axis_dir = wiring_abs.parent().expect("接线档必有父目录").join("axis");
    let axis_wanted = std::env::var(rr_hal_api::RR3_PERIPH_AXIS_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| rr_hal_api::RR3_PERIPH_AXIS_DEFAULT.to_string());
    let (axis_files, axis_notes) = rr3_axis_files_load(&axis_dir, pin_ids);
    let (axis_state, axis_notes_project) = rr3_axis_state_text(&axis_files, &axis_wanted);
    let mut axis_notes = axis_notes;
    axis_notes.extend(axis_notes_project);
    (axis_files, axis_state, axis_notes)
}
