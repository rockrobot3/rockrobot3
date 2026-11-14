//! ops 读面子（白名单组体快照）与写臂（白名单/类型域校验、写源落位）。

use serde_json::{json, Value};

use crate::boot::{rr3_brain_model_row, Rr3BrainCfg};

/// ops 槽快照（自 composite.py `rr3_ops_snapshot` 逐语义迁入，批N4 件六二 B-12：
/// `rr3-brain/ops/list` 应答体，面板脑面读口）。按原子分组、槽名与 assemble.toml
/// 申报同源（asr/llm/tts/dialog 四组）——**白名单组体**：只取六槽名单键，config
/// 其余键（含密钥/凭据/模型行 api_key_ref）一律不进应答。
///
/// llm 组取 boot 投影（`cfg.model`——Python boot 局部变量同形状，live 写不回填）；
/// asr/tts/dialog 组读 live doc（写后即见）。cloud 段键由 boot 校验保证在册，
/// 缺失时 Err 点名（Python KeyError 直取同义，fail-closed 不静默）。
pub(crate) fn rr3_ops_snapshot(cfg: &Rr3BrainCfg) -> Result<Value, String> {
    let cloud = cfg
        .doc
        .get("rr3-brain")
        .and_then(|b| b.get("cloud"))
        .ok_or("config 缺 [rr3-brain.cloud] 段")?;
    let cloud_str = |key: &str| -> Result<Value, String> {
        cloud
            .get(key)
            .cloned()
            .ok_or_else(|| format!("config [rr3-brain.cloud] 缺 {key}（boot 校验应已拦截）"))
    };
    let asr_model = cloud_str("asr_model")?;
    let base_url = cloud_str("base_url")?;
    let tts_model = cloud_str("tts_model")?;
    let timeout_s = cloud_str("timeout_s")?;
    let dialog_max = cfg
        .doc
        .get("rr3-brain")
        .and_then(|b| b.get("dialog"))
        .and_then(|d| d.get("max_utterance_ms"))
        .cloned()
        .unwrap_or(json!(8000));
    Ok(json!({
        "asr": {
            "asr_model": asr_model,
            "base_url": base_url,
            "timeout_s": timeout_s,
        },
        "llm": {
            "llm_model": cfg.model.model_id,
            "base_url": cfg.model.base_url,
            "timeout_s": timeout_s,
        },
        "tts": {
            "tts_model": tts_model,
            "base_url": base_url,
            "timeout_s": timeout_s,
        },
        "dialog": {
            "max_utterance_ms": dialog_max,
        },
    }))
}

/// 写面白名单（自 composite.py `RR3_OPS_WRITE_GROUPS` 迁入，与 rr3_ops_snapshot
/// 读面同源对偶）：组 → 槽名单。
const RR3_OPS_WRITE_GROUPS: &[(&str, &[&str])] = &[
    ("asr", &["asr_model", "base_url", "timeout_s"]),
    ("llm", &["llm_model", "base_url", "timeout_s"]),
    ("tts", &["tts_model", "base_url", "timeout_s"]),
    ("dialog", &["max_utterance_ms"]),
];

/// 字符串槽名单（对位 composite.py `RR3_OPS_STRING_SLOTS`）
const RR3_OPS_STRING_SLOTS: &[&str] = &["asr_model", "llm_model", "tts_model", "base_url"];

/// 组名单 Python sorted() 形状（错误文案逐字对位：`['asr', 'dialog', 'llm', 'tts']`）
fn rr3_ops_groups_text() -> String {
    let mut names: Vec<&str> = RR3_OPS_WRITE_GROUPS.iter().map(|(g, _)| *g).collect();
    names.sort_unstable();
    let quoted: Vec<String> = names.iter().map(|g| format!("'{g}'")).collect();
    format!("[{}]", quoted.join(", "))
}

/// 某组槽名单 Python sorted() 形状（`['asr_model', 'base_url', 'timeout_s']`）
fn rr3_ops_slots_text(group: &str) -> String {
    let mut slots: Vec<&str> = RR3_OPS_WRITE_GROUPS
        .iter()
        .find(|(g, _)| *g == group)
        .map(|(_, s)| s.to_vec())
        .unwrap_or_default();
    slots.sort_unstable();
    let quoted: Vec<String> = slots.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", quoted.join(", "))
}

