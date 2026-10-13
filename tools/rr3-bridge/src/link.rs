//! 桥的链路机制：寿命管理（三态）+ 双向透传。形状依据 0.0.7 方案 §5.1（钉死直写），
//! 本文件之外零设计决策；实现细节（块大小 512B、退避 1s→10s、日志措辞）属方案
//! 明示留给实装的自由度。

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::Duration;

use rr_ipc::transport::uart::{Rr3UartConn, RR3_UART_BAUD_DEFAULT};

/// 缺省波特率（rr-ipc 同值真源，CLI 缺省从这里取）。
pub const RR3_BRIDGE_BAUD_DEFAULT: u32 = RR3_UART_BAUD_DEFAULT;

/// 固定搬运块大小（§5.1.3"固定块搬运"；>128B 头 + 典型 payload，一次 read 常整帧）。
const CHUNK: usize = 512;
/// UDS connect 退避：1s 起步 ×2 至 10s 封顶（§5.1.5）。
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_CAP: Duration = Duration::from_secs(10);
/// 主循环等线程消息的轮询片（同时是 SIGTERM 响应上界）。
const WAIT_SLICE: Duration = Duration::from_millis(250);

/// 桥的运行参数（CLI 解析产物）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeConfig {
    /// UART 设备路径（macOS `/dev/tty.usbserial-*`、Linux `/dev/ttyUSB0`）。
    pub uart: PathBuf,
    /// 波特率（只收 115200/9600/921600，解析期已校验）。
    pub baud: u32,
    /// rrd 套接字路径（`--socket`，如 /var/run/rock3-ipc.sock）。
    pub socket: PathBuf,
}

/// 线程退出原因（上行/下行线程 → 主线程寿命管理）。
#[derive(Debug)]
enum ExitMsg {
    /// UDS 断链：UART 存活，主线程走重连（三态之三）。
    UdsDead(String),
    /// UART 断链：主线程关 UDS、重开设备（三态之二）。
    UartDead(String),
}

