//! 平台事实登记表（增补轮六 §4.6，基-22）：`contracts/platform/<平台>.toml` 的
//! 装载与形状检查。
//!
//! 生产真相只有 `contracts/platform/<平台>.toml`（按平台一份、按形态选用）。它承载
//! 一颗芯片的事实四段：`[[controller]]` 在册控制器词表（③-b 通道上界、⑧-b 键空间）、
//! `[capabilities]` 能力集（⑤-a input-only 集、⑤-b ADC 能力集）、`[[conflict]]`
//! 冲突矩阵（④-b 反推互斥）、`[construction]` 具名构造表（⑧-a 三元键对账）。
//! **不是器件型号**（D10a：装载期不认型号，器件能力住 devices.toml）、不是供给
//! 预算（住 profiles `[supplies]`）——三份数据三个主人，不许并表。
//!
//! **与 roles.rs/devices.rs 的关键差异：本表解析 fail-closed 回 `Err`**（施工图
//! Task 3 Interfaces）：roles/devices 对坏文本回空表走"表为空"点名，那是集合
//! 词表的宽容；平台表是校验 ④⑤⑧ 的**判据前件**——表读不到还当空表，④-b"平台
//! conflicts 集缺失 → 该平台所有 ④ 判据红"就会被静默跳过，正是 §18 F-6"缺字段
//! 可被留空绕过"的病。故任何一段缺、任何键坏都逐条点名回 `Err`，调用方必须
//! 拿着错误拒 boot，不许降级。
//!
//! 同文件内一致性（非跨表第二真值）：`[construction]` 键的 controller 段、
//! `[[conflict]]` 的 controllers 成员都必须命中本表 `[[controller]]` 在册——
//! 真值同源（都在本文件），判它不是立第二份词表。

use std::collections::{BTreeMap, BTreeSet};
use toml::Value;

/// 合法 kind 词域（§4.6 封闭七值；扩域走契约级评审）
pub const RR3_PLATFORM_KINDS: [&str; 7] = ["pwm", "gpio", "i2c", "spi", "vspi", "adc", "uart"];

/// `output_capable_pins` 在册取值规则（只认"对 input_only 取补集"这一条在册规则，
/// 不发明第二套规则语言；扩规则走契约级评审）
pub const RR3_PLATFORM_OUTPUT_RULES: [&str; 1] = ["complement(input_only_pins)"];

/// `[controller_wordform]` 键域（实-6 (a)，批N4 件七：host 词形派生表——封闭四值，
/// 只收"由控制器名派生取值"的物理键；physical[0]/channel/addr 类派生不经词形，
/// 不进本段）。扩键走契约级评审。
pub const RR3_PLATFORM_WORDFORM_KEYS: [&str; 4] =
    ["pwm_chip", "gpio_chip", "i2c_bus", "spi_device"];

/// rrd 装载链按 `[platform].chip` 拼文件名的契约目录（施工图 Task 3 Interfaces；
/// 路径相对 rr-registry crate 根向上三级到仓库根）
pub const RR3_CONTRACT_PLATFORM_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../contracts/platform/");

/// 一条控制器登记面
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3ControllerEntry {
    kind: String,
    channels: i64,
}

/// 平台能力集（⑤-a/⑤-b 的取值域）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rr3Capabilities {
    adc_pins: BTreeSet<i64>,
    input_only_pins: BTreeSet<i64>,
    no_open_drain: BTreeSet<i64>,
    output_rule: String,
}

/// 一条冲突登记面（④-b 反推互斥的集合：pins 与 controllers 至少一侧非空）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3ConflictEntry {
    group: String,
    pins: BTreeSet<i64>,
    controllers: BTreeSet<String>,
    why: String,
}

/// 一册平台表（不可变快照）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rr3PlatformTable {
    platform: String,
    controllers: BTreeMap<String, Rr3ControllerEntry>,
    capabilities: Rr3Capabilities,
    conflicts: Vec<Rr3ConflictEntry>,
    construction: BTreeMap<String, String>,
    /// host 词形派生表（实-6 (a)）：派生键 → 控制器名前缀。空表合法 = 本平台无
    /// host 词形派生面（rtos/bare 档显式声明）；host 档消费时查不到即 fail-closed
    /// （rrd 派生器逐键红）。
    controller_wordform: BTreeMap<String, String>,
}

