//! 装配清单（`atoms/<名>/assemble.toml`，五件套④）的形状契约与注入缺校验
//! （ADR-0019 修订 1 ③：L3 注入面从口号变成被校验的数据，0.0.5 批J G4）。
//!
//! 定形（与方案 §14 G4 行同文）：`[assemble.handles]` 声明件用哪个能力面 trait
//! ← 绑描述符哪个键、`[assemble.ops]` 声明件暴露哪些参数槽 ← 取哪个 config 键。
//! 数据形状（schema_version = 2）：
//!
//! ```toml
//! schema_version = 2
//!
//! [assemble]
//! handles = [
//!   { trait = "Rr3Power", desc_key = "i2c_bus" },
//! ]
//! ops = [
//!   { slot = "low_voltage_mv", src_key = "rr3-power.battery.low_voltage_mv" },
//! ]
//! ```
//!
//! 两级真值、两处消费（`rrd` 与裸机装配段共用本模块，不各说一套）：
//! - **形状级**（[`rr3_registry_assemble_from_text`]）：schema 版本、未知字段点名、
//!   trait ∈ 七能力面在册表、desc_key ∈ 九描述符键在册表、参数槽与来源键非空点路径。
//!   空数组**合法**，但**两数组均空时须明写 `no_injection = true`**（0.0.5 批M MC-10 L-5：
//!   把"漏填"与"本件确无注入面"分开——否则空申报与漏填不可区分，门里无内容可校验）。
//! - **注入缺级**（[`rr3_registry_assemble_injection_gaps`]）：ops 的 `src_key`
//!   解析域 = 模块 `config.toml`（点路径游走），取不到 = 注入缺。handles 的
//!   desc_key 存在性**不在本模块判**——描述符注入是 spawn 时 `RR3_HAL_BINDINGS`
//!   env 的事，存在性归探测面（实测缺），与 rr-hal 后端选择的 fail-closed 同一路。
//!
//! 装配段对缺项的处置（ADR-0019 修订 1 ③）：**该格 `Degraded`、点名到件，不 fatal**
//! ——件没病，装配声明坏了重启治不了；修数据 + 重启 rrd 是唯一解除路。
//!
//! 在册表与真值源的关系：七 trait 真值源 = `rr-hal-api`（能力面契约），九键真值源 =
//! `rr-hal` `descriptor.rs`（`[hardware]` 键集，C4 裁定**不扩键**）。两表在此按
//! 契约层纪律另立一份并由契约测试钉死——扩 trait/键都走契约级评审，届时两处同批；
//! 本表滞后 = 新 trait/键被装配校验拒绝（fail-closed 方向，不会带病放行）。

use toml::Value;

/// 装配清单 schema 版本（本文件定形即 2：handles/ops 从字符串数组升为结构化条目）
pub const RR3_ASSEMBLE_SCHEMA_VERSION: u32 = 2;

/// 七能力面 trait 在册表（真值源 rr-hal-api；handles[].trait 只认这七个名字）
pub const RR3_ASSEMBLE_HANDLE_TRAITS: &[&str] = &[
    "Rr3GpioOut",
    "Rr3PwmServo",
    "Rr3AudioSource",
    "Rr3AudioSink",
    "Rr3Power",
    "Rr3Link",
    "Rr3Display",
];

/// 描述符键在册表（真值源 rr-hal descriptor.rs `[hardware]` 键集；C4 批J 裁定
/// 不扩键，0.0.6 链④真件档按裁定流程扩 `spi_device`——两处同批）
pub const RR3_ASSEMBLE_DESC_KEYS: &[&str] = &[
    "alsa_device",
    "pwm_chip",
    "pwm_channel",
    "gpio_chip",
    "gpio_led_line",
    "gpio_relay_line",
    "i2c_bus",
    "ina219_addr",
    "link_ifname",
    "spi_device",
];

/// 一条句柄声明：件用 `trait_name` 能力面，绑描述符 `desc_key` 键
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AssembleHandle {
    pub trait_name: String,
    pub desc_key: String,
}

/// 一条参数槽声明：件暴露 `slot` 槽，取值来自 config 的 `src_key` 点路径
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AssembleOp {
    pub slot: String,
    pub src_key: String,
}

