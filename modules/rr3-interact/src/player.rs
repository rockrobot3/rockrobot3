//! 播放：TTS 音频出话（`rr3-interact/tts/text`，request）。
//!
//! M3 的 v0.0.1 播放面 = 正弦 burst 占位合成：`60ms/字符`，封顶 10s。这不是
//! 真音频，但走的是**真链路**——rr3.call 点名进（trace_id=调用方 task）→
//! `Rr3AudioSink` 写块 → `rr3_reply_call` 回 `{"played_ms"}`。真 TTS 合成随
//! 后续里程碑换进 [`rr3_tts_play`] 内部，请求/应答形状不变。
//!
//! 写块粒度 20ms：与采集侧（rr3-sense）的 frame_ms 同拍，sim 后端 write 即时
//! 返回（无时钟），实时后端（ALSA）write 本身就耗一个节拍——两种后端同一形状。

use rr_hal::{Rr3AudioSink, Rr3PcmStream, RR3_AUDIO_CFG_16K_MONO16};
use serde_json::{json, Value};

pub const SURFACE: &[&str] = &["player: rpc rr3-interact/tts/text (request)"];

/// 本模块 exposes 的 TTS 主题（module.toml [rpc].exposes 同源）
pub const RR3_TTS_TOPIC: &str = "rr3-interact/tts/text";

/// 占位合成的节奏参数：60ms/字符（legacy 语音链路的口播节奏估计）
const RR3_TTS_MS_PER_CHAR: u64 = 60;
/// 单次口播时长上界：坏输入（超长文本）不能占住播放面
const RR3_TTS_MAX_MS: u64 = 10_000;
/// 写块粒度（毫秒）
const RR3_TTS_FRAME_MS: u64 = 20;
/// 播放采样规格：16k/mono/16bit（与采集侧同一张 cfg，整条链一张表）
const RR3_TTS_SAMPLE_RATE: u32 = 16_000;

/// 占位合成时长：`字符数 × 60ms`，封顶 10s（字符按 Unicode 标量数，不是字节数）
pub fn rr3_tts_duration_ms(text: &str) -> u64 {
    (text.chars().count() as u64)
        .saturating_mul(RR3_TTS_MS_PER_CHAR)
        .min(RR3_TTS_MAX_MS)
}

/// 占位合成的第 i 个采样点：440Hz 正弦、幅度 12000（与 rr-hal sim 源同参数，
/// 但独立实现——播放面不依赖"读麦克风"，波形只借参数）。
fn rr3_tts_sample(i: usize) -> i16 {
    const AMP: f64 = 12_000.0;
    const HZ: f64 = 440.0;
    (AMP * (2.0 * std::f64::consts::PI * HZ * i as f64 / RR3_TTS_SAMPLE_RATE as f64).sin()) as i16
}

/// 从 rr3.call 的 `params.args` 里取 TTS 请求参数：`text` 必填且须非空字符串。
/// 校验失败返回 `Err`，由调用方折成 error 应答（请求-应答语义，不当致命错误）。
pub fn rr3_tts_parse_args(args: &Value) -> Result<String, String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{RR3_TTS_TOPIC} 缺 string 参数 text"))?;
    if text.is_empty() {
        return Err(format!("{RR3_TTS_TOPIC} 的 text 不能为空"));
    }
    Ok(text.to_string())
}

/// 播放一段 TTS：正弦 burst 按 20ms 分块写 sink，返回实际写出的毫秒数。
/// 写失败（后端拒/配置漂移）原样上抛，由调用方折成 error 应答。
pub fn rr3_tts_play(sink: &dyn Rr3AudioSink, text: &str) -> Result<u64, String> {
    let duration_ms = rr3_tts_duration_ms(text);
    let frames_total = (duration_ms * RR3_TTS_SAMPLE_RATE as u64 / 1000) as usize;
    let frames_per_block = (RR3_TTS_FRAME_MS * RR3_TTS_SAMPLE_RATE as u64 / 1000) as usize;
    let mut cursor = 0usize;
    while cursor < frames_total {
        let n = frames_per_block.min(frames_total - cursor);
        let samples: Vec<i16> = (0..n).map(|i| rr3_tts_sample(cursor + i)).collect();
        let pcm = Rr3PcmStream {
            cfg: RR3_AUDIO_CFG_16K_MONO16,
            samples,
        };
        rr_hal::rr3_block_on(sink.write(pcm)).map_err(|e| format!("写播放面失败: {e}"))?;
        cursor += n;
    }
    Ok(duration_ms)
}

/// `rr3-interact/tts/text` 的应答体：实际播出的毫秒数与计费字符数
/// （封顶后 played_ms < chars×60 时两者对不上，一并回给调用方对表）。
pub fn rr3_tts_result(text: &str, played_ms: u64) -> Value {
    json!({ "played_ms": played_ms, "chars": text.chars().count() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::rr3_block_on;
    use rr_hal::sim::Rr3SimAudioSink;

    #[test]
    fn duration_is_60ms_per_char_capped_at_10s() {
        assert_eq!(rr3_tts_duration_ms("你好"), 120);
        assert_eq!(rr3_tts_duration_ms("hello"), 300);
        // 166 字符 × 60ms = 9960ms，不触顶
        assert_eq!(rr3_tts_duration_ms(&"a".repeat(166)), 9_960);
        // 200 字符 × 60ms = 12000ms → 封顶 10s
        assert_eq!(rr3_tts_duration_ms(&"a".repeat(200)), 10_000);
        // 字符按 Unicode 标量数：中文一字一拍，不是一字节一拍
        assert_eq!(rr3_tts_duration_ms("你好世界"), 240);
    }

    #[test]
    fn args_require_non_empty_string_text() {
        assert_eq!(rr3_tts_parse_args(&json!({ "text": "hi" })).unwrap(), "hi");
        for bad in [
            json!({}),
            json!({ "text": 3 }),
            json!({ "text": "" }),
            json!({ "text": null }),
        ] {
            assert!(rr3_tts_parse_args(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn play_writes_exact_sample_count_and_returns_duration() {
        let sink = Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10);
        rr3_block_on(sink.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        let played = rr3_tts_play(&sink, "abc").unwrap();
        assert_eq!(played, 180, "3 字符 × 60ms");
        // 180ms @16k = 2880 采样，分块不影响总量
        assert_eq!(sink.rr3_sim_sink_written_count(), 2_880);
        // 20ms = 320 采样一块，除尾块外每块都满
        let tail = sink.rr3_sim_sink_played();
        assert!(tail.iter().any(|&v| v != 0), "正弦 burst 不是静音");
    }

    #[test]
    fn play_output_matches_standalone_sine_formula() {
        // 播出内容 = 独立公式 12000·sin(2π·440·i/16000) 的开头（不是实现回抄：
        // 公式重写在测试里，分块边界处相位必须连续）
        let sink = Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10);
        rr3_block_on(sink.open(RR3_AUDIO_CFG_16K_MONO16)).unwrap();
        rr3_tts_play(&sink, "abcd").unwrap();
        let played = sink.rr3_sim_sink_played();
        assert_eq!(played.len(), 240 * 16);
        for (i, &v) in played.iter().enumerate() {
            let want = (12_000.0f64
                * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 16_000.0).sin())
                as i16;
            assert_eq!(v, want, "sample #{i}");
        }
    }

    #[test]
    fn result_shape_carries_played_ms_and_chars() {
        let r = rr3_tts_result("你好", 120);
        assert_eq!(r["played_ms"], 120);
        assert_eq!(r["chars"], 2);
        assert_eq!(r.get("topic"), None, "应答体不带 topic");
    }
}
