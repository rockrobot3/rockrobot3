//! 写口参数面：target/键/值的形状核与白名单（parse 层）。

use super::*;

/// 写口目标（params.target 词表，parse 层裁）：domain = 装配单
/// `[[supplies.domain]]` 域块键；axis = 轴档案单键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3WriteTarget {
    Domain,
    Axis,
}

/// 写口值型（parse 层形状核后）：布尔（armed）或正整数（severe_mv /
/// current_ceil_ma）。0 在 parse 层拒——正整数是输入形状核（0 无域界语义），
/// 阈值次序（mild > severe > 0 等）不在写面重复判：判据落装载链（批N2 门 c
/// 消费者侧）则写口经预核自动继承；未落则如实放行（写面不造判据还不在的红）——
/// 写面只核形状与权限，业务合法性永远住装载链。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3WriteValue {
    Bool(bool),
    Num(u64),
}

/// 写口参数（parse 层产出，形状全合法——门与白名单在这里已裁）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3DomainWriteParams {
    pub target: Rr3WriteTarget,
    /// 域名（domain）或轴 instance 名（axis）
    pub name: String,
    /// 键名（白名单：domain → armed/severe_mv；axis → current_ceil_ma——
    /// conventions §6.1 三写口点名集，白名单外即拒）
    pub key: String,
    pub value: Rr3WriteValue,
    /// 证据引用（armed=true / severe_mv / current_ceil_ma 必填，armed=false 不要求）
    pub evidence: Option<String>,
    /// 二次确认位（execute ①的门参数；parse 层不裁门——形状归 parse、门归门序）。
    /// 由 [`Rr3DomainWriteParams::confirm_from`] 从原始 params 提取。
    pub confirm: Option<bool>,
}

/// params 拆包＋形状核（跨平台纯函数）。confirm 门不在本层（它是权限语义，
/// 归 execute 的门序；parse 只裁"形状合不合法"）。错误码全 Invalid（参数形状病）。
pub fn rr3_write_parse(params: &Value) -> Result<Rr3DomainWriteParams, (Rr3StatusCode, String)> {
    let (target, name, key) = rr3_write_head(params)?;
    let value = rr3_write_value(params, target, &key)?;
    let evidence = params
        .get("evidence")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(Rr3DomainWriteParams {
        target,
        name,
        key: key.to_string(),
        value,
        evidence,
        confirm: None,
    })
}

/// params 三段拆包（target / name / key）：逐段形状核与文案归各自的取值函数。
fn rr3_write_head(
    params: &Value,
) -> Result<(Rr3WriteTarget, String, String), (Rr3StatusCode, String)> {
    let target = rr3_write_target(params)?;
    let name = rr3_write_text(params, "name")?.to_string();
    let key = rr3_write_text(params, "key")?;
    Ok((target, name, key.to_string()))
}

/// `params.target` 词表（domain/axis）：出域即拒，文案点名两种合法取值。
fn rr3_write_target(params: &Value) -> Result<Rr3WriteTarget, (Rr3StatusCode, String)> {
    match params.get("target").and_then(Value::as_str) {
        Some("domain") => Ok(Rr3WriteTarget::Domain),
        Some("axis") => Ok(Rr3WriteTarget::Axis),
        _ => Err((
            Rr3StatusCode::Invalid,
            "缺 params.target（要求 \"domain\" 或 \"axis\"）".to_string(),
        )),
    }
}

/// 非空字符串字段（name/key 同形）：缺、非串、空串一律拒，文案带字段名。
fn rr3_write_text<'a>(params: &'a Value, field: &str) -> Result<&'a str, (Rr3StatusCode, String)> {
    let Some(s) = params
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return Err((
            Rr3StatusCode::Invalid,
            format!("缺 params.{field}（要求非空字符串）"),
        ));
    };
    Ok(s)
}

/// 值型白名单（conventions §6.1 三写口点名集）：域 armed 取布尔，域 severe_mv
/// 与轴 current_ceil_ma 取正整数（0 拒——正整数是输入形状核，无域界语义）。
/// 白名单外 = 申报面错，fail-closed 并点名合法集。
fn rr3_write_value(
    params: &Value,
    target: Rr3WriteTarget,
    key: &str,
) -> Result<Rr3WriteValue, (Rr3StatusCode, String)> {
    match (target, key) {
        (Rr3WriteTarget::Domain, "armed") => rr3_write_bool(params),
        (Rr3WriteTarget::Domain, "severe_mv") | (Rr3WriteTarget::Axis, "current_ceil_ma") => {
            rr3_write_num(params, key)
        }
        _ => Err((
            Rr3StatusCode::Invalid,
            format!(
                "key {key:?} 不在写口白名单（domain → armed/severe_mv；axis → current_ceil_ma；conventions §6.1 三写口点名集）"
            ),
        )),
    }
}

/// armed 值型：缺/非布尔一律拒。
fn rr3_write_bool(params: &Value) -> Result<Rr3WriteValue, (Rr3StatusCode, String)> {
    let Some(v) = params.get("value").and_then(Value::as_bool) else {
        return Err((
            Rr3StatusCode::Invalid,
            "armed 的 params.value 要求布尔（true/false）".to_string(),
        ));
    };
    Ok(Rr3WriteValue::Bool(v))
}

/// 正整数值型：0 与缺席/非整数分别点名。
fn rr3_write_num(params: &Value, key: &str) -> Result<Rr3WriteValue, (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let Some(n) = params.get("value").and_then(Value::as_u64) else {
        return Err(invalid(format!("{key} 的 params.value 要求正整数")));
    };
    if n == 0 {
        return Err(invalid(format!(
            "{key} 的 params.value 要求正整数（收到 0）"
        )));
    }
    Ok(Rr3WriteValue::Num(n))
}
