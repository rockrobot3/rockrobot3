// SPDX-License-Identifier: Apache-2.0
//! 本地语音分段：I2S0 双麦帧 → DC/增益 → 20 ms 块 VAD + DoA → 语音段（PSRAM 堆缓冲）
//! → dialog 队列。
//!
//! 机制复用（零第二份算法）：块能量/时差判向走 [`rr_audio_mech`]，DC 阻断与增益钳位
//! 走 capture 机制件；本件只编排"何时开录 / 何时收段 / DoA 如何落 pan"。
//!
//! 自适应判据：开机先收 128 块（≈2.56 s）语音域 mono RMS，中位数按 [`VAD_TUNE`] 倍率
//! 整定起音/收尾阈值，标定完成前不落段。阈值随环境底噪走，不依赖单次现场回填的绝对
//! 值；标定过低（无有效拾音）自动重采样。
//!
//! 周期判向：语音态内每 20 块（400 ms）重估 DoA，同档连续 2 次且置信度过门槛才转向——
//! 起音后长时间处于语音态或用户移动位置仍能跟随。半双工：`gated`（播放面 `tts_gate`）
//! 期间整段重置——扬声器回授不进 VAD，也不进云端 ASR。
//! 负载分工：本文件只放运行态与节拍；分段落盘在 [`seg`]、判向策略在 [`doa`]、开机标定在 [`cal`]。

use std::sync::{Arc, Mutex};

use rr_audio_mech::{
    rr3_chan_pick, rr3_chan_take, rr3_frame_rms, Rr3DoaBin, Rr3DoaCfg, Rr3DoaTrack, Rr3NoiseEst,
    Rr3VadConfig, Rr3VadEdge, Rr3VadState, Rr3VadTune, RR3_NOISE_FRAMES,
};

use crate::board::MIC_SR_HZ;
use crate::capture::{chunk_to_pcm, frame32_to_pair, Rr3DcBlocker};
use crate::state::Bus;

/// VAD 节拍参数（阈值是标定前的占位，开机标定后由 [`VAD_TUNE`] 覆盖）。
const VAD_BASE: Rr3VadConfig = Rr3VadConfig {
    rms_start: 450,
    rms_tail: 380,
    silence_tail_ms: 400,
    min_speech_ms: 200,
};
/// 阈值剖面：起音 1.5× 噪声中位、收尾 1.25×；下限保证数字静音也能起音，上限
/// 防高噪环境把阈值推到语音电平之上（[`rr3_vad_tune`] 夹取）。
const VAD_TUNE: Rr3VadTune = Rr3VadTune {
    start_q8: 384,
    tail_q8: 320,
    start_min: 450,
    start_max: 3000,
    tail_min: 380,
};
/// 判向参数（与 Pi 侧同源；基线 = 双麦物理间距）。
const DOA_CFG: Rr3DoaCfg = Rr3DoaCfg {
    sample_rate_hz: MIC_SR_HZ,
    frame_ms: 20,
    mic_baseline_mm: 150,
    window_rms_min: 450,
    bin_threshold_deg: 30,
    // 2026-11-23 真板：站板子左侧判 right、右侧判 left → 两麦通道序反向。
    sign: -1,
};
/// DoA 置信度门槛（Q15 ≈ 0.05）：独立双麦噪声的归一化峰值通常 <0.04，相干语音
/// 即使有混响也高于此线；门槛只压漫反射噪声，不误伤弱语音。实际值随 onset 行
/// 的 `conf=` 现场复核，必要时只动这一个策略量。
const DOA_CONF_MIN: i32 = 1_638;
/// 语音态判向节拍（20 块 = 400 ms）与稳定共识次数。
const DOA_EVAL_BLOCKS: u32 = 20;
const DOA_TRACK_NEED: u8 = 2;
/// 两次转向最小间隔（防抖；同档不重复下发）。
const STEER_GAP_MS: u64 = 1_200;
/// 标定底噪中位下限：低于此值判为无有效拾音，重开标定窗。
const CAL_MIN_MEDIAN: i64 = 60;
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
/// 单段上限：6 s（给云端 ASR 8 s 预算留上传余量；超长强制收段，不无限攒）。
const MAX_UTTER_MS: usize = 6_000;
const MAX_UTTER_SAMPLES: usize = (MIC_SR_HZ as usize / 1000) * MAX_UTTER_MS * 2;
/// spool 路径上限是单声道样本数（6 s × 16 kHz = 96 k 样本）。
const MAX_SEG_MONO: usize = MAX_UTTER_SAMPLES / 2;
/// DoA 落 pan：**收进 70/90/110**（与意图三档同值）。2026-11-20 真板实锤：右转
/// 落 120（机械行程上限）时舵机顶死堵转，300 s 内 13 次 BOD；左 60 偶发但同样
/// 贴边。留 10° 机械余量，中档回正 90（3×3 与"回到中间"都要有确定落位）。
const PAN_LEFT_DEG: u32 = 70;
const PAN_CENTER_DEG: u32 = 90;
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
    /// 当前生效判据（标定后替换；收段/重置用它重建状态机）。
    vad_cfg: Rr3VadConfig,
    /// 当前生效判向参数（`window_rms_min` 随标定后的起音阈值走）。
    doa_cfg: Rr3DoaCfg,
    /// 开机底噪标定窗。
    cal: Rr3NoiseEst,
    calibrated: bool,
    cal_tries: u32,
    /// 周期判向的稳定滤波与节拍。
    track: Rr3DoaTrack,
    doa_blocks: u32,
    last_bin: Option<Rr3DoaBin>,
    last_steer_ms: u64,
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
    /// 诊断窗（500 块 = 10 s）：VAD 域 mono RMS 均值 + 峰值 + 播放门状态。
    blocks: u32,
    rms_acc: u64,
    rms_peak: i64,
    last_gate: bool,
}

