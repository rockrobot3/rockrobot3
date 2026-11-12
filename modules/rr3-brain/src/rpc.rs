//! rr3-brain 协议面（批N32 迁移件1 立树，ADR-0017 五层表协议层）。
//!
//! ops 读写两面对位（自 Python composite.py 逐语义迁入）：[`rr3_ops_snapshot`]
//! ＝ `rr3-brain/ops/list` 应答体组装（批N4 件六二 B-12，白名单组体——密钥与
//! key_ref 不进任何应答，值零回显守脑面）；[`rr3_ops_write`] ＝ `rr3-brain/ops/set`
//! 写臂纯函数（批N4 件九三：白名单校验 → 类型域校验 → 写内存 doc，**只动内存
//! 不落盘**——应答 persisted:false 如实申报，重启回 config.toml 真值）。
//!
//! 写面载体 = boot.rs [`Rr3BrainCfg::doc`] 的 JSON 文档树（与 Python cfg 字典
//! 同构的 live 真值）：写源映射（asr/tts 组与 timeout_s → cloud 段、dialog 组 →
//! dialog 段、llm 组 llm_model/base_url → 模型行 active 在则行否则 cloud fallback）
//! 由 [`rr3_ops_write`] 动态解析——快照读哪份、写口就写哪份。llm 组实效值在
//! ops/list 回显面随 boot 投影（Python base_url/model_id boot 局部变量同形状，
//! live 写不回填，重启回新值——B-14① 双源分裂不在本单修）。

use serde_json::{json, Value};

use crate::boot::{
    rr3_base_url_check, rr3_brain_cfg_parse, rr3_brain_model_row, rr3_cloud_cfg_of, Rr3BrainCfg,
    Rr3LlmMode,
};
use crate::cloud::{rr3_key_set_of, Rr3LlmClient};
use crate::config_store::{rr3_block_rewrite, rr3_config_persist};

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
        .ok_or_else(|| "config 缺 [rr3-brain.cloud] 段".to_string())?;
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
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
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
    if slot == "timeout_s" {
        return rr3_ops_secs(group, slot, f);
    }
    rr3_ops_uint(group, slot, value, f)
}

/// 字符串槽：去空白后非空（B9 自 value_check 按值型拆出）
fn rr3_ops_str(group: &str, slot: &str, value: &Value) -> Result<Value, String> {
    let s = value.as_str().ok_or_else(|| {
        format!(
            "槽 {group}.{slot} 须字符串，收到 {}",
            rr3_py_type_name(value)
        )
    })?;
    let s = s.trim();
    if s.is_empty() {
        return Err(format!("槽 {group}.{slot} 不得为空"));
    }
    Ok(json!(s))
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
    if !(i > 0) {
        return Err(format!("槽 {group}.{slot} 须 >0，收到 {i}"));
    }
    Ok(json!(i))
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
    let brain = doc
        .get_mut("rr3-brain")
        .ok_or_else(|| "config 缺 [rr3-brain] 段".to_string())?;
    let (section_key, key) = rr3_ops_target(brain, group, slot)?;
    let old = rr3_ops_store(brain, section_key, &key, &new)?;
    Ok((old, new))
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
    } else if group == "asr" || group == "tts" || slot == "timeout_s" || rr3_no_model_rows(brain) {
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
    if section_key == "dialog" && brain.get("dialog").is_none() {
        brain
            .as_object_mut()
            .expect("brain 段是对象（TOML 表同构）")
            .insert("dialog".to_string(), json!({}));
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
    // Python str(args.get("group", "")) 同形状：缺键回落空串，键在则 str() coercion
    let py_arg = |key: &str| -> String {
        match args.get(key) {
            Some(v) => rr3_py_str(v),
            None => String::new(),
        }
    };
    let group = py_arg("group");
    let slot = py_arg("slot");
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

/// 模型行实效重解析（ops/set llm 组写后供客户端重建消费——dispatch.rs 接线；
/// 与 rr3_brain_model_row 同源单真值）。
pub(crate) fn rr3_model_row_of(cfg: &Rr3BrainCfg) -> Result<crate::boot::Rr3ModelRow, String> {
    rr3_brain_model_row(&cfg.doc)
}

// ---------- 模型行面（ADR-0023：面板 CRUD + 切换 active + 连通性测试） ----------

/// 模型行表快照（doc 内，缺表 = 空表；非数组点名拒——与 boot 同一文案）
fn rr3_model_rows(doc: &Value) -> Result<Vec<Value>, String> {
    match doc.get("rr3-brain").and_then(|b| b.get("model")) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(rows)) => Ok(rows.clone()),
        Some(_) => Err("config [[rr3-brain.model]] 须为行表（数组）".to_string()),
    }
}

/// 行内字符串字段（缺键/Null 回落空串；非串点名行号与键名）
fn rr3_row_str<'a>(row: &'a Value, key: &str, idx: usize) -> Result<&'a str, String> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(""),
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(_) => Err(format!("模型行 #{idx}.{key} 须字符串")),
    }
}

/// 表内 active 行名（无 → None：空表走 cloud fallback，没有行可点名）
fn rr3_row_active_name(rows: &[Value]) -> Option<String> {
    rows.iter()
        .find(|r| r.get("active").and_then(Value::as_bool) == Some(true))
        .and_then(|r| r.get("name").and_then(Value::as_str))
        .map(|s| s.to_string())
}

