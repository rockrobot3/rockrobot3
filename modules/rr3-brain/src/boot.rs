//! rr3-brain 接入层（批N32 迁移件1 立树，ADR-0017 五层表接入层）。
//!
//! config schema 解析（自 Python composite.py 迁入，ADR-0016 裁定 2.4 键名逐键
//! 对照）：TOML → JSON 文档树（ops/set 写面 live 真值的唯一载体）＋ boot 投影
//! （模型行实效 / cloud timeout / bus call timeout / dialog max_utterance_ms）。
//! 缺段/缺键 fail-closed 点名拒 boot——Python 侧 KeyError 裸奔文案（如 `'cloud'`）
//! 是字典直取的意外产物、非装配观测面设计，Rust 侧按五模块同款中文点名文案承载，
//! 语义（缺即拒、退出交 supervisor）不变。
//!
//! 启动序语义差（批N32 增补裁定 4）：遵 rr3mod bootstrap 固定序（先连总线后读
//! config），Python 侧「config 先于总线连接」的更早 fail-closed 不保留；代价
//! `[rr3-brain.bus].connect_timeout_s` 键随迁移退役（Rust rr3_connect 无该参数
//! 入口，连接等待上界归 rr-bus-api 内部承载），本解析面不再读它。

use serde_json::Value;

/// LLM 模式（对位 cloud/llm.py `LLM_MODE_API`/`LLM_MODE_LOCAL`——常量真值点随
/// 迁移件三归一至 [`crate::cloud`]：解析面按字面 "api"/"local" 承载，鉴权消费面
/// 在 [`crate::cloud::Rr3LlmClient`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rr3LlmMode {
    Api,
    Local,
}

/// LLM 连接行实效（`[[rr3-brain.model]]` 恰一 active 行解析产物，或无表时
/// cloud 段 fallback）。`mode = None` = 随全局档（RR3_BRAIN_CLOUD）——cloud
/// fallback 路径的 Python 同形状。key_ref/mode 的消费面（LLM 客户端鉴权/免鉴权）
/// 在 llm 件 mech [`crate::llm::Rr3LlmHistory`]（客户端在 [`crate::cloud`]）。
#[derive(Debug, Clone)]
pub(crate) struct Rr3ModelRow {
    pub(crate) base_url: String,
    pub(crate) model_id: String,
    pub(crate) key_ref: Option<String>,
    pub(crate) mode: Option<Rr3LlmMode>,
}

/// cloud 段四键实效（asr/llm/tts 三客户端装配与 ops/set 写后重建共用的取数口——
/// Python composite 读 `cloud_cfg` 字典同形状：live 写后重建读 doc 新值，重启回
/// config.toml 真值）。
#[derive(Debug, Clone)]
pub(crate) struct Rr3CloudCfg {
    pub(crate) base_url: String,
    pub(crate) asr_model: String,
    pub(crate) tts_model: String,
    pub(crate) timeout_s: f64,
}

/// cloud 段四键读取（boot 校验保证在册，缺失时 Err 点名——fail-closed 不静默）。
pub(crate) fn rr3_cloud_cfg_of(doc: &Value) -> Result<Rr3CloudCfg, String> {
    let cloud = rr3_brain_sect(doc, "cloud")?;
    Ok(Rr3CloudCfg {
        base_url: rr3_req_str(cloud, "cloud", "base_url")?.to_string(),
        asr_model: rr3_req_str(cloud, "cloud", "asr_model")?.to_string(),
        tts_model: rr3_req_str(cloud, "cloud", "tts_model")?.to_string(),
        timeout_s: rr3_req_f64(cloud, "cloud", "timeout_s")?,
    })
}

/// 本模块 config 形状：JSON 文档树（ops/set 写面 live 真值）＋ boot 投影。
/// 投影是 boot 时的解析快照——ops/list llm 组回显源（model）刻意**不随 live 写
/// 回填**（Python composite base_url/model_id boot 局部变量同形状，B-14① 双源
/// 分裂不在本单修，重启回新值）；doc 是 ops/set 的唯一写面。
#[derive(Debug, Clone)]
pub(crate) struct Rr3BrainCfg {
    pub(crate) doc: Value,
    pub(crate) model: Rr3ModelRow,
    pub(crate) cloud_timeout_s: f64,
    pub(crate) call_timeout_s: f64,
    pub(crate) max_utterance_ms: i64,
}