mod cal;
mod doa;
mod seg;

impl VoiceRun {
    pub(crate) fn new() -> Self {
        let mode = rr3_voice_mode();
        rr3_voice_mode_log(mode);
        let mut run = Self::rr3_new_cfg(mode);
        if mode != Rr3VoiceMode::Off {
            run.calibrated = false;
            println!("[mic] VAD 自标定：收 {RR3_NOISE_FRAMES} 块底噪（≈2.6s），期间不落段");
        }
        run
    }

    /// 固定判据构造（单测/无标定路径）；生产走 [`VoiceRun::new`]。
    #[cfg(test)]
    fn rr3_new_mode(mode: Rr3VoiceMode) -> Self {
        Self::rr3_new_cfg(mode)
    }

    /// 按承载方式定型（分配策略见 [`crate::psram`]）。
    fn rr3_new_cfg(mode: Rr3VoiceMode) -> Self {
        let pre_blocks = rr3_pre_blocks(mode);
        let (raw, pending, pre, utter) = rr3_bufs(mode, pre_blocks);
        crate::mem::rr3_heap_mark("mic-voice", rr3_mode_tag(mode));
        Self {
            vad: Rr3VadState::rr3_vad_new(VAD_BASE, FRAME_MS),
            vad_cfg: VAD_BASE,
            doa_cfg: DOA_CFG,
            cal: Rr3NoiseEst::rr3_new(),
            calibrated: true,
            cal_tries: 0,
            track: Rr3DoaTrack::rr3_new(DOA_TRACK_NEED),
            doa_blocks: 0,
            last_bin: None,
            last_steer_ms: 0,
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
            rms_peak: 0,
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

    /// 单块：能量择声道 → 底噪标定或 VAD 边沿 → 语音累积 → 周期判向。
    ///
    /// 每块只做一次能量择声道 + 一次抽取（`rr3_chan_pick/take`，零分配）：
    /// spool 路径起音后固定 `sel`，避免段内声道跳变。
    fn block(&mut self, block: &[i16], bus: &Arc<Mutex<Bus>>) {
        let mut mono = [0i16; BLK_MONO];
        let sel = self.block_sel(block);
        rr3_chan_take(block, 2, sel, &mut mono);
        let rms = self.voice_stats(&mono);
        self.pre_extend(block);
        if !self.calibrated {
            self.cal_tick(rms);
            return;
        }
        match self.vad.rr3_vad_feed(&mono) {
            Some(Rr3VadEdge::Onset { .. }) => self.onset(bus),
            Some(Rr3VadEdge::Offset { .. }) => self.finish(bus, "offset"),
            None => {
                if self.recording {
                    self.append(bus, &mono, block);
                }
            }
        }
        if self.recording {
            self.doa_tick(bus);
        }
    }

    /// 本块择声道：spool 起音后固定段内声道，其余按块能量择高。
    fn block_sel(&self, block: &[i16]) -> usize {
        if self.recording && self.mode == Rr3VoiceMode::Flash {
            self.sel
        } else {
            rr3_chan_pick(block, 2)
        }
    }

    /// VAD 域诊断窗（500 块 = 10 s 一行）：mono RMS 均值/峰值、播放门与当前
    /// 阈值，供现场判断"底噪多高 / 阈值标到哪 / 麦有没有进 VAD"。
    fn voice_stats(&mut self, mono: &[i16]) -> i64 {
        self.blocks = self.blocks.wrapping_add(1);
        let rms = rr3_frame_rms(mono).max(0);
        self.rms_acc += rms as u64;
        self.rms_peak = self.rms_peak.max(rms);
        if self.blocks % 500 != 0 {
            return rms;
        }
        let avg = self.rms_acc / 500;
        self.rms_acc = 0;
        let peak = std::mem::take(&mut self.rms_peak);
        println!(
            "[mic] voice mono_rms={avg} peak={peak} gate={} start={} tail={} blocks={}",
            self.last_gate, self.vad_cfg.rms_start, self.vad_cfg.rms_tail, self.blocks
        );
        rms
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
    /// 已完成的底噪标定保留（阈值跟环境走，不因一次播放回退）。
    fn reset(&mut self) {
        if self.seg {
            self.seg = false;
            crate::spool::rr3_spool_abort();
        }
        self.sent = 0;
        self.track.rr3_reset();
        self.last_bin = None;
        self.doa_blocks = 0;
        if !self.recording && self.pending.is_empty() && self.pre.is_empty() {
            return;
        }
        self.recording = false;
        self.vad = Rr3VadState::rr3_vad_new(self.vad_cfg, FRAME_MS);
        self.dc.reset();
        self.raw.clear();
        self.pending.clear();
        self.pre.clear();
        self.utter.clear();
    }
}

/// 预滚窗块数：spool 受内部 SRAM 约束只留起音窗（10 块），内存路径 30 块。
fn rr3_pre_blocks(mode: Rr3VoiceMode) -> usize {
    if mode == Rr3VoiceMode::Flash {
        PRE_ROLL_SPOOL
    } else {
        PRE_ROLL_MEM
    }
}

/// 按承载方式分配四块缓冲（PSRAM 优先；Off 全空）。
fn rr3_bufs(mode: Rr3VoiceMode, pre_blocks: usize) -> (Vec<i32>, Vec<i16>, Vec<i16>, Vec<i16>) {
    match mode {
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
    }
}

/// 承载方式诊断标签（`rr3_heap_mark` 用）。
fn rr3_mode_tag(mode: Rr3VoiceMode) -> &'static str {
    match mode {
        Rr3VoiceMode::Mem => "psram",
        Rr3VoiceMode::Flash => "spool",
        Rr3VoiceMode::Off => "off",
    }
}

/// 承载方式探测：PSRAM 优先，其次 flash spool，两者都没有则停用分段。
fn rr3_voice_mode() -> Rr3VoiceMode {
    if crate::psram::rr3_psram_total() > 0 {
        Rr3VoiceMode::Mem
    } else if crate::spool::rr3_spool_avail() {
        Rr3VoiceMode::Flash
    } else {
        Rr3VoiceMode::Off
    }
}

/// 启动一行点名承载方式（排障入口：看这一行即知走内存还是 spool）。
fn rr3_voice_mode_log(mode: Rr3VoiceMode) {
    match mode {
        Rr3VoiceMode::Mem => println!("[mic] 语音段走 PSRAM 内存缓冲（600ms 预滚）"),
        Rr3VoiceMode::Flash => println!(
            "[mic] PSRAM 未启用 → 语音段流式落 spool（200ms 预滚；上传期新段丢弃并点名）"
        ),
        Rr3VoiceMode::Off => println!(
            "[mic] 本地 VAD/分段停用：PSRAM 未启用且 spool 未装配（classic 看 [spool] 的堆预留/分区行；S3 查 CONFIG_SPIRAM）"
        ),
    }
}

#[cfg(test)]
mod tests;
