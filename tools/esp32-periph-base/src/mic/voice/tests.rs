//! voice 分段状态机单测（自 `voice.rs` 拆出，控制单文件规模）：内存路径的
//! 起音/判停、超长强制收段、半双工门重置。spool 路径依赖真实分区，归真板批。

use std::collections::VecDeque;

use rr_audio_mech::{Rr3NoiseEst, RR3_NOISE_FRAMES};

use crate::state::{Job, PeriphState};

use super::*;

/// 确定性宽带源（与机制 golden 同构）：生成左右相关/独立两种判向窗。
struct Xor(u32);

impl Xor {
    fn next_i16(&mut self, amp: i32) -> i16 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (((x as i32) >> 16) * amp >> 15) as i16
    }
}

/// 左麦领先右麦 4 样本（真板 `sign=-1` 下判 right）。
fn stereo_l_lead(frames: usize, amp: i32) -> Vec<i16> {
    let mut xs = Xor(0x1234_5678);
    let u: Vec<i16> = (0..frames + 4).map(|_| xs.next_i16(amp)).collect();
    (0..frames).flat_map(|t| [u[t], u[t + 4]]).collect()
}

/// 双麦同相（正前方声源）。
fn stereo_same(frames: usize, amp: i32) -> Vec<i16> {
    let mut xs = Xor(0x2468_ace0);
    let u: Vec<i16> = (0..frames).map(|_| xs.next_i16(amp)).collect();
    (0..frames).flat_map(|t| [u[t], u[t]]).collect()
}

/// 双麦独立噪声（低置信判向）。
fn stereo_indep(frames: usize, amp: i32) -> Vec<i16> {
    let mut a = Xor(0x1234_5678);
    let mut b = Xor(0x9e37_79b9);
    (0..frames)
        .flat_map(|_| [a.next_i16(amp), b.next_i16(amp)])
        .collect()
}

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

#[test]
fn boot_calibration_tunes_vad_from_noise_median() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    v.calibrated = false;
    v.cal = Rr3NoiseEst::rr3_new();
    for _ in 0..RR3_NOISE_FRAMES {
        v.block(&block(600), &bus);
    }
    assert!(v.calibrated, "满窗应完成标定");
    assert_eq!(v.vad_cfg.rms_start, 900, "600×1.5");
    assert_eq!(v.vad_cfg.rms_tail, 750, "600×1.25");
    for _ in 0..100 {
        v.block(&block(700), &bus);
    }
    assert!(!v.recording, "700 低于标定起音 900，不得起音");
    for _ in 0..10 {
        v.block(&block(1000), &bus);
    }
    assert!(v.recording, "1000 ≥ 900 应起音");
}

#[test]
fn periodic_doa_requires_consensus_then_steers() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    v.pre = stereo_l_lead(9600, 3000);
    v.recording = true;
    v.last_bin = None;
    v.last_steer_ms = 0;
    v.doa_blocks = DOA_EVAL_BLOCKS - 1;
    v.doa_tick(&bus);
    assert_eq!(v.last_bin, None, "单次观测不转向");
    v.doa_blocks = DOA_EVAL_BLOCKS - 1;
    v.doa_tick(&bus);
    assert_eq!(v.last_bin, Some(Rr3DoaBin::Right));
    let b = bus.lock().unwrap();
    assert!(
        matches!(
            b.jobs.front(),
            Some(Job::Move {
                pan: Some(PAN_RIGHT_DEG),
                ..
            })
        ),
        "右档应下发 pan={PAN_RIGHT_DEG}"
    );
}

#[test]
fn onset_center_steers_to_rest_pan() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    v.pre = stereo_same(9600, 3000);
    v.recording = true;
    assert!(v.steer(&bus, "onset"), "中档应有确定落位");
    assert_eq!(v.last_bin, Some(Rr3DoaBin::Center));
    let b = bus.lock().unwrap();
    assert!(matches!(
        b.jobs.front(),
        Some(Job::Move {
            pan: Some(PAN_CENTER_DEG),
            ..
        })
    ));
}

#[test]
fn low_confidence_doa_does_not_steer() {
    let bus = bus();
    let mut v = VoiceRun::rr3_new_mode(Rr3VoiceMode::Mem);
    v.pre = stereo_indep(9600, 3000);
    assert!(!v.steer(&bus, "onset"), "独立噪声置信度不足");
    assert_eq!(v.last_bin, None);
    assert!(bus.lock().unwrap().jobs.is_empty());
}