/// 脚号数组解析：逐元素判非负整数，坏的逐条点名（不静默丢弃）
fn rr3_pin_set_from(v: &Value, what: &str, errs: &mut Vec<String>) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();
    if let Some(items) = v.as_array() {
        for (i, item) in items.iter().enumerate() {
            match item.as_integer() {
                Some(n) if n >= 0 => {
                    out.insert(n);
                }
                Some(n) => errs.push(format!("{what}[{i}] 脚号须非负，收到 {n}")),
                None => errs.push(format!("{what}[{i}] 应为整数脚号，收到 {item:?}")),
            }
        }
    } else {
        errs.push(format!("{what} 应为数组，收到 {v:?}"));
    }
    out
}

/// 字符串数组解析：逐元素判非空字符串，坏的逐条点名
fn rr3_str_set_from(v: &Value, what: &str, errs: &mut Vec<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(items) = v.as_array() {
        for (i, item) in items.iter().enumerate() {
            match item.as_str() {
                Some(s) if !s.trim().is_empty() => {
                    out.insert(s.to_string());
                }
                Some(_) => errs.push(format!("{what}[{i}] 不得为空串")),
                None => errs.push(format!("{what}[{i}] 应为字符串，收到 {item:?}")),
            }
        }
    } else {
        errs.push(format!("{what} 应为数组，收到 {v:?}"));
    }
    out
}

