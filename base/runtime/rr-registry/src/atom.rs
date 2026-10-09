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
    rr3_registry_validate_atom_text(
        &text,
        &crate::slots::rr3_registry_slot_table_from_text(&slots_text),
    )
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

    // 0b) 未知字段反向断言（收口批 E'，fail-closed，与 module 侧共用同一实现）：
    //     契约外的键（typo 如 uses.slost）不报错即静默丢依赖
    errs.extend(crate::validate::rr3_registry_unknown_fields_assert(
        &root,
        &rr3_registry_atom_field_specs(),
        "atom manifest",
    ));

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
                None => errs.push(format!(
                    "字段 {} 类型应为 string，实际为 {value:?}",
                    spec.path
                )),
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
                        if (spec.path == "provides.slots" || spec.path == "uses.slots")
                            && !slots.rr3_slot_table_contains(s)
                        {
                            errs.push(format!("字段 {} 引用未登记格位 {s}", spec.path));
                        }
                    }
                }
                None => errs.push(format!(
                    "字段 {} 类型应为 list，实际为 {value:?}",
                    spec.path
                )),
            },
            "int" => {
                if value.as_integer().is_none() {
                    errs.push(format!("字段 {} 类型应为 int，实际为 {value:?}", spec.path));
                }
            }
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
        assert_eq!(specs.len(), 7);
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
        let two = battery_text().replace(
            "slots = [\"power.battery\"]",
            "slots = [\"power.battery\", \"power.charger\"]",
        );
        let errs = rr3_registry_validate_atom_text(&two, &slots());
        assert!(
            errs.iter()
                .any(|e| e.contains("恰有一项") && e.contains("2 项")),
            "{errs:?}"
        );

        let none = battery_text().replace("slots = [\"power.battery\"]", "slots = []");
        let errs = rr3_registry_validate_atom_text(&none, &slots());
        assert!(
            errs.iter()
                .any(|e| e.contains("恰有一项") && e.contains("0 项")),
            "{errs:?}"
        );
    }

    /// 板无关断言：注释里塞厂商/引言语 token 也必须命中（注释是载荷面）
    #[test]
    fn board_free_assertion_hits_tokens_anywhere() {
        let tainted = battery_text() + "\n# 后端绑定 INA219 @ GPIO2/3\n";
        let errs = rr3_registry_validate_atom_text(&tainted, &slots());
        assert!(errs.iter().any(|e| e.contains("ina219")), "{errs:?}");
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

        let bad_topic = battery_text().replace("rr3-power/battery/state", "rr3-power/batteryState");
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
        for (name, text) in [
            ("capture", capture),
            ("vad", vad),
            ("env", env),
            ("imu", imu),
        ] {
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

    /// 批C③ 重铺件钉死：rr3-interact 的两份 atom.toml 必须过契约 + 查册门 + 板无关断言
    /// （重铺件漂移当场红；每格一件由校验器跨字段规则把守）
    #[test]
    fn interact_relaid_atoms_stay_compliant() {
        let face = include_str!("../../../../modules/rr3-interact/atoms/face/atom.toml");
        let player = include_str!("../../../../modules/rr3-interact/atoms/player/atom.toml");
        for (name, text) in [("face", face), ("player", player)] {
            let errs = rr3_registry_validate_atom_text(text, &slots());
            assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
        }
    }

    /// 批C④ 重铺件钉死：rr3-connect 的两份 atom.toml 必须过契约 + 查册门 + 板无关断言
    /// （重铺件漂移当场红；每格一件由校验器跨字段规则把守）
    #[test]
    fn connect_relaid_atoms_stay_compliant() {
        let wifi = include_str!("../../../../modules/rr3-connect/atoms/wifi/atom.toml");
        let bt = include_str!("../../../../modules/rr3-connect/atoms/bt/atom.toml");
        for (name, text) in [("wifi", wifi), ("bt", bt)] {
            let errs = rr3_registry_validate_atom_text(text, &slots());
            assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
        }
    }

    /// 批C⑤ 重铺件钉死：rr3-brain（Python 面）的四份 atom.toml 必须过契约、查册门
    /// 与板无关断言（机制本体 .py 原位——Python 单文件即机制+入口合一，Rust 五件套
    /// 拆分随 ADR-0016 迁移收敛；重铺件漂移当场红）
    #[test]
    fn brain_relaid_atoms_stay_compliant() {
        let dialog = include_str!("../../../../modules/rr3-brain/atoms/dialog/atom.toml");
        let asr = include_str!("../../../../modules/rr3-brain/atoms/asr/atom.toml");
        let llm = include_str!("../../../../modules/rr3-brain/atoms/llm/atom.toml");
        let tts = include_str!("../../../../modules/rr3-brain/atoms/tts/atom.toml");
        for (name, text) in [("dialog", dialog), ("asr", asr), ("llm", llm), ("tts", tts)] {
            let errs = rr3_registry_validate_atom_text(text, &slots());
            assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
        }
    }

    /// 收口批 E'：未知字段反向断言（rr3_registry_unknown_fields_assert）——
    /// typo 键（uses.slost）与未登记段双路红，合法 manifest 零命中（fail-closed 自证）
    #[test]
    fn unknown_fields_assert_flags_typo_keys() {
        let specs = rr3_registry_atom_field_specs();
        let bad: Value = toml::from_str(
            "[atom]\nname = \"x\"\nversion = \"0.1.0\"\n[provides]\nslots = [\"ui.face\"]\n[uses]\nslost = [\"audio.playback\"]\n[bogus]\ny = 2\n",
        )
        .expect("样例可解析");
        let errs =
            crate::validate::rr3_registry_unknown_fields_assert(&bad, &specs, "atom manifest");
        assert!(
            errs.iter().any(|e| e.contains("uses.slost")),
            "typo 键必须点名: {errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("[bogus]")),
            "未登记段必须点名: {errs:?}"
        );
        let ok: Value = toml::from_str(
            "[atom]\nname = \"x\"\nversion = \"0.1.0\"\n[provides]\nslots = [\"ui.face\"]\n",
        )
        .expect("合法样例可解析");
        assert!(
            crate::validate::rr3_registry_unknown_fields_assert(&ok, &specs, "atom manifest")
                .is_empty()
        );
    }

    /// 批E 门禁接线（architecture §13"每格一件·不留空格"反向断言，查册真值处执法）：
    /// 仓扫描全部 `modules/*/atoms/*/atom.toml`——①每份过契约+查册门+板无关
    /// （新增件**不钉即红**，堵住上面 include_str! 逐份钉法"漏钉不红"的缺口）；
    /// ②provides 全集对 slots 表反查：除基座内建豁免集外**每格恰一件**（两件抢格 /
    /// 新格无件当场红），内建格不立件。
    #[test]
    fn repo_atoms_compliant_and_every_slot_uniquely_covered() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("rr-registry 位于 base/runtime/ 下三级")
            .to_path_buf();
        // 基座内建豁免集（0.0.5 方案 §12.1 终裁：boot 即在位、无独立原子目录——
        // rr-bus/rrd 承载或机制核切片承载；power.battery 虽 base 层但由 battery
        // 原子承担，不在豁免集。此常量漂移 = 覆盖账不平断言当场红）
        const BUILTIN_NO_ATOM: &[&str] = &["bus.publish", "bus.subscribe", "bus.rpc", "net.online"];
        let slots = slots();

        // ① 仓扫描：逐份全量校验 + 收格主（每格一件的唯一性在此建立）
        let mut owner: std::collections::BTreeMap<String, String> = Default::default();
        let mut scanned = 0usize;
        for m in std::fs::read_dir(root.join("modules")).expect("modules/ 必须存在") {
            let atoms = m.expect("modules 项可读").path().join("atoms");
            if !atoms.is_dir() {
                continue;
            }
            for a in std::fs::read_dir(&atoms).expect("atoms/ 必须可读") {
                let manifest = a.expect("atom 项可读").path().join("atom.toml");
                if !manifest.is_file() {
                    continue;
                }
                scanned += 1;
                let text = std::fs::read_to_string(&manifest).expect("atom.toml 可读");
                let errs = rr3_registry_validate_atom_text(&text, &slots);
                assert!(errs.is_empty(), "{} 漂移: {errs:?}", manifest.display());
                let v: Value = toml::from_str(&text).expect("atom.toml 已过 TOML 解析");
                // 批E'：目录名 = atom.name 唯一对应——⑦-b/⑦-c 门按目录名执法、
                // 契约按 atom.name，两名漂移即"合同与户口本不符"（件名两个真源）
                let dir_name = manifest
                    .parent()
                    .and_then(|p| p.file_name())
                    .expect("atom.toml 必有父目录名")
                    .to_string_lossy();
                let atom_name = v["atom"]["name"].as_str().expect("atom.name 已过必填校验");
                assert_eq!(
                    atom_name, dir_name,
                    "{dir_name}：atom.name 与目录名不一致（件名两个真源）"
                );
                let slot = v["provides"]["slots"][0]
                    .as_str()
                    .expect("provides.slots 恰一项且为字符串（validate 已断）")
                    .to_string();
                if let Some(prev) = owner.insert(slot.clone(), manifest.display().to_string()) {
                    panic!(
                        "格 {slot} 被两件抢占（每格一件，ADR-0019 裁定 1）: {prev} 与 {}",
                        manifest.display()
                    );
                }
            }
        }
        assert!(
            scanned >= 17,
            "仓扫描仅见 {scanned} 份 atom.toml（打空/缺件属门禁失效，不放行）"
        );

        // ② 不留空格 + 内建格不立件：21 格逐格反查，末尾覆盖账必须轧平
        let names = slots.rr3_slot_table_names();
        for slot in &names {
            let provided = owner.get(slot);
            if BUILTIN_NO_ATOM.contains(&slot.as_str()) {
                assert!(
                    provided.is_none(),
                    "内建格 {slot} 不立件（基座内建/机制核切片承载），却被 {} 提供",
                    provided.unwrap()
                );
            } else {
                assert!(
                    provided.is_some(),
                    "格 {slot} 无 atom 承担（不留空格）——立件或经契约级评审改格"
                );
            }
        }
        assert_eq!(
            owner.len() + BUILTIN_NO_ATOM.len(),
            names.len(),
            "覆盖账不平：atom 承担格数 + 内建豁免数 ≠ slots 表全数"
        );
    }
}
