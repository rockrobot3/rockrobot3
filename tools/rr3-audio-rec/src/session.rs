//! 会话编排：首连开口/握手、读/写/看门狗起子、收兵与汇总汇报。

use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use rr_ipc::audio_session::handshake;
use rr_ipc::Rr3UartConn;

use crate::decode::DecodeState;
use crate::reader::reader_run;
use crate::track::measured_rate;
use crate::watch::{watchdog_run, WatchState};
use crate::wav::rr3_head_write;
use crate::writer::writer_run;

/// 首连开口 + 握手（重连同款路径，见 handshake；开口翻转复位板子 → settle 后
/// 冲残留）。失败即退 1，文案与旧版逐字一致。
pub(crate) fn open_handshake(device: &str, baud: u32) -> Rr3UartConn {
    let mut conn = rr3_open_uart(device, baud);
    if let Err(e) = handshake(&mut conn) {
        eprintln!("rr3-audio-rec: 握手失败: {e}");
        std::process::exit(1);
    }
    conn
}

/// 首次开口（失败即退 1，文案与旧版逐字一致）。
fn rr3_open_uart(device: &str, baud: u32) -> Rr3UartConn {
    Rr3UartConn::open_with_baud(std::path::Path::new(device), baud).unwrap_or_else(|e| {
        eprintln!("rr3-audio-rec: 开口失败 {device}: {e}");
        std::process::exit(1)
    })
}

/// 三线程与通道面（起子产物；收兵按 reader → writer 顺序收口，见 finish_and_report）。
pub(crate) struct RecThreads {
    /// 解码面 → 写线程的分片通道（收兵 drop 触发写线程排空退出）。
    pub(crate) tx: mpsc::Sender<Vec<u8>>,
    /// 读线程 → 解码面的裸字节通道。
    pub(crate) byte_rx: mpsc::Receiver<Vec<u8>>,
    /// 停流旗（置位后读线程发 AUDIO OFF 收兵退出）。
    stop: Arc<AtomicBool>,
    writer: std::thread::JoinHandle<()>,
    reader: std::thread::JoinHandle<()>,
}

/// 三线程起子（读/写/看门狗）：conn 整体移交读线程裸读、wav 文件移交写线程
/// 独占、watch 按线程克隆；武装旗置位归主线程。
pub(crate) fn rec_spawn_threads(
    conn: Rr3UartConn,
    wav_file: File,
    watch: Arc<WatchState>,
    out_path: &std::path::Path,
    device: &str,
    baud: u32,
) -> RecThreads {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    // 写线程独占 wav 文件（占位头/逐片写透全在此线程）；mpsc 解耦见 writer_run。
    let writer = {
        let w = Arc::clone(&watch);
        let out = out_path.to_path_buf();
        std::thread::spawn(move || writer_run(wav_file, rx, w, out))
    };
    rr3_spawn_watch(Arc::clone(&watch), out_path);
    // 读线程：conn/裸字节通道/停流旗移交（静默重连与收兵 OFF 全在读侧闭环，
    // 见 reader_run）。
    let (byte_tx, byte_rx) = mpsc::channel::<Vec<u8>>();
    let stop = Arc::new(AtomicBool::new(false));
    let reader = rr3_spawn_reader(
        conn,
        byte_tx,
        Arc::clone(&stop),
        Arc::clone(&watch),
        device,
        baud,
        out_path,
    );
    RecThreads {
        tx,
        byte_rx,
        stop,
        writer,
        reader,
    }
}

/// 读线程起子：conn/裸字节通道/停流旗移交（静默重连与收兵 OFF 全在读侧闭环，
/// 见 reader_run）。
fn rr3_spawn_reader(
    conn: Rr3UartConn,
    byte_tx: mpsc::Sender<Vec<u8>>,
    stop: Arc<AtomicBool>,
    watch: Arc<WatchState>,
    device: &str,
    baud: u32,
    out_path: &std::path::Path,
) -> std::thread::JoinHandle<()> {
    let dev = device.to_string();
    let out = out_path.to_path_buf();
    std::thread::spawn(move || reader_run(conn, byte_tx, stop, watch, dev, baud, out))
}