/// 模型行列表（`rr3-brain/model/list` 应答体）：每行回 名称/API 根/模型/格式/
/// 模式/key_ref/key 是否就位/active——**只回 `key_set` 布尔，永无密钥明文**。
pub(crate) fn rr3_model_list(cfg: &Rr3BrainCfg) -> Result<Value, String> {
    let rows = rr3_model_rows(&cfg.doc)?;
    let mut models = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        models.push(rr3_model_row(row, i)?);
    }
    Ok(json!({ "models": models, "active": rr3_row_active_name(&rows) }))
}

/// 单行投影（B9 自 list 抽出）：名称/API 根/模型/格式/模式/key_ref/key 就位/
/// active——只回 `key_set` 布尔，永无密钥明文。
fn rr3_model_row(row: &Value, i: usize) -> Result<Value, String> {
    let v = rr3_row_view(row, i)?;
    let mode_enum = match v.mode {
        "local" => Some(Rr3LlmMode::Local),
        _ => Some(Rr3LlmMode::Api),
    };
    Ok(json!({
        "name": v.name,
        "base_url": rr3_row_str(row, "base_url", i)?,
        "model_id": rr3_row_str(row, "model_id", i)?,
        "api_format": v.api_format,
        "mode": v.mode,
        "key_ref": v.key_ref,
        "key_set": rr3_key_set_of(&v.key_ref, mode_enum),
        "active": row.get("active").and_then(Value::as_bool).unwrap_or(false),
    }))
}

/// 行视图（B9 自 model_row 抽出）：name/mode/api_format 的缺省归一与 key_ref
/// 三态读取同址——投影面只做拼装。
struct Rr3RowView<'a> {
    name: &'a str,
    mode: &'a str,
    api_format: &'a str,
    key_ref: Option<String>,
}

fn rr3_row_view<'a>(row: &'a Value, i: usize) -> Result<Rr3RowView<'a>, String> {
    let name = rr3_row_str(row, "name", i)?;
    if name.trim().is_empty() {
        return Err(format!("模型行 #{i} 缺 name"));
    }
    let mode = match rr3_row_str(row, "mode", i)?.trim() {
        "" => "api",
        v => v,
    };
    let api_format = match rr3_row_str(row, "api_format", i)?.trim() {
        "" => "openai",
        v => v,
    };
    let key_ref = match rr3_row_str(row, "api_key_ref", i)?.trim() {
        "" => None,
        v => Some(v.to_string()),
    };
    Ok(Rr3RowView {
        name,
        mode,
        api_format,
        key_ref,
    })
}

/// upsert 入参（一次解析；`key_ref` 三态：缺席 = 保留现值、空串 = 清除、
/// 有值 = 校验后写——`key_ref_clear` 承载"空串清除"这一态）。
struct Rr3ModelRowInput<'a> {
    base_url: &'a str,
    model_id: &'a str,
    api_format: &'a str,
    mode: &'a str,
    key_ref: Option<String>,
    key_ref_clear: bool,
    active: Option<bool>,
}

/// `api_format` 词表核（缺省 openai；本批仅 openai）。
fn rr3_api_format_of<'a>(name: &str, args: &'a Value) -> Result<&'a str, String> {
    let api_format = args
        .get("api_format")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("openai");
    if api_format != "openai" {
        return Err(format!(
            "模型行 {name} api_format 暂不支持: {api_format}（本批仅 openai）"
        ));
    }
    Ok(api_format)
}

/// `mode` 词表核（缺省 api；允许 api/local）。
fn rr3_mode_of<'a>(name: &str, args: &'a Value) -> Result<&'a str, String> {
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("api");
    if mode != "api" && mode != "local" {
        return Err(format!("模型行 {name} mode 不认: {mode}（允许 api/local）"));
    }
    Ok(mode)
}

/// `api_key_ref` 三态核：缺席 = 保留现值（None,false）；空串 = 清除（None,true）；
/// 有值 = 无空白且 `env:` 前缀后是合法变量名后写入（Some(s),false）。
fn rr3_key_ref_input(name: &str, args: &Value) -> Result<(Option<String>, bool), String> {
    let Some(v) = args.get("api_key_ref") else {
        return Ok((None, false));
    };
    let s = v
        .as_str()
        .map(str::trim)
        .ok_or_else(|| format!("模型行 {name} api_key_ref 须字符串"))?;
    if s.is_empty() {
        return Ok((None, true));
    }
    if s.chars().any(char::is_whitespace) {
        return Err(format!("模型行 {name} api_key_ref 不得含空白: '{s}'"));
    }
    if let Some(var) = s.strip_prefix(crate::cloud::RR3_KEY_REF_ENV_PREFIX) {
        if var.is_empty() || var.chars().any(char::is_whitespace) {
            return Err(format!(
                "模型行 {name} api_key_ref 的 env: 后须变量名（非空、无空白）: '{s}'"
            ));
        }
    }
    Ok((Some(s.to_string()), false))
}

