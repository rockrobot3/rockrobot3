// SPDX-License-Identifier: Apache-2.0
//! 连接与重连：UART 开口、UDS 接链、三态寿命管理与退避。

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rr_ipc::transport::uart::{Rr3UartConn, RR3_UART_BAUD_DEFAULT};

use super::route::{downstream_loop, upstream_loop};
use super::stat::{log, log_connected};

mod uds;
use uds::{uds_attach, uds_wait, WaitOutcome};

/// 缺省波特率（rr-ipc 同值真源，CLI 缺省从这里取）。
pub const RR3_BRIDGE_BAUD_DEFAULT: u32 = RR3_UART_BAUD_DEFAULT;

/// 固定搬运块大小（§5.1.3"固定块搬运"；>128B 头 + 典型 payload，一次 read 常整帧）。
pub(super) const CHUNK: usize = 512;
/// UDS connect 退避：1s 起步 ×2 至 10s 封顶（§5.1.5）。
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_CAP: Duration = Duration::from_secs(10);
/// 主循环等线程消息的轮询片（同时是 SIGTERM 响应上界）。
pub(super) const WAIT_SLICE: Duration = Duration::from_millis(250);
/// UDS 上行写窗口（与 rrd 侧 B-5 写超时同值）：对端停读时 5s 内必须显形。
/// 没有这道窗口，连接塞满内核发送缓冲后 `write_all` 永久阻塞——上行线程挂死，
/// 且再也收不到 UART 断链消息，桥只能靠人工重启（生产事故面）。
const UDS_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// UDS 连接最短存活窗：短于此视为"接了就关"的异常对端，重连走退避。
/// 没有这道窗，rrd 反复 accept-close 时桥会以微秒级空转重连打满 CPU 与日志。
const UDS_MIN_DWELL: Duration = Duration::from_secs(1);

/// 桥的运行参数（CLI 解析产物）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeConfig {
    /// UART 设备路径（macOS `/dev/tty.usbserial-*`、Linux `/dev/ttyUSB0`）。
    pub uart: PathBuf,
    /// 波特率（只收 115200/9600/921600/1500000/2000000，解析期已校验）。
    pub baud: u32,
    /// rrd 套接字路径（`--socket`，如 /var/run/rock3-ipc.sock）。
    pub socket: PathBuf,
}

/// 线程退出原因（上行/下行线程 → 主线程寿命管理）。
#[derive(Debug)]
pub(super) enum ExitMsg {
    /// UDS 断链：UART 存活，主线程走重连（三态之三）。
    UdsDead(String),
    /// UART 断链：主线程关 UDS、重开设备（三态之二）。
    UartDead(String),
}

/// 信号处理登记槽：C-ABI 回调不能捕获环境，用 OnceLock 转交退出标志。
static SIGNAL_SLOT: OnceLock<Arc<AtomicBool>> = OnceLock::new();

// SAFETY：`signal(3)` 声明与 BSD/glibc/musl 同形；处理器是无捕获 C-ABI 函数，
// 体内只写 OnceLock 中的 AtomicBool（async-signal-safe）。
extern "C" {
    /// `signal(3)`（BSD/macOS 与 glibc/musl 同形）。返回值（旧 handler）不关心。
    fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
}

/// SIGTERM(15)/SIGINT(2)：BSD 与 Linux 同值。处理函数只置位（async-signal-safe），
/// 实际收尾由主循环在 ≤WAIT_SLICE 内看到标志后执行。
extern "C" fn on_signal(_sig: i32) {
    if let Some(flag) = SIGNAL_SLOT.get() {
        flag.store(true, Ordering::SeqCst);
    }
}

/// 安装 SIGTERM/SIGINT 处理（main 用；测试自建标志不经此）。
pub fn rr3_signal_handlers(shutdown: Arc<AtomicBool>) {
    let _ = SIGNAL_SLOT.set(shutdown);
    // SAFETY：handler 是 C-ABI 且只做原子置位；signal(3) 本身可重入性无要求。
    unsafe {
        signal(15, on_signal); // SIGTERM
        signal(2, on_signal); // SIGINT
    }
}

/// 桥主入口：永不主动返回，直到 SIGTERM/SIGINT（退出码 0）。
/// UART 设备打不开时按退避持续重开（§5.1.5 三态之二的重开半边）。
pub fn run(cfg: BridgeConfig, shutdown: Arc<AtomicBool>) -> u8 {
    log(&format!(
        "rr3-bridge 启动 dev={} baud={} socket={}",
        cfg.uart.display(),
        cfg.baud,
        cfg.socket.display()
    ));
    loop {
        if rr3_bridge_link(&cfg, &shutdown) {
            return shutdown_exit();
        }
    }
}

