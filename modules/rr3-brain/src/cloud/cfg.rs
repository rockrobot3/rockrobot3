//! 云档、密钥双源与 secrets 现读（含 key_ref 语法与就位判据）。

use serde_json::Value;

use crate::boot::Rr3LlmMode;

// ---------- 常量（自 cloud/__init__.py + asr.py + llm.py + tts.py 迁入，逐字） ----------

/// 通用密钥环境变量（`env:` 前缀外的旧路径缺省键）
pub(crate) const RR3_API_KEY_ENV: &str = "RR3_CLOUD_API_KEY";
/// 云档判据环境变量（real / mock）
pub(crate) const RR3_CLOUD_MODE_ENV: &str = "RR3_BRAIN_CLOUD";
/// secrets 文件路径覆盖环境变量
pub(crate) const RR3_SECRETS_FILE_ENV: &str = "RR3_SECRETS_FILE";
/// mock ASR 的文本注入键（批F2）：判向-转向闭环 e2e 用它注入"向左/向右"类识别
/// 文本驱动意图面。取值不设/置空回落固定文本——mock 是测试注入点，真云路径永不
/// 读它（与 [`RR3_CLOUD_MODE_ENV`] 的注入点分工同一纪律）。
pub(crate) const RR3_MOCK_TEXT_ENV: &str = "RR3_BRAIN_MOCK_ASR_TEXT";
/// mock ASR 的固定识别文本
pub(crate) const RR3_MOCK_TEXT: &str = "你好，今天天气怎么样";
/// ASR 语言策略（2026-10-31 实机）：短中文唤醒词偶发被云端误判语种（曾返回
/// 韩语 "안녕"），显式 `zh` 稳定中文识别；服务端若忽略该字段则无副作用。
pub(crate) const RR3_ASR_LANGUAGE: &str = "zh";
/// ASR 输入增益（2026-10-31 实机）：板麦语音 RMS 仅 ~1000/32767，云端 VAD 对
/// 短句偶发判空/幻觉；×4 实测能稳定出文本（×1/×8 为空、×2/×4 出文本）。只作用
/// 于送 ASR 的 PCM 副本，VAD/DoA/总线 PCM 均不变。
pub(crate) const RR3_ASR_INPUT_GAIN: i32 = 4;
/// TTS 宿主兜底后端开关（`RR3_TTS_BACKEND=mac_say`）：硅基流动 /v1/audio/speech
/// 2026-10-24 起对合法请求零字节挂死（ASR/LLM 正常），宿主开发档改走 macOS `say`
/// 合成 WAV 再抽 PCM——只换 TTS 一跳，总线契约与 ASR/LLM 零改动。
pub(crate) const RR3_TTS_BACKEND_ENV: &str = "RR3_TTS_BACKEND";
pub(crate) const RR3_TTS_BACKEND_MAC_SAY: &str = "mac_say";
/// 宿主合成音色（macOS 内置 zh_CN 女声）
pub(super) const RR3_SAY_VOICE: &str = "Ting-Ting";

/// legacy/core/llm/src/llm_config.h 的原值移植（M4 不改人设与参数）
pub(crate) const RR3_SYSTEM_PROMPT: &str =
    "你是一个中文语音助手，回答要口语化、简短，一般不超过50个字，不要用列表、符号和表情。";
/// legacy llm_config.h 原值
pub(crate) const RR3_LLM_MAX_TOKENS: i64 = 150;
/// key_ref 双源语法前缀：`env:变量名` → 环境变量
pub(crate) const RR3_KEY_REF_ENV_PREFIX: &str = "env:";

// ---------- 模式与密钥（自 cloud/__init__.py 迁入） ----------

/// 云档（`RR3_BRAIN_CLOUD` 判据，每次调用现读——Python `rr3_brain_cloud_mode` 同形状）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rr3CloudMode {
    Real,
    Mock,
}

/// 云档解析：键缺席 → real（Python get(env, "real") 同形状）；键在但不认（含空串）
/// → Err 点名（fail-closed，不静默夹值）。
pub(crate) fn rr3_cloud_mode() -> Result<Rr3CloudMode, String> {
    match std::env::var(RR3_CLOUD_MODE_ENV) {
        Err(_) => Ok(Rr3CloudMode::Real),
        Ok(v) => rr3_cloud_mode_word(&v),
    }
}

