//! Atom manifest（`atoms/<名>/atom.toml`）的字段契约与校验（0.0.5 批B①，ADR-0019）。
//!
//! Atom 层 manifest 只声明四事（0.0.5 方案 §9）：占哪个格（`provides.slots`，每格一件
//! 恰一项）、装配期依赖哪些格（`uses.slots`）、publishes/subscribes（topic 形状）、
//! **板无关**（manifest 文本禁词断言）。字段契约内嵌于 `atom.schema.toml`，与
//! `module.schema.toml` 同构（`[[field]]` 数组），规则函数复用 [`crate::validate`] 的
//! `rr3_registry_check`——不建第二套规则实现。
//!
//! 查册门共用 [`crate::slots::Rr3SlotTable`] 真值（ADR-0012 裁定 5②）：`provides`/
//! `uses` 的每一格都必须命中 `contracts/slots.toml`，空格位表 fail-closed（承
//! `validate.rs`/`topics.rs` 的空注册表纪律，空表不是放行许可证）。
//!
//! 板无关断言（ADR-0019 裁定 5 硬规 2 的 manifest 侧）：Atom manifest 文本不得出现
//! 引脚/总线/厂商/产品 token——板级拓扑只进 `rr-hal` 后端与描述符绑定表，策略与器件
//! 决策只经 `ops`/config 数据化注入。禁词表是 rock3 自有清单（从本仓实况板卡与器件
//! 名提炼），ADR-0019 明确不搬外部模板禁词表。

use std::path::{Path, PathBuf};
use toml::Value;

use crate::schema::{rr3_get_by_path, Rr3FieldSpec};

/// Atom manifest 契约 schema 版本（对应 `atom.schema.toml` 的 `schema_version` 键）
pub const RR3_ATOM_SCHEMA_VERSION: u32 = 1;

const RR3_ATOM_SCHEMA_TOML: &str = include_str!("atom.schema.toml");

/// 板无关禁词表（引脚 / 硬件总线 / 设备路径 / 厂商 / 产品·芯片·器件名）。
/// 子串匹配 + 小写比较；命中即门禁失败，报错点名 token。
const RR3_BOARD_TOKENS: &[&str] = &[
    // 引脚与硬件总线：板级拓扑只进 rr-hal 后端与描述符，不进原子契约
    "gpio",
    "i2c",
    "spi",
    "uart",
    "pwm",
    "bcm",
    "/dev/",
    // 厂商
    "rockchip",
    "raspberry",
    "broadcom",
    "espressif",
    "allwinner",
    "sunxi",
    "amlogic",
    // 产品 / 芯片 / 器件（本仓实况板卡与外设名）
    "rk35",
    "rk33",
    "rv11",
    "cm4",
    "esp32",
    "ina219",
    "max7219",
    "tp4056",
    "es8388",
    "sg90",
    "cp2102",
    "mpu6050",
];

fn rr3_atom_str_field(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key).and_then(Value::as_str).map(str::to_string)
}

/// 解析内嵌 atom schema，返回字段规格列表（顺序与契约文件一致；类型与
/// module 侧 `Rr3FieldSpec` 复用，校验器共用 `rr3_registry_check`）
pub fn rr3_atom_field_specs() -> Vec<Rr3FieldSpec> {
    let root: Value = toml::from_str(RR3_ATOM_SCHEMA_TOML)
        .unwrap_or_else(|e| panic!("atom.schema.toml 解析失败: {e}"));
    let fields = root
        .get("field")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("atom.schema.toml 缺少 [[field]] 数组"));
    fields
        .iter()
        .filter_map(Value::as_table)
        .map(|f| Rr3FieldSpec {
            path: rr3_atom_str_field(f, "path").expect("field.path 必填"),
            kind: rr3_atom_str_field(f, "kind").expect("field.kind 必填"),
            required: f.get("required").and_then(Value::as_bool).unwrap_or(false),
            rule: rr3_atom_str_field(f, "rule"),
        })
        .collect()
}

/// 板无关断言：文本命中任一禁词即报错（点名 token 与规则出处）。
/// 独立成函数供三处复用：atom manifest 校验、mkmod 生成自检、批E 门禁接线。
pub fn rr3_board_free_chk(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    RR3_BOARD_TOKENS
        .iter()
        .filter(|t| lower.contains(*t))
        .map(|t| {
            format!(
                "板无关断言命中禁词 \"{t}\"（引脚/厂商/产品名不得进 Atom manifest，\
                 板级拓扑只进 rr-hal 后端与描述符——ADR-0019 裁定 5）"
            )
        })
        .collect()
}

