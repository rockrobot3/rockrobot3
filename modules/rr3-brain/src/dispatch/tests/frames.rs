//! TTS 分帧用例：空/整块/尾块/水位单调与共享基准。

use super::*;

// ---------- TTS 下行流分帧（Python test_composite.TestTtsPcmFrames 对位） ----------

/// 确定性伪 PCM（s16le，字节值随下标变化，便于逐块断言原样切分）
fn pcm_bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

/// 空 PCM 零帧且水位不动
#[test]
fn frames_empty_yields_no_frames_and_seq_unchanged() {
    let (frames, seq) = rr3_tts_pcm_frames(b"", 7);
    assert!(frames.is_empty());
    assert_eq!(seq, 7);
}

/// 整块切分：640×3 → 三帧 seq 1..3、每帧 20ms、b64 解码恰一整块
#[test]
fn frames_exact_multiple_is_split_at_20ms_blocks() {
    let (frames, seq) = rr3_tts_pcm_frames(&pcm_bytes(RR3_TTS_PCM_FRAME_BYTES * 3), 0);
    assert_eq!(
        frames
            .iter()
            .map(|f| f["seq"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(seq, 3);
    for f in &frames {
        assert_eq!(f["format"], "s16le");
        assert_eq!(f["rate"], 16000);
        assert_eq!(f["channels"], 1);
        assert_eq!(f["ms"], 20, "320 样点 @16k");
        assert_eq!(
            rr3_base64_decode(f["pcm_b64"].as_str().unwrap())
                .unwrap()
                .len(),
            RR3_TTS_PCM_FRAME_BYTES
        );
    }
}

/// 尾块 ms 按实算：640 + 320 → 尾块 160 样点 = 10ms；逐块拼回原流无损
#[test]
fn frames_tail_block_ms_derived_from_actual_samples() {
    let raw = pcm_bytes(RR3_TTS_PCM_FRAME_BYTES + RR3_TTS_PCM_FRAME_BYTES / 2);
    let (frames, seq) = rr3_tts_pcm_frames(&raw, 0);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["ms"], 20);
    assert_eq!(frames[1]["ms"], 10);
    assert_eq!(seq, 2);
    let mut joined = Vec::new();
    for f in &frames {
        joined.extend(rr3_base64_decode(f["pcm_b64"].as_str().unwrap()).unwrap());
    }
    assert_eq!(joined, raw, "切分无损");
}

/// seq 模块级单调：跨轮续接不复位
#[test]
fn frames_seq_monotonic_across_calls() {
    let (_, seq) = rr3_tts_pcm_frames(&pcm_bytes(RR3_TTS_PCM_FRAME_BYTES), 5);
    let (frames, seq) = rr3_tts_pcm_frames(&pcm_bytes(RR3_TTS_PCM_FRAME_BYTES * 2), seq);
    assert_eq!(
        frames
            .iter()
            .map(|f| f["seq"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![7, 8]
    );
    assert_eq!(seq, 8);
}

/// 跨语言一致性（MC-1③）：`contracts/golden/tts-pcm-frames.json` 是 Python 腿
/// （本模块退役前的分帧真值）与 rr3-interact 收流腿共读的同一份基准——分帧
/// 输出与共享基准逐字段一致，帧列 b64 拼接还原基准 PCM
#[test]
fn frames_match_shared_golden_vector() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/golden/tts-pcm-frames.json");
    let golden: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let raw = hex_to_bytes(golden["pcm_hex"].as_str().unwrap());
    let (frames, seq) = rr3_tts_pcm_frames(&raw, 0);
    assert_eq!(
        Value::Array(frames.clone()),
        golden["frames"],
        "分帧输出与共享基准逐字段一致"
    );
    assert_eq!(seq, golden["frames"].as_array().unwrap().len() as u64);
    let mut joined = Vec::new();
    for f in &frames {
        joined.extend(rr3_base64_decode(f["pcm_b64"].as_str().unwrap()).unwrap());
    }
    assert_eq!(joined, raw, "拼回无损");
}

/// hex 文本 → 字节列（golden 基准的 pcm_hex 载体形状）
fn hex_to_bytes(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}