/// 信号处理登记槽：C-ABI 回调不能捕获环境，用 OnceLock 转交退出标志。
static SIGNAL_SLOT: OnceLock<Arc<AtomicBool>> = OnceLock::new();

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
pub fn register_signal_handlers(shutdown: Arc<AtomicBool>) {
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
        if shutdown.load(Ordering::SeqCst) {
            return shutdown_exit();
        }
        // —— UART 开口（失败退避重开；开口净化由 Rr3UartConn::open 兜底）——
        let mut backoff = Backoff::new();
        let (rx, tx) = loop {
            if shutdown.load(Ordering::SeqCst) {
                return shutdown_exit();
            }
            match open_uart_pair(&cfg) {
                Ok(pair) => break pair,
                Err(e) => {
                    log(&format!(
                        "uart open 失败（{e}）；{}s 后重开",
                        backoff.peek().as_secs()
                    ));
                    if !sleep_sliced(backoff.next(), &shutdown) {
                        return shutdown_exit();
                    }
                }
            }
        };
        log(&format!(
            "uart link up dev={} baud={}",
            cfg.uart.display(),
            cfg.baud
        ));
        if run_link(&cfg, &shutdown, rx, tx) == LinkOutcome::Shutdown {
            return shutdown_exit();
        }
        // run_link 返回 = UART 断链（或关停），循环重开设备节点
    }
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
    let (sink_tx, sink_rx) = mpsc::channel::<UnixStream>();
    let (src_tx, src_rx) = mpsc::channel::<UnixStream>();
    {
        let exit = exit_tx.clone();
        let dropped = Arc::clone(&dropped);
        std::thread::Builder::new()
            .name("bridge-upstream".into())
            .spawn(move || upstream_loop(rx, sink_rx, dropped, exit))
            .expect("起上行线程失败");
    }
    {
        let exit = exit_tx.clone();
        std::thread::Builder::new()
            .name("bridge-downstream".into())
            .spawn(move || downstream_loop(tx, src_rx, exit))
            .expect("起下行线程失败");
    }
    drop(exit_tx); // 线程各自持一份；主线程只收不发

    'uds: loop {
        if shutdown.load(Ordering::SeqCst) {
            return LinkOutcome::Shutdown;
        }
        // —— UDS connect（三态之一：未就绪退避重试；上行丢弃计数在涨）——
        let (stream, waited) = match connect_with_backoff(&cfg.socket, shutdown) {
            None => return LinkOutcome::Shutdown,
            Some(s) => s,
        };
        if waited {
            log(&format!(
                "uds connected {}（等待期上行丢弃 {} 字节，未缓冲）",
                cfg.socket.display(),
                dropped.load(Ordering::Relaxed)
            ));
        } else {
            log(&format!("uds connected {}", cfg.socket.display()));
        }
        // 三个半边：wr→上行线程、rd→下行线程、kill 留主线程（断链时先行闭合，
        // 把还阻塞在旧流上的线程及时放出来）。
        let wr = match stream.try_clone() {
            Ok(s) => s,
            Err(e) => {
                log(&format!("uds clone 失败（{e}）；重试"));
                continue 'uds;
            }
        };
        let rd = match stream.try_clone() {
            Ok(s) => s,
            Err(e) => {
                log(&format!("uds clone 失败（{e}）；重试"));
                drop(wr);
                continue 'uds;
            }
        };
        let kill = stream; // 第三个半边归主线程
        if sink_tx.send(wr).is_err() || src_tx.send(rd).is_err() {
            // 线程都没了（UART 已死且线程退场）——回 run 重开
            return LinkOutcome::UartDead;
        }
        // —— 看守线程消息：UDS 断链重连 / UART 断链重开 / 关停 ——
        loop {
            if shutdown.load(Ordering::SeqCst) {
                return LinkOutcome::Shutdown;
            }
            match exit_rx.recv_timeout(WAIT_SLICE) {
                Ok(ExitMsg::UdsDead(why)) => {
                    log(&format!("uds 断链（{why}）；保住 UART，走重连"));
                    drop(kill); // 闭合第三个半边，放行阻塞在旧流上的对端线程
                    continue 'uds;
                }
                Ok(ExitMsg::UartDead(why)) => {
                    log(&format!("uart 断链（{why}）；关 UDS，重开设备节点"));
                    drop(kill);
                    return LinkOutcome::UartDead;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // 两个线程都已退场 = UART 面没了
                    log("uart 断链（收发线程均已退场）；关 UDS，重开设备节点");
                    return LinkOutcome::UartDead;
                }
            }
        }
    }
}

/// 上行线程：UART 读（阻塞）→ UDS 写；UDS 未就绪/断链时丢弃 + 计数（不缓冲）。
/// 循环序刻意为"读 → 换流 → 路由"：换流放在读之后、路由之前，读阻塞期间新到的
/// UDS 连接在本轮就生效——不会把字节写进已经死掉的旧流（丢字节 = 帧撕裂 = 哨兵
/// 超时重启，能自愈但能不丢就不丢）。
fn upstream_loop(
    mut rx: Rr3UartConn,
    new_sink: mpsc::Receiver<UnixStream>,
    dropped: Arc<AtomicU64>,
    exit: mpsc::Sender<ExitMsg>,
) {
    let mut buf = [0u8; CHUNK];
    let mut cur: Option<UnixStream> = None;
    let mut discarding = false; // 用于"进入丢弃期"只打一行
    loop {
        let n = match rx.read(&mut buf) {
            Ok(0) => {
                let _ = exit.send(ExitMsg::UartDead("上行 EOF".into()));
                return;
            }
            Ok(n) => n,
            Err(e) => {
                let _ = exit.send(ExitMsg::UartDead(format!("上行读失败: {e}")));
                return;
            }
        };
        while let Ok(s) = new_sink.try_recv() {
            cur = Some(s);
        }
        let chunk = &buf[..n];
        match cur.as_mut() {
            Some(s) => {
                discarding = false;
                if let Err(e) = s.write_all(chunk) {
                    cur = None;
                    let _ = dropped.fetch_add(n as u64, Ordering::Relaxed);
                    log(&format!(
                        "uds 上行写出失败（{e}）：本块 {n} 字节丢弃，改入丢弃计数"
                    ));
                    let _ = exit.send(ExitMsg::UdsDead("上行写出失败".into()));
                }
            }
            None => {
                let total = dropped.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
                if !discarding {
                    discarding = true;
                    log("uds 未就绪：UART 上行开始丢弃（不缓冲，等 rrd 就绪后自愈）");
                } else if total % 4096 < n as u64 {
                    // 每 ~4KiB 一行的节流计数（1s 内 4096/60 帧的量级不会刷屏）
                    log(&format!("UART 上行丢弃累计 {total} 字节"));
                }
            }
        }
    }
}

