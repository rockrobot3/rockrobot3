//! 双向路由：UART↔UDS 的字节搬运、换流与丢弃计数。

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use rr_ipc::transport::uart::Rr3UartConn;

use super::conn::{ExitMsg, Rr3LinkSlot, CHUNK, WAIT_SLICE};
use super::stat::{log, rr3_route_discard, rr3_route_fail, rr3_route_recover};

/// 上行线程：UART 读（阻塞）→ UDS 写；UDS 未就绪/断链时丢弃 + 计数（不缓冲）。
/// 循环序刻意为"读 → 换流 → 路由"：换流放在读之后、路由之前，读阻塞期间新到的
/// UDS 连接在本轮就生效——不会把字节写进已经死掉的旧流（丢字节 = 帧撕裂 = 哨兵
/// 超时重启，能自愈但能不丢就不丢）。
pub(super) fn upstream_loop(
    mut rx: Rr3UartConn,
    new_sink: Rr3LinkSlot,
    dropped: Arc<AtomicU64>,
    exit: mpsc::Sender<ExitMsg>,
) {
    let mut buf = [0u8; CHUNK];
    let mut cur: Option<UnixStream> = None;
    let mut discarding = false; // 用于"进入丢弃期"只打一行
    loop {
        let Some(n) = upstream_read(&mut rx, &mut buf, &exit) else {
            return;
        };
        rr3_up_swap(&mut cur, &new_sink);
        upstream_route(&mut cur, &buf[..n], n, &dropped, &mut discarding, &exit);
    }
}

/// 换流：交接槽里的新连接顶掉当前句柄（无新连接则沿用旧句柄）。
fn rr3_up_swap(cur: &mut Option<UnixStream>, new_sink: &Rr3LinkSlot) {
    if let Some(s) = new_sink.take() {
        *cur = Some(s);
    }
}

/// 读一块 UART；EOF/读错即报 UartDead 并回 None（上行线程该退场）。
fn upstream_read(
    rx: &mut Rr3UartConn,
    buf: &mut [u8],
    exit: &mpsc::Sender<ExitMsg>,
) -> Option<usize> {
    match rx.read(buf) {
        Ok(0) => rr3_up_dead(exit, "上行 EOF".to_string()),
        Ok(n) => Some(n),
        Err(e) => rr3_up_dead(exit, format!("上行读失败: {e}")),
    }
}

/// 上行断链收口：报 UartDead 并回 None。
fn rr3_up_dead(exit: &mpsc::Sender<ExitMsg>, msg: String) -> Option<usize> {
    let _ = exit.send(ExitMsg::UartDead(msg));
    None
}

/// 一块的去向：有连接写 UDS（写失败转丢弃计数并报 UdsDead），无连接计入丢弃。
fn upstream_route(
    cur: &mut Option<UnixStream>,
    chunk: &[u8],
    n: usize,
    dropped: &AtomicU64,
    discarding: &mut bool,
    exit: &mpsc::Sender<ExitMsg>,
) {
    let Some(s) = cur.as_mut() else {
        rr3_route_discard(n, dropped, discarding);
        return;
    };
    if rr3_route_write(s, chunk, n, dropped, discarding, exit) {
        *cur = None;
    }
}

/// 有连接时的一块：恢复点日志 + 写 UDS；`true` = 写失败（连接已死，调用方清空）。
fn rr3_route_write(
    s: &mut UnixStream,
    chunk: &[u8],
    n: usize,
    dropped: &AtomicU64,
    discarding: &mut bool,
    exit: &mpsc::Sender<ExitMsg>,
) -> bool {
    rr3_route_recover(discarding, dropped);
    if let Err(e) = s.write_all(chunk) {
        rr3_route_fail(e, n, dropped, exit);
        return true;
    }
    false
}

/// 下行线程：UDS 读（阻塞）→ UART 写；无连接时按 [`WAIT_SLICE`] 轮询交接槽。
/// 读阻塞可接受的理由见模块注记：rrd 断链以 FIN 及时显形，不会卡死换流。
pub(super) fn downstream_loop(
    mut tx: Rr3UartConn,
    new_src: Rr3LinkSlot,
    stop: Arc<AtomicBool>,
    exit: mpsc::Sender<ExitMsg>,
) {
    let mut buf = [0u8; CHUNK];
    let mut cur: Option<UnixStream> = None;
    loop {
        rr3_down_swap(&mut cur, &new_src);
        if !rr3_down_drive(&mut cur, &mut buf, &mut tx, &stop, &exit) {
            return;
        }
    }
}

/// 一拍下行：无连接则等一拍（关停即 false 退场），有连接则推进一块。
/// false = 写失败退场；连接已死只清空 `cur`（下一拍重新等交接槽）。
fn rr3_down_drive(
    cur: &mut Option<UnixStream>,
    buf: &mut [u8],
    tx: &mut Rr3UartConn,
    stop: &AtomicBool,
    exit: &mpsc::Sender<ExitMsg>,
) -> bool {
    let Some(stream) = cur.as_mut() else {
        return !rr3_down_idle(stop);
    };
    match rr3_down_step(stream, buf, tx, exit) {
        Some(alive) => alive,
        None => {
            *cur = None;
            true
        }
    }
}

/// 换流：交接槽里的新连接顶掉当前句柄（无新连接则沿用旧句柄）。
fn rr3_down_swap(cur: &mut Option<UnixStream>, new_src: &Rr3LinkSlot) {
    if let Some(s) = new_src.take() {
        *cur = Some(s);
    }
}

/// 无连接时等一拍；返回 true = 等待期收到关停，调用方退场。
fn rr3_down_idle(stop: &AtomicBool) -> bool {
    if stop.load(Ordering::SeqCst) {
        return true;
    }
    std::thread::sleep(WAIT_SLICE);
    false
}

/// 下行一块：UDS 读到即写 UART。None = 连接已死（cur 清零）；Some(false) = 写失败退场。
fn rr3_down_step(
    stream: &mut UnixStream,
    buf: &mut [u8],
    tx: &mut Rr3UartConn,
    exit: &mpsc::Sender<ExitMsg>,
) -> Option<bool> {
    let n = match stream.read(buf) {
        Ok(0) => {
            log("uds 下行 EOF（rrd 侧关闭）");
            let _ = exit.send(ExitMsg::UdsDead("下行 EOF".into()));
            return None;
        }
        Ok(n) => n,
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Some(true),
        Err(e) => {
            let _ = exit.send(ExitMsg::UdsDead(format!("下行读失败: {e}")));
            return None;
        }
    };
    match tx.write_all(&buf[..n]) {
        Ok(()) => Some(true),
        Err(e) => {
            let _ = exit.send(ExitMsg::UartDead(format!("下行写失败: {e}")));
            Some(false)
        }
    }
}
