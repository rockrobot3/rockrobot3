// SPDX-License-Identifier: Apache-2.0
//! UDS 接链/看守面：attach、等待结局、交接与断链退避重连。
//!
//! 只处理上下游 UDS 半边与等待语义；UART 开路与线程编排留在父模块。

use super::*;

/// 一条已交接的 UDS 连接（kill 半边归主线程）。
pub(super) struct Rr3UdsConn {
    kill: UnixStream,
    at: Instant,
}

/// 看守结局：UDS 断链重连 / UART 断链重开 / 关停。
pub(super) enum WaitOutcome {
    Reconnect,
    UartDead,
    Shutdown,
}

/// UDS 接链（三态之一：未就绪退避重试；上行丢弃计数在涨）：写窗口 → 三半边
/// 克隆 → 交接。返回 None = 等待期收到关停。
pub(super) fn uds_attach(
    cfg: &BridgeConfig,
    shutdown: &AtomicBool,
    sink: &Rr3LinkSlot,
    src: &Rr3LinkSlot,
    dropped: &AtomicU64,
) -> Option<Rr3UdsConn> {
    loop {
        let (stream, waited) = connect_with_backoff(&cfg.socket, shutdown)?;
        // 写向窗口必须在交接前设好：这是阻塞写面的唯一上界（见常量注记）。
        let Some(stream) = rr3_uds_ready(stream) else {
            continue;
        };
        let at = Instant::now();
        log_connected(&cfg.socket, waited, dropped);
        let Some((wr, rd)) = rr3_uds_halves(&stream) else {
            continue;
        };
        // 三个半边：wr→上行线程、rd→下行线程、kill 留主线程（断链时先行闭合，
        // 把还阻塞在旧流上的线程及时放出来）。
        rr3_uds_handoff(sink, src, wr, rd);
        return Some(Rr3UdsConn { kill: stream, at });
    }
}

/// 三半边交接：wr/rd 入槽（上行槽顶掉旧连接时留痕）。
fn rr3_uds_handoff(sink: &Rr3LinkSlot, src: &Rr3LinkSlot, wr: UnixStream, rd: UnixStream) {
    if sink.put(wr).is_some() {
        log("uds 上行交接：顶掉上一条未接手连接，旧句柄即刻关闭");
    }
    src.put(rd);
}

/// 交接前的写向窗口就位（设置失败打日志后重试，返回 None）。
fn rr3_uds_ready(stream: UnixStream) -> Option<UnixStream> {
    match stream.set_write_timeout(Some(UDS_WRITE_TIMEOUT)) {
        Ok(()) => Some(stream),
        Err(e) => {
            log(&format!("uds 写超时设置失败（{e}）；重试"));
            None
        }
    }
}

/// 上行/下行两个半边克隆（任一失败即重连；已克隆的句柄随返回 None 关闭）。
fn rr3_uds_halves(stream: &UnixStream) -> Option<(UnixStream, UnixStream)> {
    let wr = rr3_uds_clone(stream)?;
    let rd = rr3_uds_clone(stream)?;
    Some((wr, rd))
}

/// 克隆一个半边；失败点名后按"重试"处理（None）。
fn rr3_uds_clone(stream: &UnixStream) -> Option<UnixStream> {
    match stream.try_clone() {
        Ok(s) => Some(s),
        Err(e) => {
            log(&format!("uds clone 失败（{e}）；重试"));
            None
        }
    }
}

/// 看守线程消息：UDS 断链重连 / UART 断链重开 / 关停。断链时先闭合 kill
/// 半边，把还阻塞在旧流上的对端线程及时放出来。
pub(super) fn uds_wait(
    exit_rx: &mpsc::Receiver<ExitMsg>,
    shutdown: &AtomicBool,
    conn: Rr3UdsConn,
    dwell: &mut Backoff,
) -> WaitOutcome {
    let Rr3UdsConn { kill, at } = conn;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return WaitOutcome::Shutdown;
        }
        match exit_rx.recv_timeout(WAIT_SLICE) {
            Ok(msg) => return rr3_wait_msg(msg, kill, at, dwell, shutdown),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // 两个线程都已退场 = UART 面没了
                log("uart 断链（收发线程均已退场）；关 UDS，重开设备节点");
                return WaitOutcome::UartDead;
            }
        }
    }
}

/// 一条退出消息的处置（kill 半边交给本条：两条断链路都先闭合它）。
fn rr3_wait_msg(
    msg: ExitMsg,
    kill: UnixStream,
    at: Instant,
    dwell: &mut Backoff,
    shutdown: &AtomicBool,
) -> WaitOutcome {
    match msg {
        ExitMsg::UdsDead(why) => {
            drop(kill);
            if !uds_retire(&why, at.elapsed(), dwell, shutdown) {
                return WaitOutcome::Shutdown;
            }
            WaitOutcome::Reconnect
        }
        ExitMsg::UartDead(why) => {
            log(&format!("uart 断链（{why}）；关 UDS，重开设备节点"));
            drop(kill);
            WaitOutcome::UartDead
        }
    }
}

/// UDS 断链后的重连前处置：活过 [`UDS_MIN_DWELL`] 的连接视为正常断链（立即
/// 重连并复位退避）；短命连接按指数退避等待。返回 false = 等待期收到关停。
fn uds_retire(why: &str, lived: Duration, dwell: &mut Backoff, shutdown: &AtomicBool) -> bool {
    if lived >= UDS_MIN_DWELL {
        dwell.reset();
        log(&format!("uds 断链（{why}）；保住 UART，走重连"));
        return true;
    }
    let wait = dwell.next();
    log(&format!(
        "uds 断链（{why}）；连接仅存活 {}ms（<{}s），{}s 后重连（防接了就关的空转）",
        lived.as_millis(),
        UDS_MIN_DWELL.as_secs(),
        wait.as_secs()
    ));
    sleep_sliced(wait, shutdown)
}

/// UDS connect（三态之一的退避循环）。返回 (流, 是否经历过退避等待)。
/// 关停信号随时生效（返回 None）。
fn connect_with_backoff(socket: &Path, shutdown: &AtomicBool) -> Option<(UnixStream, bool)> {
    let mut backoff = Backoff::new();
    let mut waited = false;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return None;
        }
        match UnixStream::connect(socket) {
            Ok(s) => return Some((s, waited)),
            Err(e) => {
                rr3_connect_wait(e, &mut backoff, shutdown)?;
                waited = true;
            }
        }
    }
}

/// connect 失败的一拍：留痕后退避等待；None = 等待期收到关停。
fn rr3_connect_wait(e: std::io::Error, backoff: &mut Backoff, shutdown: &AtomicBool) -> Option<()> {
    log(&format!(
        "uds connect 失败（{e}）；{}s 后重试",
        backoff.peek().as_secs()
    ));
    if !sleep_sliced(backoff.next(), shutdown) {
        return None;
    }
    Some(())
}
