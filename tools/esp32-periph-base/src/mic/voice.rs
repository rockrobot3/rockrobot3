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
    rr3_chan_pick, rr3_chan_take, rr3_doa_fix, rr3_frame_rms, rr3_pcm_best_channel, Rr3DoaBin,
    Rr3DoaCfg, Rr3VadConfig, Rr3VadEdge, Rr3VadState,
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
/// 采集块节拍（20 ms）与每块采样数（320 帧 × 2 声道 / 单声道）。
const FRAME_MS: u64 = 20;
const BLOCK_SAMPLES: usize = (MIC_SR_HZ as usize / 1000) * 20 * 2;
const BLK_MONO: usize = BLOCK_SAMPLES / 2;
/// spool 块长必须与写线程一致（两处常量同源，编译期钉死）。
const _: () = assert!(BLK_MONO == crate::spool::RR3_SPOOL_BLK);
/// 预滚窗：VAD 起音至少 200 ms 才发 Onset；内存路径预滚 **600 ms**（30 块）确保
/// "你好洛克"这类唤醒前缀在起音偏晚时仍能进段（2026-11-21 真板：10 块预滚下
/// ASR 曾把"你好洛克，今天天气怎么样"的唤醒前缀整段丢掉）。spool 路径受内部
/// SRAM 约束只留 **200 ms**（10 块 = 起音判定窗本身），多出的前缀余量由云端
/// 容错承担。
const PRE_ROLL_MEM: usize = 30;
const PRE_ROLL_SPOOL: usize = 10;
/// 单段上限：8 s（与云 TTS 上限同量级；超长强制收段，不无限攒）。
const MAX_UTTER_MS: usize = 8_000;
const MAX_UTTER_SAMPLES: usize = (MIC_SR_HZ as usize / 1000) * MAX_UTTER_MS * 2;
/// spool 路径上限是单声道样本数（8 s × 16 kHz = 128 k 样本）。
const MAX_SEG_MONO: usize = MAX_UTTER_SAMPLES / 2;
/// DoA 左右落 pan：**收进 70/110**（与意图三档同值）。2026-11-20 真板实锤：
/// 右转落 120（机械行程上限）时舵机顶死堵转，300 s 内 13 次 BOD；左 60 偶发但
/// 同样贴边。留 10° 机械余量，方向判据不变（left/center/right 分档独立）。
const PAN_LEFT_DEG: u32 = 70;
const PAN_RIGHT_DEG: u32 = 110;

/// 段承载方式：PSRAM 内存缓冲（S3）/ flash spool 流式（classic）/ 停用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rr3VoiceMode {
    Off,
    Mem,
    Flash,
}

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
    /// 当前语音段（内存路径；spool 路径不驻留整段）。
    utter: Vec<i16>,
    mode: Rr3VoiceMode,
    /// 预滚窗块数（内存 30 / spool 10）。
    pre_blocks: usize,
    recording: bool,
    /// spool 路径：起音定声道 / 已写单声道样本数 / 本段是否已开录。
    sel: usize,
    sent: usize,
    seg: bool,
    /// 弃段计数（spool 忙/写失败/入队失败；串口抽样点名）。
    dropped: u32,
    /// 诊断窗（100 块 = 2 s）：VAD 域 mono RMS 均值 + 最近一次播放门状态。
    blocks: u32,
    rms_acc: u64,
    last_gate: bool,
}

impl VoiceRun {
    pub(crate) fn new() -> Self {
        let mode = if crate::psram::rr3_psram_total() > 0 {
            Rr3VoiceMode::Mem
        } else if crate::spool::rr3_spool_avail() {
            Rr3VoiceMode::Flash
        } else {
            Rr3VoiceMode::Off
        };
        match mode {
            Rr3VoiceMode::Mem => {
                println!("[mic] 语音段走 PSRAM 内存缓冲（600ms 预滚）")
            }
            Rr3VoiceMode::Flash => println!(
                "[mic] PSRAM 未启用 → 语音段流式落 spool（200ms 预滚；上传期新段丢弃并点名）"
            ),
            Rr3VoiceMode::Off => println!(
                "[mic] 本地 VAD/分段停用：PSRAM 未启用且 spool 未装配（classic 看 [spool] 的堆预留/分区行；S3 查 CONFIG_SPIRAM）"
            ),
        }
        Self::rr3_new_mode(mode)
    }

