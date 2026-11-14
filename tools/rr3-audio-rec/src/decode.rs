//! 解码面：DecodeState 滚动账、拆帧出尽、四道门与坏帧/重同步记账。

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Instant;

use rr_ipc::{FrameAssembler, Rr3Frame};

use crate::event::{parse_audio_event, AudioChunk};
use crate::exit::bail_with_recovery;
use crate::track::{account_chunk, rate_progress, DoaTracker, SeqTracker};
use crate::watch::WatchState;
use crate::{DECODE_TICK, ERR_STREAK_RESYNC};

/// 解码主循环的滚动账（主线程独占；`watch` 是跨线程共享原子面，不在此）。
pub(crate) struct DecodeState {
    pub(crate) tracker: SeqTracker,
    pub(crate) doa: DoaTracker,
    /// 板侧**声明**率（流内 `rate` 字段）：wav 头标称值与实测率退化兜底的事实源，
    /// 本地硬编码不兜底——板面换采样档后宿主静默写错头。
    pub(crate) declared_rate: Option<u32>,
    /// 连续段实测率锚 (锚时刻, 锚时帧数)：断档/重启/首片即重新锚定——断流空洞
    /// 不摊进实测率分母。重连/失步必然丢片（seq 断档或重启）→ 全部重锚场景覆盖。
    pub(crate) rate_anchor: Option<(Instant, u64)>,
    pub(crate) chunks: u64,
    pub(crate) total_bytes: u64,
    pub(crate) last_frame: Option<Instant>,
    pub(crate) errors: u64,
    /// 连续拆帧失败计数（好片即清零；达 [`ERR_STREAK_RESYNC`] 记重同步账并清零）。
    err_streak: u64,
    /// 失步重同步账（成片损坏点名次数；汇总打印用）。
    pub(crate) resyncs: u64,
    doa_on: bool,
}

impl DecodeState {
    pub(crate) fn new(doa_on: bool) -> Self {
        Self {
            tracker: SeqTracker {
                prev: None,
                gaps: 0,
                restarts: 0,
            },
            doa: DoaTracker::new(),
            declared_rate: None,
            rate_anchor: None,
            chunks: 0,
            total_bytes: 0,
            last_frame: None,
            errors: 0,
            err_streak: 0,
            resyncs: 0,
            doa_on,
        }
    }

    /// DoA 冒烟：开则喂片，满窗即打印一行。
    fn rr3_doa_smoke(&mut self, samples: &[u8]) {
        if self.doa_on {
            if let Some(line) = self.doa.feed(samples) {
                println!("{line}");
            }
        }
    }

    /// 好片全链：seq/声明率记账 → DoA 冒烟 → 移交写线程（零磁盘 I/O）→ 连续段
    /// 实测率推进 → 节拍打印。写线程亡 = fail-honest 补恢复头退 4。
    fn ingest(
        &mut self,
        chunk: AudioChunk,
        src_task: u64,
        errors: u64,
        watch: &WatchState,
        tx: &mpsc::Sender<Vec<u8>>,
        out_path: &std::path::Path,
    ) {
        let now = Instant::now();
        self.last_frame = Some(now);
        let broke = account_chunk(
            chunk.seq,
            chunk.rate,
            &mut self.tracker,
            &mut self.declared_rate,
            watch,
        );
        self.chunks += 1;
        // DoA 在解码面算（纯计算）；分片移交写线程
        self.rr3_doa_smoke(&chunk.samples);
        let nsamp = chunk.samples.len();
        if tx.send(chunk.samples).is_err() {
            eprintln!("rr3-audio-rec: 写线程通道断开（写线程已亡）");
            bail_with_recovery(out_path, watch);
        }
        self.total_bytes += nsamp as u64;
        watch.ingested.store(self.total_bytes, Ordering::Release);
        let frames = self.total_bytes / 4;
        rate_progress(&mut self.rate_anchor, broke, now, frames, watch);
        self.rr3_ingest_tick(errors, src_task, chunk.seq, chunk.ms);
    }

    /// 每 8 片一节拍留痕（chunk/seq/ms/bytes/gaps/restarts）。
    fn rr3_ingest_tick(&self, errors: u64, src_task: u64, seq: u32, ms: u64) {
        if self.chunks % 8 == 0 {
            println!(
                "[rec] chunk={} seq={seq} ms={ms} bytes={} gaps={} restarts={} err={errors} src_task={src_task}",
                self.chunks, self.total_bytes, self.tracker.gaps, self.tracker.restarts
            );
        }
    }
}