/// Python type(value).__name__ 对位（错误文案逐字对位的数据基础）：JSON 值 →
/// Python 侧同名类型词（json.loads 产物形状）。
fn rr3_py_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python str() 对位（ops/set args 的 group/slot 字段与 completion text 参数：
/// Python str(args.get(...)) 的 coercion 形状——str 原样，其余按 Python str()
/// 常见产物折词）。
pub(crate) fn rr3_py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        other => other.to_string(),
    }
}

/// ops 槽写臂纯函数（自 composite.py `rr3_ops_write` 逐语义迁入，批N4 件九三）：
/// 白名单校验 → 类型域值校验 → 写内存 doc。返回 `(old, new)` 供应答回显；违规
/// 一律 Err 点名拒因（fail-closed，不静默夹值）。old 是 doc 内原值（键缺席 =
/// Null——Python dict.get 同形状），写后 new 落 doc。
///
/// 生效语义 = live（调用方写后重建受影响客户端——cloud 三客户端重建随迁移件三
/// 落、dialog 参数消费随迁移件四落，本件写面先全语义落位）；密钥与 key_ref 不在
/// 写面（密钥面走 panel secrets 端点，值零回显 B-12）。
pub(crate) fn rr3_ops_write(
    doc: &mut Value,
    group: &str,
    slot: &str,
    value: &Value,
) -> Result<(Value, Value), String> {
    rr3_ops_slot_check(group, slot)?;
    let new = rr3_ops_value_check(group, slot, value)?;
    rr3_ops_apply(doc, group, slot, new)
}

/// 组/槽白名单校验（组不认/槽不认逐条点名）。
fn rr3_ops_slot_check(group: &str, slot: &str) -> Result<(), String> {
    let slots = rr3_ops_slots_of(group)?;
    if !slots.contains(&slot) {
        return Err(format!(
            "组 {group} 槽不认: '{slot}'（白名单 {}）",
            rr3_ops_slots_text(group)
        ));
    }
    Ok(())
}

/// 组白名单取槽名单（组不认点名）。
fn rr3_ops_slots_of(group: &str) -> Result<&'static [&'static str], String> {
    RR3_OPS_WRITE_GROUPS
        .iter()
        .find(|(g, _)| *g == group)
        .map(|(_, slots)| *slots)
        .ok_or_else(|| format!("组不认: '{group}'（白名单 {}）", rr3_ops_groups_text()))
}

/// 类型域值校验（Python 分支同序：字符串槽 → timeout_s → max_utterance_ms），
/// 返回落 doc 的规范化值。
fn rr3_ops_value_check(group: &str, slot: &str, value: &Value) -> Result<Value, String> {
    if RR3_OPS_STRING_SLOTS.contains(&slot) {
        return rr3_ops_str(group, slot, value);
    }
    let f = rr3_ops_num(group, slot, value)?;
    rr3_ops_num_slot(group, slot, value, f)
}

/// 数值槽按语义分流：`timeout_s` 走秒域，其余走正整数域。
fn rr3_ops_num_slot(group: &str, slot: &str, value: &Value, f: f64) -> Result<Value, String> {
    if slot == "timeout_s" {
        return rr3_ops_secs(group, slot, f);
    }
    rr3_ops_uint(group, slot, value, f)
}

/// 字符串槽：去空白后非空（B9 自 value_check 按值型拆出）
fn rr3_ops_str(group: &str, slot: &str, value: &Value) -> Result<Value, String> {
    let s = rr3_slot_str(group, slot, value)?;
    if s.is_empty() {
        return Err(format!("槽 {group}.{slot} 不得为空"));
    }
    Ok(json!(s))
}

/// 字符串槽取值（trim 后），非串点名 Python 类型词。
fn rr3_slot_str<'a>(group: &str, slot: &str, value: &'a Value) -> Result<&'a str, String> {
    value.as_str().map(str::trim).ok_or_else(|| {
        format!(
            "槽 {group}.{slot} 须字符串，收到 {}",
            rr3_py_type_name(value)
        )
    })
}

