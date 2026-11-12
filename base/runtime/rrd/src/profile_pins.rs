//! 装载期引脚能力 / 占用 / 冲突校验（校验③-a/③-b/③-c/④-b/⑤-a/⑤-b/⑧-b）。
//!
//! 自 `profile.rs` 拆出（批N45 第二刀）：本模块只持**判据**，数据与词表全部来自
//! 装配单实体（[`Rr3Profile`]）与平台/角色/器件契约表——判定不读文件、不持状态，
//! 纯函数可单测。调用序（capabilities → occupancy）由装载链持有，本模块不编排。
//! 校验项与错误文案逐字保持原样（门禁按文案点名消费，改文案即改契约）。

use std::collections::BTreeMap;

use rr_registry::{Rr3DeviceTable, Rr3PlatformTable, Rr3RoleTable};

use crate::profile::{Rr3PinEntry, Rr3Profile};

/// 装载期校验⑤-a/⑤-b/⑧-b（增补轮六 §5，后果 = 拒；施工图 Task 4）：方向与能力
/// 合法、控制器在册，判据真值 = 平台事实表（`contracts/platform/<chip>.toml`，
/// 基-22）。三条判定逐条可判：
/// - ⑧-b `pin.controller ∈ 平台在册控制器词表`（词表不是器件型号，D10a 同族；
///   controller 留空的缺失语义归校验①-b requires 判，此处不重复点名）；
/// - ⑤-a `direction = out` 且角色 `input_only_ok = false` → `physical` 不得落平台
///   input-only 脚集（input-only 脚无输出驱动能力，写 out 即假账）；
/// - ⑤-b 模拟测点必须落平台 ADC 能力集——测点 = 域 `sense`/`current_sense` 按
///   pin_id 引用的条目（⑥-d 的同一外键语法），其 `physical` 元素逐个对 ADC 集；
///   I2C 测点无直接脚（INA219 在片上量），`physical` 空即无判据对象，不算红。
pub(crate) fn rr3_pin_capabilities(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    platform: &Rr3PlatformTable,
) -> Vec<String> {
    let mut errs = Vec::new();
    let plat = platform.rr3_platform_name();
    for pin in &profile.pins {
        rr3_pin_caps(pin, roles, platform, plat, &mut errs);
    }
    rr3_pin_adc(profile, platform, plat, &mut errs);
    errs
}

/// 单脚能力判据：⑧-b 控制器在册 + ⑤-a out 落 input-only 集。
fn rr3_pin_caps(
    pin: &Rr3PinEntry,
    roles: &Rr3RoleTable,
    platform: &Rr3PlatformTable,
    plat: &str,
    errs: &mut Vec<String>,
) {
    let label = format!("pins[{}]", pin.rr3_pin_id());
    // ⑧-b 控制器在册（词表随平台表，装配单不得自造控制器）
    if !pin.controller.is_empty()
        && !platform
            .rr3_ctl_names()
            .iter()
            .any(|c| c == &pin.controller)
    {
        errs.push(format!(
            "{label} controller \"{}\" 不在平台 {plat} 的在册控制器词表（校验⑧-b——词表源 contracts/platform/{plat}.toml 的 [[controller]]，装配单不得自造控制器）",
            pin.controller
        ));
    }
    // ⑤-a output 落 input-only 集（角色 input_only_ok 的另一侧输入取 roles 表）
    if pin.direction == "out" && roles.rr3_role_input_only(&pin.role) == Some(false) {
        for p in &pin.physical {
            if platform.rr3_input_only_pins().contains(p) {
                errs.push(format!(
                    "{label} physical 元素 {p} 落在平台 {plat} 的 input-only 脚集（校验⑤-a：direction = out 且角色 {} input_only_ok = false，input-only 脚无输出驱动能力）",
                    pin.role
                ));
            }
        }
    }
}

/// ⑤-b 模拟测点判据：域 sense/current_sense 引用的 pin physical 全落 ADC 集。
fn rr3_pin_adc(
    profile: &Rr3Profile,
    platform: &Rr3PlatformTable,
    plat: &str,
    errs: &mut Vec<String>,
) {
    // 测点引用面 = 域绑定位，与 ⑥-d 同一 pin_id 语法
    for d in &profile.supplies.domains {
        for (key, val) in [("sense", &d.sense), ("current_sense", &d.current_sense)] {
            if val == "none" {
                continue;
            }
            let Some(pin) = profile.pins.iter().find(|p| &p.rr3_pin_id() == val) else {
                continue; // 悬空 pin_id 由 ⑥-d 点名，此处不重复报
            };
            rr3_adc_phys_check(pin, &d.name, key, platform, plat, errs);
        }
    }
}

