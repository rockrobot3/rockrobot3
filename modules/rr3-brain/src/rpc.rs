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
fn rr3_ops_group_whitelist_text() -> String {
    let mut names: Vec<&str> = RR3_OPS_WRITE_GROUPS.iter().map(|(g, _)| *g).collect();
    names.sort_unstable();
    let quoted: Vec<String> = names.iter().map(|g| format!("'{g}'")).collect();
    format!("[{}]", quoted.join(", "))
}

/// 某组槽名单 Python sorted() 形状（`['asr_model', 'base_url', 'timeout_s']`）
fn rr3_ops_slot_whitelist_text(group: &str) -> String {
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

/// Python str() 对位（ops/set args 的 group/slot 字段：Python str(args.get(...))
/// 的 coercion 形状——str 原样，其余按 Python str() 常见产物折词）。
fn rr3_py_str(v: &Value) -> String {
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
    let (_, slots) = RR3_OPS_WRITE_GROUPS
        .iter()
        .find(|(g, _)| *g == group)
        .ok_or_else(|| {
            format!("组不认: '{group}'（白名单 {}）", rr3_ops_group_whitelist_text())
        })?;
    if !slots.contains(&slot) {
        return Err(format!(
            "组 {group} 槽不认: '{slot}'（白名单 {}）",
            rr3_ops_slot_whitelist_text(group)
        ));
    }

    // 类型域值校验（Python 分支同序：字符串槽 → timeout_s → max_utterance_ms）
    let new: Value = if RR3_OPS_STRING_SLOTS.contains(&slot) {
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
        json!(s)
    } else if slot == "timeout_s" {
        if value.is_boolean() || !value.is_number() {
            return Err(format!(
                "槽 {group}.{slot} 须数值，收到 {}",
                rr3_py_type_name(value)
            ));
        }
        let f = value.as_f64().unwrap_or(0.0);
        if !(f > 0.0) {
            return Err(format!("槽 {group}.{slot} 须 >0，收到 {}", rr3_py_float_str(f)));
        }
        json!(f)
    } else {
        // max_utterance_ms（白名单仅此一整数槽，走这里即它）
        if value.is_boolean() || !value.is_number() {
            return Err(format!(
                "槽 {group}.{slot} 须数值，收到 {}",
                rr3_py_type_name(value)
            ));
        }
        let is_int = value.is_i64() || value.is_u64();
        let f = value.as_f64().unwrap_or(0.0);
        if !is_int && f.fract() != 0.0 {
            return Err(format!(
                "槽 {group}.{slot} 须整数，收到 {}",
                rr3_py_float_str(f)
            ));
        }
        let i = f as i64;
        if !(i > 0) {
            return Err(format!("槽 {group}.{slot} 须 >0，收到 {i}"));
        }
        json!(i)
    };

    // 写源映射（Python rr3_ops_write 尾段同形状）：dialog → dialog 段（缺段建段，
    // setdefault 同义）；asr/tts 全槽与 llm.timeout_s → cloud 段；llm 组
    // llm_model/base_url → 实效源（模型行 active 在则写行、否则 cloud fallback）
    let brain = doc
        .get_mut("rr3-brain")
        .ok_or_else(|| "config 缺 [rr3-brain] 段".to_string())?;
    let (section_key, key): (&str, &str) = if group == "dialog" {
        ("dialog", slot)
    } else if group == "asr" || group == "tts" || slot == "timeout_s" {
        // asr/tts 全槽与 llm.timeout_s 读源都在 cloud 段
        ("cloud", slot)
    } else {
        // llm 组 llm_model/base_url：实效源 = 模型行 active 在则行、否则 cloud fallback
        let rows_empty = brain
            .get("model")
            .and_then(Value::as_array)
            .map(|r| r.is_empty())
            .unwrap_or(true);
        if rows_empty {
            ("cloud", slot)
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
            ("model", slot)
        }
    };
    // model 路写键名换算（行内键：llm_model → model_id；base_url → base_url）
    let key = if section_key == "model" && slot == "llm_model" {
        "model_id"
    } else {
        key
    };
    // dialog 段缺段建段（Python setdefault 同义）；cloud/model 段 boot 校验在册
    if section_key == "dialog" && brain.get("dialog").is_none() {
        brain
            .as_object_mut()
            .expect("brain 段是对象（TOML 表同构）")
            .insert("dialog".to_string(), json!({}));
    }
    let old = if section_key == "model" {
        // 模型行写源：写进唯一 active 行（结构已验 actives==1），不是行表本身
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
        old
    } else {
        let section = brain
            .get_mut(section_key)
            .ok_or_else(|| format!("config 缺 [rr3-brain.{section_key}] 段"))?;
        let old = section.get(key).cloned().unwrap_or(Value::Null);
        section
            .as_object_mut()
            .ok_or_else(|| format!("config [rr3-brain.{section_key}] 须为表"))?
            .insert(key.to_string(), new.clone());
        old
    };
    Ok((old, new))
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
    let (old, new) = rr3_ops_write(&mut cfg.doc, &group, &slot, args.get("value").unwrap_or(&Value::Null))?;
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

/// 模型行实效重解析（ops/set llm 组写后供客户端重建消费——迁移件三接线；
/// 本件先落纯函数与 rr3_brain_model_row 同源单真值）。
#[allow(dead_code)] // 迁移件三（客户端重建取数口）消费；本件先落同源单真值
pub(crate) fn rr3_model_row_of(cfg: &Rr3BrainCfg) -> Result<crate::boot::Rr3ModelRow, String> {
    rr3_brain_model_row(&cfg.doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::rr3_brain_config_parse;

    /// 与仓内 config.toml 同形状的文档树夹具（boot 解析产出，写面真载体）
    fn repo_cfg() -> Rr3BrainCfg {
        let text = r#"
[rr3-brain.cloud]
base_url = "https://api.siliconflow.cn"
asr_model = "iic/SenseVoiceSmall"
llm_model = "deepseek-ai/DeepSeek-V3"
tts_model = "FunAudioLLM/CosyVoice2-0.5B"
timeout_s = 20

[[rr3-brain.model]]
name = "DeepSeek官方"
mode = "api"
api_format = "openai"
base_url = "https://api.deepseek.com"
model_id = "deepseek-chat"
api_key_ref = "DeepSeek官方"
active = true

[rr3-brain.bus]
call_timeout_s = 30.0

[rr3-brain.dialog]
max_utterance_ms = 8000
"#;
        rr3_brain_config_parse(text).unwrap()
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
                    "base_url": "https://api.deepseek.com",
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
        rr3_ops_write(&mut cfg.doc, "llm", "llm_model", &json!("deepseek-reasoner")).unwrap();
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
            e,
            "组不认: 'memory'（白名单 ['asr', 'dialog', 'llm', 'tts']）",
            "组不认文案逐字"
        );
        let e = rr3_ops_write(&mut cfg.doc, "asr", "sample_rate", &json!(1)).unwrap_err();
        assert_eq!(
            e,
            "组 asr 槽不认: 'sample_rate'（白名单 ['asr_model', 'base_url', 'timeout_s']）",
            "槽不认文案逐字"
        );
    }

    /// 字符串槽三径：非串点名（Python 类型词）、空白拒、strip 落值
    #[test]
    fn ops_set_string_slots_validate() {
        let mut cfg = repo_cfg();
        let e = rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!(42)).unwrap_err();
        assert_eq!(e, "槽 asr.asr_model 须字符串，收到 int", "类型词 Python 对位");
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
        assert_eq!(e, "槽 asr.timeout_s 须数值，收到 bool", "布尔先拒（Python 同序）");
        let e = rr3_ops_write(&mut cfg.doc, "llm", "timeout_s", &json!("30")).unwrap_err();
        assert_eq!(e, "槽 llm.timeout_s 须数值，收到 str");
        let e = rr3_ops_write(&mut cfg.doc, "tts", "timeout_s", &json!(0)).unwrap_err();
        assert_eq!(e, "槽 tts.timeout_s 须 >0，收到 0.0", ">0 执法（Python float 形状）");
        let e = rr3_ops_write(&mut cfg.doc, "tts", "timeout_s", &json!(-1.5)).unwrap_err();
        assert_eq!(e, "槽 tts.timeout_s 须 >0，收到 -1.5");
        let (old, new) = rr3_ops_write(&mut cfg.doc, "llm", "timeout_s", &json!(25)).unwrap();
        assert_eq!(old, json!(20), "old 是 doc 原值（TOML 整数 20）");
        assert_eq!(new, json!(25.0), "new 转 float（Python float(value) 同形状）");
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
        assert_eq!(e, "槽 dialog.max_utterance_ms 须整数，收到 8000.5", "小数拒");
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
        assert_eq!(old, Value::Null, "键缺席 old = null（Python dict.get 同形状）");
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
        let (old, new) =
            rr3_ops_write(&mut cfg.doc, "llm", "llm_model", &json!("deepseek-reasoner")).unwrap();
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
        let (_, new) =
            rr3_ops_write(&mut cfg.doc, "llm", "llm_model", &json!("new-llm")).unwrap();
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
            e,
            "[[rr3-brain.model]] 须恰一行 active=true（现 2 行）——写面不修结构错误",
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
        let e =
            rr3_ops_set_arm(&mut cfg, &json!({"group": "nope", "slot": "x", "value": 1}))
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
            e,
            "组不认: ''（白名单 ['asr', 'dialog', 'llm', 'tts']）",
            "缺字段回落空串同形状"
        );
        let e = rr3_ops_set_arm(&mut cfg, &json!({"group": 5, "slot": null})).unwrap_err();
        assert_eq!(
            e,
            "组不认: '5'（白名单 ['asr', 'dialog', 'llm', 'tts']）",
            "str(5)='5' coercion 同形状"
        );
    }

    /// 模型行实效重解析（迁移件三客户端重建的取数口）：live 写后读新值
    #[test]
    fn model_row_of_reflects_live_write() {
        let mut cfg = repo_cfg();
        rr3_ops_write(&mut cfg.doc, "llm", "llm_model", &json!("deepseek-reasoner")).unwrap();
        let row = rr3_model_row_of(&cfg).unwrap();
        assert_eq!(row.model_id, "deepseek-reasoner", "重解析读 doc 真值");
    }
}
