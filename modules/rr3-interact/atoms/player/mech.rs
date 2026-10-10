//! player 原子件机制主体（五件套③，自原 src/player.rs 逐字迁入，逻辑零改动）。
//!
//! 占位合成：`60ms/字符`，封顶 10s。波形本体（440Hz、幅度 12000，32 项查表 +
//! 整型插值）已迁 rr-hal sim 后端（0.0.5 批I I-10：AMP/HZ 是 sim 后端参数，
//! 件面去 `std` 数学；每拍 `Vec` 分配改栈上 320 样点定长缓冲——bare 档可达）。
//! 写块粒度 20ms：与采集侧（rr3-sense）的 frame_ms 同拍，sim 后端 write 即时
//! 返回（无时钟），实时后端（ALSA）write 本身就耗一个节拍——两种后端同一形状。
//!
//! 载荷组装（`rr3_tts_result` 的 json!）与点名下钻（`rr3_tts_parse_args`）
//! 已上移承载层（0.0.5 批I I-8，ADR-0019 修订 4）：件面只交时长/分块机制，
//! 组装归 `src/composite.rs`，线上字节逐字不变。

use rr_hal::sim::rr3_sim_tts_block;
use rr_hal::Rr3AudioSink;

/// 占位合成的节奏参数：60ms/字符（legacy 语音链路的口播节奏估计）
const RR3_TTS_MS_PER_CHAR: u64 = 60;
/// 单次口播时长上界：坏输入（超长文本）不能占住播放面
const RR3_TTS_MAX_MS: u64 = 10_000;
/// 播放采样规格：16k/mono/16bit（与采集侧同一张 cfg，整条链一张表）
const RR3_TTS_SAMPLE_RATE: u32 = 16_000;
/// 写块样点数：20ms @ 16k = 320（栈上定长，零分配；16k/20ms 恰整除）
const RR3_TTS_BLOCK_SAMPLES: usize = 320;

/// 占位合成时长：`字符数 × 60ms`，封顶 10s（字符按 Unicode 标量数，不是字节数）
pub fn rr3_tts_duration_ms(text: &str) -> u64 {
    (text.chars().count() as u64)
        .saturating_mul(RR3_TTS_MS_PER_CHAR)
        .min(RR3_TTS_MAX_MS)
}

/// 播放一段 TTS：占位正弦 burst（rr-hal sim 后端出数，I-10 迁入）按 20ms
/// 分块写 sink，返回实际写出的毫秒数。写失败（后端拒）原样上抛，由调用方
/// 折成 error 应答（F1 定形：采样规格在 open 固定，write 不再校验配置不符）。
pub fn rr3_tts_play(sink: &mut dyn Rr3AudioSink, text: &str) -> Result<u64, String> {
    let duration_ms = rr3_tts_duration_ms(text);
    let frames_total = (duration_ms * RR3_TTS_SAMPLE_RATE as u64 / 1000) as usize;
    let mut block = [0i16; RR3_TTS_BLOCK_SAMPLES];
    let mut cursor = 0usize;
    while cursor < frames_total {
        let n = RR3_TTS_BLOCK_SAMPLES.min(frames_total - cursor);
        rr3_sim_tts_block(&mut block[..n], cursor);
        sink.write(&block[..n])
            .map_err(|e| format!("写播放面失败: {e}"))?;
        cursor += n;
    }
    Ok(duration_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::Rr3SimAudioSink;
    use rr_hal::RR3_AUDIO_CFG_16K_MONO16;

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
    fn play_writes_exact_sample_count_and_returns_duration() {
        let mut sink = Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10);
        sink.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
        let played = rr3_tts_play(&mut sink, "abc").unwrap();
        assert_eq!(played, 180, "3 字符 × 60ms");
        // 180ms @16k = 2880 采样，分块不影响总量
        assert_eq!(sink.rr3_sim_sink_written_count(), 2_880);
        // 20ms = 320 采样一块，除尾块外每块都满
        let tail = sink.rr3_sim_sink_played();
        assert!(tail.iter().any(|&v| v != 0), "正弦 burst 不是静音");
    }

    #[test]
    fn play_output_tracks_sine_formula_within_lut_budget() {
        // 播出内容 = 独立公式 12000·sin(2π·440·i/16000) 的开头（不是实现回抄：
        // 公式重写在测试里，分块边界处相位必须连续）。I-10 查表化后是近似合成，
        // 预算 = 全程偏差 ≤100（幅度 12000 的 0.8%，与 rr-hal sim 后端同一条预算）
        let mut sink = Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10);
        sink.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
        rr3_tts_play(&mut sink, "abcd").unwrap();
        let played = sink.rr3_sim_sink_played();
        assert_eq!(played.len(), 240 * 16);
        for (i, &v) in played.iter().enumerate() {
            let want = (12_000.0f64
                * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 16_000.0).sin())
                as i64;
            assert!((v as i64 - want).abs() <= 100, "sample #{i}: {v} vs {want}");
        }
    }
}