/// 总线 RPC 超时缺省（B-13①：`[rr3-brain.bus].call_timeout_s` 缺键时的回落值——
/// 对位 composite.py `RR3_BUS_CALL_TIMEOUT_DEFAULT_S`，真值点 = config.toml 注释）
pub(crate) const RR3_BUS_CALL_TIMEOUT_DEFAULT_S: f64 = 30.0;

/// dialog 话轮封顶缺省（`[rr3-brain.dialog].max_utterance_ms` 缺键回落；
/// 对位 composite.py `dialog_cfg.get("max_utterance_ms", 8000)`）
pub(crate) const RR3_DIALOG_MAX_UTTERANCE_DEFAULT_MS: i64 = 8000;

// ---------- 文档树下钻助手（点名文案与 power/sense 同款中文形状） ----------

/// 取 `[rr3-brain.<sect>]` 段（缺段点名拒）。
fn rr3_brain_sect<'a>(doc: &'a Value, sect: &str) -> Result<&'a Value, String> {
    doc.get("rr3-brain")
        .and_then(|b| b.get(sect))
        .ok_or_else(|| format!("config 缺 [rr3-brain.{sect}] 段"))
}

/// 段内字符串键（缺键/非串点名拒）。
fn rr3_req_str<'a>(sect: &'a Value, sect_name: &str, key: &str) -> Result<&'a str, String> {
    sect.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("config [rr3-brain.{sect_name}] 缺 {key}（字符串）"))
}

/// 段内数值键（缺键/非数点名拒；Python float() 连数字字符串也收，Rust 只认
/// 数值——数字串是配置劣化，点名优于静默收编）。
fn rr3_req_f64(sect: &Value, sect_name: &str, key: &str) -> Result<f64, String> {
    sect.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("config [rr3-brain.{sect_name}] 缺 {key}（数值）"))
}