/// 看门狗线程（武装后停滞即补 wav 恢复头退 3）。
fn rr3_spawn_watch(watch: Arc<WatchState>, out_path: &std::path::Path) {
    let path = out_path.to_string_lossy().into_owned();
    std::thread::spawn(move || watchdog_run(watch, path));
}

/// 收兵第一段：停流旗 → 读线程退场 → 解武装 → 关通道让写线程排空；
/// 任一线程异常即整进程退，不冒领成功。
fn finish_threads(threads: RecThreads, watch: &WatchState) {
    threads.stop.store(true, Ordering::Release);
    rr3_join_reader(threads.reader);
    watch.armed.store(false, Ordering::Release);
    // 关通道让写线程排空收兵（逐片已 flush，无尾数据）；写线程异常即整进程退，
    // 不冒领成功。
    drop(threads.tx);
    if let Err(e) = threads.writer.join() {
        eprintln!("rr3-audio-rec: 写线程异常退出: {e:?}");
        std::process::exit(1);
    }
}

/// 读线程收口（异常即整进程退，不冒领成功）。
fn rr3_join_reader(reader: std::thread::JoinHandle<()>) {
    if reader.join().is_err() {
        eprintln!("rr3-audio-rec: 读线程异常退出");
        std::process::exit(1);
    }
}

/// 收兵 + 汇总：停流旗 → 读线程发 AUDIO OFF 退出（板面队列余片照发，本端不再
/// 读，conn 随线程收口关闭）。OFF 后解武装；关通道让写线程排空收兵；最后按
/// 实测率补真头并打印汇总。任一线程异常即整进程退，不冒领成功。
pub(crate) fn finish_and_report(
    threads: RecThreads,
    watch: &WatchState,
    st: DecodeState,
    out_path: &std::path::Path,
    doa: bool,
) {
    finish_threads(threads, watch);
    rr3_finish_zero(&st, out_path);
    rr3_finish_summary(&st, watch, out_path, doa);
}

/// 收兵汇总：实测率补真头 + 总数行（`--doa` 再打方位角汇总）。
fn rr3_finish_summary(st: &DecodeState, watch: &WatchState, out_path: &std::path::Path, doa: bool) {
    let measured = measured_rate(
        st.rate_anchor,
        st.last_frame,
        st.total_bytes / 4,
        st.declared_rate,
    );
    rr3_finish_head(out_path, st, watch, measured);
    if doa {
        println!("{}", st.doa.summary());
    }
}

/// 0 分片收口：点名退出 1，占位空壳不留垃圾。
fn rr3_finish_zero(st: &DecodeState, out_path: &std::path::Path) {
    if st.total_bytes == 0 {
        eprintln!("[rec] 0 分片（查板面 AUDIO ON 应答/I2S 面波特率档）");
        let _ = std::fs::remove_file(out_path); // 占位空壳不留垃圾
        std::process::exit(1);
    }
}

/// 实测率终值补真头 + 收兵汇总（写失败即整进程退）。
fn rr3_finish_head(
    out_path: &std::path::Path,
    st: &DecodeState,
    watch: &WatchState,
    measured: u32,
) {
    let data_len = watch.disk_bytes.load(Ordering::Acquire) as u32;
    rr3_head_write(out_path, data_len, measured);
    println!(
        "[rec] ✓ {}：chunks={} gaps={} restarts={} err={} resyncs={} reconnects={} frames={} rate={}Hz（实测连续段）dur≈{:.1}s size={data_len}B",
        out_path.display(),
        st.chunks,
        st.tracker.gaps,
        st.tracker.restarts,
        st.errors,
        st.resyncs,
        watch.reconnects.load(Ordering::Acquire),
        st.total_bytes / 4,
        measured,
        st.total_bytes as f64 / 4.0 / measured as f64,
    );
}
