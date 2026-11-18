//! 读线程：大块裸读推裸字节通道；静默判据与就地重连闭环。

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use rr_ipc::audio_session::{
    handshake, HANDSHAKE_RETRY_PAUSE, HANDSHAKE_TRIES, READ_BLOCK, READ_TIMEOUT_MS,
    RESYNC_ZERO_BAIL,
};
use rr_ipc::Rr3UartConn;

use crate::exit::bail_with_recovery;
use crate::watch::WatchState;

/// 读线程（收流端机制面，判据② A/B 定案改造）：**只做大块裸读**——单次
/// [`Rr3UartConn::rr3_read_timeout`]（poll 等就绪 + read 一次 [`READ_BLOCK`] 窗）
/// 整块推进裸字节通道，分相读的 128B 头/plen 载荷两段间不再让出节奏（A/B
/// 决定性实验：python read(65536) 同款节奏 600s 满速零污染；分相读同等链路
/// 2-16s 起逐帧损坏风暴——病灶是读路径节奏，不是链路也不是板）。
///
/// 静默检测与重连同线程闭环（证据面在读侧）：见过数据后连续
/// [`RESYNC_ZERO_BAIL`] 次零读（1.6s 连续静默 = 宿主侧 USB RX 冻结实锤形状）
/// → 就地重开口 + 握手（[`reader_reconnect`]，[HANDSHAKE_TRIES] 次），武装旗
/// 骑行期解武装（看门狗不计停滞）。停流旗置位即发 AUDIO OFF 收兵（收兵语义
/// 与旧版同序：先停板面流，写线程排空由主线程收口）。
pub(crate) fn reader_run(
    mut conn: Rr3UartConn,
    tx: mpsc::Sender<Vec<u8>>,
    stop: Arc<AtomicBool>,
    watch: Arc<WatchState>,
    device: String,
    baud: u32,
    out_path: PathBuf,
) {
    let mut buf = vec![0u8; READ_BLOCK];
    let mut seen_data = false; // 本段（首连/重连后）见过数据才算真静默
    let mut zeros = 0u32;
    loop {
        if stop.load(Ordering::Acquire) {
            let _ = conn.write_all(b"AUDIO OFF\n");
            return;
        }
        match conn.rr3_read_timeout(&mut buf, READ_TIMEOUT_MS) {
            Ok(n) if n > 0 => {
                seen_data = true;
                zeros = 0;
                if tx.send(buf[..n].to_vec()).is_err() {
                    return; // 解码面已亡（主线程先退），主线程/看门狗收口
                }
            }
            _ => rr3_reader_gap(
                &mut conn,
                &mut seen_data,
                &mut zeros,
                &device,
                baud,
                &watch,
                &out_path,
            ),
        }
    }
}

/// 零读分支：静默计数与就地重连；重连成功后复位本段计数。
fn rr3_reader_gap(
    conn: &mut Rr3UartConn,
    seen: &mut bool,
    zeros: &mut u32,
    device: &str,
    baud: u32,
    watch: &WatchState,
    out_path: &std::path::Path,
) {
    if !rr3_gap_count(seen, zeros) {
        return;
    }
    eprintln!("[rec] 1.6s 连续静默 → 线上断流，读线程重连");
    rr3_gap_reconnect(conn, device, baud, watch, out_path);
    *seen = false;
    *zeros = 0;
}

/// 零读计数：false = 还没到断流判据（首片前的静默合法）。
fn rr3_gap_count(seen: &mut bool, zeros: &mut u32) -> bool {
    if !*seen {
        return false;
    }
    *zeros += 1;
    *zeros >= RESYNC_ZERO_BAIL
}

/// 断流重连：解武装 → 重开 fd/握手 → 记重连账再武装（全败 fail-honest 退 4）。
fn rr3_gap_reconnect(
    conn: &mut Rr3UartConn,
    device: &str,
    baud: u32,
    watch: &WatchState,
    out_path: &std::path::Path,
) {
    watch.armed.store(false, Ordering::Release);
    if !reader_reconnect(conn, device, baud) {
        eprintln!("[rec] 读线程重连全败");
        bail_with_recovery(out_path, watch); // exit(4)，盘侧真相保数据
    }
    watch.reconnects.fetch_add(1, Ordering::Relaxed);
    watch.armed.store(true, Ordering::Release);
}

/// 读线程重连（三级防护第 ③ 级，判据② 改造后搬进读线程）：[HANDSHAKE_TRIES]
/// 次、每轮重开 fd（宿主侧 USB RX 冻结唯一捅法）+ 防御停流握手。全败 false
/// 交调用方 fail-honest 收场。
fn reader_reconnect(conn: &mut Rr3UartConn, device: &str, baud: u32) -> bool {
    let mut hs_err: Option<String> = None;
    for attempt in 1..=HANDSHAKE_TRIES {
        if attempt > 1 {
            eprintln!(
                "[rec] 重连握手重试 {attempt}/{HANDSHAKE_TRIES}（歇 {}s 后重开口…）",
                HANDSHAKE_RETRY_PAUSE.as_secs()
            );
            std::thread::sleep(HANDSHAKE_RETRY_PAUSE);
        }
        match Rr3UartConn::open_with_baud(std::path::Path::new(device), baud) {
            Ok(c) => *conn = c, // 赋值即 drop 旧 conn（close 旧 fd）
            Err(e) => {
                eprintln!("[rec] 重连开口失败（第 {attempt} 次）: {e}");
                hs_err = Some(format!("开口失败: {e}"));
                continue;
            }
        }
        match handshake(conn) {
            Ok(_) => return true,
            Err(e) => {
                eprintln!("[rec] 重连握手失败（第 {attempt} 次）: {e}");
                hs_err = Some(e);
                // 尽力停板面流（下一轮重开还会再发防御 OFF；板死则无效）
                let _ = conn.write_all(b"AUDIO OFF\n");
            }
        }
    }
    if let Some(e) = hs_err {
        eprintln!("[rec] 重连握手 {HANDSHAKE_TRIES} 次全败: {e}");
    }
    false
}
