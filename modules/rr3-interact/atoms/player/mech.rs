//! player 原子件机制主体（五件套③，0.0.5 MC-1 重铺：占位合成退役，件面只搬运）。
//!
//! M3 的正弦 burst 占位合成（60ms/字符、封顶 10s）随 MC-1 退役：brain 的 TTS
//! 引擎经 `rr3-brain/audio/tts_pcm` 流下发真 PCM（s16le/16k/mono），件面只做
//! **搬运**——20ms（320 样点）分块写 sink，块边界不改造样本。写块粒度与采集
//! 侧（rr3-sense）的 frame_ms 同拍：sim 后端 write 即时返回（无时钟），实时
//! 后端（ALSA）write 本身就耗一个节拍——两种后端同一形状。
//!
//! 载荷组装（`rr3_tts_result` 的 json!）、流收缓冲与点名下钻（`rr3_tts_parse_args`）
//! 在承载层 `src/composite.rs`（0.0.5 批I I-8，ADR-0019 修订 4）：件面只交播放
//! 机制，线上字节归承载层。
//!
//! ⑧-b：机制主体不 `use std`、不分配——`&[i16]` 借用进、`u64` 毫秒出。

use rr_hal::Rr3AudioSink;

/// 播放采样规格：16k/mono/16bit（与采集侧同一张 cfg，整条链一张表）
const RR3_TTS_SAMPLE_RATE: u32 = 16_000;
/// 写块样点数：20ms @ 16k = 320（16k/20ms 恰整除）
const RR3_TTS_BLOCK_SAMPLES: usize = 320;

/// 播放一段已解码的 PCM 样点：按 20ms 分块写 sink（透传语义——块边界不改
/// 造样本），返回实际写出的毫秒数 = `样点数 × 1000 / 16000`（整除截断，与
/// brain 分帧的逐帧 ms 同一公式：整块 20ms 恰整除、尾块截断，两侧对表零漂移）。
/// 写失败（后端拒）原样上抛，由调用方折成 error 应答（F1 定形：采样规格在
/// open 固定，write 不再校验配置不符）。
pub fn rr3_tts_play(sink: &mut dyn Rr3AudioSink, samples: &[i16]) -> Result<u64, String> {
    for block in samples.chunks(RR3_TTS_BLOCK_SAMPLES) {
        sink.write(block).map_err(|e| format!("写播放面失败: {e}"))?;
    }
    Ok(samples.len() as u64 * 1000 / RR3_TTS_SAMPLE_RATE as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::Rr3SimAudioSink;
    use rr_hal::RR3_AUDIO_CFG_16K_MONO16;

    /// 记账 sink：记每次 write 的块长并原样收流——钉分块形状与透传语义
    struct ChunkSpySink {
        lens: Vec<usize>,
        written: Vec<i16>,
    }

    impl ChunkSpySink {
        fn new() -> Self {
            Self {
                lens: Vec::new(),
                written: Vec::new(),
            }
        }
    }

    impl Rr3AudioSink for ChunkSpySink {
        fn open(&mut self, _cfg: rr_hal::Rr3AudioCfg) -> Result<(), rr_hal::Rr3HalError> {
            Ok(())
        }
        fn write(&mut self, samples: &[i16]) -> Result<(), rr_hal::Rr3HalError> {
            self.lens.push(samples.len());
            self.written.extend_from_slice(samples);
            Ok(())
        }
    }

    #[test]
    fn play_passes_samples_through_in_20ms_blocks() {
        let mut sink = ChunkSpySink::new();
        let input: Vec<i16> = (0..700).map(|i| (i as i32 * 37 - 5000) as i16).collect();
        let played = rr3_tts_play(&mut sink, &input).unwrap();
        // 700 样点 = 320 + 320 + 60：满块 20ms、尾块如实
        assert_eq!(sink.lens, vec![320, 320, 60]);
        // 700 × 1000 / 16000 = 43.75 → 43（整除截断）
        assert_eq!(played, 43);
        // 透传语义：块边界不改造样本，拼接流与输入逐样本相等（含负值）
        assert_eq!(sink.written, input);
    }

    #[test]
    fn play_empty_input_writes_nothing_and_is_zero_ms() {
        let mut sink = ChunkSpySink::new();
        assert_eq!(rr3_tts_play(&mut sink, &[]).unwrap(), 0);
        assert!(sink.lens.is_empty(), "空输入不产生任何 write");
    }

    #[test]
    fn play_write_error_propagates_verbatim() {
        // 未 open 的 sim sink 首次 write 即拒（后端拒）——错误原样上抛，带
        // "写播放面失败" 前缀，由调用方折成 error 应答
        let mut sink = Rr3SimAudioSink::rr3_sim_sink_new(16000);
        let e = rr3_tts_play(&mut sink, &[0i16; 320]).unwrap_err();
        assert!(e.contains("写播放面失败"), "{e}");
    }

    // ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②；
    // ---------- MC-1 重铺后语义 = 透传 + 实收样点计费） ----------

    #[test]
    fn parity_leg1_player_output_train_is_deterministic() {
        // 腿一·确定性：同一段 PCM 两跑，应答时长与写出样本流逐样本相等
        fn run() -> (u64, Vec<i16>) {
            let mut sink = Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10);
            sink.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
            let input: Vec<i16> = (0..1000).map(|i| (i as i32 * 13 - 3000) as i16).collect();
            let played = rr3_tts_play(&mut sink, &input).unwrap();
            (played, sink.rr3_sim_sink_played().to_vec())
        }
        assert_eq!(run(), run(), "同输入两跑逐样本相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_player_matches_ruling_semantics() {
        // 腿二·正确性：透传 + 计费语义独立重写——ms = 样点数×1000/16000（整除
        // 截断），写出流与输入逐样本相等（任意形状含负值/静音），非整块长度
        // 验证块边界（含 1 样点尾块）不改造样本
        let mut sink = Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10);
        sink.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
        let mut input = vec![0i16; 320 * 3 + 1];
        input[0] = i16::MIN;
        input[960] = i16::MAX; // 尾块（1 样点）上的极值
        let played = rr3_tts_play(&mut sink, &input).unwrap();
        assert_eq!(played, 961 * 1000 / 16000, "毫秒 = 样点数×1000/16000 截断");
        assert_eq!(sink.rr3_sim_sink_written_count(), input.len());
        assert_eq!(sink.rr3_sim_sink_played(), &input[..], "写出流 == 输入（透传）");
    }
}