/// 一份装配清单的解析产物（空数组合法，但空面须明写 no_injection，见下）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rr3AssembleFacts {
    pub handles: Vec<Rr3AssembleHandle>,
    pub ops: Vec<Rr3AssembleOp>,
    /// `no_injection = true`：件明写"本件不需注入面"（0.0.5 批M MC-10 L-5）。
    /// `handles`/`ops` 两数组**均空**时此键必填——否则与"漏填"不可区分，一律判红。
    pub no_injection: bool,
}

/// 点路径合法性：按 `.` 切分后每段非空（空串/首尾点/连续点都不认）
fn rr3_dotted_key_ok(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(|seg| !seg.is_empty())
}

/// handles 数组条目级解析与校验（件级与组合件级**同形同规则**，composite.rs 共用；
/// 点名文案两处一致——一份判据一份真值）
pub(crate) fn rr3_handles_from_array(items: &[Value]) -> (Vec<Rr3AssembleHandle>, Vec<String>) {
    let mut handles = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Some(h) = item.as_table() else {
            errs.push(format!("handles[{i}] 应为内联表 {{ trait, desc_key }}"));
            continue;
        };
        for k in h.keys() {
            if k != "trait" && k != "desc_key" {
                errs.push(format!("handles[{i}] 未知键 {k}（只认 trait / desc_key）"));
            }
        }
        let trait_name = h.get("trait").and_then(Value::as_str);
        let desc_key = h.get("desc_key").and_then(Value::as_str);
        match trait_name {
            Some(tr) if RR3_ASSEMBLE_HANDLE_TRAITS.contains(&tr) => {}
            Some(tr) => errs.push(format!(
                "handles[{i}].trait \"{tr}\" 未在册（七能力面表见 assemble.rs，扩面走契约级评审）"
            )),
            None => errs.push(format!("handles[{i}].trait 应为字符串")),
        }
        match desc_key {
            Some(dk) if RR3_ASSEMBLE_DESC_KEYS.contains(&dk) => {}
            Some(dk) => errs.push(format!(
                "handles[{i}].desc_key \"{dk}\" 未在册（键表见 assemble.rs / descriptor.rs，扩键走契约级评审）"
            )),
            None => errs.push(format!("handles[{i}].desc_key 应为字符串")),
        }
        if let (Some(tr), Some(dk)) = (trait_name, desc_key) {
            handles.push(Rr3AssembleHandle {
                trait_name: tr.into(),
                desc_key: dk.into(),
            });
        }
    }
    (handles, errs)
}

/// ops 数组条目级解析与校验（同上：件级与组合件级共用）
pub(crate) fn rr3_ops_from_array(items: &[Value]) -> (Vec<Rr3AssembleOp>, Vec<String>) {
    let mut ops = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Some(o) = item.as_table() else {
            errs.push(format!("ops[{i}] 应为内联表 {{ slot, src_key }}"));
            continue;
        };
        for k in o.keys() {
            if k != "slot" && k != "src_key" {
                errs.push(format!("ops[{i}] 未知键 {k}（只认 slot / src_key）"));
            }
        }
        let slot = o.get("slot").and_then(Value::as_str);
        let src_key = o.get("src_key").and_then(Value::as_str);
        match slot {
            Some(s) if !s.trim().is_empty() => {}
            Some(_) => errs.push(format!("ops[{i}].slot 不得为空")),
            None => errs.push(format!("ops[{i}].slot 应为字符串")),
        }
        match src_key {
            Some(k) if rr3_dotted_key_ok(k) => {}
            Some(k) => errs.push(format!(
                "ops[{i}].src_key \"{k}\" 不是合法点路径（按 . 切分后每段须非空）"
            )),
            None => errs.push(format!("ops[{i}].src_key 应为字符串")),
        }
        if let (Some(s), Some(k)) = (slot, src_key) {
            ops.push(Rr3AssembleOp {
                slot: s.into(),
                src_key: k.into(),
            });
        }
    }
    (ops, errs)
}