/// 从 atom manifest 路径向上定位 `contracts/slots.toml`（与 validate.rs 同构）
fn rr3_atom_slots_path(manifest: &Path) -> PathBuf {
    manifest
        .ancestors()
        .find(|a| a.join("contracts/slots.toml").is_file())
        .map(|root| root.join("contracts/slots.toml"))
        .unwrap_or_else(|| PathBuf::from("contracts/slots.toml"))
}

/// 校验单个 atom.toml 文件：读文件 → 定位并解析 `contracts/slots.toml` → 全量校验。
/// 生成器（tools/mkmodule）staging 回灌与 CI 用；路径须位于仓内以找到契约。
pub fn rr3_atom_gate(path: &Path) -> Vec<String> {
    let (text, slots_text) = match rr3_atom_texts(path) {
        Ok(v) => v,
        Err(e) => return e,
    };
    rr3_atom_chk(&text, &crate::slots::rr3_slot_parse(&slots_text))
}

/// 读 atom manifest 与其格位契约两份文本（任一读不到即点名回错）。
fn rr3_atom_texts(path: &Path) -> Result<(String, String), Vec<String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| vec![format!("无法读取 atom manifest {}: {e}", path.display())])?;
    let slots_path = rr3_atom_slots_path(path);
    let slots_text = std::fs::read_to_string(&slots_path).map_err(|e| {
        vec![format!(
            "无法读取格位契约 {}（atom 查册门需要它）: {e}",
            slots_path.display()
        )]
    })?;
    Ok((text, slots_text))
}

/// 校验一段 atom.toml 文本 + 调用方给定的格位表（查册门真值注入侧）。
/// 错误逐条点名字段；空格位表 fail-closed，不做任何后续校验。
pub fn rr3_atom_chk(text: &str, slots: &crate::slots::Rr3SlotTable) -> Vec<String> {
    let mut errs = Vec::new();

    if slots.rr3_slot_is_empty() {
        errs.push("格位表为空：拒绝校验任何 atom manifest（空注册表不是放行许可证）".to_string());
        return errs;
    }

    let Some(root) = rr3_atom_root(text, &mut errs) else {
        return errs;
    };

    // 0) 板无关断言（对 manifest 全文本，含注释——注释也是 manifest 的载荷面）
    errs.extend(rr3_board_free_chk(text));

    // 0b) 未知字段反向断言（收口批 E'，fail-closed，与 module 侧共用同一实现）：
    //     契约外的键（typo 如 uses.slost）不报错即静默丢依赖
    errs.extend(crate::validate::rr3_no_unknown_keys(
        &root,
        &rr3_atom_field_specs(),
        "atom manifest",
    ));

    // 1) 逐字段：存在性 + 类型 + 规则（与 validate.rs 逐字段循环同构，字段集来自 atom.schema.toml）
    rr3_atom_fields(&root, slots, &mut errs);

    // 2) 跨字段：每格一件（ADR-0019 裁定 1——Atom 填一个格、不可再分，恰一项）
    rr3_atom_one_slot(&root, &mut errs);

    errs
}

/// TOML 解析：失败入 errs 并回 None（调用方据此提前收兵）。
fn rr3_atom_root(text: &str, errs: &mut Vec<String>) -> Option<Value> {
    let parsed = toml::from_str(text);
    if let Err(e) = &parsed {
        errs.push(format!("atom manifest TOML 解析失败: {e}"));
    }
    parsed.ok()
}

/// 逐字段校验（字段集来自 atom.schema.toml）。
fn rr3_atom_fields(root: &Value, slots: &crate::slots::Rr3SlotTable, errs: &mut Vec<String>) {
    for spec in rr3_atom_field_specs() {
        rr3_atom_one(&spec, root, slots, errs);
    }
}

/// provides.slots 恰一项；缺键已由必填字段检查点名，不重复报。
fn rr3_atom_one_slot(root: &Value, errs: &mut Vec<String>) {
    let Some(arr) = rr3_get_by_path(root, "provides.slots").and_then(Value::as_array) else {
        return;
    };
    if arr.len() != 1 {
        errs.push(format!(
            "provides.slots 必须恰有一项（每格一件，ADR-0019 裁定 1），实际 {} 项",
            arr.len()
        ));
    }
}

