// SPDX-License-Identifier: Apache-2.0
//! 模型行投影面：行表读取、字段读取、active 点名与单行投影。
//!
//! 只做"行数据 → 可回显视图"，不碰落盘与 CRUD；入口由父模块调用。

use serde_json::{json, Value};

use crate::boot::Rr3LlmMode;
use crate::cloud::rr3_key_set_of;

/// 模型行表快照（doc 内，缺表 = 空表；非数组点名拒——与 boot 同一文案）
pub(super) fn rr3_model_rows(doc: &Value) -> Result<Vec<Value>, String> {
    match doc.get("rr3-brain").and_then(|b| b.get("model")) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(rows)) => Ok(rows.clone()),
        Some(_) => Err("config [[rr3-brain.model]] 须为行表（数组）".to_string()),
    }
}

/// 行内字符串字段（缺键/Null 回落空串；非串点名行号与键名）
pub(super) fn rr3_row_str<'a>(row: &'a Value, key: &str, idx: usize) -> Result<&'a str, String> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(""),
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(_) => Err(format!("模型行 #{idx}.{key} 须字符串")),
    }
}

/// 表内 active 行名（无 → None：空表走 cloud fallback，没有行可点名）
pub(super) fn rr3_row_active_name(rows: &[Value]) -> Option<String> {
    rows.iter()
        .find(|r| r.get("active").and_then(Value::as_bool) == Some(true))
        .and_then(|r| r.get("name").and_then(Value::as_str))
        .map(|s| s.to_string())
}

/// 单行投影（B9 自 list 抽出）：名称/API 根/模型/格式/模式/key_ref/key 就位/
/// active——只回 `key_set` 布尔，永无密钥明文。
pub(super) fn rr3_model_row(row: &Value, i: usize) -> Result<Value, String> {
    let v = rr3_row_view(row, i)?;
    let mode_enum = Some(if v.mode == "local" {
        Rr3LlmMode::Local
    } else {
        Rr3LlmMode::Api
    });
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
    let mode = rr3_row_str(row, "mode", i)?.trim();
    let mode = if mode.is_empty() { "api" } else { mode };
    let api_format = rr3_row_str(row, "api_format", i)?.trim();
    let api_format = if api_format.is_empty() {
        "openai"
    } else {
        api_format
    };
    let key_ref = Some(rr3_row_str(row, "api_key_ref", i)?.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    Ok(Rr3RowView {
        name,
        mode,
        api_format,
        key_ref,
    })
}