/// 解析 + 全量形状校验一份平台表文本。fail-closed：任何一段缺、任何键坏、
/// 任何取值出域都逐条点名回 `Err`（见模块头注释——宽容 = ④⑤⑧ 静默不判）。
pub fn rr3_registry_platform_table_from_text(text: &str) -> Result<Rr3PlatformTable, Vec<String>> {
    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("平台表 TOML 解析失败: {e}")]),
    };
    let Some(t) = root.as_table() else {
        return Err(vec![format!("平台表顶层必须是 TOML 表，收到 {root:?}")]);
    };
    let mut errs: Vec<String> = Vec::new();

    // 顶层未知键点名（只认七键）
    for k in t.keys() {
        if ![
            "schema_version",
            "platform",
            "controller",
            "capabilities",
            "conflict",
            "construction",
            "controller_wordform",
        ]
        .contains(&k.as_str())
        {
            errs.push(format!(
                "平台表顶层未知键 {k}（只认 schema_version / platform / controller / capabilities / conflict / construction / controller_wordform）"
            ));
        }
    }

    // schema_version：平台表自己的版本域（当前 1；破坏性变更才 bump）
    match t.get("schema_version") {
        Some(Value::Integer(1)) => {}
        Some(Value::Integer(v)) => errs.push(format!("平台表 schema_version 必须为 1，收到 {v}")),
        Some(other) => errs.push(format!("平台表 schema_version 应为整数，收到 {other:?}")),
        None => errs.push("平台表缺 schema_version（必须为 1）".into()),
    }

    let mut table = Rr3PlatformTable::default();

    // platform：非空字符串（与 profiles [platform] 的平台标识同源，§4.6）
    match t.get("platform") {
        Some(Value::String(s)) if !s.trim().is_empty() => table.platform = s.clone(),
        Some(Value::String(_)) => errs.push("平台表 platform 不得为空串".into()),
        Some(other) => errs.push(format!("平台表 platform 应为字符串，收到 {other:?}")),
        None => errs.push("平台表缺 platform 键".into()),
    }

    // [[controller]]：必须在册且非空（⑧-b 的键空间第一元；空表 = 词表事故）。
    // 先逐条收集再建映射，重复名在此点名。
    let mut entries: Vec<(String, String, i64)> = Vec::new();
    match t.get("controller") {
        Some(Value::Array(items)) if !items.is_empty() => {
            for (i, item) in items.iter().enumerate() {
                let Some(c) = item.as_table() else {
                    errs.push(format!(
                        "controller[{i}] 应为内联表 {{ name, kind, channels }}"
                    ));
                    continue;
                };
                for k in c.keys() {
                    if k != "name" && k != "kind" && k != "channels" {
                        errs.push(format!(
                            "controller[{i}] 未知键 {k}（只认 name / kind / channels）"
                        ));
                    }
                }
                let name = match c.get("name").and_then(Value::as_str) {
                    Some(n) if !n.trim().is_empty() => Some(n.to_string()),
                    Some(_) => {
                        errs.push(format!("controller[{i}].name 不得为空串"));
                        None
                    }
                    None => {
                        errs.push(format!("controller[{i}].name 应为字符串"));
                        None
                    }
                };
                let kind = match c.get("kind").and_then(Value::as_str) {
                    Some(k) if RR3_PLATFORM_KINDS.contains(&k) => Some(k.to_string()),
                    Some(k) => {
                        errs.push(format!(
                            "controller[{i}].kind \"{k}\" 非法（封闭七值 {RR3_PLATFORM_KINDS:?}，扩域走契约级评审）"
                        ));
                        None
                    }
                    None => {
                        errs.push(format!("controller[{i}].kind 应为字符串"));
                        None
                    }
                };
                let channels = match c.get("channels") {
                    Some(Value::Integer(n)) if *n >= 0 => Some(*n),
                    Some(Value::Integer(n)) => {
                        errs.push(format!("controller[{i}].channels 须非负，收到 {n}"));
                        None
                    }
                    Some(other) => {
                        errs.push(format!("controller[{i}].channels 应为整数，收到 {other:?}"));
                        None
                    }
                    None => {
                        errs.push(format!(
                            "controller[{i}] 缺 channels（0 = 无通道维，须显式写）"
                        ));
                        None
                    }
                };
                if let (Some(n), Some(k), Some(ch)) = (name, kind, channels) {
                    entries.push((n, k, ch));
                }
            }
        }
        Some(Value::Array(_)) => errs.push(
            "平台表 [[controller]] 为空（在册控制器词表为空 = ⑧-b 无键空间可判，词表事故）".into(),
        ),
        Some(other) => errs.push(format!(
            "平台表 controller 应为数组（[[controller]]），收到 {other:?}"
        )),
        None => errs.push("平台表缺 [[controller]] 段（在册控制器词表必填）".into()),
    }
    for (n, k, ch) in entries {
        if table.controllers.contains_key(&n) {
            errs.push(format!("controller {n} 重复（词表一名一条）"));
        } else {
            table.controllers.insert(
                n,
                Rr3ControllerEntry {
                    kind: k,
                    channels: ch,
                },
            );
        }
    }

    // [capabilities]：段缺即红（裁定①；空集合法但必须显式写键）
    match t.get("capabilities") {
        Some(Value::Table(caps)) => {
            for k in caps.keys() {
                if k != "adc_pins"
                    && k != "input_only_pins"
                    && k != "no_open_drain"
                    && k != "output_capable_pins"
                {
                    errs.push(format!(
                        "[capabilities] 未知键 {k}（只认 adc_pins / input_only_pins / no_open_drain / output_capable_pins）"
                    ));
                }
            }
            match caps.get("adc_pins") {
                Some(v) => table.capabilities.adc_pins = rr3_pin_set_from(v, "adc_pins", &mut errs),
                None => errs.push(
                    "[capabilities] 缺 adc_pins（无 ADC 须显式写 [] 并带行级注记，裁定②）".into(),
                ),
            }
            match caps.get("input_only_pins") {
                Some(v) => {
                    table.capabilities.input_only_pins =
                        rr3_pin_set_from(v, "input_only_pins", &mut errs)
                }
                None => {
                    errs.push("[capabilities] 缺 input_only_pins（无此类脚须显式写 []）".into())
                }
            }
            match caps.get("no_open_drain") {
                Some(v) => {
                    table.capabilities.no_open_drain =
                        rr3_pin_set_from(v, "no_open_drain", &mut errs)
                }
                None => errs.push("[capabilities] 缺 no_open_drain（无此类脚须显式写 []）".into()),
            }
            match caps.get("output_capable_pins").and_then(Value::as_str) {
                Some(r) if RR3_PLATFORM_OUTPUT_RULES.contains(&r) => {
                    table.capabilities.output_rule = r.to_string();
                }
                Some(r) => errs.push(format!(
                    "[capabilities].output_capable_pins \"{r}\" 不在册（在册规则 {RR3_PLATFORM_OUTPUT_RULES:?}，扩规则走契约级评审）"
                )),
                None => errs.push(
                    "[capabilities] 缺 output_capable_pins（取值规则集，§4.6：写规则不写枚举）"
                        .into(),
                ),
            }
        }
        Some(other) => errs.push(format!(
            "平台表 capabilities 应为表（[capabilities]），收到 {other:?}"
        )),
        None => errs.push("平台表缺 [capabilities] 段（⑤-a/⑤-b 判据前件，缺段即红，裁定①）".into()),
    }

    // conflict：键必须存在（空数组合法但显式，裁定②）；每条 group 唯一、
    // pins/controllers 至少一侧非空、why 必填（无可读理由的冲突不许进册）
    let mut groups_seen = BTreeSet::<String>::new();
    match t.get("conflict") {
        Some(Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                let Some(c) = item.as_table() else {
                    errs.push(format!(
                        "conflict[{i}] 应为内联表 {{ group, pins/controllers, why }}"
                    ));
                    continue;
                };
                for k in c.keys() {
                    if k != "group" && k != "pins" && k != "controllers" && k != "why" {
                        errs.push(format!(
                            "conflict[{i}] 未知键 {k}（只认 group / pins / controllers / why）"
                        ));
                    }
                }
                let group = match c.get("group").and_then(Value::as_str) {
                    Some(g) if !g.trim().is_empty() => Some(g.to_string()),
                    Some(_) => {
                        errs.push(format!("conflict[{i}].group 不得为空串"));
                        None
                    }
                    None => {
                        errs.push(format!("conflict[{i}].group 应为字符串"));
                        None
                    }
                };
                let mut pins = BTreeSet::new();
                let mut controllers = BTreeSet::new();
                if let Some(v) = c.get("pins") {
                    pins = rr3_pin_set_from(v, &format!("conflict[{i}].pins"), &mut errs);
                }
                if let Some(v) = c.get("controllers") {
                    controllers =
                        rr3_str_set_from(v, &format!("conflict[{i}].controllers"), &mut errs);
                }
                if pins.is_empty() && controllers.is_empty() {
                    errs.push(format!(
                        "conflict[{i}]（组 {}）pins 与 controllers 均空：点名不到任何成员的冲突集无判据对象",
                        group.as_deref().unwrap_or("")
                    ));
                }
                for ctl in &controllers {
                    if !table.controllers.contains_key(ctl) {
                        errs.push(format!(
                            "conflict[{i}] 的 controllers 成员 {ctl} 不在本表 [[controller]] 在册（同文件真值，先立词表再引）"
                        ));
                    }
                }
                match c.get("why").and_then(Value::as_str) {
                    Some(w) if !w.trim().is_empty() => {}
                    Some(_) => errs.push(format!("conflict[{i}].why 不得为空串")),
                    None => errs.push(format!("conflict[{i}] 缺 why（进册的冲突必须带可读理由）")),
                }
                if let Some(g) = group {
                    if !g.trim().is_empty() && !(pins.is_empty() && controllers.is_empty()) {
                        if !groups_seen.insert(g.clone()) {
                            errs.push(format!(
                                "conflict[{i}] 组名 {g} 重复（一组一条，互斥集不许同名两册）"
                            ));
                        }
                        table.conflicts.push(Rr3ConflictEntry {
                            group: g,
                            pins,
                            controllers,
                            why: c
                                .get("why")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                        });
                    }
                }
            }
        }
        Some(other) => errs.push(format!(
            "平台表 conflict 应为数组（[[conflict]] / []），收到 {other:?}"
        )),
        None => errs.push(
            "平台表缺 conflict 键（无冲突也必须显式写 conflict = [] 并带'评审确认无冲突矩阵'注记，裁定②）"
                .into(),
        ),
    }

    // [construction]：段缺即红（裁定①）；空表合法（无绑定形态显式空）。键 =
    // "controller:role:instance" 三元串（⑧-a 定形），controller 段必须在本表
    // [[controller]] 在册（同文件真值）。
    match t.get("construction") {
        Some(Value::Table(cons)) => {
            for (k, v) in cons {
                let segs: Vec<&str> = k.split(':').collect();
                if segs.len() != 3 {
                    errs.push(format!(
                        "construction 键 \"{k}\" 不是三元串（⑧-a：controller:role:instance，非 per_instance 角色第三段留空）"
                    ));
                    continue;
                }
                if segs[0].is_empty() || segs[1].is_empty() {
                    errs.push(format!(
                        "construction 键 \"{k}\" 的 controller/role 段不得为空（instance 段可空）"
                    ));
                    continue;
                }
                if !table.controllers.contains_key(segs[0]) {
                    errs.push(format!(
                        "construction 键 \"{k}\" 的 controller 段 {} 不在本表 [[controller]] 在册（同文件真值，先立词表再引）",
                        segs[0]
                    ));
                    continue;
                }
                match v.as_str() {
                    Some(p) if !p.trim().is_empty() => {
                        table.construction.insert(k.clone(), p.to_string());
                    }
                    Some(_) => errs.push(format!("construction[\"{k}\"] 构造路径不得为空串")),
                    None => errs.push(format!(
                        "construction[\"{k}\"] 应为字符串（平台侧构造路径，§4.5），收到 {v:?}"
                    )),
                }
            }
        }
        Some(other) => errs.push(format!(
            "平台表 construction 应为表（[construction]），收到 {other:?}"
        )),
        None => errs
            .push("平台表缺 [construction] 段（⑧ 对账前件；无绑定形态显式写空表，裁定①）".into()),
    }

    // [controller_wordform]：host 词形派生表（实-6 (a)，批N4 件七——规则自 rrd
    // 硬编码下移平台表）。段缺即红（裁定①）；空表合法 = 显式声明本平台无 host
    // 词形派生面（rtos/bare 档经 [[construction]] 具名构造直给，esp32 形状）；
    // host 档消费侧（rrd 派生器）对查不到的词形逐键 fail-closed，空表配 host 档
    // 必红。键域封闭四值，值 = 非空前缀字符串。
    match t.get("controller_wordform") {
        Some(Value::Table(wf)) => {
            for (k, v) in wf {
                if !RR3_PLATFORM_WORDFORM_KEYS.contains(&k.as_str()) {
                    errs.push(format!(
                        "[controller_wordform] 未知键 {k}（只认词形派生四键 {RR3_PLATFORM_WORDFORM_KEYS:?}；physical/channel/addr 类派生不经词形）"
                    ));
                    continue;
                }
                match v.as_str() {
                    Some(p) if !p.trim().is_empty() => {
                        table.controller_wordform.insert(k.clone(), p.to_string());
                    }
                    Some(_) => errs.push(format!(
                        "[controller_wordform].{k} 词形前缀不得为空串（无词形须删键走空表显式）"
                    )),
                    None => errs.push(format!(
                        "[controller_wordform].{k} 应为字符串前缀，收到 {v:?}"
                    )),
                }
            }
        }
        Some(other) => errs.push(format!(
            "平台表 controller_wordform 应为表（[controller_wordform]），收到 {other:?}"
        )),
        None => errs.push(
            "平台表缺 [controller_wordform] 段（host 词形派生表，实-6 (a)；无词形派生面须显式写空表并带注记，裁定①）"
                .into(),
        ),
    }

    if errs.is_empty() {
        Ok(table)
    } else {
        Err(errs)
    }
}