/// 下行线程：UDS 读（阻塞）→ UART 写；无连接时等流（100ms 轮询）。
/// 读阻塞可接受的理由见模块注记：rrd 断链以 FIN 及时显形，不会卡死换流。
fn downstream_loop(
    mut tx: Rr3UartConn,
    new_src: mpsc::Receiver<UnixStream>,
    exit: mpsc::Sender<ExitMsg>,
) {
    let mut buf = [0u8; CHUNK];
    let mut cur: Option<UnixStream> = None;
    loop {
        while let Ok(s) = new_src.try_recv() {
            cur = Some(s);
        }
        let stream = match cur.as_mut() {
            Some(s) => s,
            None => match new_src.recv_timeout(WAIT_SLICE) {
                Ok(s) => {
                    cur = Some(s);
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return, // 主线程已弃链
            },
        };
        let n = match stream.read(&mut buf) {
            Ok(0) => {
                cur = None;
                log("uds 下行 EOF（rrd 侧关闭）");
                let _ = exit.send(ExitMsg::UdsDead("下行 EOF".into()));
                continue;
            }
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                cur = None;
                let _ = exit.send(ExitMsg::UdsDead(format!("下行读失败: {e}")));
                continue;
            }
        };
        if let Err(e) = tx.write_all(&buf[..n]) {
            let _ = exit.send(ExitMsg::UartDead(format!("下行写失败: {e}")));
            return;
        }
    }
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
                log(&format!(
                    "uds connect 失败（{e}）；{}s 后重试",
                    backoff.peek().as_secs()
                ));
                waited = true;
                if !sleep_sliced(backoff.next(), shutdown) {
                    return None;
                }
            }
        }
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

fn log(msg: &str) {
    eprintln!("[bridge] {msg}");
}