/// 模型行进参解析：必填/域名/键引用/active 逐项按旧文案 fail-closed。
fn rr3_model_row_input<'a>(name: &str, args: &'a Value) -> Result<Rr3ModelRowInput<'a>, String> {
    let base_url = args
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let model_id = args
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if base_url.is_empty() || model_id.is_empty() {
        return Err("upsert 须同时给 base_url 与 model_id（非空）".to_string());
    }
    rr3_base_url_check(base_url).map_err(|e| format!("模型行 {name} {e}"))?;
    let api_format = rr3_api_format_of(name, args)?;
    let mode = rr3_mode_of(name, args)?;
    let (key_ref, key_ref_clear) = rr3_key_ref_input(name, args)?;
    let active = match args.get("active") {
        None => None,
        Some(v) => Some(
            v.as_bool()
                .ok_or_else(|| format!("模型行 {name} active 须布尔"))?,
        ),
    };
    Ok(Rr3ModelRowInput {
        base_url,
        model_id,
        api_format,
        mode,
        key_ref,
        key_ref_clear,
        active,
    })
}

/// upsert 一行（同名即更新；新行在表内无 active 时自动接任）
fn rr3_model_upsert(rows: &mut Vec<Value>, name: &str, args: &Value) -> Result<(), String> {
    let inp = rr3_model_row_input(name, args)?;
    let (i, is_new) = rr3_row_slot(rows, name);
    let others_active = rows
        .iter()
        .enumerate()
        .any(|(j, r)| j != i && r.get("active").and_then(Value::as_bool) == Some(true));
    let prev_active = rows[i].get("active").and_then(Value::as_bool) == Some(true);
    let active = match inp.active {
        Some(a) => a,
        None if is_new => !others_active, // 表内无 active（空表/结构坏）→ 新行接任
        None => prev_active,
    };
    rr3_row_fill(rows, i, name, &inp, active)?;
    if active {
        rr3_row_deactivate(rows, i);
    }
    Ok(())
}

/// 行槽定位（B9 自 upsert 抽出）：同名行下标；缺席即补空行、返回 (下标, 是否新增)
fn rr3_row_slot(rows: &mut Vec<Value>, name: &str) -> (usize, bool) {
    match rows
        .iter()
        .position(|r| r.get("name").and_then(Value::as_str) == Some(name))
    {
        Some(i) => (i, false),
        None => {
            rows.push(json!({}));
            (rows.len() - 1, true)
        }
    }
}

/// 行写入（B9 自 upsert 抽出）：六键 + key_ref 三态（有值写/空串清/缺席保留）
fn rr3_row_fill(
    rows: &mut [Value],
    i: usize,
    name: &str,
    inp: &Rr3ModelRowInput<'_>,
    active: bool,
) -> Result<(), String> {
    let obj = rows[i]
        .as_object_mut()
        .ok_or_else(|| format!("模型行 #{i} 须为表（TOML 行表同构）"))?;
    obj.insert("name".to_string(), json!(name));
    obj.insert("mode".to_string(), json!(inp.mode));
    obj.insert("api_format".to_string(), json!(inp.api_format));
    obj.insert("base_url".to_string(), json!(inp.base_url));
    obj.insert("model_id".to_string(), json!(inp.model_id));
    rr3_row_key_ref(obj, inp);
    obj.insert("active".to_string(), json!(active));
    Ok(())
}

/// key_ref 三态：有值写 / 空串清 / 缺席保留。
fn rr3_row_key_ref(obj: &mut serde_json::Map<String, Value>, inp: &Rr3ModelRowInput<'_>) {
    if let Some(k) = &inp.key_ref {
        obj.insert("api_key_ref".to_string(), json!(k));
    } else if inp.key_ref_clear {
        obj.remove("api_key_ref");
    }
}

/// active 唯一性（B9 自 upsert 抽出）：新 active 行就位后其余行一律置 false
fn rr3_row_deactivate(rows: &mut [Value], keep: usize) {
    for (j, r) in rows.iter_mut().enumerate() {
        if j != keep {
            rr3_row_clear(r);
        }
    }
}

/// 单行 active 清零。
fn rr3_row_clear(r: &mut Value) {
    if let Some(o) = r.as_object_mut() {
        o.insert("active".to_string(), json!(false));
    }
}

/// 切换 active（其余行一律置 false）
fn rr3_model_activate(rows: &mut [Value], name: &str) -> Result<(), String> {
    let Some(i) = rows
        .iter()
        .position(|r| r.get("name").and_then(Value::as_str) == Some(name))
    else {
        return Err(format!("模型行不认: '{name}'"));
    };
    for (j, r) in rows.iter_mut().enumerate() {
        if let Some(o) = r.as_object_mut() {
            o.insert("active".to_string(), json!(j == i));
        }
    }
    Ok(())
}

/// 删除非 active 行（active 行拒删——先切换再删，保证表内始终有连接）
fn rr3_model_delete(rows: &mut Vec<Value>, name: &str) -> Result<(), String> {
    let Some(i) = rows
        .iter()
        .position(|r| r.get("name").and_then(Value::as_str) == Some(name))
    else {
        return Err(format!("模型行不认: '{name}'"));
    };
    if rows[i].get("active").and_then(Value::as_bool) == Some(true) {
        return Err(format!("模型行 {name} 是 active，先切换 active 再删"));
    }
    rows.remove(i);
    Ok(())
}