impl Rr3PlatformTable {
    pub fn rr3_platform_name(&self) -> &str {
        &self.platform
    }

    /// 在册控制器名（升序，BTreeMap 序）
    pub fn rr3_platform_controller_names(&self) -> Vec<String> {
        self.controllers.keys().cloned().collect()
    }

    /// 控制器通道上界（③-b 判 (controller, channel) 不双占的分母；0 = 无通道维）
    pub fn rr3_platform_controller_channels(&self, name: &str) -> Option<i64> {
        self.controllers.get(name).map(|e| e.channels)
    }

    /// ⑤-b 判据源：模拟测点必须落此集
    pub fn rr3_platform_adc_pins(&self) -> &BTreeSet<i64> {
        &self.capabilities.adc_pins
    }

    /// ⑤-a 判据源：direction=out 且 input_only_ok=false 的 physical 不得落此集
    pub fn rr3_platform_input_only_pins(&self) -> &BTreeSet<i64> {
        &self.capabilities.input_only_pins
    }

    pub fn rr3_platform_no_open_drain_pins(&self) -> &BTreeSet<i64> {
        &self.capabilities.no_open_drain
    }

    /// ④-b 判据源：冲突矩阵（每条 = 一组互斥成员 + 组名 + 可读理由）
    pub fn rr3_platform_conflicts(&self) -> &[Rr3ConflictEntry] {
        &self.conflicts
    }