// ---------------------------------------------------------------------------
// 链路机制集成测试（pty + UDS 对跑——宿主档的"假 rrd"半边）。
// 线程在测试结束后自然随进程回收；shutdown 标志让 run_link 里的退避循环
// 不会空转（测试结束时置位即可）。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::os::raw::c_char;
    use std::os::unix::io::FromRawFd;

    extern "C" {
        /// pty 对（测试伪串口，与 rr-ipc uart 测试同源）。
        fn openpty(
            amaster: *mut i32,
            aslave: *mut i32,
            name: *mut c_char,
            termp: *const std::ffi::c_void,
            winp: *const std::ffi::c_void,
        ) -> i32;
    }

    /// pty 对 + slave 设备路径（macOS /dev/ttysNNN）。master 返给测试做"对端写手"，
    /// slave 保活 fd 由测试持有（bridge 会按路径 open 它自己的两个句柄）。
    // c_char 平台差异：aarch64-linux 上是 u8（`c as u8` 冗余），x86_64-linux/macOS
    // 上是 i8（cast 必要）——不能裸删，target 无关写法只能保留 cast 抑制该 lint。
    #[allow(clippy::unnecessary_cast)]
    fn pty_with_path() -> (std::fs::File, std::fs::File, PathBuf) {
        let mut master = 0i32;
        let mut slave = 0i32;
        let mut name = [0 as c_char; 128];
        // SAFETY：三个出参都是本栈合法内存，其余按 NULL 传。
        let rc = unsafe {
            openpty(
                &mut master,
                &mut slave,
                name.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(rc, 0, "openpty 失败: {}", std::io::Error::last_os_error());
        let len = name.iter().position(|&c| c == 0).expect("name 未 NUL 结尾");
        let path: std::vec::Vec<u8> = name[..len].iter().map(|&c| c as u8).collect();
        let path = String::from_utf8(path).expect("pty 路径非 UTF-8");
        // SAFETY：master/slave 都是 openpty 刚出的合法 fd，File 接管并负责 close。
        unsafe {
            (
                std::fs::File::from_raw_fd(master),
                std::fs::File::from_raw_fd(slave),
                PathBuf::from(path),
            )
        }
    }

    /// 每用例独占的套接字路径（按测试名 + 进程号，避免并行互踩）。
    fn sock_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rr3-bridge-test-{}-{tag}.sock", std::process::id()))
    }

    /// 起一条测试链路（后台线程跑 run 的单链面），返回关停标志。
    fn spawn_link(dev: PathBuf, sock: PathBuf) -> Arc<AtomicBool> {
        let shutdown = Arc::new(AtomicBool::new(false));
        let cfg = BridgeConfig {
            uart: dev,
            baud: RR3_BRIDGE_BAUD_DEFAULT,
            socket: sock,
        };
        let flag = Arc::clone(&shutdown);
        std::thread::Builder::new()
            .name("bridge-under-test".into())
            .spawn(move || {
                let _ = run(cfg, flag);
            })
            .expect("起被测桥失败");
        shutdown
    }

    /// 每用例看门狗：pty/UDS/桥任一环节挂死（master 是 File，没有读超时面，
    /// 桥不转发时 read 会永远阻塞）→ 30s 强杀测试进程，套件不整体挂死。
    fn watchdog() {
        std::thread::Builder::new()
            .name("test-watchdog".into())
            .spawn(|| {
                std::thread::sleep(Duration::from_secs(30));
                eprintln!("[test] 看门狗到点：用例 30s 未完成，强杀（101）");
                std::process::exit(101);
            })
            .expect("起看门狗失败");
    }

    /// accept 返回后等桥主线程把流交接给收发线程（`sink_tx.send` 在 connect
    /// 建立后几条指令才落地；不等这一拍，首块 UART 数据可能落进"无连接"窗口
    /// 被丢弃计数吃掉——那是真实语义，不是缺陷，测试得让开这个窗口）。
    fn settle() {
        std::thread::sleep(Duration::from_millis(100));
    }

    /// 带 timeout 的 accept（桥 connect 退避最长 1+2+4+…，给足 15s）。
    fn accept_timeout(lis: &std::os::unix::net::UnixListener) -> UnixStream {
        lis.set_nonblocking(true).expect("设非阻塞失败");
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            match lis.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).expect("恢复阻塞失败");
                    return s;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "15s 内未见桥 connect（桥没起来？）"
                    );
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("accept 失败: {e}"),
            }
        }
    }

    /// UDS 侧带 timeout 的 read_exact（等桥的转发落地；逐读 5s）。
    fn read_stream_exact(s: &mut UnixStream, buf: &mut [u8]) {
        s.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("设读超时失败");
        read_exact_timeout(s, buf);
    }

    /// read_exact 本体（master File 侧无超时面，由看门狗兜底）。
    fn read_exact_timeout<R: Read>(s: &mut R, buf: &mut [u8]) {
        let mut done = 0;
        while done < buf.len() {
            let n = s.read(&mut buf[done..]).expect("读转发流失败");
            assert!(n > 0, "对端提前关闭（转发丢了 {done}/{}）", buf.len());
            done += n;
        }
    }

    /// 双向透传逐字节恒等（含坏 magic 垃圾——桥不认识帧是零策略的字节面证据）。
    /// 1000B 载荷跨 512B 块边界，顺带证明分块重组不丢不改。
    #[test]
    fn relay_is_byte_identical_both_directions() {
        watchdog();
        let (mut master, _keep, dev) = pty_with_path();
        let sock = sock_path("relay");
        let _ = std::fs::remove_file(&sock);
        let lis = std::os::unix::net::UnixListener::bind(&sock).expect("bind 失败");
        let _shutdown = spawn_link(dev, sock.clone());
        let mut conn = accept_timeout(&lis);
        settle();

        // 上行：master（UART 对端）→ 桥 → UDS
        let up: std::vec::Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        use std::io::Write as _;
        master.write_all(&up).expect("写 master 失败");
        let mut got = vec![0u8; up.len()];
        read_stream_exact(&mut conn, &mut got);
        assert_eq!(got, up, "上行必须逐字节恒等（跨块）");

        // 下行：UDS → 桥 → master；开头塞坏 magic 头字节——桥必须原样搬运
        let mut down: std::vec::Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x52, 0x52, 0x33];
        down.extend((0..600u32).map(|i| (i % 253) as u8));
        conn.write_all(&down).expect("写 conn 失败");
        let mut got2 = vec![0u8; down.len()];
        read_exact_timeout(&mut master, &mut got2);
        assert_eq!(got2, down, "下行必须逐字节恒等（垃圾也不改写）");
    }

    /// 三态之一：UDS 未就绪期间上行丢弃且**不缓冲**——先写的字节在连接建立后
    /// 不得冒出来，后写的照常透传。
    #[test]
    fn upstream_discarded_while_uds_absent_not_buffered() {
        watchdog();
        let (mut master, _keep, dev) = pty_with_path();
        let sock = sock_path("discard");
        let _ = std::fs::remove_file(&sock); // 刻意无监听
        let _shutdown = spawn_link(dev, sock.clone());
        use std::io::Write as _;

        // 断连窗口：桥在退避里，此刻写的一律丢弃
        std::thread::sleep(Duration::from_millis(300));
        let lost: std::vec::Vec<u8> = (0..700u32).map(|i| (i % 249) as u8).collect();
        master.write_all(&lost).expect("写丢失段失败");
        std::thread::sleep(Duration::from_millis(400)); // 丢弃落地（不缓冲）

        // 建立"rrd"：桥应在其后的退避拍上连进来
        let lis = std::os::unix::net::UnixListener::bind(&sock).expect("bind 失败");
        let mut conn = accept_timeout(&lis);
        settle();
        let kept: std::vec::Vec<u8> = (0..400u32).map(|i| (i % 241) as u8).collect();
        master.write_all(&kept).expect("写保留段失败");
        let mut got = vec![0u8; kept.len()];
        read_stream_exact(&mut conn, &mut got);
        assert_eq!(
            got, kept,
            "连接后的字节必须原样透传，断连窗口的字节不得冒头（不缓冲）"
        );
    }

    /// 三态之三：UDS 断链而 UART 存活——rrd 侧断开后，桥保住 UART 并重连，
    /// 第二条连接上透传恢复。
    #[test]
    fn uds_death_keeps_uart_and_reconnects() {
        watchdog();
        let (mut master, _keep, dev) = pty_with_path();
        let sock = sock_path("heal");
        let _ = std::fs::remove_file(&sock);
        let lis = std::os::unix::net::UnixListener::bind(&sock).expect("bind 失败");
        let _shutdown = spawn_link(dev, sock.clone());
        let conn1 = accept_timeout(&lis);
        use std::io::Write as _;

        // rrd 侧断链（模拟 rrd 重启窗口）：丢弃测试侧半边，桥的 rd 侧见 EOF
        drop(conn1);
        let mut conn2 = accept_timeout(&lis); // 桥应在退避后重连
        settle();
        let payload: std::vec::Vec<u8> = (0..500u32).map(|i| (i % 247) as u8).collect();
        master.write_all(&payload).expect("重连后写失败");
        let mut got = vec![0u8; payload.len()];
        read_stream_exact(&mut conn2, &mut got);
        assert_eq!(got, payload, "断链重连后透传必须恢复（UART 未重开）");
    }
}
