//! boot 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;
use serde_json::json;

/// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
#[test]
fn config_parse_covers_repo_shape() {
    let cfg = rr3_brain_cfg_parse(&rr3_repo_cfg_fixture()).unwrap();
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
    let cfg = rr3_brain_cfg_parse(&rr3_fallback_cfg()).unwrap();
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
    let cfg = rr3_brain_cfg_parse(text).unwrap();
    assert_eq!(cfg.call_timeout_s, 30.0);
    assert_eq!(cfg.max_utterance_ms, 8000);
}

/// 超时值域（本轮审计）：NaN/inf/非正/超上界一律拒——这些值进
/// `Duration::from_secs_f64` 会在首个云端调用/RPC 上 panic，必须挡在启动前。
#[test]
fn config_rejects_out_of_domain_timeouts() {
    let text = |timeout: &str, bus: &str| {
        format!(
                "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = {timeout}\n[rr3-brain.dialog]\n[rr3-brain.bus]\ncall_timeout_s = {bus}\n"
            )
    };
    for t in ["0", "-1", "inf", "nan", "3601"] {
        let e = rr3_brain_cfg_parse(&text(t, "30")).unwrap_err();
        assert!(e.contains("timeout_s"), "{t} 必须被拒: {e}");
    }
    for t in ["0", "-1", "inf", "nan", "3601"] {
        let e = rr3_brain_cfg_parse(&text("20", t)).unwrap_err();
        assert!(e.contains("call_timeout_s"), "{t} 必须被拒: {e}");
    }
    assert!(
        rr3_brain_cfg_parse(&text("20", "30")).is_ok(),
        "仓内档必须仍可解析"
    );
}

/// dialog 话轮封顶：<=0 必须拒（否则状态机起轮即过期，与 ops/set 写臂判据一致）
#[test]
fn config_rejects_nonpositive_max_utterance_ms() {
    let text = "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\nmax_utterance_ms = 0\n";
    let e = rr3_brain_cfg_parse(text).unwrap_err();
    assert!(e.contains("max_utterance_ms") && e.contains(">0"), "{e}");
}

/// 缺段/缺键点名（fail-closed；cloud 四键＋dialog 段必填，bus 段可选）
#[test]
fn config_parse_rejects_missing_sections_and_fields() {
    let e = rr3_brain_cfg_parse("[rr3-brain]\nlog_level = \"info\"\n").unwrap_err();
    assert!(e.contains("[rr3-brain.cloud]"), "cloud 缺段点名: {e}");
    let e = rr3_brain_cfg_parse(
        "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\ntts_model = \"t\"\n",
    )
    .unwrap_err();
    assert!(e.contains("timeout_s"), "cloud 缺 timeout_s 点名: {e}");
    let e = rr3_brain_cfg_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n",
        )
        .unwrap_err();
    assert!(e.contains("[rr3-brain.dialog]"), "dialog 缺段点名: {e}");
    let e = rr3_brain_cfg_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n",
        )
        .unwrap_err();
    assert!(e.contains("asr_model"), "cloud 缺 asr_model 点名: {e}");
    let e = rr3_brain_cfg_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n[rr3-brain.bus]\ncall_timeout_s = \"30\"\n",
        )
        .unwrap_err();
    assert!(
        e.contains("call_timeout_s 须数值"),
        "字符串超时点名（Rust 只认数值）: {e}"
    );
    let e = rr3_brain_cfg_parse("garbage =\n").unwrap_err();
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
    let e = rr3_brain_cfg_parse(&two_active).unwrap_err();
    assert_eq!(
        e, "[[rr3-brain.model]] 须恰一行 active=true（现 2 行: ['甲', '乙']）",
        "两 active 红且名单逐字（Python 列表 repr 形状）"
    );
    let none_active = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \"u1\"\nmodel_id = \"m1\"\nactive = false\n"
        );
    let e = rr3_brain_cfg_parse(&none_active).unwrap_err();
    assert!(e.contains("现 0 行"), "零 active 同红: {e}");
    let bad_mode = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nmode = \"ssh\"\nbase_url = \"u1\"\nmodel_id = \"m1\"\nactive = true\n"
        );
    let e = rr3_brain_cfg_parse(&bad_mode).unwrap_err();
    assert_eq!(
        e, "模型行 甲 mode 不认: ssh（允许 api/local）",
        "mode 不认文案逐字"
    );
    let bad_fmt = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nmode = \"api\"\napi_format = \"anthropic\"\nbase_url = \"u1\"\nmodel_id = \"m1\"\nactive = true\n"
        );
    let e = rr3_brain_cfg_parse(&bad_fmt).unwrap_err();
    assert_eq!(
        e, "模型行 甲 api_format 暂不支持: anthropic（本批仅 openai）",
        "api_format 不认文案逐字"
    );
    let missing = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nmode = \"api\"\nmodel_id = \"m1\"\nactive = true\n"
        );
    let e = rr3_brain_cfg_parse(&missing).unwrap_err();
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
    let e = rr3_brain_cfg_parse(text).unwrap_err();
    assert!(e.contains("llm_model"), "fallback 缺 llm_model 点名: {e}");
}