/// 单 pin 的 physical 全落 ADC 集判据。
fn rr3_adc_phys_check(
    pin: &Rr3PinEntry,
    domain_name: &str,
    key: &str,
    platform: &Rr3PlatformTable,
    plat: &str,
    errs: &mut Vec<String>,
) {
    for p in &pin.physical {
        if !platform.rr3_adc_pins().contains(p) {
            errs.push(format!(
                "pins[{}] 是域 {domain_name} 的模拟测点（{key} 绑定），physical 元素 {p} 不在平台 {plat} 的 ADC 能力集（校验⑤-b：测点落非 ADC 脚读回恒值，档位恒真或恒假——F-9）",
                pin.rr3_pin_id()
            ));
        }
    }
}

/// 装载期校验③-a/③-b/③-c + ④-b（增补轮六 §5，后果 = 拒；施工图 Task 5）：不双占
/// 与互斥。三个资源空间各自 ≤ 1 次（启用集 = [pins] 全集——[[pin]] 无使能位，
/// 在册即启用）：
/// - ③-a 同一 `physical` 元素 ≤ 1 次；
/// - ③-b 同一 `(controller, channel)` ≤ 1 次（channel 缺席不占通道维，F-7：脚 +
///   通道两维，`board.rs:146` 双轴两通道即证）；
/// - ③-c 同一 `(bus, addr)` 的多条 pin 须**同 device 且型号在册**（批N3 件八精化，
///   全案 §5 :437：addr = "none" 是显式不适值，不占地址维；"双占"由无条件的 ≤ 1 次
///   精化为"共址须同器件"——P-5 裁定 charger 与 battery.meter 共用同一 INA219 读数
///   源是合法形态，一个物理器件服务两个角色不等于地址冲突；真冲突 = 同址而器件
///   身份不同或不可判（device 缺省 = 空串），两条各点名）。device 非空即须在册
///   （门 o 装载侧执法——装配单不得自造型号词），单条 pin 也判；
/// - ④-b 平台 `conflicts` 命中同一子集而未申报同组 ⇒ 漏声明即红——组名取自平台表
///   `[[conflict]].group`，装机档不得自起名（否则 `mux_group` 退回自愿填写）；命中
///   是单向含入：构造/冲突表按芯片事实收全，profile 现役绑定只是子集，平台表超出
///   现役的部分不算红（芯片事实 ≠ 现役占用）。
pub(crate) fn rr3_pin_occupancy(
    profile: &Rr3Profile,
    platform: &Rr3PlatformTable,
    devices: &Rr3DeviceTable,
) -> Vec<String> {
    let mut errs = Vec::new();
    // ③-a physical 元素唯一（BTreeMap 定化点名序：首占者记录）。owner 值用
    // pin 序号判"同一条"，不用 pin_id 字符串——per_instance=false 角色的多条
    // pin 无 instance 可分，pin_id 同名会把双占误判成"自己撞自己"而吞红。
    let mut phys_owner: BTreeMap<i64, usize> = BTreeMap::new();
    // ③-b (controller, channel) 唯一
    let mut chan_owner: BTreeMap<(String, i64), usize> = BTreeMap::new();
    // ③-c (bus, addr) 共址对账：owner 只记首占者索引，同址时对 device 判等
    let mut addr_owner: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (idx, pin) in profile.pins.iter().enumerate() {
        let label = format!("pins[{}]", pin.rr3_pin_id());
        rr3_pin_dev_check(&label, pin, devices, &mut errs);
        rr3_pin_phys_take(&label, profile, idx, pin, &mut phys_owner, &mut errs);
        rr3_pin_chan_take(&label, profile, idx, pin, &mut chan_owner, &mut errs);
        rr3_pin_addr_take(&label, profile, idx, pin, &mut addr_owner, &mut errs);
    }
    rr3_pin_mux_check(profile, platform, &mut errs);
    errs
}

/// 门 o 装载侧执法：device 非空即在册（型号词取值域 = contracts/devices.toml
/// 在册名单，装配单不得自造——单条 pin 也判，与共址无关）。
fn rr3_pin_dev_check(
    label: &str,
    pin: &Rr3PinEntry,
    devices: &Rr3DeviceTable,
    errs: &mut Vec<String>,
) {
    if !pin.device.is_empty() && !devices.rr3_dev_contains(&pin.device) {
        errs.push(format!(
            "{label} device \"{}\" 未在器件词表在册（门 o：device 外键 → contracts/devices.toml，装配单不得自造型号词）",
            pin.device
        ));
    }
}

