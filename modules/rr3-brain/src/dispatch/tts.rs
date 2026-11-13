//! TTS 下行流：突发节流发布与收尾判据。

use super::*;

/// TTS 下行突发节流（本轮审计）：一轮 PCM 是整段预合成（播放前），帧数可达
/// 数百；一次性灌入会把 rrd 的有界广播队列（128 帧）顶满——即使 rrd 满侧只丢帧
/// 不摘连接，interact 的收流对账也会因缺帧拒播。每 16 帧让出 1ms 给 fanout/
/// writer 排空：10s 语音（500 帧）仅增 ~31ms，相对 ASR/LLM/TTS 的秒级耗时无量级影响。
const RR3_TTS_PUB_BURST: usize = 16;

/// 按 [`rr3_tts_pcm_frames`] 分帧发布 tts_pcm 流，返回新的 seq 水位
/// （发布写不出去 Err 上抛——传输性失败致命）。
pub(super) fn rr3_publish_tts_pcm<B: Rr3TurnBus>(
    bus: &B,
    pcm: &[u8],
    seq: u64,
) -> Result<u64, String> {
    let (frames, seq) = rr3_tts_pcm_frames(pcm, seq);
    for (i, body) in frames.into_iter().enumerate() {
        bus.publish(RR3_TOPIC_TTS_PCM, body)?;
        rr3_tts_burst_gap(i);
    }
    Ok(seq)
}

/// 分块发布节流：每 `RR3_TTS_PUB_BURST` 帧歇 1ms（给总线消费侧留拍）。
fn rr3_tts_burst_gap(i: usize) {
    if (i + 1).is_multiple_of(RR3_TTS_PUB_BURST) {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// 预合成结果分类：非空 PCM 入表，空/失败只留痕（首轮按需重试）。
pub(super) fn rr3_ack_pcm_of(started: Instant, res: Result<Vec<u8>, String>) -> Option<Vec<u8>> {
    match res {
        Ok(pcm) => rr3_ack_pcm_hit(started, pcm),
        Err(e) => {
            eprintln!("唤醒应答预合成失败，首个唤醒轮将按需重试（{e}）");
            None
        }
    }
}

/// 预合成成功分支：非空 PCM 入表并点名耗时，空 PCM 只留痕（首轮按需重试）。
fn rr3_ack_pcm_hit(started: Instant, pcm: Vec<u8>) -> Option<Vec<u8>> {
    if pcm.is_empty() {
        eprintln!("唤醒应答预合成返回空 PCM，首个唤醒轮将按需重试");
        return None;
    }
    eprintln!(
        "唤醒应答预合成就绪: {}ms pcm_bytes={}",
        started.elapsed().as_millis(),
        pcm.len()
    );
    Some(pcm)
}
