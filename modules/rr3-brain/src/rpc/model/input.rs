//! 模型行入参面：必填/域名/键引用/active 的逐项 fail-closed 校验。
//!
//! `Rr3ModelRowInput` 是 upsert 的唯一入参形状；`key_ref` 三态由
//! `key_ref_clear` 承载（缺席保留 / 空串清除 / 有值校验后写）。

use serde_json::Value;

use crate::boot::rr3_base_url_check;

/// upsert 入参（一次解析；`key_ref` 三态：缺席 = 保留现值、空串 = 清除、
/// 有值 = 校验后写——`key_ref_clear` 承载"空串清除"这一态）。
pub(super) struct Rr3ModelRowInput<'a> {
    pub(super) base_url: &'a str,
    pub(super) model_id: &'a str,
    pub(super) api_format: &'a str,
    pub(super) mode: &'a str,
    pub(super) key_ref: Option<String>,
    pub(super) key_ref_clear: bool,
    pub(super) active: Option<bool>,
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
    if s.strip_prefix(crate::cloud::RR3_KEY_REF_ENV_PREFIX)
        .is_some_and(|var| var.is_empty() || var.chars().any(char::is_whitespace))
    {
        return Err(format!(
            "模型行 {name} api_key_ref 的 env: 后须变量名（非空、无空白）: '{s}'"
        ));
    }
    Ok((Some(s.to_string()), false))
}

/// `active` 三态核：缺席 = 保留现值（None）；在场须布尔。
fn rr3_active_input(name: &str, args: &Value) -> Result<Option<bool>, String> {
    let Some(v) = args.get("active") else {
        return Ok(None);
    };
    v.as_bool()
        .map(Some)
        .ok_or_else(|| format!("模型行 {name} active 须布尔"))
}

/// 模型行进参解析：必填/域名/键引用/active 逐项按旧文案 fail-closed。
pub(super) fn rr3_model_row_input<'a>(
    name: &str,
    args: &'a Value,
) -> Result<Rr3ModelRowInput<'a>, String> {
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
    let active = rr3_active_input(name, args)?;
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