    /// 按承载方式定型（测试可显式选内存路径，不依赖宿主 PSRAM）。
    fn rr3_new_mode(mode: Rr3VoiceMode) -> Self {
        let pre_blocks = if mode == Rr3VoiceMode::Flash {
            PRE_ROLL_SPOOL
        } else {
            PRE_ROLL_MEM
        };
        let (raw, pending, pre, utter) = match mode {
            Rr3VoiceMode::Mem => (
                crate::psram::rr3_psram_vec(2048),
                crate::psram::rr3_psram_vec(BLOCK_SAMPLES * 2),
                crate::psram::rr3_psram_vec(pre_blocks * BLOCK_SAMPLES + BLOCK_SAMPLES),
                crate::psram::rr3_psram_vec(MAX_UTTER_SAMPLES + BLOCK_SAMPLES),
            ),
            Rr3VoiceMode::Flash => (
                crate::psram::rr3_psram_vec(2048),
                crate::psram::rr3_psram_vec(BLOCK_SAMPLES * 2),
                crate::psram::rr3_psram_vec(pre_blocks * BLOCK_SAMPLES + BLOCK_SAMPLES),
                Vec::new(),
            ),
            Rr3VoiceMode::Off => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        };
        crate::mem::rr3_heap_mark(
            "mic-voice",
            match mode {
                Rr3VoiceMode::Mem => "psram",
                Rr3VoiceMode::Flash => "spool",
                Rr3VoiceMode::Off => "off",
            },
        );
        Self {
            vad: Rr3VadState::rr3_vad_new(VAD_CFG, FRAME_MS),
            dc: Rr3DcBlocker::new(),
            raw,
            pending,
            pre,
            utter,
            mode,
            pre_blocks,
            recording: false,
            sel: 0,
            sent: 0,
            seg: false,
            dropped: 0,
            blocks: 0,
            rms_acc: 0,
            last_gate: false,
        }
    }

