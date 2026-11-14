//! 链路机制集成测试（pty + UDS 对跑——宿主档的"假 rrd"半边）。

// 线程在测试结束后自然随进程回收；shutdown 标志让 run_link 里的退避循环
// 不会空转（测试结束时置位即可）。

use super::conn::Rr3LinkSlot;
use super::*;

use std::io::{ErrorKind, Read, Write};
use std::os::raw::c_char;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// 交接槽语义：只保留最新一条，旧句柄在换手瞬间关闭（防"接了就关"的
/// 反复重连把 fd 攒到耗尽）。旧句柄关闭用对端 EOF 证明。
#[test]
fn slot_keeps_latest_and_closes_previous() {
    let slot = Rr3LinkSlot::new();
    assert!(slot.take().is_none(), "空槽取走必须为 None");
    let (a, mut a_peer) = UnixStream::pair().expect("pair 失败");
    let (b, mut b_peer) = UnixStream::pair().expect("pair 失败");
    assert!(slot.put(a).is_none(), "首个放入不顶替");
    assert!(slot.put(b).is_some(), "第二个放入必须顶掉旧的");
    // macOS 的 AF_UNIX 上 SO_RCVTIMEO 不被接受（EINVAL），这里用非阻塞读：
    // 旧句柄已关 → 立即 0（EOF）；未关 → WouldBlock。
    a_peer.set_nonblocking(true).expect("设非阻塞失败");
    let mut one = [0u8; 1];
    match a_peer.read(&mut one) {
        Ok(0) => {}
        other => panic!("旧句柄必须在换手时关闭（读得 {other:?}）"),
    }
    let mut got = slot.take().expect("槽里应有最新连接");
    assert!(slot.take().is_none(), "取走后槽必须为空");
    b_peer.write_all(b"x").expect("写最新连接对端失败");
    got.set_nonblocking(true).expect("设非阻塞失败");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match got.read(&mut one) {
            Ok(1) => break,
            Ok(_) => panic!("最新连接读到空"),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "最新连接 2s 内不可读");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("读最新连接失败: {e}"),
        }
    }
    assert_eq!(one[0], b'x', "最新连接必须仍可用");
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
    // 桥对板链路的**首个下行突发**带空闲前导 `\n`（rr-ipc Rr3UartConn
    // tx_idle_preamble：帧失步根因处置，板面把孤立 \n 当空行跳过）——
    // master 侧先吃掉它，再按恒等比对其余字节。
    let mut pre = [0u8; 1];
    read_exact_timeout(&mut master, &mut pre);
    assert_eq!(pre[0], 0x0A, "下行首突发必须以空闲前导 \\n 开头");
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
