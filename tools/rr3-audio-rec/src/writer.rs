//! 写线程：占位头 → 逐片写透并推进盘侧真相（失败补恢复头退 1）。

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};

use crate::exit::exit_with_recovery;
use crate::watch::WatchState;
use crate::wav::wav_header;

/// 写线程（落盘独占面）：占位头 → 逐片写透（write_all + flush，崩溃面数据风险
/// 与旧逐片同步落盘等价）→ 通道关闭即收兵退出。错误即整进程退（fail-honest，
/// 不带病续跑冒领数据），退出前补恢复头（磁盘满残片也可解码）；`disk_bytes` 只
/// 在实际写透后推进——恢复头 data_len 的事实源。最终头（write_at(0)）由主线程
/// join 后补，无并发窗口。无界通道：写线程被磁盘反压时分片在内存排队（87.5KB/s
/// 流速下秒级尖峰至多 MB 级），收流面不被磁盘 I/O 拖住（冻结放大器解耦的落点）。
pub(crate) fn writer_run(
    file: File,
    rx: mpsc::Receiver<Vec<u8>>,
    watch: Arc<WatchState>,
    out_path: PathBuf,
) {
    let mut w = BufWriter::new(file);
    // 占位头（data_len=0）：正常收兵与看门狗/重连/写失败三路都回头 write_at(0) 补
    // 真头。**立即 flush**：磁盘恒有合法头，恢复路径不依赖 BufWriter 析构刷出。
    rr3_writer_head(&mut w);
    let mut written = 0u64;
    for buf in rx {
        rr3_writer_chunk(&mut w, &buf, &out_path, &watch, &mut written);
    }
}

/// 单分片写透 + 盘侧真相推进（写失败补恢复头退 1）。
fn rr3_writer_chunk(
    w: &mut BufWriter<File>,
    buf: &[u8],
    out_path: &std::path::Path,
    watch: &WatchState,
    written: &mut u64,
) {
    if let Err(e) = w.write_all(buf).and_then(|()| w.flush()) {
        eprintln!("rr3-audio-rec: wav 写线程写失败（磁盘满/IO 错）: {e}");
        exit_with_recovery(out_path, watch, 1);
    }
    *written += buf.len() as u64;
    watch.disk_bytes.store(*written, Ordering::Release);
}

/// 占位 wav 头写入（失败即整进程退，文案逐字保留）。
fn rr3_writer_head(w: &mut BufWriter<File>) {
    if let Err(e) = w
        .write_all(&wav_header(0, 2, 16000, 16))
        .and_then(|()| w.flush())
    {
        eprintln!("rr3-audio-rec: wav 占位头写入失败: {e}");
        std::process::exit(1);
    }
}