/// 模型行 CRUD 写臂（`rr3-brain/model/set`）：克隆行表改 → 渲染块 → 解析验证 →
/// 原子落盘（`.bak` + rename）→ 提交内存。任一步失败：内存与旧文件都不变。
pub(crate) fn rr3_model_set_arm(cfg: &mut Rr3BrainCfg, args: &Value) -> Result<Value, String> {
    let (op, name) = rr3_model_pick(args)?;
    let rows = rr3_model_rows(&cfg.doc)?;
    rr3_model_write(cfg, rows, op, name, args)
}

/// 写臂：落改动 → 结构门（写面自守，不依赖 boot 兜底：恰一 active）→ 提交。
fn rr3_model_write(
    cfg: &mut Rr3BrainCfg,
    mut rows: Vec<Value>,
    op: &str,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    rr3_model_op_apply(&mut rows, op, name, args)?;
    rr3_model_one_active(&rows)?;
    rr3_model_commit(cfg, rows, op, name)
}

/// 写面取参：op/name 去空白（缺省空串），空名点名拒。
fn rr3_model_pick(args: &Value) -> Result<(&str, &str), String> {
    let op = args
        .get("op")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if name.is_empty() {
        return Err("模型行 name 不得为空".to_string());
    }
    Ok((op, name))
}

/// op 分派（upsert/activate/delete；空串与词表外点名拒）。
fn rr3_model_op_apply(
    rows: &mut Vec<Value>,
    op: &str,
    name: &str,
    args: &Value,
) -> Result<(), String> {
    match op {
        "upsert" => rr3_model_upsert(rows, name, args),
        "activate" => rr3_model_activate(rows, name),
        "delete" => rr3_model_delete(rows, name),
        "" => Err("缺 op（允许 upsert/activate/delete）".to_string()),
        other => Err(format!("op 不认: '{other}'（允许 upsert/activate/delete）")),
    }
}

/// 结构门：恰一行 active=true（多 active/零 active 都是误配，写面自守）。
fn rr3_model_one_active(rows: &[Value]) -> Result<(), String> {
    let active_n = rows
        .iter()
        .filter(|r| r.get("active").and_then(Value::as_bool) == Some(true))
        .count();
    if active_n != 1 {
        return Err(format!("模型行表须恰一行 active=true（现 {active_n} 行）"));
    }
    Ok(())
}

/// 落盘 + 内存提交：文本手术 → boot 解析器预核 → 原子落盘 → 文档树/模型行同步。
/// 预核在落盘前，解析不过零字节进正档（失败面全部走完才动文件）。
fn rr3_model_commit(
    cfg: &mut Rr3BrainCfg,
    rows: Vec<Value>,
    op: &str,
    name: &str,
) -> Result<Value, String> {
    let path = cfg.config_path.clone().ok_or_else(|| {
        "模型行写面未接 config 路径（cfg.config_path 未注入）——本次未改任何文件".to_string()
    })?;
    let old_text = std::fs::read_to_string(&path)
        .map_err(|e| format!("读 config {} 失败: {e}", path.display()))?;
    let new_text = rr3_block_rewrite(&old_text, &rows)?;
    // 落盘前验证：新文本必须被 boot 解析器吃下（恰一 active/格式/URL 全过）
    let parsed = rr3_brain_cfg_parse(&new_text)?;
    rr3_config_persist(&path, &old_text, &new_text)?;
    // 提交内存（解析产物与行表同源；失败面已在落盘前全部走完）
    let mut doc = cfg.doc.clone();
    let brain = doc
        .get_mut("rr3-brain")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config 缺 [rr3-brain] 段".to_string())?;
    brain.insert("model".to_string(), Value::Array(rows.clone()));
    cfg.doc = doc;
    cfg.model = parsed.model;
    Ok(json!({
        "ok": true,
        "op": op,
        "name": name,
        "active": rr3_row_active_name(&rows),
        "persisted": true,
        "applies": "live",
    }))
}

