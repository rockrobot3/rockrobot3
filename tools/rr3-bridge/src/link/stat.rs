//! 丢弃统计与日志：上行丢弃累计、恢复点与连接日志。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use super::conn::ExitMsg;

pub(super) fn log(msg: &str) {
    eprintln!("[bridge] {msg}");
}

/// 连接日志（等待过退避时带丢弃累计——运维据它判断"看起来在跑"是否真在跑）。
pub(super) fn log_connected(socket: &Path, waited: bool, dropped: &AtomicU64) {
    let tail = if waited {
        format!(
            "（等待期上行丢弃 {} 字节，未缓冲）",
            dropped.load(Ordering::Relaxed)
        )
    } else {
        String::new()
    };
    log(&format!("uds connected {}{tail}", socket.display()));
}

/// 写失败收口：计丢弃、留痕、报 UdsDead。
pub(super) fn rr3_route_fail(
    e: std::io::Error,
    n: usize,
    dropped: &AtomicU64,
    exit: &mpsc::Sender<ExitMsg>,
) {
    let _ = dropped.fetch_add(n as u64, Ordering::Relaxed);
    log(&format!(
        "uds 上行写出失败（{e}）：本块 {n} 字节丢弃，改入丢弃计数"
    ));
    let _ = exit.send(ExitMsg::UdsDead("上行写出失败".into()));
}

/// 丢弃期结束的恢复点：必须报累计（运维据它判断"看起来在跑"是否真在跑）。
pub(super) fn rr3_route_recover(discarding: &mut bool, dropped: &AtomicU64) {
    if *discarding {
        *discarding = false;
        log(&format!(
            "uds 上行恢复：此前丢弃累计 {} 字节（帧撕裂面由哨兵超时自愈）",
            dropped.load(Ordering::Relaxed)
        ));
    }
}

/// 无连接时的一块：计入丢弃，首次进入与每 ~4KiB 节流各打一行。
pub(super) fn rr3_route_discard(n: usize, dropped: &AtomicU64, discarding: &mut bool) {
    let total = dropped.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
    if !*discarding {
        *discarding = true;
        log("uds 未就绪：UART 上行开始丢弃（不缓冲，等 rrd 就绪后自愈）");
    } else if total % 4096 < n as u64 {
        // 每 ~4KiB 一行的节流计数（1s 内 4096/60 帧的量级不会刷屏）
        log(&format!("UART 上行丢弃累计 {total} 字节"));
    }
}