/// Python float 的打印形状（config 就绪行逐字对位）：整值补 .0（20 → "20.0"），
/// 小数走最短往返（17.25 → "17.25"）。
pub(crate) fn rr3_py_float_repr(x: f64) -> String {
    if x.fract() == 0.0 && x.is_finite() {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

/// 模型行 `base_url` 语义（ADR-0023）：**完整 OpenAI 兼容 API 根**（含版本段），
/// 客户端只追加 `/chat/completions`。各厂商版本段各写各的——DeepSeek
/// `https://api.deepseek.com/v1`、GLM `https://open.bigmodel.cn/api/paas/v4`、
/// Qwen `https://dashscope.aliyuncs.com/compatible-mode/v1`——**不按主机名猜**，
/// 否则就是换了个方式的厂商绑定。
///
/// cloud 段 fallback 路径的 `cloud.base_url` 是主机根（ASR/TTS 的
/// `{base}/v1/audio/*` 同源消费），此处补成完整根：路径为空补 `/v1`，已有路径原样。
fn rr3_model_api_root(host_or_root: &str) -> String {
    let s = host_or_root.trim().trim_end_matches('/');
    if s.is_empty() {
        return String::new();
    }
    let has_path = match s.split_once("://") {
        Some((_, rest)) => rest.contains('/'),
        None => s.contains('/'),
    };
    if has_path {
        s.to_string()
    } else {
        format!("{s}/v1")
    }
}

/// 模型行 `base_url` 取值裁决（boot 解析与面板写面共用一档）：非空、
/// `http(s)://` 开头、不得含 `/chat/completions`（客户端自拼，防双拼 404）。
pub(crate) fn rr3_model_base_url_validate(base_url: &str) -> Result<(), String> {
    if base_url.is_empty() {
        return Err("base_url 不得为空".to_string());
    }
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(format!(
            "base_url 须为完整 API 根（http:// 或 https:// 开头）: {base_url}"
        ));
    }
    if base_url.trim_end_matches('/').ends_with("/chat/completions") {
        return Err(format!(
            "base_url 是完整 API 根，不得含 /chat/completions（客户端自拼）: {base_url}"
        ));
    }
    Ok(())
}

/// LLM 连接行解析（自 composite.py `rr3_brain_model_row` 逐语义迁入，0.0.6 抽象批）：
/// `[[rr3-brain.model]]` 恰一 active 行；无表 → cloud 段 fallback 四元组。
/// fail-closed：active 数≠1、mode/api_format 不认、缺 base_url/model_id 一律
/// Err 点名行 name，不静默挑行。错误文案逐字对位 Python。
pub(crate) fn rr3_brain_model_row(doc: &Value) -> Result<Rr3ModelRow, String> {
    let brain = doc
        .get("rr3-brain")
        .ok_or_else(|| "config 缺 [rr3-brain] 段".to_string())?;
    let rows: &[Value] = match brain.get("model") {
        None | Some(Value::Null) => &[],
        Some(Value::Array(rows)) => rows.as_slice(),
        Some(_) => return Err("config [[rr3-brain.model]] 须为行表（数组）".to_string()),
    };
    if rows.is_empty() {
        // 无表 → 旧路径四元组（cloud 段 base_url + llm_model，key_ref=None 走
        // 通用环境变量键，mode=None 随全局档）。cloud.base_url 是主机根（ASR/TTS
        // 亦消费），LLM 腿在此补成完整 API 根：路径为空补 `/v1`。
        let cloud = rr3_brain_sect(doc, "cloud")?;
        return Ok(Rr3ModelRow {
            base_url: rr3_model_api_root(rr3_req_str(cloud, "cloud", "base_url")?),
            model_id: rr3_req_str(cloud, "cloud", "llm_model")?.to_string(),
            key_ref: None,
            mode: None,
        });
    }
    let names: Vec<String> = rows
        .iter()
        .map(|r| {
            r.get("name")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string()
        })
        .collect();
    let active: Vec<&Value> = rows
        .iter()
        .filter(|r| r.get("active").and_then(Value::as_bool) == Some(true))
        .collect();
    if active.len() != 1 {
        let names_fmt = rr3_py_list_repr(&names);
        return Err(format!(
            "[[rr3-brain.model]] 须恰一行 active=true（现 {} 行: {names_fmt}）",
            active.len()
        ));
    }
    let row = active[0];
    let name = row.get("name").and_then(Value::as_str).unwrap_or("?");
    let mode = match row.get("mode").and_then(Value::as_str).unwrap_or("api") {
        "api" => Rr3LlmMode::Api,
        "local" => Rr3LlmMode::Local,
        other => {
            return Err(format!(
                "模型行 {name} mode 不认: {other}（允许 api/local）"
            ))
        }
    };
    let fmt = row.get("api_format").and_then(Value::as_str).unwrap_or("openai");
    if fmt != "openai" {
        return Err(format!(
            "模型行 {name} api_format 暂不支持: {fmt}（本批仅 openai）"
        ));
    }
    let base_url = row
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let model_id = row
        .get("model_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if base_url.is_empty() || model_id.is_empty() {
        return Err(format!("模型行 {name} 缺 base_url/model_id"));
    }
    rr3_model_base_url_validate(&base_url).map_err(|e| format!("模型行 {name} {e}"))?;
    // key_ref 双源引用：`env:变量名` → 环境变量，裸名 → 面板 secrets 文件（语义
    // 承载在 cloud/llm.py，迁移件三落）；空白串折 None（Python strip() or None）
    let key_ref = row
        .get("api_key_ref")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(Rr3ModelRow {
        base_url,
        model_id,
        key_ref,
        mode: Some(mode),
    })
}

/// Python 列表 repr 形状（错误文案逐字对位）：`['a', 'b']`——单引号、逗号空格。
fn rr3_py_list_repr(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", quoted.join(", "))
}

/// TOML 文本 → Rr3BrainCfg。文档树整体进 [`Rr3BrainCfg::doc`]（ops/set 写面），
/// boot 投影当场解析——缺段/缺键/类型不合法一律 Err 拒 boot（fail-closed）。
pub(crate) fn rr3_brain_config_parse(text: &str) -> Result<Rr3BrainCfg, String> {
    let doc: Value =
        toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    // cloud 段必填四键 + timeout_s（Python cfg["rr3-brain"]["cloud"] 直取同义；
    // llm_model 只在无模型行表的 fallback 路径消费，由 rr3_brain_model_row 点名）
    let cloud = rr3_brain_sect(&doc, "cloud")?;
    rr3_req_str(cloud, "cloud", "base_url")?;
    rr3_req_str(cloud, "cloud", "asr_model")?;
    rr3_req_str(cloud, "cloud", "tts_model")?;
    let cloud_timeout_s = rr3_req_f64(cloud, "cloud", "timeout_s")?;
    // 模型行实效（fail-closed 同 boot：active 数≠1 等在连接注册之后仍拒不放行）
    let model = rr3_brain_model_row(&doc)?;
    // bus 段可选（Python cfg["rr3-brain"].get("bus", {}) 同形状）；
    // connect_timeout_s 已随迁移退役（批N32 增补裁定 4），本面不读
    let call_timeout_s = match doc.get("rr3-brain").and_then(|b| b.get("bus")) {
        Some(bus) => match bus.get("call_timeout_s") {
            Some(v) => v.as_f64().ok_or_else(|| {
                "config [rr3-brain.bus] call_timeout_s 须数值".to_string()
            })?,
            None => RR3_BUS_CALL_TIMEOUT_DEFAULT_S,
        },
        None => RR3_BUS_CALL_TIMEOUT_DEFAULT_S,
    };
    // dialog 段必填（Python dialog_cfg = cfg["rr3-brain"]["dialog"] 直取同义），
    // 键可选缺省 8000
    let dialog = rr3_brain_sect(&doc, "dialog")?;
    let max_utterance_ms = match dialog.get("max_utterance_ms") {
        Some(v) => v
            .as_i64()
            .ok_or_else(|| "config [rr3-brain.dialog] max_utterance_ms 须整数".to_string())?,
        None => RR3_DIALOG_MAX_UTTERANCE_DEFAULT_MS,
    };
    Ok(Rr3BrainCfg {
        doc,
        model,
        cloud_timeout_s,
        call_timeout_s,
        max_utterance_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-brain]
log_level = "info"

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
base_url = "https://api.deepseek.com/v1"
model_id = "deepseek-chat"
api_key_ref = "DeepSeek官方"
active = true

[rr3-brain.bus]
call_timeout_s = 30.0

[rr3-brain.dialog]
max_utterance_ms = 8000
"#;
        let cfg = rr3_brain_config_parse(text).unwrap();
        assert_eq!(cfg.cloud_timeout_s, 20.0);
        assert_eq!(cfg.call_timeout_s, 30.0);
        assert_eq!(cfg.max_utterance_ms, 8000);
        // 模型行实效：active 行覆盖 cloud fallback（base_url 为完整 API 根）
        assert_eq!(cfg.model.base_url, "https://api.deepseek.com/v1");
        assert_eq!(cfg.model.model_id, "deepseek-chat");
        assert_eq!(cfg.model.key_ref.as_deref(), Some("DeepSeek官方"));
        assert_eq!(cfg.model.mode, Some(Rr3LlmMode::Api));
        // 文档树整树在册（ops/set 写面载体）
        assert_eq!(
            cfg.doc["rr3-brain"]["cloud"]["asr_model"],
            json!("iic/SenseVoiceSmall")
        );
    }

    /// 无 [[rr3-brain.model]] 行表 → cloud 段 fallback 四元组（mode/key_ref 双 None）
    #[test]
    fn config_parse_falls_back_to_cloud_without_model_table() {
        let text = r#"
[rr3-brain.cloud]
base_url = "https://api.siliconflow.cn"
asr_model = "iic/SenseVoiceSmall"
llm_model = "deepseek-ai/DeepSeek-V3"
tts_model = "FunAudioLLM/CosyVoice2-0.5B"
timeout_s = 20

[rr3-brain.dialog]
max_utterance_ms = 8000
"#;
        let cfg = rr3_brain_config_parse(text).unwrap();
        assert_eq!(
            cfg.model.base_url, "https://api.siliconflow.cn/v1",
            "cloud 主机根 fallback 补版本段成完整 API 根"
        );
        assert_eq!(cfg.model.model_id, "deepseek-ai/DeepSeek-V3");
        assert_eq!(cfg.model.key_ref, None);
        assert_eq!(cfg.model.mode, None, "fallback 路径随全局档（mode=None）");
        // bus 段缺省回落 30.0（B-13① 真值点 = config.toml 注释）
        assert_eq!(cfg.call_timeout_s, 30.0);
    }

    /// bus 段缺键回落、dialog 键缺省 8000（Python .get(…, default) 同形状）
    #[test]
    fn config_parse_defaults_for_optional_keys() {
        let text = r#"
[rr3-brain.cloud]
base_url = "https://api.siliconflow.cn"
asr_model = "a"
llm_model = "b"
tts_model = "c"
timeout_s = 20

[rr3-brain.bus]

[rr3-brain.dialog]
"#;
        let cfg = rr3_brain_config_parse(text).unwrap();
        assert_eq!(cfg.call_timeout_s, 30.0);
        assert_eq!(cfg.max_utterance_ms, 8000);
    }

    /// 缺段/缺键点名（fail-closed；cloud 四键＋dialog 段必填，bus 段可选）
    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_brain_config_parse("[rr3-brain]\nlog_level = \"info\"\n").unwrap_err();
        assert!(e.contains("[rr3-brain.cloud]"), "cloud 缺段点名: {e}");
        let e = rr3_brain_config_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\ntts_model = \"t\"\n",
        )
        .unwrap_err();
        assert!(e.contains("timeout_s"), "cloud 缺 timeout_s 点名: {e}");
        let e = rr3_brain_config_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n",
        )
        .unwrap_err();
        assert!(e.contains("[rr3-brain.dialog]"), "dialog 缺段点名: {e}");
        let e = rr3_brain_config_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n",
        )
        .unwrap_err();
        assert!(e.contains("asr_model"), "cloud 缺 asr_model 点名: {e}");
        let e = rr3_brain_config_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n[rr3-brain.bus]\ncall_timeout_s = \"30\"\n",
        )
        .unwrap_err();
        assert!(e.contains("call_timeout_s 须数值"), "字符串超时点名（Rust 只认数值）: {e}");
        let e = rr3_brain_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }

    /// 行表 fail-closed 四红：active 数≠1（两行/零行）、mode 不认、api_format 不认、
    /// 缺 base_url/model_id——文案逐字对位 Python
    #[test]
    fn model_row_rejects_structural_violations() {
        let cloud = "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n";
        let two_active = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \"u1\"\nmodel_id = \"m1\"\nactive = true\n[[rr3-brain.model]]\nname = \"乙\"\nbase_url = \"u2\"\nmodel_id = \"m2\"\nactive = true\n"
        );
        let e = rr3_brain_config_parse(&two_active).unwrap_err();
        assert_eq!(
            e,
            "[[rr3-brain.model]] 须恰一行 active=true（现 2 行: ['甲', '乙']）",
            "两 active 红且名单逐字（Python 列表 repr 形状）"
        );
        let none_active = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \"u1\"\nmodel_id = \"m1\"\nactive = false\n"
        );
        let e = rr3_brain_config_parse(&none_active).unwrap_err();
        assert!(e.contains("现 0 行"), "零 active 同红: {e}");
        let bad_mode = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nmode = \"ssh\"\nbase_url = \"u1\"\nmodel_id = \"m1\"\nactive = true\n"
        );
        let e = rr3_brain_config_parse(&bad_mode).unwrap_err();
        assert_eq!(
            e,
            "模型行 甲 mode 不认: ssh（允许 api/local）",
            "mode 不认文案逐字"
        );
        let bad_fmt = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nmode = \"api\"\napi_format = \"anthropic\"\nbase_url = \"u1\"\nmodel_id = \"m1\"\nactive = true\n"
        );
        let e = rr3_brain_config_parse(&bad_fmt).unwrap_err();
        assert_eq!(
            e,
            "模型行 甲 api_format 暂不支持: anthropic（本批仅 openai）",
            "api_format 不认文案逐字"
        );
        let missing = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nmode = \"api\"\nmodel_id = \"m1\"\nactive = true\n"
        );
        let e = rr3_brain_config_parse(&missing).unwrap_err();
        assert_eq!(e, "模型行 甲 缺 base_url/model_id", "缺连接键文案逐字");
    }

    /// fallback 路径缺 cloud.llm_model 点名（只在无行表时必填）
    #[test]
    fn model_row_fallback_requires_cloud_llm_model() {
        let text = r#"
[rr3-brain.cloud]
base_url = "u"
asr_model = "a"
tts_model = "t"
timeout_s = 20

[rr3-brain.dialog]
"#;
        let e = rr3_brain_config_parse(text).unwrap_err();
        assert!(e.contains("llm_model"), "fallback 缺 llm_model 点名: {e}");
    }

    /// key_ref 空白串折 None（Python strip() or None 同形状）；缺 api_key_ref 同 None
    #[test]
    fn model_row_key_ref_strip_and_none() {
        let cloud = "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n";
        let blank = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \"http://u1\"\nmodel_id = \"m1\"\napi_key_ref = \"  \"\nactive = true\n"
        );
        let cfg = rr3_brain_config_parse(&blank).unwrap();
        assert_eq!(cfg.model.key_ref, None, "空白 key_ref 折 None");
        let spaced = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \" http://u1 \"\nmodel_id = \" m1 \"\napi_key_ref = \" 甲 \"\nactive = true\n"
        );
        let cfg = rr3_brain_config_parse(&spaced).unwrap();
        assert_eq!(cfg.model.base_url, "http://u1", "base_url strip");
        assert_eq!(cfg.model.model_id, "m1", "model_id strip");
        assert_eq!(cfg.model.key_ref.as_deref(), Some("甲"), "key_ref strip");
    }

    /// API 根语义（ADR-0023）：模型行 base_url 是完整根，各厂商版本段各写各的；
    /// cloud 主机根 fallback 只在路径为空时补 `/v1`——不按主机名猜厂商版本段
    #[test]
    fn api_root_appends_v1_only_when_path_absent() {
        for (input, want) in [
            ("https://api.deepseek.com", "https://api.deepseek.com/v1"),
            ("https://api.deepseek.com/", "https://api.deepseek.com/v1"),
            (" https://api.siliconflow.cn ", "https://api.siliconflow.cn/v1"),
            (
                "https://api.deepseek.com/v1",
                "https://api.deepseek.com/v1",
            ),
            (
                "https://open.bigmodel.cn/api/paas/v4",
                "https://open.bigmodel.cn/api/paas/v4",
            ),
            (
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
            ),
        ] {
            assert_eq!(rr3_model_api_root(input), want, "input={input}");
        }
    }

    /// 模型行 base_url 裁决：非空 / http(s) 前缀 / 不得自带 /chat/completions
    #[test]
    fn model_base_url_validation_rejects_malformed_roots() {
        assert!(rr3_model_base_url_validate("").is_err());
        assert!(rr3_model_base_url_validate("u1").is_err());
        assert!(rr3_model_base_url_validate("ftp://x/v1").is_err());
        assert!(rr3_model_base_url_validate("https://x/v1/chat/completions").is_err());
        assert!(rr3_model_base_url_validate("https://x/chat/completions/").is_err());
        assert!(rr3_model_base_url_validate("https://api.deepseek.com/v1").is_ok());
        assert!(rr3_model_base_url_validate("http://127.0.0.1:8080/v1").is_ok());
        // 行解析侧点名带行名（boot 拒 boot 不放行）
        let cloud = "[rr3-brain.cloud]\nbase_url = \"https://api.siliconflow.cn\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n";
        let bad = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \"api.deepseek.com/v1\"\nmodel_id = \"m1\"\nactive = true\n"
        );
        let e = rr3_brain_config_parse(&bad).unwrap_err();
        assert!(e.starts_with("模型行 甲 base_url 须为完整 API 根"), "{e}");
    }

    /// Python float 打印形状（config 就绪行逐字对位的数据基础）
    #[test]
    fn py_float_repr_matches_python_str() {
        assert_eq!(rr3_py_float_repr(20.0), "20.0");
        assert_eq!(rr3_py_float_repr(30.0), "30.0");
        assert_eq!(rr3_py_float_repr(17.25), "17.25");
        assert_eq!(rr3_py_float_repr(0.5), "0.5");
    }
}