/// L-5 空面规则（0.0.5 批M MC-10）：两数组均在文本中出现且均为空 ⇒ 必须明写
/// no_injection = true；明写 no_injection 时两数组必须都空。件级与组合件级共用。
pub(crate) fn rr3_empty_surface_errors(
    handles_present: bool,
    handles_empty: bool,
    ops_present: bool,
    ops_empty: bool,
    no_injection: bool,
) -> Vec<String> {
    let mut errs: Vec<String> = Vec::new();
    if handles_present && handles_empty && ops_present && ops_empty && !no_injection {
        errs.push(
            "注入面两数组均空却未明写 no_injection = true（L-5：或补齐 handles/ops，或明写该件不需注入面）"
                .into(),
        );
    }
    if no_injection && !(handles_present && handles_empty && ops_present && ops_empty) {
        errs.push("no_injection = true 只允许与空的 handles/ops 并存（该声明与注入面互斥）".into());
    }
    errs
}

/// 解析 + 形状级校验一份装配清单文本。fail-closed：一条不静默放行，全部错误
/// 逐条点名（与 manifest/描述符校验同一口径）。空 `[assemble]` 数组合法。
pub fn rr3_registry_assemble_from_text(text: &str) -> Result<Rr3AssembleFacts, Vec<String>> {
    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("装配清单 TOML 解析失败: {e}")]),
    };
    let Some(t) = root.as_table() else {
        return Err(vec![format!("装配清单顶层必须是 TOML 表，收到 {root:?}")]);
    };

    let mut errs: Vec<String> = Vec::new();
    // schema_version：必须为 2（v1 是"无代码读它"时代的死数据形状，直接点名不迁就）
    match t.get("schema_version") {
        Some(Value::Integer(v)) if *v == RR3_ASSEMBLE_SCHEMA_VERSION as i64 => {}
        Some(Value::Integer(v)) => errs.push(format!(
            "装配清单 schema_version 必须为 {RR3_ASSEMBLE_SCHEMA_VERSION}，收到 {v}"
        )),
        Some(other) => errs.push(format!("装配清单 schema_version 应为整数，收到 {other:?}")),
        None => errs.push(format!(
            "装配清单缺 schema_version（必须为 {RR3_ASSEMBLE_SCHEMA_VERSION}）"
        )),
    }
    // 顶层未知键点名（只认 schema_version + assemble）
    for k in t.keys() {
        if k != "schema_version" && k != "assemble" {
            errs.push(format!(
                "装配清单顶层未知键 {k}（只认 schema_version / assemble）"
            ));
        }
    }

    let Some(asm) = t.get("assemble").and_then(Value::as_table) else {
        errs.push("装配清单缺 [assemble] 表（handles / ops 两键都必填，空数组合法）".into());
        return if errs.is_empty() {
            Ok(Rr3AssembleFacts::default())
        } else {
            Err(errs)
        };
    };
    // [assemble] 未知键点名
    for k in asm.keys() {
        if k != "handles" && k != "ops" && k != "no_injection" {
            errs.push(format!(
                "[assemble] 未知键 {k}（只认 handles / ops / no_injection）"
            ));
        }
    }

    let mut facts = Rr3AssembleFacts::default();
    // 两数组"已在文本中出现"（供空面规则判"漏填 vs 明写不需"）：缺键与形状坏另有其错，
    // 不能替本规则挡刀，也不能被本规则牵连出第二遍点名。
    let mut handles_present = false;
    let mut handles_empty = false;
    let mut ops_present = false;
    let mut ops_empty = false;
    // no_injection = true 是"本件不需注入面"的显式声明（L-5）：只认布尔 true。
    match asm.get("no_injection") {
        Some(Value::Boolean(true)) => facts.no_injection = true,
        Some(Value::Boolean(false)) => {
            errs.push("no_injection 只认 true（本件不需注入面时明写；非空面不得写此键）".into())
        }
        Some(other) => errs.push(format!("no_injection 应为布尔 true，收到 {other:?}")),
        None => {}
    }

    // handles：数组，每条 = { trait, desc_key }，两键都在册表内（解析与组合件级共用一份）
    match asm.get("handles") {
        Some(Value::Array(items)) => {
            handles_present = true;
            handles_empty = items.is_empty();
            let (parsed, mut item_errs) = rr3_handles_from_array(items);
            facts.handles = parsed;
            errs.append(&mut item_errs);
        }
        Some(other) => errs.push(format!("[assemble].handles 应为数组，收到 {other:?}")),
        None => errs.push("[assemble] 缺 handles 键（空数组合法，键本身必填）".into()),
    }

    // ops：数组，每条 = { slot, src_key }，slot 非空、src_key 为合法点路径（共用同上）
    match asm.get("ops") {
        Some(Value::Array(items)) => {
            ops_present = true;
            ops_empty = items.is_empty();
            let (parsed, mut item_errs) = rr3_ops_from_array(items);
            facts.ops = parsed;
            errs.append(&mut item_errs);
        }
        Some(other) => errs.push(format!("[assemble].ops 应为数组，收到 {other:?}")),
        None => errs.push("[assemble] 缺 ops 键（空数组合法，键本身必填）".into()),
    }

    // 空面规则（L-5）：两数组均在文本中出现且均为空 ⇒ 必须明写 no_injection = true；
    // 反之，明写 no_injection 时两数组必须都空。这条把"漏填"与"本件确无注入面"分开——
    // 光看空数组分不清，于是此前 14 份空申报在门里无内容可校验（方案 §12.4 C3）。
    errs.extend(rr3_empty_surface_errors(
        handles_present,
        handles_empty,
        ops_present,
        ops_empty,
        facts.no_injection,
    ));

    if errs.is_empty() {
        Ok(facts)
    } else {
        Err(errs)
    }
}

