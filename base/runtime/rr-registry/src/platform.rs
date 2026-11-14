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
pub(super) fn rr3_pin_set_from(v: &Value, what: &str, errs: &mut Vec<String>) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();
    let Some(items) = v.as_array() else {
        errs.push(format!("{what} 应为数组，收到 {v:?}"));
        return out;
    };
    for (i, item) in items.iter().enumerate() {
        if let Some(n) = rr3_pin_item(item, i, what, errs) {
            out.insert(n);
        }
    }
    out
}

/// 单个脚号元素：非负整数入集，坏的逐条点名。
fn rr3_pin_item(item: &Value, i: usize, what: &str, errs: &mut Vec<String>) -> Option<i64> {
    let Some(n) = item.as_integer() else {
        errs.push(format!("{what}[{i}] 应为整数脚号，收到 {item:?}"));
        return None;
    };
    if n >= 0 {
        return Some(n);
    }
    errs.push(format!("{what}[{i}] 脚号须非负，收到 {n}"));
    None
}

/// 字符串数组解析：逐元素判非空字符串，坏的逐条点名
pub(super) fn rr3_str_set_from(v: &Value, what: &str, errs: &mut Vec<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(items) = v.as_array() else {
        errs.push(format!("{what} 应为数组，收到 {v:?}"));
        return out;
    };
    for (i, item) in items.iter().enumerate() {
        if let Some(s) = rr3_str_item(item, i, what, errs) {
            out.insert(s);
        }
    }
    out
}

/// 单个字符串元素：非空串入集，坏的逐条点名。
fn rr3_str_item(item: &Value, i: usize, what: &str, errs: &mut Vec<String>) -> Option<String> {
    let Some(s) = item.as_str() else {
        errs.push(format!("{what}[{i}] 应为字符串，收到 {item:?}"));
        return None;
    };
    if !s.trim().is_empty() {
        return Some(s.to_string());
    }
    errs.push(format!("{what}[{i}] 不得为空串"));
    None
}

/// 解析 + 全量形状校验一份平台表文本。fail-closed：任何一段缺、任何键坏、
/// 任何取值出域都逐条点名回 `Err`（见模块头注释——宽容 = ④⑤⑧ 静默不判）。
pub fn rr3_plat_parse(text: &str) -> Result<Rr3PlatformTable, Vec<String>> {
    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("平台表 TOML 解析失败: {e}")]),
    };
    let Some(t) = root.as_table() else {
        return Err(vec![format!("平台表顶层必须是 TOML 表，收到 {root:?}")]);
    };
    let mut errs: Vec<String> = Vec::new();
    let mut table = Rr3PlatformTable::default();

    rr3_plat_head(t, &mut table, &mut errs);

    // [[controller]]：必须在册且非空（⑧-b 的键空间第一元；空表 = 词表事故）。
    rr3_plat_ctl(t, &mut table, &mut errs);

    // [capabilities]：段缺即红（裁定①；空集合法但必须显式写键）
    rr3_plat_caps(t, &mut table, &mut errs);

    // conflict：键必须存在（空数组合法但显式，裁定②）；每条 group 唯一、
    // pins/controllers 至少一侧非空、why 必填（无可读理由的冲突不许进册）
    rr3_plat_conf(t, &mut table, &mut errs);

    // [construction]：三元键具名构造表；[controller_wordform]：host 词形派生表。
    rr3_plat_cons(t, &mut table, &mut errs);
    rr3_plat_wf(t, &mut table, &mut errs);

    rr3_plat_finish(table, errs)
}

/// 收尾：错误集空即交表，否则整册拒（收集式校验，文案次序不变）。
fn rr3_plat_finish(
    table: Rr3PlatformTable,
    errs: Vec<String>,
) -> Result<Rr3PlatformTable, Vec<String>> {
    if errs.is_empty() {
        Ok(table)
    } else {
        Err(errs)
    }
}

mod caps;
mod conf;
mod cons;
mod ctl;
mod head;
mod view;

use caps::*;
use conf::*;
use cons::*;
use ctl::*;
use head::*;