/// ③-a：同一 physical 元素在启用集里 ≤ 1 次（首占者入表，后者点名双占）。
fn rr3_pin_phys_take(
    label: &str,
    profile: &Rr3Profile,
    idx: usize,
    pin: &Rr3PinEntry,
    owner: &mut BTreeMap<i64, usize>,
    errs: &mut Vec<String>,
) {
    for p in &pin.physical {
        match owner.get(p) {
            Some(first) if *first != idx => errs.push(format!(
                "{label} physical 元素 {p} 与 pins[{}] 双占（校验③-a：同一 physical 元素在启用集里 ≤ 1 次）",
                profile.pins[*first].rr3_pin_id()
            )),
            None => {
                owner.insert(*p, idx);
            }
            _ => {}
        }
    }
}

/// ③-b：同一 (controller, channel) ≤ 1 次（F-7 脚+通道两维；channel 缺席不占通道维）。
fn rr3_pin_chan_take(
    label: &str,
    profile: &Rr3Profile,
    idx: usize,
    pin: &Rr3PinEntry,
    owner: &mut BTreeMap<(String, i64), usize>,
    errs: &mut Vec<String>,
) {
    if let (true, Some(ch)) = (!pin.controller.is_empty(), pin.channel) {
        let key = (pin.controller.clone(), ch);
        match owner.get(&key) {
            Some(first) if *first != idx => errs.push(format!(
                "{label} (controller {}, channel {ch}) 与 pins[{}] 双占（校验③-b：同一 (controller, channel) ≤ 1 次——F-7 脚+通道两维）",
                pin.controller,
                profile.pins[*first].rr3_pin_id()
            )),
            None => {
                owner.insert(key, idx);
            }
            _ => {}
        }
    }
}

/// ③-c：同一 (bus, addr) 的多条 pin 须同 device 且非空（批N3 件八精化）。
/// 器件身份不同 = 真冲突（坑 7）；device 缺省 = 身份不可判，fail-closed 红。
fn rr3_pin_addr_take(
    label: &str,
    profile: &Rr3Profile,
    idx: usize,
    pin: &Rr3PinEntry,
    owner: &mut BTreeMap<(String, String), usize>,
    errs: &mut Vec<String>,
) {
    if pin.controller.is_empty() || pin.addr.is_empty() || pin.addr == "none" {
        return;
    }
    let key = (pin.controller.clone(), pin.addr.clone());
    match owner.get(&key) {
        Some(first) => rr3_addr_conflict(label, pin, &profile.pins[*first], errs),
        None => {
            owner.insert(key, idx);
        }
    }
}

/// ③-c 共址对账：同址须同 device 且非空（device 缺省 = 身份不可判，fail-closed 红）。
fn rr3_addr_conflict(
    label: &str,
    pin: &Rr3PinEntry,
    first_pin: &Rr3PinEntry,
    errs: &mut Vec<String>,
) {
    if pin.device.is_empty() || first_pin.device.is_empty() {
        errs.push(format!(
            "{label} (bus {}, addr {}) 与 pins[{}] 共址而 device 缺省（校验③-c：同一 (bus, addr) 的多条 pin 须同 device 且型号在册——共读源须显式登记器件身份，缺省 = 不可判即红）",
            pin.controller, pin.addr, first_pin.rr3_pin_id()
        ));
    } else if pin.device != first_pin.device {
        errs.push(format!(
            "{label} (bus {}, addr {}) 与 pins[{}] 共址而 device 不同（{} vs {}）——校验③-c：同一 (bus, addr) 的多条 pin 须同 device 且型号在册（器件身份不同 = 地址冲突，坑 7）",
            pin.controller,
            pin.addr,
            first_pin.rr3_pin_id(),
            first_pin.device,
            pin.device
        ));
    }
}

/// ④-b conflicts 反推互斥：同一条冲突登记面的命中者 ≥ 2 且任一未申报该组 → 红
/// （命中是单向含入：平台表里现役绑定之外的部分不算红）。
fn rr3_pin_mux_check(profile: &Rr3Profile, platform: &Rr3PlatformTable, errs: &mut Vec<String>) {
    let plat = platform.rr3_platform_name();
    for entry in platform.rr3_conflicts() {
        let group = entry.rr3_conflict_group();
        let hits: Vec<&Rr3PinEntry> = profile
            .pins
            .iter()
            .filter(|p| {
                p.physical
                    .iter()
                    .any(|x| entry.rr3_conflict_pins().contains(x))
                    || (!p.controller.is_empty()
                        && entry.rr3_conflict_ctls().contains(&p.controller))
            })
            .collect();
        if hits.len() < 2 {
            continue; // 冲突未成形：另一成员不在现役绑定集（单向含入语义）
        }
        for p in hits {
            if !p.mux_group.iter().any(|g| g == group) {
                errs.push(format!(
                    "pins[{}] 命中平台 {plat} conflicts 组 {group}（{}）但 mux_group 未申报该组（校验④-b：漏声明即红——组名取自平台表，装机档不得自起名）",
                    p.rr3_pin_id(),
                    entry.rr3_conflict_why()
                ));
            }
        }
    }
}