    /// ⑧-a 判定：三元 (controller, role, instance) 是否有具名构造（非 per_instance
    /// 角色的 instance 传空串，与数据文件"第三段留空"同形）
    pub fn rr3_platform_construction_contains(
        &self,
        controller: &str,
        role: &str,
        instance: &str,
    ) -> bool {
        self.construction
            .contains_key(&format!("{controller}:{role}:{instance}"))
    }

    /// ⑧ 对账反向用：某三元键的具名构造路径
    pub fn rr3_platform_construction_get(&self, key: &str) -> Option<&str> {
        self.construction.get(key).map(String::as_str)
    }

    pub fn rr3_platform_construction_keys(&self) -> Vec<String> {
        self.construction.keys().cloned().collect()
    }

    /// host 词形前缀查询（实-6 (a)，批N4 件七）：派生键 → 控制器名前缀。查不到 =
    /// 本平台无该键词形（空表或未录）——rrd 派生器拿 None 必须 fail-closed 红，
    /// 不许回退硬编码前缀（这正是下移的纪律面）。
    pub fn rr3_platform_wordform(&self, key: &str) -> Option<&str> {
        self.controller_wordform.get(key).map(String::as_str)
    }

    /// 词形表全量（BTreeMap 升序引用）——装载期 ①-f 词形漂移判据遍历用：表里
    /// 有什么判什么，rrd 零硬编码词形清单（新增词形键 = 改数据，判据自动扩）。
    pub fn rr3_platform_wordform_map(&self) -> &BTreeMap<String, String> {
        &self.controller_wordform
    }
}

