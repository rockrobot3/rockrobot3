//! voice 分段状态机单测（自 `voice.rs` 拆出，控制单文件规模）：内存路径的
//! 起音/判停、超长强制收段、半双工门重置。spool 路径依赖真实分区，归真板批。

use std::collections::VecDeque;

use crate::state::PeriphState;

use super::*;

fn bus() -> Arc<Mutex<Bus>> {
    Arc::new(Mutex::new(Bus {
        jobs: VecDeque::new(),
        outbox: VecDeque::new(),
        audio_q: VecDeque::new(),
        st: PeriphState::boot(),
    }))
}

fn block(amp: i16) -> [i16; BLOCK_SAMPLES] {
    [amp; BLOCK_SAMPLES]
}

#[test]
fn noise_below_threshold_never_opens() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    for i in 0..100 {
        v.block(&block(300), &bus);
        assert!(
            !v.recording,
            "第 {i} 块底噪 300 低于 rms_start=450，不得起音"
        );
    }
}

#[test]
fn hysteresis_tail_holds_between_thresholds() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    for _ in 0..10 {
        v.block(&block(500), &bus);
    }
    assert!(v.recording, "500 ≥ 起音 450 应起音");
    for _ in 0..20 {
        v.block(&block(400), &bus);
    }
    assert!(v.recording, "400 ≥ 收尾 380，不得判停");
    for _ in 0..20 {
        v.block(&block(300), &bus);
    }
    assert!(!v.recording, "300 < 380 连续 400ms 应收段");
}

#[test]
fn constant_speech_opens_then_offset_closes() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    for i in 0..9 {
        v.block(&block(3000), &bus);
        assert!(!v.recording, "第 {i} 块不足 min_speech，不得开录");
    }
    v.block(&block(3000), &bus);
    assert!(v.recording, "第 10 块（200ms）应起音");
    assert!(v.utter.len() >= BLOCK_SAMPLES, "预滚窗应补进段头");
    for _ in 0..20 {
        v.block(&block(3000), &bus);
    }
    for _ in 0..20 {
        v.block(&block(0), &bus);
    }
    assert!(!v.recording, "silence_tail 到点应收段");
    assert!(v.utter.is_empty(), "收段后段缓冲清空");
}

#[test]
fn overlong_utterance_is_forced_closed() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    for _ in 0..10 {
        v.block(&block(3000), &bus);
    }
    let blocks = MAX_UTTER_SAMPLES / BLOCK_SAMPLES + 2;
    for _ in 0..blocks {
        v.block(&block(3000), &bus);
    }
    assert!(!v.recording, "超过 6 s 必须强制收段");
    assert!(v.utter.is_empty());
}

#[test]
fn gated_batch_resets_state() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    for _ in 0..10 {
        v.block(&block(3000), &bus);
    }
    assert!(v.recording);
    v.batch(&[0u8; 8], 8, &bus, true);
    assert!(!v.recording);
    assert!(v.pre.is_empty() && v.utter.is_empty());
}
