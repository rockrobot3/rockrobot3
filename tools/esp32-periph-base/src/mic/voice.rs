//! 本地语音分段（W6-3）：I2S0 双麦帧 → DC/增益 → 20 ms 块 VAD + DoA →
//! 语音段（PSRAM 堆缓冲）→ dialog 队列。
//!
//! 机制复用（零第二份算法）：块能量/时差判向走 [`rr_audio_mech`]（VAD 状态机 +
//! TDOA DoA），DC 阻断与增益钳位走 capture 机制件。本件只编排"何时开录 / 何时
//! 收段 / DoA 如何落 pan"。
//!
//! 半双工（W4-2 语义）：`gated`（播放面 `tts_gate`）期间整段重置——扬声器回授
//! 不进 VAD，也不进云端 ASR。
//!
//! 判据参数取 Pi 侧 esp32 档标定值（config-esp32.toml）：rms_start=500、
//! silence_tail=400 ms、min_speech=200 ms、DoA 阈值 30°、麦间距 150 mm。

use std::sync::{Arc, Mutex};

use rr_audio_mech::{
    rr3_doa_fix, rr3_frame_rms, rr3_pcm_best_channel, Rr3DoaBin, Rr3DoaCfg, Rr3VadConfig,
    Rr3VadEdge, Rr3VadState,
};

use crate::board::MIC_SR_HZ;
use crate::capture::{chunk_to_pcm, frame32_to_pair, Rr3DcBlocker};
use crate::state::{lock_bus, push_job, rr3_expr_set, Bus, Job};

/// VAD 判据（2026-11-20 S3 单板近讲实测订正：底噪 370–400，用户正常音量
/// 2 s 窗均值 499，650 阈值全程不起音；下调到 500。若环境噪声触发误起音，
/// 再按 `voice mono_rms` 回填 550）。
const VAD_CFG: Rr3VadConfig = Rr3VadConfig {
    rms_start: 500,
    silence_tail_ms: 400,
    min_speech_ms: 200,
};
/// 判向参数（与 Pi 侧同源；基线 = 双麦物理间距）。
const DOA_CFG: Rr3DoaCfg = Rr3DoaCfg {
    sample_rate_hz: MIC_SR_HZ,
    frame_ms: 20,
    mic_baseline_mm: 150,
    window_rms_min: 500,
    bin_threshold_deg: 30,
};
/// 采集块节拍（20 ms）与每块采样数（320 帧 × 2 声道）。
const FRAME_MS: u64 = 20;
const BLOCK_SAMPLES: usize = (MIC_SR_HZ as usize / 1000) * 20 * 2;
/// 预滚窗：VAD 起音至少 200 ms 才发 Onset；预滚 **600 ms**（30 块）确保
/// "你好洛克"这类唤醒前缀在起音偏晚时仍能进段（2026-11-21 真板：10 块预滚下
/// ASR 曾把"你好洛克，今天天气怎么样"的唤醒前缀整段丢掉）。
const PRE_ROLL_BLOCKS: usize = 30;
/// 单段上限：8 s（与云 TTS 上限同量级；超长强制收段，不无限攒）。
const MAX_UTTER_MS: usize = 8_000;
const MAX_UTTER_SAMPLES: usize = (MIC_SR_HZ as usize / 1000) * MAX_UTTER_MS * 2;
/// DoA 左右落 pan：**收进 70/110**（与意图三档同值）。2026-11-20 真板实锤：
/// 右转落 120（机械行程上限）时舵机顶死堵转，300 s 内 13 次 BOD；左 60 偶发但
/// 同样贴边。留 10° 机械余量，方向判据不变（left/center/right 分档独立）。
const PAN_LEFT_DEG: u32 = 70;
const PAN_RIGHT_DEG: u32 = 110;

/// 语音分段运行态（单线程：mic 采集线程独占）。
pub(crate) struct VoiceRun {
    vad: Rr3VadState,
    dc: Rr3DcBlocker,
    /// 本 read 的 24bit 原始帧（交错 L,R；复用容量）。
    raw: Vec<i32>,
    /// 未满 20 ms 的 s16 立体声样本（交错）。
    pending: Vec<i16>,
    /// 滚动预滚窗（立体声交错）。
    pre: Vec<i16>,
    /// 当前语音段（立体声交错；起音时用预滚窗补头）。
    utter: Vec<i16>,
    recording: bool,
    /// PSRAM 在位数（false = 本地分段停用，不拿内部 SRAM 硬撑 512 KB 段缓冲）。
    enabled: bool,
    /// 入队失败计数（dialog 未装配/队列满；串口抽样点名）。
    dropped: u32,
    /// 诊断窗（100 块 = 2 s）：VAD 域 mono RMS 均值 + 最近一次播放门状态。
    blocks: u32,
    rms_acc: u64,
    last_gate: bool,
}