/// 云档词表（real/mock；词表外点名拒）。
fn rr3_cloud_mode_word(v: &str) -> Result<Rr3CloudMode, String> {
    match v.trim().to_lowercase().as_str() {
        "real" => Ok(Rr3CloudMode::Real),
        "mock" => Ok(Rr3CloudMode::Mock),
        other => Err(format!(
            "{RR3_CLOUD_MODE_ENV} 取值不认: {other}（允许 real/mock）"
        )),
    }
}

/// 取密钥；缺失即报错。错误文案只点名环境变量，不回显任何凭据。
pub(crate) fn rr3_cloud_api_key() -> Result<String, String> {
    let key = std::env::var(RR3_API_KEY_ENV).unwrap_or_default();
    let key = key.trim();
    if key.is_empty() {
        return Err(format!("缺少环境变量 {RR3_API_KEY_ENV}，云端链路不可用"));
    }
    Ok(key.to_string())
}

/// secrets 缺省路径（Python `expanduser(~/.rr3/secrets.json)` 同形状：HOME 缺失
/// 保留 `~` 字面——后续打开必然失败折 None，语义一致）。
fn rr3_secrets_def_path() -> String {
    match std::env::var("HOME") {
        Ok(home) => format!("{home}/.rr3/secrets.json"),
        Err(_) => "~/.rr3/secrets.json".to_string(),
    }
}

/// secrets 文件现读 ref（面板维护面）：每次调用现读——面板填完下一轮即生效，无需
/// 重启。缺文件/缺键/坏 json/值非串一律 None，容忍与否由调用方裁定（real 档缺
/// key 报错只点名 ref，不回显任何值）。
pub(crate) fn rr3_secrets_get(reference: &str) -> Option<String> {
    let path = std::env::var(RR3_SECRETS_FILE_ENV).unwrap_or_else(|_| rr3_secrets_def_path());
    let text = std::fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    let val = data.get("keys")?.get(reference)?.as_str()?;
    let val = val.trim();
    Some(val).filter(|s| !s.is_empty()).map(|s| s.to_string())
}

/// 裸名 ref 取值：`env:VAR` → 环境变量；裸名 → secrets 文件（每次调用现读）。
pub(super) fn rr3_ref_key(r: &str) -> Result<String, String> {
    if let Some(var) = r.strip_prefix(RR3_KEY_REF_ENV_PREFIX) {
        return rr3_env_ref_key(var, r);
    }
    rr3_secrets_get(r).ok_or_else(|| format!("secrets 缺引用 {r}（面板填 key 后下一轮生效）"))
}

/// `env:VAR` 分支取值（空值点名，不回显凭据）。
pub(super) fn rr3_env_ref_key(var: &str, r: &str) -> Result<String, String> {
    let val = std::env::var(var).unwrap_or_default();
    let val = val.trim();
    if val.is_empty() {
        return Err(format!("缺少环境变量 {var}（模型行 key_ref={r}）"));
    }
    Ok(val.to_string())
}

/// 模型行密钥是否就位（面板模型列表只回布尔 `key_set`，值零回显）：local 免鉴权
/// 恒 true；`env:` 引用看环境变量非空；裸名看 secrets 现读；无引用走通用环境变量键。
pub(crate) fn rr3_key_set_of(key_ref: &Option<String>, mode: Option<Rr3LlmMode>) -> bool {
    if mode == Some(Rr3LlmMode::Local) {
        return true;
    }
    key_ref
        .as_deref()
        .map_or_else(|| rr3_cloud_api_key().is_ok(), rr3_key_ref_set)
}

/// 引用式 key_ref 是否就位：`env:` 看环境变量非空；裸名看 secrets 现读。
fn rr3_key_ref_set(r: &str) -> bool {
    match r.strip_prefix(RR3_KEY_REF_ENV_PREFIX) {
        Some(var) => std::env::var(var)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false),
        None => rr3_secrets_get(r).is_some(),
    }
}
