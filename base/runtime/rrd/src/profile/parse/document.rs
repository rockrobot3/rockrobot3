//! 装配单解读入口：根表 schema_version 门与各段解析的装配顺序（错误追加序＝段序）。

use std::path::{Path, PathBuf};

use super::super::model::*;
use super::bindings::{rr3_bindings_parse, rr3_enabled_parse};
use super::hardware::rr3_hardware_parse;
use super::pins::rr3_pins_parse;
use super::platform::rr3_platform_parse;
use super::runtime::rr3_runtime_parse;
use super::slots::rr3_slots_parse;
use super::supplies::rr3_supplies_parse;

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
