//! 总线契约形状：wire manifest 文本、主题常量与留痕判据。

use std::path::Path;

use crate::drill::{rr3_manifest_ext, RR3_SIM_MODULE_A, RR3_SIM_MODULE_B, RR3_SIM_MODULE_BRAIN};

/// 模块健康参数：与两面演练同值（比生产紧、比测试宽）
const RR3_BRAIN_HB_MS: u64 = 200;
const RR3_BRAIN_WD_MS: u64 = 1000;
/// brain 日志里的轮次完成判据（__main__.py 的完成行前缀）
pub const RR3_BRAIN_TURN_MARK: &str = "对话轮完成";
/// interact 日志里的播放判据（player 的 tts 完成行前缀）
pub const RR3_INTERACT_TTS_MARK: &str = "tts:";

/// brain 的演练判据主题（名字必须真实在契约里，tests 有对账用例）
pub const RR3_BRAIN_TOPIC_INTENT: &str = "rr3-brain/intent/result";
pub const RR3_BRAIN_TOPIC_COMPLETION: &str = "rr3-brain/llm/completion";
pub const RR3_BRAIN_TOPIC_TTS_PCM: &str = "rr3-brain/audio/tts_pcm";
pub const RR3_SENSE_TOPIC_PCM: &str = "rr3-sense/audio/pcm";

/// 三份 wire manifest 文本（纯函数，tests 直测形状）
pub fn rr3_brain_manifests(
    sense_entry: &Path,
    interact_entry: &Path,
    brain_entry: &Path,
) -> Vec<(String, String)> {
    vec![
        (
            RR3_SIM_MODULE_A.to_string(),
            rr3_manifest_ext(
                RR3_SIM_MODULE_A,
                sense_entry,
                "module",
                "rust",
                RR3_BRAIN_HB_MS,
                RR3_BRAIN_WD_MS,
                "always",
                &[RR3_SENSE_TOPIC_PCM, "rr3-sense/audio/vad_start"],
                &["rr3-runtime/module/state"],
                &[],
            ),
        ),
        (
            RR3_SIM_MODULE_B.to_string(),
            rr3_manifest_ext(
                RR3_SIM_MODULE_B,
                interact_entry,
                "module",
                "rust",
                RR3_BRAIN_HB_MS,
                RR3_BRAIN_WD_MS,
                "never",
                &[],
                &["rr3-runtime/module/state", RR3_BRAIN_TOPIC_TTS_PCM],
                &["rr3-interact/tts/text", "rr3-interact/face/expression"],
            ),
        ),
        (
            RR3_SIM_MODULE_BRAIN.to_string(),
            rr3_manifest_ext(
                RR3_SIM_MODULE_BRAIN,
                brain_entry,
                "brain",
                "rust",
                RR3_BRAIN_HB_MS,
                RR3_BRAIN_WD_MS,
                "never",
                &[RR3_BRAIN_TOPIC_INTENT, RR3_BRAIN_TOPIC_TTS_PCM],
                // C14（ADR-0019 修订 2 ②，2026-10-10 评审裁定）：asr/text retired 撤申报、
                // net/link 撤申报（不补实现）；订阅面对齐实然面 = pcm + vad_start（B-1 补齐）
                &[RR3_SENSE_TOPIC_PCM, "rr3-sense/audio/vad_start"],
                &[RR3_BRAIN_TOPIC_COMPLETION],
            ),
        ),
    ]
}