/// key_ref 空白串折 None（Python strip() or None 同形状）；缺 api_key_ref 同 None
#[test]
fn model_row_key_ref_strip_and_none() {
    let cloud = "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n";
    let blank = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \"http://u1\"\nmodel_id = \"m1\"\napi_key_ref = \"  \"\nactive = true\n"
        );
    let cfg = rr3_brain_cfg_parse(&blank).unwrap();
    assert_eq!(cfg.model.key_ref, None, "空白 key_ref 折 None");
    let spaced = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \" http://u1 \"\nmodel_id = \" m1 \"\napi_key_ref = \" 甲 \"\nactive = true\n"
        );
    let cfg = rr3_brain_cfg_parse(&spaced).unwrap();
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
        (
            " https://api.siliconflow.cn ",
            "https://api.siliconflow.cn/v1",
        ),
        ("https://api.deepseek.com/v1", "https://api.deepseek.com/v1"),
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
    assert!(rr3_base_url_check("").is_err());
    assert!(rr3_base_url_check("u1").is_err());
    assert!(rr3_base_url_check("ftp://x/v1").is_err());
    assert!(rr3_base_url_check("https://x/v1/chat/completions").is_err());
    assert!(rr3_base_url_check("https://x/chat/completions/").is_err());
    assert!(rr3_base_url_check("https://api.deepseek.com/v1").is_ok());
    assert!(rr3_base_url_check("http://127.0.0.1:8080/v1").is_ok());
    // 行解析侧点名带行名（boot 拒 boot 不放行）
    let cloud = "[rr3-brain.cloud]\nbase_url = \"https://api.siliconflow.cn\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n";
    let bad = format!(
            "{cloud}[[rr3-brain.model]]\nname = \"甲\"\nbase_url = \"api.deepseek.com/v1\"\nmodel_id = \"m1\"\nactive = true\n"
        );
    let e = rr3_brain_cfg_parse(&bad).unwrap_err();
    assert!(e.starts_with("模型行 甲 base_url 须为完整 API 根"), "{e}");
}

/// 唤醒段（可选，2026-10-31）：缺段 = 关闭 + 缺省值；全量注入生效；
/// 类型错/空短语表/空应答/负窗一律拒 boot（配置劣化不静默收编）。
#[test]
fn wake_section_defaults_off_and_parses_full() {
    let base = "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n";
    let cfg = rr3_brain_cfg_parse(base).unwrap();
    assert_eq!(cfg.wake, Rr3WakeCfg::default(), "缺段 = 关闭 + 缺省值");
    let cfg = rr3_brain_cfg_parse(&format!(
            "{base}[rr3-brain.wake]\nenable = true\nphrases = [\"你好洛克\", \"洛克洛克\"]\nack = \"在呢\"\nfollowup_ms = 5000\n"
        ))
        .unwrap();
    assert!(cfg.wake.enable);
    assert_eq!(
        cfg.wake.phrases,
        vec!["你好洛克".to_string(), "洛克洛克".to_string()]
    );
    assert_eq!(cfg.wake.ack, "在呢");
    assert_eq!(cfg.wake.followup_ms, 5000);
    for bad in [
        format!("{base}[rr3-brain.wake]\nenable = \"on\"\n"),
        format!("{base}[rr3-brain.wake]\nphrases = []\n"),
        format!("{base}[rr3-brain.wake]\nphrases = [1]\n"),
        format!("{base}[rr3-brain.wake]\nack = \"\"\n"),
        format!("{base}[rr3-brain.wake]\nfollowup_ms = -1\n"),
    ] {
        assert!(rr3_brain_cfg_parse(&bad).is_err(), "应拒: {bad}");
    }
}

/// Python float 打印形状（config 就绪行逐字对位的数据基础）
#[test]
fn py_float_repr_matches_python_str() {
    assert_eq!(rr3_py_float_repr(20.0), "20.0");
    assert_eq!(rr3_py_float_repr(30.0), "30.0");
    assert_eq!(rr3_py_float_repr(17.25), "17.25");
    assert_eq!(rr3_py_float_repr(0.5), "0.5");
}
