//! mock 云与行为问答：演练 config 文本、真云端点换算与后端缺省面。

use crate::drill::Rr3SimArgs;

/// brain 演练专属 config 的判停上限：burst 源 1s 语音/1s 静音，正常轮由 offset 判停，
/// 时长上限只是兜底（防判停逻辑失效时闭环卡死）
const RR3_BRAIN_MAX_UTTERANCE_MS: u64 = 1500;

/// brain 演练专属 config。mock 档（缺省）：死地址 + mock 模型，闭环不碰网络、
/// burst 源 1.5s 短判停。real 档（`--brain-cloud real`）：真端点 + 生产模型名
/// （真值源 modules/rr3-brain/config.toml，此处按值镜像）+ 人声 8s 判停（生产值，
/// 真人说话远慢于 burst 周期）。0.0.6 模型连接抽象批：real 档补 `[[rr3-brain.model]]`
/// 连接行——`--brain-base-url` 旗标按主机根传入，行 base_url 补成完整 API 根
/// （ADR-0023：LLM 只拼 /chat/completions），LLM 腿走 openai 格式，密钥引用
/// `env:RR3_CLOUD_API_KEY`（演练面沿用旧环境变量契约，面板 secrets 是板上/宿主面）。
pub fn rr3_brain_cfg_text(cloud_real: bool, base_url: &str) -> String {
    if cloud_real {
        const ASR_MODEL: &str = "iic/SenseVoiceSmall";
        const LLM_MODEL: &str = "deepseek-ai/DeepSeek-V3";
        const TTS_MODEL: &str = "FunAudioLLM/CosyVoice2-0.5B";
        // 演练面两义分离（ADR-0023）：cloud.base_url = 主机根（ASR/TTS 自拼
        // /v1/audio/*）；模型行 base_url = 完整 API 根（LLM 只拼 /chat/completions）。
        let llm_root = rr3_llm_root_of(base_url);
        format!(
            "[rr3-brain]\nlog_level = \"info\"\n\n[rr3-brain.cloud]\n\
             base_url = \"{base_url}\"\nasr_model = \"{ASR_MODEL}\"\nllm_model = \"{LLM_MODEL}\"\n\
             tts_model = \"{TTS_MODEL}\"\ntimeout_s = 20\n\n[rr3-brain.dialog]\n\
             max_utterance_ms = 8000\n\n[[rr3-brain.model]]\nname = \"演练真云\"\n\
             mode = \"api\"\napi_format = \"openai\"\nbase_url = \"{llm_root}\"\n\
             model_id = \"{LLM_MODEL}\"\napi_key_ref = \"env:RR3_CLOUD_API_KEY\"\nactive = true\n"
        )
    } else {
        format!(
            "[rr3-brain]\nlog_level = \"info\"\n\n[rr3-brain.cloud]\n\
             base_url = \"http://127.0.0.1:9/v1\"\nasr_model = \"mock\"\nllm_model = \"mock\"\n\
             tts_model = \"mock\"\ntimeout_s = 5\n\n[rr3-brain.dialog]\nsilence_tail_ms = 700\n\
             max_turn_chars = 800\nmax_utterance_ms = {RR3_BRAIN_MAX_UTTERANCE_MS}\n"
        )
    }
}

/// `--brain-base-url` 取值换算（与 rr3-brain boot 的 cloud fallback 同义）：主机根
/// 路径为空补 `/v1`；已带路径（GLM `/api/paas/v4`、Qwen `/compatible-mode/v1`）原样。
fn rr3_llm_root_of(base_url: &str) -> String {
    let s = base_url.trim().trim_end_matches('/');
    if rr3_url_has_path(s) {
        s.to_string()
    } else {
        format!("{s}/v1")
    }
}

/// `scheme://host` 之后是否已带路径（GLM/Qwen 等自带路径档原样保留）。
fn rr3_url_has_path(s: &str) -> bool {
    let (_, rest) = s.split_once("://").unwrap_or(("", s));
    rest.contains('/')
}

/// 演练缺省面：音频/回放后端缺省 sim（CI/无头纪律），回放跟随源面；云端缺省
/// mock、base_url 占位（real 必给已在参数面 fail-closed）。
pub(crate) fn rr3_brain_defaults(a: &Rr3SimArgs) -> (String, String, bool, String) {
    let audio_backend = a.audio_backend.clone().unwrap_or_else(|| "sim".to_string());
    let sink_backend = a
        .sink_backend
        .clone()
        .unwrap_or_else(|| audio_backend.clone());
    let cloud_real = a.brain_cloud.as_deref() == Some("real");
    let brain_base_url = a
        .brain_base_url
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:9/v1".to_string());
    (audio_backend, sink_backend, cloud_real, brain_base_url)
}
