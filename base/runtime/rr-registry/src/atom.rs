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

use crate::schema::{rr3_registry_get_by_path, Rr3FieldSpec};

/// Atom manifest 契约 schema 版本（对应 `atom.schema.toml` 的 `schema_version` 键）
pub const RR3_ATOM_SCHEMA_VERSION: u32 = 1;

const RR3_ATOM_SCHEMA_TOML: &str = include_str!("atom.schema.toml");

/// 板无关禁词表（引脚 / 硬件总线 / 设备路径 / 厂商 / 产品·芯片·器件名）。
/// 子串匹配 + 小写比较；命中即门禁失败，报错点名 token。
const RR3_BOARD_TOKENS: &[&str] = &[
    // 引脚与硬件总线：板级拓扑只进 rr-hal 后端与描述符，不进原子契约
    "gpio", "i2c", "spi", "uart", "pwm", "bcm", "/dev/",
    // 厂商
    "rockchip", "raspberry", "broadcom", "espressif", "allwinner", "sunxi", "amlogic",
    // 产品 / 芯片 / 器件（本仓实况板卡与外设名）
    "rk35", "rk33", "rv11", "cm4", "esp32", "ina219", "max7219", "tp4056", "es8388", "sg90",
    "cp2102", "mpu6050",
];

fn rr3_atom_str_field(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key).and_then(Value::as_str).map(str::to_string)
}

/// 解析内嵌 atom schema，返回字段规格列表（顺序与契约文件一致；类型与
/// module 侧 `Rr3FieldSpec` 复用，校验器共用 `rr3_registry_check`）
pub fn rr3_registry_atom_field_specs() -> Vec<Rr3FieldSpec> {
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
pub fn rr3_registry_board_free_assert(text: &str) -> Vec<String> {
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
pub fn rr3_registry_validate_atom(path: &Path) -> Vec<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return vec![format!("无法读取 atom manifest {}: {e}", path.display())],
    };
    let slots_path = rr3_atom_slots_path(path);
    let slots_text = match std::fs::read_to_string(&slots_path) {
        Ok(t) => t,
        Err(e) => {
            return vec![format!(
                "无法读取格位契约 {}（atom 查册门需要它）: {e}",
                slots_path.display()
            )]
        }
    };
    rr3_registry_validate_atom_text(&text, &crate::slots::rr3_registry_slot_table_from_text(&slots_text))
}