impl Rr3ConflictEntry {
    pub fn rr3_conflict_group(&self) -> &str {
        &self.group
    }

    pub fn rr3_conflict_pins(&self) -> &BTreeSet<i64> {
        &self.pins
    }

    pub fn rr3_conflict_controllers(&self) -> &BTreeSet<String> {
        &self.controllers
    }

    pub fn rr3_conflict_why(&self) -> &str {
        &self.why
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // include_str! 以本文件所在目录为基准：src → rr-registry → runtime → base → 仓库根
    const ESP32: &str = include_str!("../../../../contracts/platform/esp32.toml");
    const ESP32S3: &str = include_str!("../../../../contracts/platform/esp32s3.toml");
    const RK3566: &str = include_str!("../../../../contracts/platform/rk3566.toml");
    const RPI4: &str = include_str!("../../../../contracts/platform/raspberry-pi-4.toml");

    /// 四份在册平台表全部可解析（演练两份的"空集显式"是裁定②的形状，不是缺段）
    #[test]
    fn contract_tables_parse() {
        let esp = rr3_registry_platform_table_from_text(ESP32).expect("esp32 表须可解析");
        assert_eq!(esp.rr3_platform_name(), "esp32");
        let s3 = rr3_registry_platform_table_from_text(ESP32S3).expect("esp32s3 表须可解析");
        assert_eq!(s3.rr3_platform_name(), "esp32s3");
        let rk = rr3_registry_platform_table_from_text(RK3566).expect("rk3566 表须可解析");
        assert_eq!(rk.rr3_platform_name(), "rk3566");
        let rpi = rr3_registry_platform_table_from_text(RPI4).expect("rpi4 表须可解析");
        assert_eq!(rpi.rr3_platform_name(), "raspberry-pi-4");
    }

    /// esp32 表形状 pin（与 board.rs 引脚真值段同源；改数据须同批改这里）
    #[test]
    fn esp32_shape_is_pinned() {
        let esp = rr3_registry_platform_table_from_text(ESP32).expect("esp32 表须可解析");
        assert_eq!(
            esp.rr3_platform_controller_names(),
            vec!["adc1", "gpio0", "i2c0", "ledc0", "uart2", "vspi"]
        );
        assert_eq!(esp.rr3_platform_controller_channels("ledc0"), Some(16));
        assert_eq!(esp.rr3_platform_controller_channels("gpio0"), Some(0));
        // ⑤-b：GPIO34（ADC_RAIL_PIN）在 ADC 集。GPIO32/33 双职能（LEDC ch0/ch1 +
        // ADC1_CH4/CH5，ESP32 IO 矩阵）——芯片事实两集同在，"现役占用是否冲突"
        // 由绑定/构造对账判（⑧），能力面不裁现役。
        assert!(esp.rr3_platform_adc_pins().contains(&34));
        assert!(esp.rr3_platform_adc_pins().contains(&32));
        // ⑤-a：GPIO34 input-only；GPIO32 可输出（LEDC ch0 在用）
        assert!(esp.rr3_platform_input_only_pins().contains(&34));
        assert!(!esp.rr3_platform_input_only_pins().contains(&32));
        // 实-6 (a)：esp32 为 rtos 档，词形派生面空表显式（非缺段——缺段在
        // missing_sections_and_keys_are_named 反例判）
        assert!(esp.rr3_platform_wordform_map().is_empty());
        // ④-b：I2C0 硬线 {21,22} 与 UART2 {18,19} 两组互斥集
        assert!(esp
            .rr3_platform_conflicts()
            .iter()
            .any(|c| c.rr3_conflict_group() == "i2c0"
                && c.rr3_conflict_pins() == &BTreeSet::from([21, 22])));
        assert!(esp
            .rr3_platform_conflicts()
            .iter()
            .any(|c| c.rr3_conflict_group() == "uart2"
                && c.rr3_conflict_pins() == &BTreeSet::from([18, 19])));
        // ⑧-a：板侧现役两个构造三元键（GPIO32 pan / GPIO33 tilt）
        assert!(esp.rr3_platform_construction_contains("ledc0", "act.axis.pwm", "pan"));
        assert!(esp.rr3_platform_construction_contains("ledc0", "act.axis.pwm", "tilt"));
        assert_eq!(
            esp.rr3_platform_construction_get("ledc0:act.axis.pwm:pan"),
            Some("ledc")
        );
    }

    /// 演练两份的形状 pin：空集显式 + 构造表与现役 profile 对账
    #[test]
    fn rehearsal_shapes_are_pinned() {
        let rk = rr3_registry_platform_table_from_text(RK3566).expect("rk3566 表须可解析");
        assert!(rk.rr3_platform_adc_pins().is_empty());
        assert!(rk.rr3_platform_conflicts().is_empty());
        // default.toml [pins] 现役四绑定逐条有构造
        assert!(rk.rr3_platform_construction_contains("pwm0", "act.axis.pwm", "pan"));
        assert!(rk.rr3_platform_construction_contains("gpiochip0", "act.led.gpio", ""));
        assert!(rk.rr3_platform_construction_contains("gpiochip0", "act.relay.gpio", ""));
        assert!(rk.rr3_platform_construction_contains("i2c1", "power.battery.meter", ""));
        let rpi = rr3_registry_platform_table_from_text(RPI4).expect("rpi4 表须可解析");
        assert!(rpi.rr3_platform_adc_pins().is_empty());
        assert!(rpi.rr3_platform_conflicts().is_empty());
        assert!(rpi.rr3_platform_construction_keys().is_empty());
        assert_eq!(rpi.rr3_platform_controller_channels("pwm0"), Some(2));
        assert!(rpi
            .rr3_platform_controller_names()
            .contains(&"spidev0".to_string()));
        assert_eq!(rpi.rr3_platform_controller_channels("spidev0"), Some(1));
        // 实-6 (a)：host 形态两份词形派生表同形同值（Linux sysfs 命名习惯）——
        // rr3 派生器的词形前件真值点，四键封闭
        for t in [rk, rpi] {
            assert_eq!(
                t.rr3_platform_wordform_map(),
                &BTreeMap::from([
                    ("pwm_chip".to_string(), "pwm".to_string()),
                    ("gpio_chip".to_string(), "gpiochip".to_string()),
                    ("i2c_bus".to_string(), "i2c".to_string()),
                    ("spi_device".to_string(), "spidev".to_string()),
                ])
            );
        }
    }

    /// 缺失语义（裁定①）：最小文本一次点名全部五段；单键缺与段缺同红
    #[test]
    fn missing_sections_and_keys_are_named() {
        // 五段全缺的最小文本：controller / capabilities / conflict / construction /
        // controller_wordform 各点名一条
        let errs = rr3_registry_platform_table_from_text("schema_version = 1\nplatform = \"t\"\n")
            .expect_err("缺五段必须红");
        for needle in [
            "controller",
            "capabilities",
            "conflict",
            "construction",
            "controller_wordform",
        ] {
            assert!(
                errs.iter().any(|e| e.contains(needle)),
                "{needle}: {errs:?}"
            );
        }

        // 缺 [capabilities]（施工图 Task 3 Step 3 反例原文形状）
        let no_caps = ESP32.replace("[capabilities]", "[capabilities_typo]");
        let errs =
            rr3_registry_platform_table_from_text(&no_caps).expect_err("缺 [capabilities] 须红");
        assert!(errs.iter().any(|e| e.contains("capabilities")), "{errs:?}");

        // [capabilities] 缺单键：显式空集的纪律——缺键 ≠ 空
        let no_adc = ESP32.replace("adc_pins = [32, 33, 34, 35, 36, 39]", "");
        let errs = rr3_registry_platform_table_from_text(&no_adc).expect_err("缺 adc_pins 须红");
        assert!(errs.iter().any(|e| e.contains("adc_pins")), "{errs:?}");
    }

    /// 形状坏逐条点名：kind 出域 / 构造键非三元 / 构造 controller 不在册 /
    /// 冲突集双空 / 组名重复 / output 规则不在册 / 脚号负数
    #[test]
    fn bad_shapes_are_named() {
        let bad_kind = ESP32.replace("kind = \"pwm\"", "kind = \"dma\"");
        let errs = rr3_registry_platform_table_from_text(&bad_kind).expect_err("kind 出域须红");
        assert!(
            errs.iter().any(|e| e.contains("kind") && e.contains("dma")),
            "{errs:?}"
        );

        let bad_key = ESP32.replace(
            "\"ledc0:act.axis.pwm:pan\" = \"ledc\"",
            "\"ledc0.act.axis.pwm.pan\" = \"ledc\"",
        );
        let errs = rr3_registry_platform_table_from_text(&bad_key).expect_err("构造键非三元须红");
        assert!(errs.iter().any(|e| e.contains("三元串")), "{errs:?}");

        let bad_ctl = ESP32.replace(
            "\"ledc0:act.axis.pwm:pan\" = \"ledc\"",
            "\"ledc9:act.axis.pwm:pan\" = \"ledc\"",
        );
        let errs = rr3_registry_platform_table_from_text(&bad_ctl)
            .expect_err("构造 controller 不在册须红");
        assert!(errs.iter().any(|e| e.contains("ledc9")), "{errs:?}");

        let empty_conflict = ESP32.replace(
            "pins = [21, 22]\ngroup = \"i2c0\"",
            "pins = []\ngroup = \"i2c0\"",
        );
        let errs =
            rr3_registry_platform_table_from_text(&empty_conflict).expect_err("冲突集双空须红");
        assert!(errs.iter().any(|e| e.contains("无判据对象")), "{errs:?}");

        let dup_group = ESP32.replace("group = \"uart2\"", "group = \"i2c0\"");
        let errs = rr3_registry_platform_table_from_text(&dup_group).expect_err("组名重复须红");
        assert!(errs.iter().any(|e| e.contains("重复")), "{errs:?}");

        let bad_rule = ESP32.replace(
            "output_capable_pins = \"complement(input_only_pins)\"",
            "output_capable_pins = \"everything\"",
        );
        let errs =
            rr3_registry_platform_table_from_text(&bad_rule).expect_err("output 规则不在册须红");
        assert!(errs.iter().any(|e| e.contains("不在册")), "{errs:?}");

        let neg_pin = ESP32.replace(
            "input_only_pins = [34, 35, 36, 39]",
            "input_only_pins = [-1]",
        );
        let errs = rr3_registry_platform_table_from_text(&neg_pin).expect_err("负脚号须红");
        assert!(errs.iter().any(|e| e.contains("非负")), "{errs:?}");

        // 实-6 (a) 词形段三反例：键出域 / 值空串 / 段缺（esp32 空表换段名即段缺）
        let bad_wf_key = ESP32.replace(
            "[controller_wordform]",
            "[controller_wordform]\ngpio_line = \"gpio\"",
        );
        let errs = rr3_registry_platform_table_from_text(&bad_wf_key).expect_err("词形键出域须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("controller_wordform") && e.contains("gpio_line")),
            "{errs:?}"
        );

        let empty_wf_val = RPI4.replace("pwm_chip = \"pwm\"", "pwm_chip = \"\"");
        let errs =
            rr3_registry_platform_table_from_text(&empty_wf_val).expect_err("词形前缀空串须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("pwm_chip") && e.contains("不得为空串")),
            "{errs:?}"
        );

        let no_wf = RK3566.replace("[controller_wordform]", "[controller_wordform_x]");
        let errs = rr3_registry_platform_table_from_text(&no_wf).expect_err("词形段缺须红");
        assert!(
            errs.iter().any(|e| e.contains("缺 [controller_wordform]")),
            "{errs:?}"
        );
        // 顶层未知键点名同步照出（改名后的 controller_wordform_x 是顶层未知键）
        assert!(errs.iter().any(|e| e.contains("顶层未知键")), "{errs:?}");
    }
}