/// 数值前置门：布尔与一切非数值一律拒（Python 侧 bool 是 int 子类，必须显式拦）
fn rr3_ops_num(group: &str, slot: &str, value: &Value) -> Result<f64, String> {
    if value.is_boolean() || !value.is_number() {
        return Err(format!(
            "槽 {group}.{slot} 须数值，收到 {}",
            rr3_py_type_name(value)
        ));
    }
    Ok(value.as_f64().unwrap_or(0.0))
}

/// timeout_s：有限正浮点（NaN 不可比，显式拒，判据"必须 >0"不得让 NaN 漏过）
fn rr3_ops_secs(group: &str, slot: &str, f: f64) -> Result<Value, String> {
    if f.is_nan() || f <= 0.0 {
        return Err(format!(
            "槽 {group}.{slot} 须 >0，收到 {}",
            rr3_py_float_str(f)
        ));
    }
    Ok(json!(f))
}

/// 整数槽（白名单仅 max_utterance_ms）：整值判据 + 正值判据
fn rr3_ops_uint(group: &str, slot: &str, value: &Value, f: f64) -> Result<Value, String> {
    if !rr3_uint_shaped(value, f) {
        return Err(format!(
            "槽 {group}.{slot} 须整数，收到 {}",
            rr3_py_float_str(f)
        ));
    }
    let i = f as i64;
    rr3_pos_int(group, slot, i)?;
    Ok(json!(i))
}

/// 正整数判据（0/负值点名，>0 放行）。
fn rr3_pos_int(group: &str, slot: &str, i: i64) -> Result<(), String> {
    if i > 0 {
        return Ok(());
    }
    Err(format!("槽 {group}.{slot} 须 >0，收到 {i}"))
}

/// 整值判据：JSON 整数，或整值浮点（`.0` 尾）。
fn rr3_uint_shaped(value: &Value, f: f64) -> bool {
    value.is_i64() || value.is_u64() || f.fract() == 0.0
}

/// 写源落位（Python rr3_ops_write 尾段同形状）：返回 (old, new)。
fn rr3_ops_apply(
    doc: &mut Value,
    group: &str,
    slot: &str,
    new: Value,
) -> Result<(Value, Value), String> {
    let brain = rr3_ops_brain_mut(doc)?;
    let (section_key, key) = rr3_ops_target(brain, group, slot)?;
    let old = rr3_ops_store(brain, section_key, &key, &new)?;
    Ok((old, new))
}

/// 写臂根段：`[rr3-brain]` 缺席即回错。
fn rr3_ops_brain_mut(doc: &mut Value) -> Result<&mut Value, String> {
    doc.get_mut("rr3-brain")
        .ok_or("config 缺 [rr3-brain] 段".to_string())
}

/// 写源映射（Python rr3_ops_write 尾段同形状，B9 自 apply 抽出）：dialog →
/// dialog 段；asr/tts 全槽与 llm.timeout_s → cloud 段；llm 组 llm_model/base_url
/// → 实效源（模型行 active 在则写行、否则 cloud fallback）。返回 (段名, 行内键)。
fn rr3_ops_target(
    brain: &Value,
    group: &str,
    slot: &str,
) -> Result<(&'static str, String), String> {
    let section = if group == "dialog" {
        "dialog"
    } else if ["asr", "tts"].contains(&group) || slot == "timeout_s" || rr3_no_model_rows(brain) {
        // asr/tts/timeout 恒写 cloud；llm 组无模型行时写行无处落，同落 cloud
        "cloud"
    } else {
        let actives = brain
            .get("model")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter(|r| r.get("active").and_then(Value::as_bool) == Some(true))
                    .count()
            })
            .unwrap_or(0);
        if actives != 1 {
            return Err(format!(
                "[[rr3-brain.model]] 须恰一行 active=true（现 {actives} 行）——写面不修结构错误"
            ));
        }
        "model"
    };
    // model 路写键名换算（行内键：llm_model → model_id；base_url → base_url）
    let key = if section == "model" && slot == "llm_model" {
        "model_id".to_string()
    } else {
        slot.to_string()
    };
    Ok((section, key))
}

