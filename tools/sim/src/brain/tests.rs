//! brain 演练单测：契约身份、manifest 形状与 mock/real 云 config。

use super::*;
use std::path::Path;

use crate::drill::{RR3_SIM_MODULE_A, RR3_SIM_MODULE_B, RR3_SIM_MODULE_BRAIN};

/// 钉住"演练用的名字/主题必须真实在契约里"——契约改名时这里红，不是演练跑到一半红
#[test]
fn brain_sim_identities_exist_in_the_real_contract() {
    let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/topics.toml");
    let text = std::fs::read_to_string(&contract)
        .unwrap_or_else(|e| panic!("读 {}: {e}", contract.display()));
    for needle in [
        RR3_SIM_MODULE_A,
        RR3_SIM_MODULE_B,
        RR3_SIM_MODULE_BRAIN,
        RR3_BRAIN_TOPIC_INTENT,
        RR3_BRAIN_TOPIC_COMPLETION,
        RR3_BRAIN_TOPIC_TTS_PCM,
        RR3_SENSE_TOPIC_PCM,
        "rr3-sense/audio/vad_start",
        "rr3-interact/tts/text",
        "rr3-connect/net/link",
    ] {
        assert!(text.contains(needle), "契约缺 {needle}");
    }
}

#[test]
fn manifests_have_absolute_entries_and_contract_shapes() {
    let ms = rr3_brain_manifests(
        Path::new("/abs/rr3-sense"),
        Path::new("/abs/rr3-interact"),
        Path::new("/abs/rr3-brain"),
    );
    assert_eq!(ms.len(), 3);
    let sense = &ms[0].1;
    assert!(sense.contains("entry = \"/abs/rr3-sense\""), "{sense}");
    assert!(
        sense.contains("publishes = [\"rr3-sense/audio/pcm\", \"rr3-sense/audio/vad_start\"]"),
        "{sense}"
    );
    let brain = &ms[2].1;
    assert!(brain.contains("type = \"brain\""), "{brain}");
    assert!(brain.contains("lang = \"rust\""), "{brain}");
    assert!(
        brain.contains("exposes = [\"rr3-brain/llm/completion\"]"),
        "{brain}"
    );
}

#[test]
fn config_shapes() {
    // mock 档（缺省）：死地址 + mock 模型 + burst 短判停
    let cfg = rr3_brain_cfg_text(false, "http://127.0.0.1:9/v1");
    assert!(cfg.contains("max_utterance_ms = 1500"), "{cfg}");
    assert!(
        cfg.contains("base_url = \"http://127.0.0.1:9/v1\""),
        "{cfg}"
    );
    assert!(cfg.contains("llm_model = \"mock\""), "{cfg}");
}

#[test]
fn real_cloud_config_carries_production_models_and_endpoint() {
    // 真值镜像 modules/rr3-brain/config.toml：cloud.base_url 是主机根（ASR/TTS
    // 自拼 /v1/audio/*）；模型行 base_url 是完整 API 根（LLM 只拼 /chat/completions）
    let cfg = rr3_brain_cfg_text(true, "https://api.siliconflow.cn");
    assert!(
        cfg.contains("base_url = \"https://api.siliconflow.cn\""),
        "{cfg}"
    );
    assert!(
        cfg.contains("base_url = \"https://api.siliconflow.cn/v1\""),
        "模型行补版本段: {cfg}"
    );
    // 已带路径的 API 根原样（GLM / Qwen 各写各的版本段，不按主机名猜）
    let glm = rr3_brain_cfg_text(true, "https://open.bigmodel.cn/api/paas/v4");
    assert!(
        glm.contains("base_url = \"https://open.bigmodel.cn/api/paas/v4\""),
        "{glm}"
    );
    // 生产模型名与 modules/rr3-brain/config.toml 同值（此处按值镜像，改名两侧同红）
    assert!(cfg.contains("asr_model = \"iic/SenseVoiceSmall\""), "{cfg}");
    assert!(
        cfg.contains("llm_model = \"deepseek-ai/DeepSeek-V3\""),
        "{cfg}"
    );
    assert!(
        cfg.contains("tts_model = \"FunAudioLLM/CosyVoice2-0.5B\""),
        "{cfg}"
    );
    assert!(cfg.contains("timeout_s = 20"), "{cfg}");
    // 0.0.6 模型连接抽象批：real 档必须携带 [[rr3-brain.model]] 连接行
    // （恰一 active、openai 格式、密钥走 env 引用——演练面沿用 RR3_CLOUD_API_KEY）
    assert!(cfg.contains("[[rr3-brain.model]]"), "{cfg}");
    assert!(cfg.contains("mode = \"api\""), "{cfg}");
    assert!(cfg.contains("api_format = \"openai\""), "{cfg}");
    assert!(
        cfg.contains("model_id = \"deepseek-ai/DeepSeek-V3\""),
        "{cfg}"
    );
    assert!(
        cfg.contains("api_key_ref = \"env:RR3_CLOUD_API_KEY\""),
        "{cfg}"
    );
    assert!(cfg.contains("active = true"), "{cfg}");
    // 人声 8s 判停（生产值）——burst 的 1.5s 兜底对真人太紧
    assert!(cfg.contains("max_utterance_ms = 8000"), "{cfg}");
    assert!(!cfg.contains("127.0.0.1:9"), "{cfg}");
}