/// 解码主循环：裸字节通道节拍收料 → 内存拆帧出尽 → dst/Event/topic/b64 四道门
/// → 好片移交写线程。坏帧弃扫即时完成（Err 起点已弃，续扫由下次 pull 接上），
/// 成片损坏按 [`ERR_STREAK_RESYNC`] 记失步重同步账。
pub(crate) fn decode_loop(
    byte_rx: &mpsc::Receiver<Vec<u8>>,
    asm: &mut FrameAssembler,
    st: &mut DecodeState,
    deadline: Instant,
    watch: &WatchState,
    tx: &mpsc::Sender<Vec<u8>>,
    out_path: &std::path::Path,
) {
    while Instant::now() < deadline {
        decode_feed(byte_rx, asm, watch, out_path);
        decode_drain(asm, st, watch, tx, out_path);
    }
}

/// 一拍收料：节拍取一块裸字节喂拆帧器（超时=空拍；通道断=读线程已亡）。
fn decode_feed(
    byte_rx: &mpsc::Receiver<Vec<u8>>,
    asm: &mut FrameAssembler,
    watch: &WatchState,
    out_path: &std::path::Path,
) {
    match byte_rx.recv_timeout(DECODE_TICK) {
        Ok(bytes) => asm.feed(&bytes),
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            eprintln!("[rec] 裸字节通道断开（读线程已亡）");
            bail_with_recovery(out_path, watch);
        }
    }
}

/// 出尽成帧：坏帧弃扫记账、好片交写线程，成片损坏记一次失步重同步账。
fn decode_drain(
    asm: &mut FrameAssembler,
    st: &mut DecodeState,
    watch: &WatchState,
    tx: &mpsc::Sender<Vec<u8>>,
    out_path: &std::path::Path,
) {
    while rr3_drain_once(asm, st, watch, tx, out_path) {}
}

/// 一拍出尽：false = 拆帧器已空（调用方收工）。
fn rr3_drain_once(
    asm: &mut FrameAssembler,
    st: &mut DecodeState,
    watch: &WatchState,
    tx: &mpsc::Sender<Vec<u8>>,
    out_path: &std::path::Path,
) -> bool {
    match asm.pull() {
        Ok(Some((header, payload))) => {
            decode_frame(Rr3Frame { header, payload }, st, watch, tx, out_path);
        }
        Ok(None) => return false,
        Err((dropped, why)) => {
            st.errors += 1;
            st.err_streak += 1;
            eprintln!("[rec] 帧头拒收（弃 {dropped}B 续扫）: {why}");
        }
    }
    // 失步重同步账（三级防护第 ② 级的记账面）：坏帧弃扫已在内存即时
    // 完成，这里只把成片损坏点名为日志（丢片由 seq 对账如实记账）。
    rr3_resync_note(st, asm);
    true
}

/// 连续拆帧失败达阈值即记一次失步重同步账并清零。
fn rr3_resync_note(st: &mut DecodeState, asm: &FrameAssembler) {
    if st.err_streak >= ERR_STREAK_RESYNC {
        st.resyncs += 1;
        st.err_streak = 0;
        eprintln!(
            "[rec] 失步重同步 #{}：内存弃扫 {}B 后对齐续收",
            st.resyncs,
            asm.discarded()
        );
    }
}

/// 单帧四道门：全过即交写线程；拒收计错误与连续失败账。
fn decode_frame(
    frame: Rr3Frame,
    st: &mut DecodeState,
    watch: &WatchState,
    tx: &mpsc::Sender<Vec<u8>>,
    out_path: &std::path::Path,
) {
    match parse_audio_event(&frame) {
        Ok(chunk) => {
            st.err_streak = 0;
            st.ingest(chunk, frame.header.src_task, st.errors, watch, tx, out_path);
        }
        Err(e) => rr3_frame_reject(st, &e),
    }
}

/// 拒收记账：错误数 + 连续失败数推进，并点名拒因。
fn rr3_frame_reject(st: &mut DecodeState, e: &str) {
    st.errors += 1;
    st.err_streak += 1;
    eprintln!("[rec] 事件门拒收: {e}");
}