/// 校验一段 atom.toml 文本 + 调用方给定的格位表（查册门真值注入侧）。
/// 错误逐条点名字段；空格位表 fail-closed，不做任何后续校验。
pub fn rr3_registry_validate_atom_text(
    text: &str,
    slots: &crate::slots::Rr3SlotTable,
) -> Vec<String> {
    let mut errs = Vec::new();

    if slots.rr3_slot_table_is_empty() {
        errs.push("格位表为空：拒绝校验任何 atom manifest（空注册表不是放行许可证）".to_string());
        return errs;
    }

    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            errs.push(format!("atom manifest TOML 解析失败: {e}"));
            return errs;
        }
    };

    // 0) 板无关断言（对 manifest 全文本，含注释——注释也是 manifest 的载荷面）
    errs.extend(rr3_registry_board_free_assert(text));

    // 1) 逐字段：存在性 + 类型 + 规则（与 validate.rs 逐字段循环同构，字段集来自 atom.schema.toml）
    for spec in rr3_registry_atom_field_specs() {
        let Some(value) = rr3_registry_get_by_path(&root, &spec.path) else {
            if spec.required {
                errs.push(format!("缺少必填字段 {}", spec.path));
            }
            continue;
        };
        match spec.kind.as_str() {
            "string" => match value.as_str() {
                Some(s) => {
                    if let Some(rule) = &spec.rule {
                        errs.extend(crate::validate::rr3_registry_check(rule, s));
                    }
                }
                None => errs.push(format!("字段 {} 类型应为 string，实际为 {value:?}", spec.path)),
            },
            "list" => match value.as_array() {
                Some(arr) => {
                    for (idx, item) in arr.iter().enumerate() {
                        let Some(s) = item.as_str() else {
                            errs.push(format!(
                                "字段 {} 第 {} 项应为字符串，实际为 {item:?}",
                                spec.path,
                                idx + 1
                            ));
                            continue;
                        };
                        if let Some(rule) = &spec.rule {
                            errs.extend(crate::validate::rr3_registry_check(rule, s));
                        }
                        // 查册门：provides/uses 的每一格都必须命中格位表（裁定 5②）
                        if spec.path == "provides.slots" || spec.path == "uses.slots" {
                            if !slots.rr3_slot_table_contains(s) {
                                errs.push(format!("字段 {} 引用未登记格位 {s}", spec.path));
                            }
                        }
                    }
                }
                None => errs.push(format!("字段 {} 类型应为 list，实际为 {value:?}", spec.path)),
            },
            other => panic!("atom schema 未知 kind {other}（契约与代码不同步）"),
        }
    }

    // 2) 跨字段：每格一件（ADR-0019 裁定 1——Atom 填一个格、不可再分，恰一项）
    let provided = rr3_registry_get_by_path(&root, "provides.slots").and_then(Value::as_array);
    match provided {
        Some(arr) if arr.len() == 1 => {}
        Some(arr) => errs.push(format!(
            "provides.slots 必须恰有一项（每格一件，ADR-0019 裁定 1），实际 {} 项",
            arr.len()
        )),
        None => {} // 缺失已由必填字段检查点名，不重复报
    }

    errs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slots::{rr3_registry_slot_table_from_text, Rr3SlotTable};

    // include_str! 以本文件所在目录（src/）为基准：src → rr-registry → runtime → base → 仓库根
    const CONTRACT: &str = include_str!("../../../../contracts/slots.toml");

    fn slots() -> Rr3SlotTable {
        rr3_registry_slot_table_from_text(CONTRACT)
    }

    fn battery_text() -> String {
        r#"
# battery 原子件 manifest（批B 打样）
schema_version = 1

[atom]
name = "battery"
version = "0.0.1"

[provides]
slots = ["power.battery"]

[uses]
slots = []

[events]
publishes = ["rr3-power/battery/state", "rr3-power/battery/low"]
subscribes = []
"#
        .to_string()
    }

    /// 合法 atom 文本零报错；schema 字段集与 atom.schema.toml 同步（路径唯一）
    #[test]
    fn valid_battery_atom_passes() {
        let errs = rr3_registry_validate_atom_text(&battery_text(), &slots());
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn atom_field_paths_are_unique() {
        let specs = rr3_registry_atom_field_specs();
        let mut seen: Vec<&str> = specs.iter().map(|s| s.path.as_str()).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "字段路径不得重复");
        assert_eq!(specs.len(), 6);
    }

    /// 查册门：未登记格位拒绝；空格位表 fail-closed
    #[test]
    fn unregistered_slot_is_rejected_and_empty_table_fail_closes() {
        let bad = battery_text().replace("power.battery", "power.solar");
        let errs = rr3_registry_validate_atom_text(&bad, &slots());
        assert!(
            errs.iter().any(|e| e.contains("未登记格位 power.solar")),
            "{errs:?}"
        );

        let errs = rr3_registry_validate_atom_text(&battery_text(), &Rr3SlotTable::default());
        assert_eq!(
            errs,
            vec!["格位表为空：拒绝校验任何 atom manifest（空注册表不是放行许可证）"]
        );
    }

    /// 每格一件：provides.slots 恰一项，多项/零项都点名裁定出处
    #[test]
    fn provides_slots_must_be_exactly_one() {
        let two = battery_text()
            .replace("slots = [\"power.battery\"]", "slots = [\"power.battery\", \"power.charger\"]");
        let errs = rr3_registry_validate_atom_text(&two, &slots());
        assert!(errs.iter().any(|e| e.contains("恰有一项") && e.contains("2 项")), "{errs:?}");

        let none = battery_text().replace("slots = [\"power.battery\"]", "slots = []");
        let errs = rr3_registry_validate_atom_text(&none, &slots());
        assert!(errs.iter().any(|e| e.contains("恰有一项") && e.contains("0 项")), "{errs:?}");
    }

    /// 板无关断言：注释里塞厂商/引言语 token 也必须命中（注释是载荷面）
    #[test]
    fn board_free_assertion_hits_tokens_anywhere() {
        let tainted = battery_text() + "\n# 后端绑定 INA219 @ GPIO2/3\n";
        let errs = rr3_registry_validate_atom_text(&tainted, &slots());
        assert!(
            errs.iter().any(|e| e.contains("ina219")),
            "{errs:?}"
        );
        assert!(errs.iter().any(|e| e.contains("gpio")), "{errs:?}");

        // 断言独立可用（生成器与批E 门禁的复用入口）
        let errs = rr3_registry_board_free_assert("clean manifest");
        assert!(errs.is_empty());
        assert_eq!(rr3_registry_board_free_assert("MAX7219").len(), 1);
    }

    /// 必填字段与 topic 形状：缺字段点名、坏主题点名、uses/事件缺省放行
    #[test]
    fn required_fields_and_topic_shape_are_enforced() {
        let missing = battery_text().replace("version = \"0.0.1\"\n", "");
        let errs = rr3_registry_validate_atom_text(&missing, &slots());
        assert!(
            errs.iter().any(|e| e.contains("缺少必填字段 atom.version")),
            "{errs:?}"
        );

        let bad_topic =
            battery_text().replace("rr3-power/battery/state", "rr3-power/batteryState");
        let errs = rr3_registry_validate_atom_text(&bad_topic, &slots());
        assert!(errs.iter().any(|e| e.contains("topic 非法")), "{errs:?}");

        let minimal = r#"
schema_version = 1
[atom]
name = "vad"
version = "0.1.0"
[provides]
slots = ["audio.vad"]
"#;
        let errs = rr3_registry_validate_atom_text(minimal, &slots());
        assert!(errs.is_empty(), "uses/events 缺省即合规: {errs:?}");
    }

    /// 批B③ 打样件钉死：rr3-power 的两份 atom.toml 必须过契约 + 查册门 + 板无关断言
    /// （打样件漂移当场红；include_str! 相对 src/：src → rr-registry → runtime → base → 仓根）
    #[test]
    fn power_pilot_atoms_stay_compliant() {
        let battery = include_str!("../../../../modules/rr3-power/atoms/battery/atom.toml");
        let charger = include_str!("../../../../modules/rr3-power/atoms/charger/atom.toml");
        let errs = rr3_registry_validate_atom_text(battery, &slots());
        assert!(errs.is_empty(), "battery 打样件漂移: {errs:?}");
        let errs = rr3_registry_validate_atom_text(charger, &slots());
        assert!(errs.is_empty(), "charger 打样件漂移: {errs:?}");
    }

    /// 批C① 重铺件钉死：rr3-sense 的四份 atom.toml 必须过契约 + 查册门 + 板无关断言
    /// （重铺件漂移当场红；每格一件由校验器跨字段规则把守）
    #[test]
    fn sense_relaid_atoms_stay_compliant() {
        let capture = include_str!("../../../../modules/rr3-sense/atoms/capture/atom.toml");
        let vad = include_str!("../../../../modules/rr3-sense/atoms/vad/atom.toml");
        let env = include_str!("../../../../modules/rr3-sense/atoms/env/atom.toml");
        let imu = include_str!("../../../../modules/rr3-sense/atoms/imu/atom.toml");
        for (name, text) in [("capture", capture), ("vad", vad), ("env", env), ("imu", imu)] {
            let errs = rr3_registry_validate_atom_text(text, &slots());
            assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
        }
    }

    /// 批C② 重铺件钉死：rr3-act 的三份 atom.toml 必须过契约 + 查册门 + 板无关断言
    /// （重铺件漂移当场红；每格一件由校验器跨字段规则把守）
    #[test]
    fn act_relaid_atoms_stay_compliant() {
        let servo = include_str!("../../../../modules/rr3-act/atoms/servo/atom.toml");
        let led = include_str!("../../../../modules/rr3-act/atoms/led/atom.toml");
        let relay = include_str!("../../../../modules/rr3-act/atoms/relay/atom.toml");
        for (name, text) in [("servo", servo), ("led", led), ("relay", relay)] {
            let errs = rr3_registry_validate_atom_text(text, &slots());
            assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
        }
    }
}