/// 一轮链路（`true` = 关停收兵）：UART 开口 → 记上线 → 跑到断链或关停。
fn rr3_bridge_link(cfg: &BridgeConfig, shutdown: &Arc<AtomicBool>) -> bool {
    if shutdown.load(Ordering::SeqCst) {
        return true;
    }
    // —— UART 开口（失败退避重开；开口净化由 Rr3UartConn::open 兜底）——
    let Some((rx, tx)) = rr3_uart_pair(cfg, shutdown) else {
        return true;
    };
    log(&format!(
        "uart link up dev={} baud={}",
        cfg.uart.display(),
        cfg.baud
    ));
    // 返回 = UART 断链（或关停）；关停即收兵，否则由 run 循环重开设备节点。
    run_link(cfg, shutdown, rx, tx) == LinkOutcome::Shutdown
}

/// UART 开口循环（三态之二）：失败按退避重开，None = 等待期收到关停。
fn rr3_uart_pair(cfg: &BridgeConfig, shutdown: &AtomicBool) -> Option<(Rr3UartConn, Rr3UartConn)> {
    let mut backoff = Backoff::new();
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return None;
        }
        match open_uart_pair(cfg) {
            Ok(pair) => return Some(pair),
            Err(e) => rr3_uart_retry(e, &mut backoff, shutdown)?,
        }
    }
}

/// 开口失败的一拍：留痕后退避等待；None = 等待期收到关停。
fn rr3_uart_retry(e: std::io::Error, backoff: &mut Backoff, shutdown: &AtomicBool) -> Option<()> {
    log(&format!(
        "uart open 失败（{e}）；{}s 后重开",
        backoff.peek().as_secs()
    ));
    if !sleep_sliced(backoff.next(), shutdown) {
        return None;
    }
    Some(())
}

/// 开一对 rx/tx 句柄（同一设备节点 open 两次，读/写分体——模块注记的理由）。
/// 档位合法性已由 CLI 解析期校验；open_with_baud 内部同表再校一次。
fn open_uart_pair(cfg: &BridgeConfig) -> std::io::Result<(Rr3UartConn, Rr3UartConn)> {
    let rx = Rr3UartConn::open_with_baud(&cfg.uart, cfg.baud)?;
    match Rr3UartConn::open_with_baud(&cfg.uart, cfg.baud) {
        Ok(tx) => Ok((rx, tx)),
        Err(e) => {
            drop(rx); // 归还已开的 rx，不留下半开的设备
            Err(e)
        }
    }
}

/// run_link 的结局：关停（SIGTERM）或 UART 断链（回 run 重开）。
#[derive(Debug, PartialEq, Eq)]
enum LinkOutcome {
    Shutdown,
    UartDead,
}

/// 连接交接槽：同一方向只保留"最新一条待接手的连接"，换手即关旧句柄。
///
/// 为什么不是队列：UDS 断链重连期间，若 UART 静默（上行线程阻塞在 read，
/// 不换手），队列会把每次重连的旧句柄攒住；rrd 若"接了就关"地反复重连，
/// 句柄按重连次数无界增长，直到 fd 表耗尽连 UART 都开不出来。槽语义让
/// "最新连接"恒为唯一真值，旧句柄在放入时立刻关闭，增长面为零。
#[derive(Clone)]
pub(super) struct Rr3LinkSlot(Arc<Mutex<Option<UnixStream>>>);

impl Rr3LinkSlot {
    pub(super) fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    /// 放入最新连接；返回被顶掉的旧连接（由调用方 drop，即关闭句柄）。
    pub(super) fn put(&self, s: UnixStream) -> Option<UnixStream> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).replace(s)
    }

    /// 取走待接手的连接（无则 None）。
    pub(super) fn take(&self) -> Option<UnixStream> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

/// 一条链路的一生（§5.1.5）：起上行/下行线程，逐条 UDS 连接接力，直到
/// UART 断链或关停。UART 断链时先关 UDS（kill 半边）再返回。
fn run_link(
    cfg: &BridgeConfig,
    shutdown: &Arc<AtomicBool>,
    rx: Rr3UartConn,
    tx: Rr3UartConn,
) -> LinkOutcome {
    let dropped = Arc::new(AtomicU64::new(0));
    let (exit_tx, exit_rx) = mpsc::channel::<ExitMsg>();
    let (sink, src) = rr3_link_threads(rx, tx, shutdown, &dropped, exit_tx);
    rr3_link_serve(cfg, shutdown, &sink, &src, &dropped, &exit_rx)
}

