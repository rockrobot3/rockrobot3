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
//!
//! 错误面零堆化（0.0.8 批N24 件四，L-3 形状清理）：`rr3_tts_play` 返回
//! [`Rr3TtsPlayError`] 零堆枚举（写腿失败携 `Rr3HalError` 原样随行，机制层
//! 不落 `String` 堆），人读文案由编排层渲染（`src/composite.rs` 的
//! `rr3_tts_play_error_text`，与改前逐字一致）；HAL 句柄入参改泛型
//! （`S: Rr3AudioSink + ?Sized`，调用点单态化不改行为）。

use rr_hal_api::{Rr3AudioSink, Rr3HalError};

/// 播放采样规格：16k/mono/16bit（与采集侧同一张 cfg，整条链一张表）
pub const RR3_TTS_SAMPLE_RATE: u32 = 16_000;
/// 写块样点数：20ms @ 16k = 320（16k/20ms 恰整除）
const RR3_TTS_BLOCK_SAMPLES: usize = 320;

/// 播放增益（MC-2）：`volume_pct` 整型饱和缩放——i64 中间量乘除（不上浮
/// f32），结果夹回 i16（饱和不回卷）。0% → 全零、100% → 逐字原样（×100/100
/// 恒等）、>100% 超界样点饱和夹断。原地改写不分配（⑧-b：机制不分配）。
pub fn rr3_tts_apply_gain(samples: &mut [i16], volume_pct: u32) {
    for s in samples.iter_mut() {
        let v = *s as i64 * volume_pct as i64 / 100;
        *s = v.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
    }
}

/// 起播水位（MC-2，legacy 0cf029e 语义回归）：缓冲达水位才起播（抗网络
/// 抖动），流已完整则未攒满也播（短回答不卡水位——水位取
/// `min(prebuffer_ms, total_ms)`）。三参全毫秒域：
/// - `buffered_ms` 起播时刻缓冲实收时长；
/// - `total_ms` 本轮流总时长（点名 `pcm_bytes` 对账一侧导出）；
/// - `prebuffer_ms` 策略水位（config 注入）。
pub fn rr3_tts_prebuffer_ready(buffered_ms: u64, total_ms: u64, prebuffer_ms: u64) -> bool {
    buffered_ms >= prebuffer_ms.min(total_ms)
}

/// [`rr3_tts_play`] 的零堆错误（批N24 件四）：写腿失败一变体，HAL 错原样
/// 随行——结构化数据进枚举，`String` 文案在编排层渲染（机制与策略分离：
/// 机制层不落堆、不攒人读文案）。
#[derive(Debug, PartialEq, Eq)]
pub enum Rr3TtsPlayError {
    /// 分块写播放面失败（后端拒）
    Write(Rr3HalError),
}

/// 播放一段已解码的 PCM 样点：按 20ms 分块写 sink（透传语义——块边界不改
/// 造样本），返回实际写出的毫秒数 = `样点数 × 1000 / 16000`（整除截断，与
/// brain 分帧的逐帧 ms 同一公式：整块 20ms 恰整除、尾块截断，两侧对表零漂移）。
/// 写失败（后端拒）原样上抛，由调用方折成 error 应答（F1 定形：采样规格在
/// open 固定，write 不再校验配置不符）。泛型 `?Sized`：调用点持
/// `&mut dyn Rr3AudioSink` 或具体类型都可直接传（单态化不改行为）。
pub fn rr3_tts_play<S: Rr3AudioSink + ?Sized>(
    sink: &mut S,
    samples: &[i16],
) -> Result<u64, Rr3TtsPlayError> {
    for block in samples.chunks(RR3_TTS_BLOCK_SAMPLES) {
        sink.write(block).map_err(Rr3TtsPlayError::Write)?;
    }
    Ok(samples.len() as u64 * 1000 / RR3_TTS_SAMPLE_RATE as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::Rr3SimAudioSink;
    use rr_hal_api::RR3_AUDIO_CFG_16K_MONO16;

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
        fn open(&mut self, _cfg: rr_hal_api::Rr3AudioCfg) -> Result<(), rr_hal_api::Rr3HalError> {
            Ok(())
        }
        fn write(&mut self, samples: &[i16]) -> Result<(), rr_hal_api::Rr3HalError> {
            self.lens.push(samples.len());
            self.written.extend_from_slice(samples);
            Ok(())
        }
    }

    // ---------- 增益与水位（0.0.5 批M MC-2：播放面参数真正生效） ----------

    /// 增益三档（MC-2 工单判据）：0% → 全零 / 100% → 逐字原样 / 饱和不回卷
    #[test]
    fn gain_three_rungs_zero_full_and_saturating() {
        // 0% → 全零样点
        let mut s = vec![123i16, -456, i16::MAX, i16::MIN];
        rr3_tts_apply_gain(&mut s, 0);
        assert_eq!(s, vec![0i16; 4], "0% 增益 = 全零样点");
        // 100% → 逐字原样（×100/100 恒等，含极值）
        let mut s = vec![123i16, -456, i16::MAX, i16::MIN];
        rr3_tts_apply_gain(&mut s, 100);
        assert_eq!(s, vec![123i16, -456, i16::MAX, i16::MIN], "100% 增益恒等");
        // 200% 超界样点饱和夹断：30000×2=60000 → 32767；-30000×2=-60000 → -32768
        // （夹回 i16 极值，不回卷成反号）
        let mut s = vec![30000i16, -30000, 123, -456];
        rr3_tts_apply_gain(&mut s, 200);
        assert_eq!(
            s,
            vec![i16::MAX, i16::MIN, 246, -912],
            "200% 饱和夹断不回卷"
        );
    }

    /// 起播水位（MC-2，legacy 0cf029e 语义）：达水位起播 / 未达不起播 /
    /// 短回答（总长 < 水位）流完整即播——水位取 min 不卡短语音
    #[test]
    fn prebuffer_ready_gate_min_semantics() {
        // 达水位：缓冲 800ms ≥ 水位 500ms → 播
        assert!(rr3_tts_prebuffer_ready(800, 800, 500));
        // 恰好等于水位（边界含等号）→ 播
        assert!(rr3_tts_prebuffer_ready(500, 800, 500));
        // 未达水位且流长于水位 → 不播（抗抖动：水位未满不起播）
        assert!(!rr3_tts_prebuffer_ready(200, 1000, 500));
        // 短回答放行：流总长 300ms < 水位 500ms，缓冲已完整（300ms）→ 播
        // （legacy："短回答没攒满也播"）
        assert!(rr3_tts_prebuffer_ready(300, 300, 500));
        // 短回答未攒满：缓冲 200ms < 总长 300ms < 水位 500ms → 不播（流未完整）
        assert!(!rr3_tts_prebuffer_ready(200, 300, 500));
        // 水位 0 = 无预缓冲：缓冲非零即播
        assert!(rr3_tts_prebuffer_ready(1, 1000, 0));
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
        // 未 open 的 sim sink 首次 write 即拒（后端拒）——错误按零堆枚举原样
        // 上抛（批N24 件四），人读文案由编排层渲染（composite 用例钉逐字）
        let mut sink = Rr3SimAudioSink::rr3_sim_sink_new(16000);
        let e = rr3_tts_play(&mut sink, &[0i16; 320]).unwrap_err();
        assert!(
            matches!(e, Rr3TtsPlayError::Write(_)),
            "写腿失败携 HAL 错随行: {e:?}"
        );
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
        assert_eq!(
            sink.rr3_sim_sink_played(),
            &input[..],
            "写出流 == 输入（透传）"
        );
    }
}