/// 注入缺校验：ops 的每条 `src_key` 必须能在 `config_text`（模块 config.toml）上
/// 按点路径取到值；取不到 = 注入缺，逐条点名（参数槽 + 来源键）。config 文本
/// 解析失败 = 全部来源键不可证，逐条点同名缺口。handles 不在此判（见模块头注）。
/// 返回空 = 无缺口。
pub fn rr3_registry_assemble_injection_gaps(
    facts: &Rr3AssembleFacts,
    config_text: &str,
) -> Vec<String> {
    let cfg: Value = match toml::from_str(config_text) {
        Ok(v) => v,
        Err(e) => {
            return facts
                .ops
                .iter()
                .map(|o| {
                    format!(
                        "参数槽 {} 的来源键 {} 不可证：config 文本解析失败 ({e})",
                        o.slot, o.src_key
                    )
                })
                .collect();
        }
    };
    facts
        .ops
        .iter()
        .filter(|o| {
            let mut cur = &cfg;
            for seg in o.src_key.split('.') {
                match cur.get(seg) {
                    Some(v) => cur = v,
                    None => return true,
                }
            }
            false
        })
        .map(|o| {
            format!(
                "参数槽 {} 的来源键 {} 在 config 中取不到（注入缺）",
                o.slot, o.src_key
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK_SAMPLE: &str = r#"
schema_version = 2

[assemble]
handles = [
  { trait = "Rr3Power", desc_key = "i2c_bus" },
  { trait = "Rr3Power", desc_key = "ina219_addr" },
]
ops = [
  { slot = "low_voltage_mv", src_key = "rr3-power.battery.low_voltage_mv" },
]
"#;

    #[test]
    fn sample_and_empty_arrays_parse() {
        let f = rr3_registry_assemble_from_text(OK_SAMPLE).expect("正例必须过");
        assert_eq!(f.handles.len(), 2);
        assert_eq!(f.handles[0].trait_name, "Rr3Power");
        assert_eq!(f.handles[0].desc_key, "i2c_bus");
        assert_eq!(f.ops.len(), 1);
        assert_eq!(f.ops[0].slot, "low_voltage_mv");

        // 空数组合法，但须明写 no_injection（L-5）
        let f = rr3_registry_assemble_from_text(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .expect("明写不需注入面的空数组必须合法");
        assert!(f.handles.is_empty() && f.ops.is_empty() && f.no_injection);
    }

    /// L-5 空面规则：两数组均空而漏写 no_injection ⇒ 红；no_injection 与非空面并存 ⇒ 红。
    #[test]
    fn empty_surface_requires_no_injection_marker() {
        // 漏写 → 点名"未明写 no_injection"
        let errs = rr3_registry_assemble_from_text(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n",
        )
        .expect_err("空面漏写声明必须红");
        assert!(
            errs.iter().any(|e| e.contains("未明写 no_injection")),
            "{errs:?}"
        );

        // 写了 no_injection 却又给了 handles → 互斥，红
        let errs = rr3_registry_assemble_from_text(
            "schema_version = 2\n\n[assemble]\nno_injection = true\nhandles = [ { trait = \"Rr3Power\", desc_key = \"i2c_bus\" } ]\nops = []\n",
        )
        .expect_err("声明与注入面并存必须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("只允许与空的 handles/ops 并存")),
            "{errs:?}"
        );

        // no_injection = false 不认（只认 true）
        let errs = rr3_registry_assemble_from_text(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = false\n",
        )
        .expect_err("no_injection=false 必须红");
        assert!(errs.iter().any(|e| e.contains("只认 true")), "{errs:?}");
    }

    #[test]
    fn bad_shapes_are_all_named() {
        let text = concat!(
            "schema_version = 1\n", // 版本不认
            "mystery = 1\n",        // 顶层未知键
            "\n[assemble]\n",
            "extra = 1\n",   // [assemble] 未知键
            "handles = [\n", // 条目两处不在册
            "  { trait = \"Rr3Nope\", desc_key = \"nope_key\" },\n",
            "  \"bare-string\",\n", // 非表条目
            "]\n",
            "ops = [\n",
            "  { slot = \"\", src_key = \"a..b\" },\n", // 空槽 + 坏点路径
            "  { slot = \"s\", src_key = 3 },\n",       // 非字符串
            "]\n",
        );
        let errs = rr3_registry_assemble_from_text(text).expect_err("坏形必须报错");
        assert!(
            errs.iter().any(|e| e.contains("schema_version 必须为 2")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("顶层未知键 mystery")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("[assemble] 未知键 extra")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("trait \"Rr3Nope\" 未在册")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("desc_key \"nope_key\" 未在册")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("handles[1] 应为内联表")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("ops[0].slot 不得为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("src_key \"a..b\" 不是合法点路径")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("ops[1].src_key 应为字符串")),
            "{errs:?}"
        );
    }

    #[test]
    fn missing_required_keys_are_named() {
        // 缺 schema_version / 缺 [assemble] / 缺两键
        let errs = rr3_registry_assemble_from_text("[assemble]\nhandles = []\nops = []\n")
            .expect_err("缺版本必须点名");
        assert!(errs[0].contains("缺 schema_version"), "{errs:?}");

        let errs = rr3_registry_assemble_from_text("schema_version = 2\n")
            .expect_err("缺 [assemble] 必须点名");
        assert!(
            errs.iter().any(|e| e.contains("缺 [assemble] 表")),
            "{errs:?}"
        );

        let errs = rr3_registry_assemble_from_text("schema_version = 2\n[assemble]\n")
            .expect_err("缺 handles/ops 必须点名");
        assert!(errs.iter().any(|e| e.contains("缺 handles 键")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("缺 ops 键")), "{errs:?}");
    }

    #[test]
    fn injection_gaps_walk_config_paths() {
        let facts = rr3_registry_assemble_from_text(OK_SAMPLE).expect("正例必须过");
        let cfg = "[rr3-power.battery]\nlow_voltage_mv = 3300\n";
        // i2c 句柄不参与注入缺；ops 的一条在、一条缺 → 恰一条缺口
        let gaps = rr3_registry_assemble_injection_gaps(&facts, cfg);
        assert_eq!(gaps.len(), 0, "{gaps:?}");

        let facts2 = rr3_registry_assemble_from_text(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = [\n  { slot = \"a\", src_key = \"rr3-power.battery.low_voltage_mv\" },\n  { slot = \"b\", src_key = \"rr3-power.battery.missing_key\" },\n  { slot = \"c\", src_key = \"no.such.path\" },\n]\n",
        )
        .expect("必须过形状级");
        let gaps = rr3_registry_assemble_injection_gaps(&facts2, cfg);
        assert_eq!(gaps.len(), 2, "{gaps:?}");
        assert!(
            gaps[0].contains("missing_key") && gaps[0].contains("注入缺"),
            "{gaps:?}"
        );

        // config 解析失败 = 全部来源键不可证
        let gaps = rr3_registry_assemble_injection_gaps(&facts, "garbage =\n");
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        assert!(gaps[0].contains("解析失败"), "{gaps:?}");

        // ops 为空 → 永无缺口（config 坏也不点名）；空面须明写 no_injection（L-5）
        let empty = rr3_registry_assemble_from_text(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .unwrap();
        assert!(rr3_registry_assemble_injection_gaps(&empty, "garbage =\n").is_empty());
    }
}