impl VoiceRun {
    pub(crate) fn new() -> Self {
        let enabled = crate::psram::rr3_psram_total() > 0;
        if !enabled {
            println!("[mic] PSRAM 未启用：本地 VAD/分段停用（8s 语音段无法落内部 SRAM；S3 查 CONFIG_SPIRAM，classic 见 W7 §20.2）");
        }
        let (raw, pending, pre, utter) = if enabled {
            (
                crate::psram::rr3_psram_vec(2048),
                crate::psram::rr3_psram_vec(BLOCK_SAMPLES * 2),
                crate::psram::rr3_psram_vec(PRE_ROLL_BLOCKS * BLOCK_SAMPLES + BLOCK_SAMPLES),
                crate::psram::rr3_psram_vec(MAX_UTTER_SAMPLES + BLOCK_SAMPLES),
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };
        crate::mem::rr3_heap_mark("mic-voice", if enabled { "psram" } else { "off" });
        Self {
            vad: Rr3VadState::rr3_vad_new(VAD_CFG, FRAME_MS),
            dc: Rr3DcBlocker::new(),
            raw,
            pending,
            pre,
            utter,
            recording: false,
            enabled,
            dropped: 0,
            blocks: 0,
            rms_acc: 0,
            last_gate: false,
        }
    }

    /// 一次 I2S read：原始帧 → DC/增益 → 20 ms 块 → VAD/DoA/分段。
    pub(crate) fn batch(&mut self, buf: &[u8], nbytes: usize, bus: &Arc<Mutex<Bus>>, gated: bool) {
        self.last_gate = gated;
        if gated || !self.enabled {
            self.reset();
            return;
        }
        self.raw.clear();
        for f in 0..nbytes / 8 {
            let o = f * 8;
            let (l, r) = frame32_to_pair(&buf[o..o + 8]);
            self.raw.push(l);
            self.raw.push(r);
        }
        let pcm = chunk_to_pcm(&self.raw, crate::board::MIC_GAIN, &mut self.dc);
        self.pending.extend(
            pcm.chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]])),
        );
        while self.pending.len() >= BLOCK_SAMPLES {
            let mut block = [0i16; BLOCK_SAMPLES];
            block.copy_from_slice(&self.pending[..BLOCK_SAMPLES]);
            self.pending.drain(..BLOCK_SAMPLES);
            self.block(&block, bus);
        }
    }

    /// 单块：预滚 → VAD 边沿（起音开录 / 判停收段）→ 录音累积。
    fn block(&mut self, block: &[i16], bus: &Arc<Mutex<Bus>>) {
        let mono = rr3_pcm_best_channel(block, 2);
        self.voice_stats(&mono);
        let edge = self.vad.rr3_vad_feed(&mono);
        self.pre_extend(block);
        match edge {
            Some(Rr3VadEdge::Onset { .. }) => self.onset(bus),
            Some(Rr3VadEdge::Offset { .. }) => self.finish(bus, "offset"),
            None if self.recording => self.append(block, bus),
            None => {}
        }
    }

    /// VAD 域诊断窗（100 块 = 2 s 一行）：mono RMS 均值与播放门状态，供现场
    /// 标定 rms_start / 排查"麦克风有没有进 VAD"。
    fn voice_stats(&mut self, mono: &[i16]) {
        self.blocks = self.blocks.wrapping_add(1);
        self.rms_acc += rr3_frame_rms(mono).max(0) as u64;
        if self.blocks % 100 != 0 {
            return;
        }
        let avg = self.rms_acc / 100;
        self.rms_acc = 0;
        println!(
            "[mic] voice mono_rms={avg} gate={} blocks={}",
            self.last_gate, self.blocks
        );
    }

    /// 起音：预滚窗作段头（含本块）→ DoA 转头/聆听表情。
    fn onset(&mut self, bus: &Arc<Mutex<Bus>>) {
        self.recording = true;
        self.utter.clear();
        self.utter.extend_from_slice(&self.pre);
        let steered = self.steer(bus);
        rr3_expr_set(bus, if steered { "turn" } else { "listen" });
        println!(
            "[mic] VAD onset（预滚 {}ms）doa={}",
            self.utter.len() * 1000 / (MIC_SR_HZ as usize * 2),
            if steered { "steer" } else { "center/none" }
        );
    }

    /// 录音累积 + 超长强制收段（定容缓冲：先截余量再 push，绝不触发 realloc）。
    fn append(&mut self, block: &[i16], bus: &Arc<Mutex<Bus>>) {
        let room = MAX_UTTER_SAMPLES.saturating_sub(self.utter.len());
        if block.len() > room {
            self.utter.extend_from_slice(&block[..room]);
            self.finish(bus, "max-utterance");
            return;
        }
        self.utter.extend_from_slice(block);
        if self.utter.len() >= MAX_UTTER_SAMPLES {
            self.finish(bus, "max-utterance");
        }
    }

    /// 收段：双麦取能量高通道 → 单声道 16 kHz → dialog 队列；VAD 重置。
    fn finish(&mut self, bus: &Arc<Mutex<Bus>>, why: &str) {
        let mono = rr3_pcm_best_channel(&self.utter, 2);
        let ms = mono.len() * 1000 / MIC_SR_HZ as usize;
        self.recording = false;
        self.utter.clear();
        self.vad = Rr3VadState::rr3_vad_new(VAD_CFG, FRAME_MS);
        if mono.is_empty() {
            return;
        }
        // 标定保持模式（oled_hold）：只做 VAD/DoA 提示，不进 dialog——避免云端
        // 往返/播放打断 3×10 的节奏，也避免回复声反馈污染下一轮判向。
        if lock_bus(bus).st.oled_hold {
            println!("[mic] 语音段 {ms}ms（{why}；hold 模式跳过 dialog）");
            return;
        }
        match crate::cloud::rr3_turn(mono) {
            Ok(()) => println!("[mic] 语音段入队 {ms}ms（{why}）"),
            Err(e) => {
                self.dropped = self.dropped.saturating_add(1);
                if self.dropped == 1 || self.dropped % 20 == 0 {
                    println!(
                        "[mic] 语音段入队失败 {ms}ms（{why}）: {e}（累计 {}）",
                        self.dropped
                    );
                }
            }
        }
        rr3_expr_set(bus, "think");
    }

    /// DoA 落 pan：左/右档推 Move（中档不动）；返回是否发生转向。
    fn steer(&self, bus: &Arc<Mutex<Bus>>) -> bool {
        let Some(fix) = rr3_doa_fix(&self.pre, 2, &DOA_CFG) else {
            return false;
        };
        let pan = match fix.bin {
            Rr3DoaBin::Left => PAN_LEFT_DEG,
            Rr3DoaBin::Right => PAN_RIGHT_DEG,
            Rr3DoaBin::Center => {
                // 中档也点名角度，供现场判断"站得不够偏"还是算法判错。
                println!("[mic] DoA bin=center deg={}", fix.deg);
                return false;
            }
        };
        lock_bus(bus).st.wander_on = false; // 外部点名优先，自主扫摆让位
        push_job(
            bus,
            Job::Move {
                pan: Some(pan),
                tilt: None,
            },
        );
        println!(
            "[mic] DoA bin={} deg={} → pan={pan}",
            fix.bin.rr3_doa_bin_as_str(),
            fix.deg
        );
        true
    }

    /// 预滚窗滚动（上限 `PRE_ROLL_BLOCKS` 块）。
    fn pre_extend(&mut self, block: &[i16]) {
        self.pre.extend_from_slice(block);
        let cap = PRE_ROLL_BLOCKS * BLOCK_SAMPLES;
        if self.pre.len() > cap {
            let drop = self.pre.len() - cap;
            self.pre.drain(..drop);
        }
    }

    /// 半双工/断流重置：VAD 与缓冲全部清零，回到未起音态。
    fn reset(&mut self) {
        if !self.recording && self.pending.is_empty() && self.pre.is_empty() {
            return;
        }
        self.recording = false;
        self.vad = Rr3VadState::rr3_vad_new(VAD_CFG, FRAME_MS);
        self.dc.reset();
        self.raw.clear();
        self.pending.clear();
        self.pre.clear();
        self.utter.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    use crate::state::PeriphState;

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
    fn constant_speech_opens_then_offset_closes() {
        let bus = bus();
        let mut v = VoiceRun::new();
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
        let mut v = VoiceRun::new();
        for _ in 0..10 {
            v.block(&block(3000), &bus);
        }
        let blocks = MAX_UTTER_SAMPLES / BLOCK_SAMPLES + 2;
        for _ in 0..blocks {
            v.block(&block(3000), &bus);
        }
        assert!(!v.recording, "超过 8 s 必须强制收段");
        assert!(v.utter.is_empty());
    }

    #[test]
    fn gated_batch_resets_state() {
        let bus = bus();
        let mut v = VoiceRun::new();
        for _ in 0..10 {
            v.block(&block(3000), &bus);
        }
        assert!(v.recording);
        v.batch(&[0u8; 8], 8, &bus, true);
        assert!(!v.recording);
        assert!(v.pre.is_empty() && v.utter.is_empty());
    }
}