    /// 一次 I2S read：原始帧 → DC/增益 → 20 ms 块 → VAD/DoA/分段。
    pub(crate) fn batch(&mut self, buf: &[u8], nbytes: usize, bus: &Arc<Mutex<Bus>>, gated: bool) {
        self.last_gate = gated;
        if gated || self.mode == Rr3VoiceMode::Off {
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
    ///
    /// 每块只做一次能量择声道 + 一次抽取（`rr3_chan_pick/take`，零分配）：
    /// spool 路径起音后固定 `sel`，避免段内声道跳变。
    fn block(&mut self, block: &[i16], bus: &Arc<Mutex<Bus>>) {
        let mut mono = [0i16; BLK_MONO];
        let sel = if self.recording && self.mode == Rr3VoiceMode::Flash {
            self.sel
        } else {
            rr3_chan_pick(block, 2)
        };
        rr3_chan_take(block, 2, sel, &mut mono);
        self.voice_stats(&mono);
        let edge = self.vad.rr3_vad_feed(&mono);
        self.pre_extend(block);
        match edge {
            Some(Rr3VadEdge::Onset { .. }) => self.onset(bus),
            Some(Rr3VadEdge::Offset { .. }) => self.finish(bus, "offset"),
            None if self.recording => self.append(bus, &mono, block),
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

    /// 起音：DoA 转头/聆听表情 → 按承载方式开段（内存补预滚 / spool 逐块落盘）。
    ///
    /// `oled_hold`（标定保持）：只判向与出表情，不落段——避免云端往返/回授打断
    /// 3×10 的标定节奏，也避免无谓的 flash 写入。
    fn onset(&mut self, bus: &Arc<Mutex<Bus>>) {
        self.recording = true;
        let steered = self.steer(bus);
        rr3_expr_set(bus, if steered { "turn" } else { "listen" });
        let pre_ms = self.pre.len() * 1000 / (MIC_SR_HZ as usize * 2);
        let doa = if steered { "steer" } else { "center/none" };
        let hold = lock_bus(bus).st.oled_hold;
        if hold {
            self.recording = false;
            println!("[mic] VAD onset（预滚 {pre_ms}ms）doa={doa} hold=只判向不落段");
            return;
        }
        match self.mode {
            Rr3VoiceMode::Mem => {
                self.utter.clear();
                self.utter.extend_from_slice(&self.pre);
                println!("[mic] VAD onset（预滚 {pre_ms}ms）doa={doa} path=mem");
            }
            Rr3VoiceMode::Flash => {
                self.flash_open();
                println!(
                    "[mic] VAD onset（预滚 {pre_ms}ms）doa={doa} path=spool sel={} seg={}",
                    self.sel, self.seg
                );
            }
            Rr3VoiceMode::Off => self.recording = false,
        }
    }

    /// spool 开段：起音窗定声道（能量高者）→ 预滚逐块落盘；失败即弃段。
    fn flash_open(&mut self) {
        self.sel = rr3_chan_pick(&self.pre, 2);
        self.sent = 0;
        self.seg = crate::spool::rr3_spool_begin();
        if !self.seg {
            self.recording = false;
            self.dropped = self.dropped.saturating_add(1);
            println!(
                "[mic] spool 忙/未装配：弃段（累计 {}；上一段待上传或写线程退出）",
                self.dropped
            );
            return;
        }
        // 预滚窗临时移出：迭代期间还要可变借用 self 写 spool（Vec move 零拷贝），
        // 写完原样放回，下一段起音的预测窗从当前滚动位置继续。
        let pre = std::mem::take(&mut self.pre);
        for chunk in pre.chunks_exact(BLOCK_SAMPLES) {
            let mut mono = [0i16; BLK_MONO];
            rr3_chan_take(chunk, 2, self.sel, &mut mono);
            if !self.flash_push(&mono) {
                break;
            }
        }
        self.pre = pre;
    }

    /// spool 写一块（非阻塞）；失败即弃段并回报调用方停止累积。
    fn flash_push(&mut self, mono: &[i16; BLK_MONO]) -> bool {
        if !self.seg {
            return false;
        }
        if crate::spool::rr3_spool_block(mono) {
            self.sent += mono.len();
            return true;
        }
        self.seg = false;
        self.recording = false;
        self.dropped = self.dropped.saturating_add(1);
        println!("[mic] spool 写失败/队列满：弃段（累计 {}）", self.dropped);
        crate::spool::rr3_spool_abort();
        false
    }

    /// 录音累积 + 超长强制收段；spool 路径逐块落盘，不驻留整段。
    fn append(&mut self, bus: &Arc<Mutex<Bus>>, mono: &[i16; BLK_MONO], block: &[i16]) {
        if self.mode == Rr3VoiceMode::Flash {
            if !self.seg {
                self.recording = false;
                return;
            }
            if self.sent + mono.len() > MAX_SEG_MONO {
                self.finish(bus, "max-utterance");
                return;
            }
            self.flash_push(mono);
            return;
        }
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

    /// 收段：按承载方式交 dialog（内存）或提交 spool（写线程入云队列）。
    fn finish(&mut self, bus: &Arc<Mutex<Bus>>, why: &str) {
        if self.mode == Rr3VoiceMode::Flash {
            return self.finish_flash(bus, why);
        }
        let mono = rr3_pcm_best_channel(&self.utter, 2);
        let ms = mono.len() * 1000 / MIC_SR_HZ as usize;
        self.recording = false;
        self.utter.clear();
        self.vad = Rr3VadState::rr3_vad_new(VAD_CFG, FRAME_MS);
        if mono.is_empty() {
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

    /// spool 收段：提交记录（写线程校 CRC 后入云队列），本线程立即可录下一段。
    fn finish_flash(&mut self, bus: &Arc<Mutex<Bus>>, why: &str) {
        let ms = self.sent * 1000 / MIC_SR_HZ as usize;
        let seg = self.seg;
        self.seg = false;
        self.sent = 0;
        self.recording = false;
        self.vad = Rr3VadState::rr3_vad_new(VAD_CFG, FRAME_MS);
        if seg {
            if crate::spool::rr3_spool_finish() {
                println!("[mic] 语音段 {ms}ms 已提交 spool（{why}）");
            } else {
                self.dropped = self.dropped.saturating_add(1);
                println!(
                    "[mic] spool 提交投递失败 {ms}ms（{why}；累计 {}）",
                    self.dropped
                );
                crate::spool::rr3_spool_abort();
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

    /// 预滚窗滚动（上限 `pre_blocks` 块）。
    fn pre_extend(&mut self, block: &[i16]) {
        self.pre.extend_from_slice(block);
        let cap = self.pre_blocks * BLOCK_SAMPLES;
        if self.pre.len() > cap {
            let drop = self.pre.len() - cap;
            self.pre.drain(..drop);
        }
    }

    /// 半双工/断流重置：VAD 与缓冲全部清零，回到未起音态；spool 段显式弃掉。
    fn reset(&mut self) {
        if self.seg {
            self.seg = false;
            crate::spool::rr3_spool_abort();
        }
        self.sent = 0;
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
mod tests;