/// 单字段校验（存在性→类型→规则→查册）：错误逐条点名，文案逐字保留。
/// `kind` 未知即 panic——schema 与代码不同步属契约事故，不能静默放行。
fn rr3_atom_one(
    spec: &Rr3FieldSpec,
    root: &Value,
    slots: &crate::slots::Rr3SlotTable,
    errs: &mut Vec<String>,
) {
    let Some(value) = rr3_atom_req(spec, root, errs) else {
        return;
    };
    let kind = spec.kind.as_str();
    if kind == "string" {
        return rr3_atom_str(spec, value, errs);
    }
    if kind == "list" {
        return rr3_atom_list(spec, value, slots, errs);
    }
    if kind == "int" {
        return rr3_atom_int(spec, value, errs);
    }
    panic!("atom schema 未知 kind {kind}（契约与代码不同步）");
}

/// 字段存在性与必填：缺键且必填即点名；`None` = 无值，类型/规则不判。
fn rr3_atom_req<'a>(
    spec: &Rr3FieldSpec,
    root: &'a Value,
    errs: &mut Vec<String>,
) -> Option<&'a Value> {
    let Some(value) = rr3_get_by_path(root, &spec.path) else {
        if spec.required {
            errs.push(format!("缺少必填字段 {}", spec.path));
        }
        return None;
    };
    Some(value)
}

/// string 型：类型判定 + 词形规则校验
fn rr3_atom_str(spec: &Rr3FieldSpec, value: &Value, errs: &mut Vec<String>) {
    let Some(s) = value.as_str() else {
        errs.push(format!(
            "字段 {} 类型应为 string，实际为 {value:?}",
            spec.path
        ));
        return;
    };
    if let Some(rule) = &spec.rule {
        errs.extend(crate::validate::rr3_registry_check(rule, s));
    }
}

/// list 型：逐项字符串判定 + 词形规则 + 查册门（provides/uses 每格须命中格位表）
fn rr3_atom_list(
    spec: &Rr3FieldSpec,
    value: &Value,
    slots: &crate::slots::Rr3SlotTable,
    errs: &mut Vec<String>,
) {
    let Some(arr) = value.as_array() else {
        errs.push(format!(
            "字段 {} 类型应为 list，实际为 {value:?}",
            spec.path
        ));
        return;
    };
    for (idx, item) in arr.iter().enumerate() {
        rr3_atom_list_item(spec, item, idx, slots, errs);
    }
}

/// 单条 list 项：字符串判定 → 词形规则 → provides/uses 查册门。
fn rr3_atom_list_item(
    spec: &Rr3FieldSpec,
    item: &Value,
    idx: usize,
    slots: &crate::slots::Rr3SlotTable,
    errs: &mut Vec<String>,
) {
    let Some(s) = item.as_str() else {
        errs.push(format!(
            "字段 {} 第 {} 项应为字符串，实际为 {item:?}",
            spec.path,
            idx + 1
        ));
        return;
    };
    if let Some(rule) = &spec.rule {
        errs.extend(crate::validate::rr3_registry_check(rule, s));
    }
    rr3_atom_item_slot(spec, s, slots, errs);
}

/// 查册门：provides/uses 的每一格都必须命中格位表（裁定 5②）。
fn rr3_atom_item_slot(
    spec: &Rr3FieldSpec,
    s: &str,
    slots: &crate::slots::Rr3SlotTable,
    errs: &mut Vec<String>,
) {
    if rr3_atom_slot_field(spec) && !slots.rr3_slot_contains(s) {
        errs.push(format!("字段 {} 引用未登记格位 {s}", spec.path));
    }
}

/// 该字段是查册字段（provides.slots / uses.slots）。
fn rr3_atom_slot_field(spec: &Rr3FieldSpec) -> bool {
    spec.path == "provides.slots" || spec.path == "uses.slots"
}

/// int 型：JSON 整数判定（浮点/字符串不算）
fn rr3_atom_int(spec: &Rr3FieldSpec, value: &Value, errs: &mut Vec<String>) {
    if value.as_integer().is_none() {
        errs.push(format!("字段 {} 类型应为 int，实际为 {value:?}", spec.path));
    }
}

#[cfg(test)]
mod tests;