/// 模型行表是否缺省（缺表/空表走 cloud fallback；非数组视同非空由调用方拦）
fn rr3_no_model_rows(brain: &Value) -> bool {
    brain
        .get("model")
        .and_then(Value::as_array)
        .map(|r| r.is_empty())
        .unwrap_or(true)
}

/// 写源落盘（B9 自 apply 抽出）：model → 唯一 active 行；dialog 缺段建段；
/// 其余段直写。old = 键现值（缺键 Null），new 由调用方克隆写入。
fn rr3_ops_store(
    brain: &mut Value,
    section_key: &str,
    key: &str,
    new: &Value,
) -> Result<Value, String> {
    if section_key == "model" {
        return rr3_row_store(brain, key, new);
    }
    if section_key == "dialog" {
        brain
            .as_object_mut()
            .expect("brain 段是对象（TOML 表同构）")
            .entry("dialog")
            .or_insert(json!({}));
    }
    let section = brain
        .get_mut(section_key)
        .ok_or_else(|| format!("config 缺 [rr3-brain.{section_key}] 段"))?;
    let old = section.get(key).cloned().unwrap_or(Value::Null);
    section
        .as_object_mut()
        .ok_or_else(|| format!("config [rr3-brain.{section_key}] 须为表"))?
        .insert(key.to_string(), new.clone());
    Ok(old)
}

/// 模型行写源（B9 自 apply 抽出）：写进唯一 active 行（结构已验 actives==1），
/// 不是行表本身。
fn rr3_row_store(brain: &mut Value, key: &str, new: &Value) -> Result<Value, String> {
    let row = brain
        .get_mut("model")
        .and_then(Value::as_array_mut)
        .expect("model 是行表（上方结构校验已拦截）")
        .iter_mut()
        .find(|r| r.get("active").and_then(Value::as_bool) == Some(true))
        .expect("active 行在（上方结构校验已拦截）");
    let old = row.get(key).cloned().unwrap_or(Value::Null);
    row.as_object_mut()
        .expect("模型行是表（TOML 行表同构）")
        .insert(key.to_string(), new.clone());
    Ok(old)
}

/// Python float 的 f"{value}" 形状（错误文案逐字对位的数据基础）：整值不带 .0
/// （Python f"{8000.0}" = "8000.0"，f"{1.5}" = "1.5"——注意与 str() 同形）。
fn rr3_py_float_str(x: f64) -> String {
    crate::boot::rr3_py_float_repr(x)
}

/// ops/set 臂（自 composite.py 主循环 ops/set 分支逐语义迁入）：args 剥
/// group/slot/value（Python str() coercion 同形状）→ [`rr3_ops_write`] → 成功应答
/// 六键体（ok/group/slot/old/new/persisted:false/applies:live）。业务失败返 Err
/// 由 serve 折 `{"error": …}` 应答（与 Python try/except RuntimeError 同 wire）。
/// 受影响客户端重建随迁移件三落（cloud 三客户端 Rust 化后接管）；dialog 参数
/// 消费随迁移件四落。
pub(crate) fn rr3_ops_set_arm(cfg: &mut Rr3BrainCfg, args: &Value) -> Result<Value, String> {
    let (group, slot) = rr3_set_args(args);
    let (old, new) = rr3_ops_write(
        &mut cfg.doc,
        &group,
        &slot,
        args.get("value").unwrap_or(&Value::Null),
    )?;
    Ok(json!({
        "ok": true,
        "group": group,
        "slot": slot,
        "old": old,
        "new": new,
        "persisted": false,
        "applies": "live",
    }))
}

/// 取参：group/slot 按 Python `str(args.get(key, ""))` 同形状（缺键回落空串）。
fn rr3_set_args(args: &Value) -> (String, String) {
    let py_arg = |key: &str| -> String {
        match args.get(key) {
            Some(v) => rr3_py_str(v),
            None => String::new(),
        }
    };
    (py_arg("group"), py_arg("slot"))
}

/// 模型行实效重解析（ops/set llm 组写后供客户端重建消费——dispatch.rs 接线；
/// 与 rr3_brain_model_row 同源单真值）。
pub(crate) fn rr3_model_row_of(cfg: &Rr3BrainCfg) -> Result<crate::boot::Rr3ModelRow, String> {
    rr3_brain_model_row(&cfg.doc)
}