/// 连通性测试（`rr3-brain/model/test`）：按名取行造临时客户端探测——业务失败
/// （缺 key/HTTP 非 2xx/不可达）也回正常应答，用 `ok:false` 表达，不折 Err。
pub(crate) fn rr3_model_test_arm(cfg: &Rr3BrainCfg, args: &Value) -> Result<Value, String> {
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if name.is_empty() {
        return Err("模型行 name 不得为空".to_string());
    }
    let rows = rr3_model_rows(&cfg.doc)?;
    let Some(row) = rows
        .iter()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name))
    else {
        return Err(format!("模型行不认: '{name}'"));
    };
    let base_url = row
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let model_id = row
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if base_url.is_empty() || model_id.is_empty() {
        return Err(format!("模型行 {name} 缺 base_url/model_id"));
    }
    let mode = match row.get("mode").and_then(Value::as_str).unwrap_or("api") {
        "local" => Rr3LlmMode::Local,
        _ => Rr3LlmMode::Api,
    };
    let key_ref = row
        .get("api_key_ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let timeout_s = rr3_cloud_cfg_of(&cfg.doc)?.timeout_s;
    let client = Rr3LlmClient::new(base_url, model_id, timeout_s, key_ref, Some(mode));
    match client.rr3_probe() {
        Ok((latency_ms, mock)) => Ok(json!({
            "ok": true,
            "name": name,
            "status": 200,
            "latency_ms": latency_ms,
            "mock": mock,
        })),
        Err(e) => Ok(json!({
            "ok": false,
            "name": name,
            "latency_ms": 0,
            "error": e,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::{
        rr3_brain_cfg_parse, rr3_repo_cfg_fixture, RR3_CFG_TAIL_TOML, RR3_CLOUD_TOML,
        RR3_MODEL_ROWS_TOML,
    };

    /// 与仓内 config.toml 同形状的文档树夹具（boot 解析产出，写面真载体）
    fn repo_cfg() -> Rr3BrainCfg {
        rr3_brain_cfg_parse(&rr3_repo_cfg_fixture()).unwrap()
    }

    /// live 写模型行 model_id 的公共底座（快照不回填 vs doc 真值两用例同款）
    fn write_llm_model(cfg: &mut Rr3BrainCfg, v: serde_json::Value) {
        rr3_ops_write(&mut cfg.doc, "llm", "llm_model", &v).unwrap();
    }

    // ---------- ops/list 读面 ----------

    /// 白名单组体：四组六槽逐键在册、**零多余键**（密钥/api_key_ref 不进应答，
    /// B-12 值零回显守脑面）
    #[test]
    fn ops_snapshot_whitelist_groups_exactly() {
        let cfg = repo_cfg();
        let v = rr3_ops_snapshot(&cfg).unwrap();
        assert_eq!(
            v,
            json!({
                "asr": {
                    "asr_model": "iic/SenseVoiceSmall",
                    "base_url": "https://api.siliconflow.cn",
                    "timeout_s": 20,
                },
                "llm": {
                    "llm_model": "deepseek-chat",
                    "base_url": "https://api.deepseek.com/v1",
                    "timeout_s": 20,
                },
                "tts": {
                    "tts_model": "FunAudioLLM/CosyVoice2-0.5B",
                    "base_url": "https://api.siliconflow.cn",
                    "timeout_s": 20,
                },
                "dialog": {
                    "max_utterance_ms": 8000,
                },
            }),
            "四组六槽逐键逐字（timeout_s 是 TOML 整数 20 的原样透传）"
        );
    }

    /// dialog 键缺席 → 快照缺省 8000（Python .get("max_utterance_ms", 8000) 同形状）
    #[test]
    fn ops_snapshot_dialog_defaults_8000_when_key_absent() {
        let mut cfg = repo_cfg();
        cfg.doc["rr3-brain"]
            .as_object_mut()
            .unwrap()
            .remove("dialog");
        // boot 面要求 dialog 段必填，此处直造写面载体测读面缺省路径
        let v = rr3_ops_snapshot(&cfg).unwrap();
        assert_eq!(v["dialog"]["max_utterance_ms"], 8000);
    }

    /// llm 组回显随 boot 投影（Python base_url/model_id 局部变量同形状）：live 写
    /// 模型行后快照**不回填**、重启回新值——B-14① 双源分裂的如实镜像，本测钉死
    #[test]
    fn ops_snapshot_llm_group_stays_at_boot_projection_after_live_write() {
        let mut cfg = repo_cfg();
        // live 写模型行 model_id（写源 = active 行）
        write_llm_model(&mut cfg, json!("deepseek-reasoner"));
        let v = rr3_ops_snapshot(&cfg).unwrap();
        assert_eq!(
            v["llm"]["llm_model"],
            json!("deepseek-chat"),
            "快照回显 boot 投影，live 写不回填（Python 同形状）"
        );
        assert_eq!(
            cfg.doc["rr3-brain"]["model"][0]["model_id"],
            json!("deepseek-reasoner"),
            "doc 真值已写（客户端重建/重启面读新值）"
        );
    }

    /// asr/tts/dialog 组快照随 live 写即见（读源在 cloud/dialog 段）
    #[test]
    fn ops_snapshot_asr_tts_dialog_reflect_live_writes() {
        let mut cfg = repo_cfg();
        rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!("new-asr")).unwrap();
        rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(9000)).unwrap();
        let v = rr3_ops_snapshot(&cfg).unwrap();
        assert_eq!(v["asr"]["asr_model"], json!("new-asr"));
        assert_eq!(v["dialog"]["max_utterance_ms"], 9000);
    }

    // ---------- ops/set 写面 ----------

    /// 组/槽白名单两红（文案逐字对位 Python，含 Python 列表 repr 形状）
    #[test]
    fn ops_set_rejects_unknown_group_and_slot() {
        let mut cfg = repo_cfg();
        let e = rr3_ops_write(&mut cfg.doc, "memory", "x", &json!(1)).unwrap_err();
        assert_eq!(
            e, "组不认: 'memory'（白名单 ['asr', 'dialog', 'llm', 'tts']）",
            "组不认文案逐字"
        );
        let e = rr3_ops_write(&mut cfg.doc, "asr", "sample_rate", &json!(1)).unwrap_err();
        assert_eq!(
            e, "组 asr 槽不认: 'sample_rate'（白名单 ['asr_model', 'base_url', 'timeout_s']）",
            "槽不认文案逐字"
        );
    }

    /// 字符串槽三径：非串点名（Python 类型词）、空白拒、strip 落值
    #[test]
    fn ops_set_string_slots_validate() {
        let mut cfg = repo_cfg();
        let e = rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!(42)).unwrap_err();
        assert_eq!(
            e, "槽 asr.asr_model 须字符串，收到 int",
            "类型词 Python 对位"
        );
        let e = rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!(null)).unwrap_err();
        assert_eq!(e, "槽 asr.asr_model 须字符串，收到 NoneType");
        let e = rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!("   ")).unwrap_err();
        assert_eq!(e, "槽 asr.asr_model 不得为空", "strip 后空拒");
        let (old, new) =
            rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!("  new-asr  ")).unwrap();
        assert_eq!(old, json!("iic/SenseVoiceSmall"));
        assert_eq!(new, json!("new-asr"), "strip 落值");
        assert_eq!(
            cfg.doc["rr3-brain"]["cloud"]["asr_model"],
            json!("new-asr"),
            "写源 = cloud 段"
        );
    }

    /// timeout_s 槽：布尔拒（先于数值——Python isinstance(bool) 同序）、非数点名、
    /// ≤0 拒、数值转 float 落盘（写源 = cloud 段，四组任一组的 timeout_s 同路）
    #[test]
    fn ops_set_timeout_s_validates_and_writes_cloud() {
        let mut cfg = repo_cfg();
        let e = rr3_ops_write(&mut cfg.doc, "asr", "timeout_s", &json!(true)).unwrap_err();
        assert_eq!(
            e, "槽 asr.timeout_s 须数值，收到 bool",
            "布尔先拒（Python 同序）"
        );
        let e = rr3_ops_write(&mut cfg.doc, "llm", "timeout_s", &json!("30")).unwrap_err();
        assert_eq!(e, "槽 llm.timeout_s 须数值，收到 str");
        let e = rr3_ops_write(&mut cfg.doc, "tts", "timeout_s", &json!(0)).unwrap_err();
        assert_eq!(
            e, "槽 tts.timeout_s 须 >0，收到 0.0",
            ">0 执法（Python float 形状）"
        );
        let e = rr3_ops_write(&mut cfg.doc, "tts", "timeout_s", &json!(-1.5)).unwrap_err();
        assert_eq!(e, "槽 tts.timeout_s 须 >0，收到 -1.5");
        let (old, new) = rr3_ops_write(&mut cfg.doc, "llm", "timeout_s", &json!(25)).unwrap();
        assert_eq!(old, json!(20), "old 是 doc 原值（TOML 整数 20）");
        assert_eq!(
            new,
            json!(25.0),
            "new 转 float（Python float(value) 同形状）"
        );
        assert_eq!(
            cfg.doc["rr3-brain"]["cloud"]["timeout_s"],
            json!(25.0),
            "llm 组的 timeout_s 也写 cloud 段（写源映射；dialog 组白名单无 timeout_s）"
        );
    }

    /// max_utterance_ms 槽：布尔拒、非整数拒、≤0 拒、整数落盘（写源 = dialog 段）
    #[test]
    fn ops_set_max_utterance_validates_and_writes_dialog() {
        let mut cfg = repo_cfg();
        let e =
            rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(false)).unwrap_err();
        assert_eq!(e, "槽 dialog.max_utterance_ms 须数值，收到 bool");
        let e =
            rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(8000.5)).unwrap_err();
        assert_eq!(
            e, "槽 dialog.max_utterance_ms 须整数，收到 8000.5",
            "小数拒"
        );
        let e = rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(0)).unwrap_err();
        assert_eq!(e, "槽 dialog.max_utterance_ms 须 >0，收到 0");
        let (old, new) =
            rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(9000)).unwrap();
        assert_eq!(old, json!(8000));
        assert_eq!(new, json!(9000));
        assert_eq!(
            cfg.doc["rr3-brain"]["dialog"]["max_utterance_ms"],
            json!(9000),
            "写源 = dialog 段"
        );
    }

    /// old 键缺席 = Null（Python dict.get 同形状）：dialog 段无键时首写 old=null
    #[test]
    fn ops_set_old_is_null_when_key_absent() {
        let mut cfg = repo_cfg();
        cfg.doc["rr3-brain"]["dialog"]
            .as_object_mut()
            .unwrap()
            .remove("max_utterance_ms");
        let (old, new) =
            rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(9000)).unwrap();
        assert_eq!(
            old,
            Value::Null,
            "键缺席 old = null（Python dict.get 同形状）"
        );
        assert_eq!(new, json!(9000));
    }

    /// dialog 段整段缺席 → setdefault 建段后照写（Python setdefault 同义）
    #[test]
    fn ops_set_creates_missing_dialog_section() {
        let mut cfg = repo_cfg();
        cfg.doc["rr3-brain"]
            .as_object_mut()
            .unwrap()
            .remove("dialog");
        rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(9000)).unwrap();
        assert_eq!(
            cfg.doc["rr3-brain"]["dialog"]["max_utterance_ms"],
            json!(9000),
            "缺段建段照写"
        );
    }

    /// llm 组写源两态：行表在 → 写 active 行（llm_model → model_id 键名换算）；
    /// 行表缺 → 写 cloud 段（fallback 路径）
    #[test]
    fn ops_set_llm_write_source_two_states() {
        let mut cfg = repo_cfg();
        let (old, new) = rr3_ops_write(
            &mut cfg.doc,
            "llm",
            "llm_model",
            &json!("deepseek-reasoner"),
        )
        .unwrap();
        assert_eq!(old, json!("deepseek-chat"));
        assert_eq!(new, json!("deepseek-reasoner"));
        assert_eq!(
            cfg.doc["rr3-brain"]["model"][0]["model_id"],
            json!("deepseek-reasoner"),
            "行表在 → 写 active 行 model_id 键"
        );
        assert_eq!(
            cfg.doc["rr3-brain"]["cloud"]["llm_model"],
            json!("deepseek-ai/DeepSeek-V3"),
            "cloud 段不动（实效源在行）"
        );
        // 行表缺 → cloud fallback
        let mut cfg = repo_cfg();
        cfg.doc["rr3-brain"]
            .as_object_mut()
            .unwrap()
            .remove("model");
        let (_, new) = rr3_ops_write(&mut cfg.doc, "llm", "llm_model", &json!("new-llm")).unwrap();
        assert_eq!(new, json!("new-llm"));
        assert_eq!(
            cfg.doc["rr3-brain"]["cloud"]["llm_model"],
            json!("new-llm"),
            "行表缺 → 写 cloud 段"
        );
    }

    /// 行表结构坏（active 数≠1）写面点名拒（"写面不修结构错误"——直造文档树，
    /// 绕开 boot 校验，钉写面自身防线）
    #[test]
    fn ops_set_llm_rejects_broken_row_structure() {
        let mut doc: Value = serde_json::from_str(
            r#"{"rr3-brain": {"cloud": {"base_url": "u"}, "model": [
                {"name": "甲", "active": true}, {"name": "乙", "active": true}]}}"#,
        )
        .unwrap();
        let e = rr3_ops_write(&mut doc, "llm", "llm_model", &json!("x")).unwrap_err();
        assert_eq!(
            e, "[[rr3-brain.model]] 须恰一行 active=true（现 2 行）——写面不修结构错误",
            "两 active 行结构坏点名逐字（单 active 是合法结构会照写）"
        );
    }

    /// ops/set 臂应答六键体（ok/group/slot/old/new/persisted/applies——live 内存
    /// 生效不落盘如实申报）
    #[test]
    fn ops_set_arm_reply_shape() {
        let mut cfg = repo_cfg();
        let v = rr3_ops_set_arm(
            &mut cfg,
            &json!({"group": "dialog", "slot": "max_utterance_ms", "value": 9000}),
        )
        .unwrap();
        assert_eq!(
            v,
            json!({
                "ok": true,
                "group": "dialog",
                "slot": "max_utterance_ms",
                "old": 8000,
                "new": 9000,
                "persisted": false,
                "applies": "live",
            }),
            "六键应答体逐字（persisted:false 不落盘如实申报）"
        );
        // 业务失败走 Err（serve 折 {"error": …} 应答——Python try/except 同 wire）
        let e = rr3_ops_set_arm(&mut cfg, &json!({"group": "nope", "slot": "x", "value": 1}))
            .unwrap_err();
        assert!(e.contains("组不认"), "违规走 Err: {e}");
    }

    /// group/slot 的 str() coercion（Python str(args.get(key, "")) 同形状）：缺字段
    /// 回落空串 → 组不认点名
    #[test]
    fn ops_set_arm_coerces_group_slot_like_python_str() {
        let mut cfg = repo_cfg();
        let e = rr3_ops_set_arm(&mut cfg, &json!({})).unwrap_err();
        assert_eq!(
            e, "组不认: ''（白名单 ['asr', 'dialog', 'llm', 'tts']）",
            "缺字段回落空串同形状"
        );
        let e = rr3_ops_set_arm(&mut cfg, &json!({"group": 5, "slot": null})).unwrap_err();
        assert_eq!(
            e, "组不认: '5'（白名单 ['asr', 'dialog', 'llm', 'tts']）",
            "str(5)='5' coercion 同形状"
        );
    }

    /// 模型行实效重解析（迁移件三客户端重建的取数口）：live 写后读新值
    #[test]
    fn model_row_of_reflects_live_write() {
        let mut cfg = repo_cfg();
        write_llm_model(&mut cfg, json!("deepseek-reasoner"));
        let row = rr3_model_row_of(&cfg).unwrap();
        assert_eq!(row.model_id, "deepseek-reasoner", "重解析读 doc 真值");
    }

    // ---------- 模型行面（ADR-0023） ----------

    /// set 臂的真载体：磁盘上一份与 repo_cfg 同文本的 config.toml + 注入路径
    fn temp_cfg() -> (std::path::PathBuf, std::path::PathBuf, Rr3BrainCfg) {
        // 无 [rr3-brain] 头：三段常量拼装（与 boot 夹具同一份文本判据）
        let text = format!(
            "{}\n{}\n{}",
            RR3_CLOUD_TOML, RR3_MODEL_ROWS_TOML, RR3_CFG_TAIL_TOML
        );
        let dir = std::env::temp_dir().join(format!(
            "rr3-model-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, &text).unwrap();
        let mut cfg = rr3_brain_cfg_parse(&text).unwrap();
        cfg.config_path = Some(path.clone());
        (dir, path, cfg)
    }

    /// 列表快照：逐键形状齐备、只回 key_ref/key_set（无密钥明文）、active 点名
    #[test]
    fn model_list_snapshot_reports_rows_without_secret() {
        let cfg = repo_cfg();
        let v = rr3_model_list(&cfg).unwrap();
        assert_eq!(v["active"], json!("DeepSeek官方"));
        let row = &v["models"][0];
        assert_eq!(row["name"], json!("DeepSeek官方"));
        assert_eq!(row["mode"], json!("api"));
        assert_eq!(row["api_format"], json!("openai"));
        assert_eq!(row["base_url"], json!("https://api.deepseek.com/v1"));
        assert_eq!(row["model_id"], json!("deepseek-chat"));
        assert_eq!(row["key_ref"], json!("DeepSeek官方"));
        assert!(row["key_set"].is_boolean(), "只回布尔，不回密钥值");
        assert_eq!(row["active"], json!(true));
    }

    /// 空行表：models 空、active = null（cloud fallback 无行可点名）
    #[test]
    fn model_list_snapshot_empty_table_has_null_active() {
        let mut cfg = repo_cfg();
        cfg.doc["rr3-brain"]
            .as_object_mut()
            .unwrap()
            .remove("model");
        let v = rr3_model_list(&cfg).unwrap();
        assert_eq!(v, json!({ "models": [], "active": null }));
    }

    /// upsert：新行落盘（.bak 备份 + 正式文件新值）、内存同步、应答 persisted/live
    #[test]
    fn model_set_upsert_persists_and_hot_swaps_active() {
        let (dir, path, mut cfg) = temp_cfg();
        let reply = rr3_model_set_arm(
            &mut cfg,
            &json!({
                "op": "upsert",
                "name": "GLM",
                "base_url": "https://open.bigmodel.cn/api/paas/v4",
                "model_id": "glm-4.5",
                "api_key_ref": "GLM",
                "active": true,
            }),
        )
        .unwrap();
        assert_eq!(
            reply,
            json!({
                "ok": true,
                "op": "upsert",
                "name": "GLM",
                "active": "GLM",
                "persisted": true,
                "applies": "live",
            })
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("name = \"GLM\""), "{text}");
        assert!(
            text.contains("base_url = \"https://open.bigmodel.cn/api/paas/v4\""),
            "{text}"
        );
        assert_eq!(text.matches("active = true").count(), 1, "{text}");
        // .bak = 旧全量；内存投影同步为新 active 行
        let bak = std::fs::read_to_string(dir.join("config.toml.bak")).unwrap();
        assert!(bak.contains("name = \"DeepSeek官方\""), "{bak}");
        assert!(!bak.contains("name = \"GLM\""), "{bak}");
        assert_eq!(cfg.model.model_id, "glm-4.5");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// activate 切行 / active 行拒删 / 删后剩行仍恰一 active
    #[test]
    fn model_set_activate_and_delete_rules() {
        let (dir, _path, mut cfg) = temp_cfg();
        rr3_model_set_arm(
            &mut cfg,
            &json!({
                "op": "upsert",
                "name": "Qwen",
                "base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "model_id": "qwen-plus",
                "active": false,
            }),
        )
        .unwrap();
        // active 行拒删（先切换再删）
        let e = rr3_model_set_arm(&mut cfg, &json!({"op": "delete", "name": "DeepSeek官方"}))
            .unwrap_err();
        assert!(e.contains("先切换 active 再删"), "{e}");
        // 切换后旧 active 变可删
        rr3_model_set_arm(&mut cfg, &json!({"op": "activate", "name": "Qwen"})).unwrap();
        assert_eq!(cfg.model.model_id, "qwen-plus");
        let reply =
            rr3_model_set_arm(&mut cfg, &json!({"op": "delete", "name": "DeepSeek官方"})).unwrap();
        assert_eq!(reply["active"], json!("Qwen"));
        let v = rr3_model_list(&cfg).unwrap();
        assert_eq!(v["models"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 写面自守两红：base_url 非完整 API 根 / 未注入 config 路径（都不动文件）
    #[test]
    fn model_set_rejects_bad_root_and_missing_path() {
        let (dir, _path, mut cfg) = temp_cfg();
        let e = rr3_model_set_arm(
            &mut cfg,
            &json!({
                "op": "upsert",
                "name": "Bad",
                "base_url": "https://x/v1/chat/completions",
                "model_id": "m",
                "active": false,
            }),
        )
        .unwrap_err();
        assert!(e.contains("不得含 /chat/completions"), "{e}");
        cfg.config_path = None;
        let e = rr3_model_set_arm(
            &mut cfg,
            &json!({
                "op": "upsert",
                "name": "Ok",
                "base_url": "https://x/v1",
                "model_id": "m",
                "active": false,
            }),
        )
        .unwrap_err();
        assert!(e.contains("未接 config 路径"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 连通性测试：mock 档回 ok:true + mock:true（业务失败走 ok:false，不折 Err）
    #[test]
    fn model_test_arm_mock_and_unknown_row() {
        std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "mock");
        let cfg = repo_cfg();
        let v = rr3_model_test_arm(&cfg, &json!({"name": "DeepSeek官方"})).unwrap();
        assert_eq!(v["ok"], json!(true));
        assert_eq!(v["mock"], json!(true));
        let e = rr3_model_test_arm(&cfg, &json!({"name": "不存在"})).unwrap_err();
        assert!(e.contains("模型行不认"), "{e}");
    }
}