/// 起上行/下行线程（断链由它们经 `ExitMsg` 报回主线程）。
fn rr3_link_threads(
    rx: Rr3UartConn,
    tx: Rr3UartConn,
    shutdown: &Arc<AtomicBool>,
    dropped: &Arc<AtomicU64>,
    exit_tx: mpsc::Sender<ExitMsg>,
) -> (Rr3LinkSlot, Rr3LinkSlot) {
    let sink = Rr3LinkSlot::new();
    let src = Rr3LinkSlot::new();
    rr3_up_spawn(rx, &sink, dropped, exit_tx.clone());
    rr3_down_spawn(tx, &src, shutdown, exit_tx.clone());
    drop(exit_tx); // 线程各自持一份；主线程只收不发
    (sink, src)
}

/// 起上行线程（UART 读 → UDS 写）。
fn rr3_up_spawn(
    rx: Rr3UartConn,
    sink: &Rr3LinkSlot,
    dropped: &Arc<AtomicU64>,
    exit: mpsc::Sender<ExitMsg>,
) {
    let dropped = Arc::clone(dropped);
    let sink = sink.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("bridge-upstream".into())
        .spawn(move || upstream_loop(rx, sink, dropped, exit))
    {
        eprintln!("起上行线程失败（数据面不可用，显式停机）: {e:?}");
        std::process::abort();
    }
}

/// 起下行线程（UDS 读 → UART 写）。
fn rr3_down_spawn(
    tx: Rr3UartConn,
    src: &Rr3LinkSlot,
    shutdown: &Arc<AtomicBool>,
    exit: mpsc::Sender<ExitMsg>,
) {
    let src = src.clone();
    let stop = Arc::clone(shutdown);
    if let Err(e) = std::thread::Builder::new()
        .name("bridge-downstream".into())
        .spawn(move || downstream_loop(tx, src, stop, exit))
    {
        eprintln!("起下行线程失败（数据面不可用，显式停机）: {e:?}");
        std::process::abort();
    }
}

/// 逐条 UDS 连接接力（三态之三）：直到 UART 断链或关停。
fn rr3_link_serve(
    cfg: &BridgeConfig,
    shutdown: &Arc<AtomicBool>,
    sink: &Rr3LinkSlot,
    src: &Rr3LinkSlot,
    dropped: &Arc<AtomicU64>,
    exit_rx: &mpsc::Receiver<ExitMsg>,
) -> LinkOutcome {
    // 短命连接退避（见 [`UDS_MIN_DWELL`]）；连接活过窗口即复位。
    let mut dwell = Backoff::new();
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return LinkOutcome::Shutdown;
        }
        let Some(conn) = uds_attach(cfg, shutdown, sink, src, dropped) else {
            return LinkOutcome::Shutdown;
        };
        if let Some(outcome) = rr3_serve_outcome(uds_wait(exit_rx, shutdown, conn, &mut dwell)) {
            return outcome;
        }
    }
}

/// 看守结局分流：None = 重连（继续接力循环）。
fn rr3_serve_outcome(w: WaitOutcome) -> Option<LinkOutcome> {
    match w {
        WaitOutcome::Reconnect => None,
        WaitOutcome::UartDead => Some(LinkOutcome::UartDead),
        WaitOutcome::Shutdown => Some(LinkOutcome::Shutdown),
    }
}

/// 指数退避：1s 起步 ×2 至 10s 封顶（§5.1.5 参数）。
struct Backoff {
    cur: Duration,
}

impl Backoff {
    fn new() -> Self {
        Self { cur: BACKOFF_START }
    }
    fn peek(&self) -> Duration {
        self.cur
    }
    fn reset(&mut self) {
        self.cur = BACKOFF_START;
    }
    fn next(&mut self) -> Duration {
        let w = self.cur;
        self.cur = std::cmp::min(self.cur * 2, BACKOFF_CAP);
        w
    }
}

/// 切片睡眠：等待期间每 WAIT_SLICE 查一次关停标志。false = 收到关停。
fn sleep_sliced(total: Duration, shutdown: &AtomicBool) -> bool {
    let mut left = total;
    while left > Duration::ZERO {
        if shutdown.load(Ordering::SeqCst) {
            return false;
        }
        let step = std::cmp::min(left, WAIT_SLICE);
        std::thread::sleep(step);
        left = left.saturating_sub(step);
    }
    !shutdown.load(Ordering::SeqCst)
}

/// 关停收尾（§5.1.5：逐条关 UDS → 释放 UART 设备节点 → 退出码 0）。
/// fd 的生命周期都在 RAII 句柄里（UnixStream/Rr3UartConn drop 即 close），
/// 主循环各 return 路径的 drop 顺序天然是"先 UDS 后 UART"；这里只打行 + 定码。
fn shutdown_exit() -> u8 {
    log("收到关停信号：逐条关 UDS，释放 UART 设备节点，退出");
    0
}
