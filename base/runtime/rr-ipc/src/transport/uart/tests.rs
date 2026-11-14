//! UART 传输单测（自同名单测模块原样抽出，判据不变）。

use super::*;
// no_std 显式化（0.0.7 批B 口径）：std prelude 不随 `#![no_std]` 隐式注入
use crate::protocol::header::{opcode, Rr3HdrError, Rr3Header};
use crate::transport::buf::Rr3FrameBuf;
use crate::transport::frame::{rr3_frame_to_bytes, Rr3Frame, Rr3FrameError};
use crate::transport::Rr3TransportConn;
use std::os::unix::io::FromRawFd;
// no_std 显式化：String 不在 no_std prelude 里，测试面按 std 路径显式引入
use std::string::String;
use std::vec;

extern "C" {
    /// pty 对（测试伪串口）。glibc ≥2.34 并入 libc，musl/macOS 原生就有。
    fn openpty(
        amaster: *mut i32,
        aslave: *mut i32,
        name: *mut CChar,
        termp: *const Termios,
        winp: *const std::ffi::c_void,
    ) -> i32;
}

fn hdr(payload_len: u32) -> Rr3Header {
    let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload_len);
    h.trace_id = 0x1234_5678;
    h.timestamp_ns = 100;
    h.src_task = 7;
    h
}

/// 构造与读回比对的帧：头里的 CRC 由载荷算出（线面真值，不是发送前的 0）
fn frame(header: Rr3Header, payload: &[u8]) -> Rr3Frame {
    let mut f = Rr3Frame {
        header,
        payload: payload.to_vec(),
    };
    f.header.payload_crc32 = crate::protocol::header::rr3_crc32(payload);
    f
}

/// 伪串口对：master 是裸 fd（测试经 File 写入），slave 走与真设备同一条
/// termios 配置路径（from_fd → cfmakeraw/VMIN/speed/tcsetattr）。
fn pty_pair(baud: u32) -> (std::fs::File, Rr3UartConn) {
    let mut master = 0i32;
    let mut slave = 0i32;
    // SAFETY：两个出参都是本栈 i32，其余按 NULL 传。
    let rc = unsafe {
        openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(rc, 0, "openpty 失败：{}", std::io::Error::last_os_error());
    let conn = Rr3UartConn::from_fd(slave, baud).expect("slave termios 配置失败");
    // SAFETY：master 是 openpty 刚出的合法 fd，File 接管并负责 close。
    (unsafe { std::fs::File::from_raw_fd(master) }, conn)
}

/// 伪串口对（带 slave 设备路径）：openpty 的 `name` 出参给出 slave 路径
/// （macOS /dev/ttysNNN、Linux /dev/pts/N），供"按路径 open"的用例走真实
/// `open()` 入口（含 configure 全链：termios + 开口净化）。返回的 slave File
/// 由调用方持有保活（不参与收发，仅保证 slave 端在线——openpty 的出参 fd
/// 不归还就会丢，测试进程短命，随进程回收）。
#[cfg(test)]
fn pty_pair_with_path() -> (std::fs::File, std::fs::File, std::path::PathBuf) {
    let mut master = 0i32;
    let mut slave = 0i32;
    let mut name = [0 as CChar; 128];
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
    assert_eq!(rc, 0, "openpty 失败：{}", std::io::Error::last_os_error());
    let len = name
        .iter()
        .position(|&c| c == 0)
        .expect("openpty name 未 NUL 结尾");
    let path: std::vec::Vec<u8> = name[..len].iter().map(|&c| c as u8).collect();
    let path = String::from_utf8(path).expect("pty 路径非 UTF-8");
    // SAFETY：master/slave 都是 openpty 刚出的合法 fd，各自 File 接管并负责 close。
    unsafe {
        (
            std::fs::File::from_raw_fd(master),
            std::fs::File::from_raw_fd(slave),
            std::path::PathBuf::from(path),
        )
    }
}

/// 半帧用例需要"读超时"而不是阻塞：把读侧切到 VMIN=0/VTIME=1（100ms 静默即返回 0）。
fn set_read_timeout(conn: &Rr3UartConn) {
    conn.rr3_set_read_timeout(1).expect("设读超时失败");
}

/// 宿主往返必须能醒来：VMIN=0/VTIME 档下无数据静默即回错误（Closed 形状），
/// 而不是像缺省阻塞读那样永久挂住——板死/断线时调用线程必须能被释放。
#[test]
fn read_timeout_bounds_empty_recv() {
    let (_master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
    conn.rr3_set_read_timeout(2).expect("设读超时失败");
    let t0 = std::time::Instant::now();
    let r = conn.rr3_recv_buf();
    let dt = t0.elapsed();
    assert!(r.is_err(), "无数据必须报错（不得假装收到帧）: {r:?}");
    assert!(
        dt >= std::time::Duration::from_millis(150),
        "不得提前返回（应等满静默窗口）={dt:?}"
    );
    assert!(
        dt < std::time::Duration::from_secs(2),
        "超时档未生效（仍在阻塞）={dt:?}"
    );
}

/// 空闲前导：`last_tx` 归 None（开口后首突发 / 长空闲）后首个写必须先出
/// 一个 `\n` 再出正式字节——帧失步根因处置的线上行为钉在这里（2026-10-30，
/// 判据实验与机理见 [`Rr3UartConn::tx_idle_preamble`]）。
#[test]
fn tx_idle_preamble_precedes_burst_after_idle() {
    let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
    // 模拟"开口后从未写过"：from_fd 出厂置 Some(now)，这里显式归 None。
    conn.last_tx = None;
    let back = frame(hdr(3), b"abc");
    conn.rr3_send_buf(Rr3FrameBuf::from(&back)).unwrap();
    use std::io::Read as _;
    let mut pre = [0u8; 1];
    master.read_exact(&mut pre).unwrap();
    assert_eq!(pre[0], 0x0A, "空闲后首突发必须以 \\n 前导开头");
    // 紧接着就是完整帧（2ms 间隔远小于 pty 读的阻塞窗）
    let want = rr3_frame_to_bytes(&back).unwrap();
    let mut echo = vec![0u8; want.len()];
    master.read_exact(&mut echo).unwrap();
    assert_eq!(echo, want, "前导之后正式帧必须逐字节完整");

    // 连续第二次发送：last_tx 刚更新，不得再插前导
    conn.rr3_send_buf(Rr3FrameBuf::from(&back)).unwrap();
    let mut echo2 = vec![0u8; want.len()];
    master.read_exact(&mut echo2).unwrap();
    assert_eq!(echo2, want, "紧邻的第二次突发不得插前导");
}

/// 正常帧：整帧写入 → 整帧读回（阻塞路径）；反向发送同一条 blanket impl 面验证。
#[test]
fn uart_round_trip_normal_frame() {
    let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
    let f = frame(hdr(5), b"hello");
    let bytes = rr3_frame_to_bytes(&f).unwrap();
    use std::io::Write as _;
    master.write_all(&bytes).unwrap();

    let got = conn.rr3_recv_buf().expect("正常帧必须整帧读回");
    assert_eq!(
        Rr3Frame::try_from(got).unwrap(),
        f,
        "PTY 往返逐字节一致（raw 模式不得吃改写任何字节）"
    );

    // 反向：conn 发送（blanket impl → write_all + flush）→ master 读回
    let back = frame(hdr(3), b"abc");
    conn.rr3_send_buf(Rr3FrameBuf::from(&back)).unwrap();
    let mut echo = vec![0u8; bytes_len(&back)];
    master.read_exact(&mut echo).unwrap();
    assert_eq!(echo, rr3_frame_to_bytes(&back).unwrap());
}

fn bytes_len(f: &Rr3Frame) -> usize {
    crate::protocol::header::RR3_IPC_HDR_SIZE + f.payload.len()
}

/// 坏 magic：头层拒收路径在真实 fd 上保持同语义（Header(BadMagic)），且
/// 拒收后连接不报废——后续正常帧照常收发。
#[test]
fn uart_rejects_bad_magic_then_resumes() {
    let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
    let mut bad = hdr(2).to_bytes().to_vec();
    bad[0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    use std::io::Write as _;
    master.write_all(&bad).unwrap();
    assert!(
        matches!(
            conn.rr3_recv_buf(),
            Err(Rr3FrameError::Header(Rr3HdrError::BadMagic { .. }))
        ),
        "坏 magic 必须被头层拒收"
    );

    let f = frame(hdr(1), b"z");
    master.write_all(&rr3_frame_to_bytes(&f).unwrap()).unwrap();
    let got = conn.rr3_recv_buf().expect("拒收后必须能重新同步收帧");
    assert_eq!(Rr3Frame::try_from(got).unwrap(), f);
}

/// 半帧：只写 64 字节即静默——读侧 VTIME 超时吐 0，read_fill 计数给出
/// `TornHeader { got: 64 }`（半帧不得当新帧起始）；随后整帧到达仍能整帧读回
/// （runbook ⑤ 的"注入后不死锁"要求在单测里的等价形状）。
#[test]
fn uart_rejects_torn_header_then_resyncs() {
    let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
    set_read_timeout(&conn);
    let mut torn = hdr(9).to_bytes().to_vec();
    torn.truncate(64);
    use std::io::Write as _;
    master.write_all(&torn).unwrap();
    assert_eq!(
        conn.rr3_recv_buf(),
        Err(Rr3FrameError::TornHeader { got: 64 }),
        "半帧必须按已读计数拒收"
    );

    // 恢复阻塞语义再收完整帧（与真设备默认路径一致）
    let f = frame(hdr(4), b"res4");
    master.write_all(&rr3_frame_to_bytes(&f).unwrap()).unwrap();
    let got = conn.rr3_recv_buf().expect("半帧后新帧必须完整读回");
    assert_eq!(Rr3Frame::try_from(got).unwrap(), f);
}

/// 超时读（机制面新增护栏，d4 600s 实锤）：静默必须 `Ok(0)` 且**按时醒来**
/// （不死锁不空转）；来数据必须读到，且超时档读过的字节与原帧逐字节一致。
#[test]
fn uart_read_timeout_idle_zero_then_data() {
    let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
    let mut buf = [0u8; 64];
    let t0 = std::time::Instant::now();
    assert_eq!(
        conn.rr3_read_timeout(&mut buf, 80).unwrap(),
        0,
        "静默必须 Ok(0)（此刻无数据语义）"
    );
    assert!(
        t0.elapsed() >= std::time::Duration::from_millis(70),
        "必须等满超时才返回（未按时醒来 = 语义漂移）"
    );
    use std::io::Write as _;
    let f = frame(hdr(3), b"t/o");
    let bytes = rr3_frame_to_bytes(&f).unwrap();
    master.write_all(&bytes).unwrap();
    // 就绪读可能是部分读，循环收满整帧
    let mut got = std::vec::Vec::new();
    while got.len() < bytes.len() {
        let n = conn
            .rr3_read_timeout(&mut buf, 2000)
            .expect("有数据必须能读");
        assert!(n > 0, "有数据不得 Ok(0)");
        got.extend_from_slice(&buf[..n]);
    }
    assert_eq!(got, bytes, "超时档读出的字节必须与原帧逐字节一致");
}

/// 设备不存在 → open 报 IO 错（错误路径不 panic、不静默）。
#[test]
fn uart_open_rejects_missing_device() {
    assert!(Rr3UartConn::open(Path::new("/nonexistent/rr3-uart-test")).is_err());
}

/// 开口即净（批D1 敞口处置的守门用例）：开口**之前**积压在输入队列的垃圾
/// （坏 magic 头字节，CP2102 残留同形）必须被 configure 的 tcflush 冲掉——
/// 随后开口写入的完整帧整帧读回，垃圾不得混入。走真实 open(path) 入口。
#[test]
fn uart_open_flushes_stale_input() {
    let (mut master, _slave_keepalive, slave_path) = pty_pair_with_path();
    use std::io::Write as _;
    // 残留字节：一条坏 magic 头 + 尾巴（若不被冲掉，开口首读必 BadMagic）
    let mut stale = hdr(4).to_bytes().to_vec();
    stale[0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    stale.extend_from_slice(b"leftover");
    master.write_all(&stale).unwrap();
    // 开口（configure 末尾 tcflush(TCIFLUSH) 冲队列）
    let mut conn = Rr3UartConn::open_with_baud(&slave_path, RR3_UART_BAUD_DEFAULT)
        .expect("按路径 open pty slave 失败");
    // 开口后的新帧必须整帧读回（残留不得当帧前缀混入）
    let f = frame(hdr(5), b"fresh");
    master.write_all(&rr3_frame_to_bytes(&f).unwrap()).unwrap();
    let got = conn.rr3_recv_buf().expect("开口后首帧必须完整读回");
    assert_eq!(
        Rr3Frame::try_from(got).unwrap(),
        f,
        "开口前积压字节必须被 tcflush 净化，开口后字节零改写"
    );
}

/// 波特率档位表：三档内放行，档位外拒绝（静默换档是线路事故）。
#[test]
fn uart_baud_speed_table() {
    assert!(rr3_uart_baud_speed(115200).is_ok());
    assert!(rr3_uart_baud_speed(9600).is_ok());
    assert!(rr3_uart_baud_speed(921600).is_ok());
    assert!(rr3_uart_baud_speed(0).is_err());
    assert!(
        rr3_uart_baud_speed(230400).is_err(),
        "档位动物园不得静默扩张"
    );
}

/// 排他开口·同进程重入（批G G0 守门用例）：同一节点同进程两次 open 都放行
/// ——桥 rx/tx 双开（open_uart_pair）是仓内合法形状，flock 账本只对跨进程
/// 生效。走真实 open(path) 入口。
///
/// 重开的有界重试（批N21 件一拆修）：并行测试里本用例与跨进程用例的
/// spawn 同刻竞争——`Command::spawn` 复制父进程 fd 表的瞬间，fork 副本
/// 持有本进程 guard fd 的 open file description 引用，把 flock 锁扣到
/// 子进程 exec 完成（O_CLOEXEC 生效）才放——毫秒级窗口撞上"账退尽后
/// 重开"即 EAGAIN。这是 macOS posix_spawn 语义的固有瞬态（lsof 抓不到
/// 锁主：诊断窗内 exec 已完成、fd 已关），不是账本泄漏；有界重试即过。
/// 真泄漏形状（退账漏 close）锁是永久的，重试恒败照常 panic——守门不弱化。
#[test]
fn uart_open_same_process_pair_reentrant() {
    let (_master, _slave_keepalive, slave_path) = pty_pair_with_path();
    let _rx = Rr3UartConn::open(&slave_path).expect("首次 open 必须成功");
    let _tx = Rr3UartConn::open(&slave_path).expect("同进程双开（桥 rx/tx 形状）必须放行");
    // 双双 drop 退账到零，随后第三次 open 仍须成功（账目可复用，无泄漏）。
    drop(_rx);
    drop(_tx);
    let mut _again = None;
    for attempt in 0..10 {
        match Rr3UartConn::open(&slave_path) {
            Ok(conn) => {
                _again = Some(conn);
                break;
            }
            Err(e) if format!("{e}").contains("flock 排他") && attempt < 9 => {
                std::eprintln!(
                    "uart 重入用例：重开撞上并发 spawn 的 flock 瞬态（第 {} 次），有界重试",
                    attempt + 1
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => panic!("账退尽后重开必须成功: {e}"),
        }
    }
    assert!(
        _again.is_some(),
        "有界重试后重开仍失败＝锁非瞬态（账本泄漏形状）"
    );
}

/// 排他开口·跨进程被拒（批G G0 守门用例，批E E7 实锤）：另一进程持锁时本
/// 进程 open 必须以文案点名拒绝——E7 "残留旧桥静默截流" 的现场形状。
/// 子进程探针：复用本测试二进制，holder 模式下按父进程传入的 pty 路径 open，
/// 就绪文件握手（harness 自身的 stdout 输出不可作握手信道），父进程关 stdin
/// 即收兵。
#[test]
fn uart_excl_cross_process_second_process_rejected() {
    let (_master, _slave_keepalive, slave_path) = pty_pair_with_path();
    if std::env::var("RR3_UART_EXCL_PROBE").as_deref() == Ok("holder") {
        let path = std::env::var("RR3_UART_EXCL_PATH").expect("holder 必须拿到 pty 路径");
        let conn = Rr3UartConn::open(std::path::Path::new(&path)).expect("holder open 必须成功");
        let ready = std::env::var("RR3_UART_EXCL_READY").expect("holder 必须拿到就绪文件路径");
        std::fs::write(&ready, b"R").expect("holder 写就绪文件失败");
        let mut b = [0u8; 1];
        let _ = std::io::stdin().read(&mut b); // 父进程关 stdin 即收兵
        drop(conn);
        return;
    }
    let ready = std::env::temp_dir().join(format!("rr3-excl-probe-{}.r", std::process::id()));
    let _ = std::fs::remove_file(&ready);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "transport::uart::tests::uart_excl_cross_process_second_process_rejected",
            "--exact",
            "--nocapture",
        ])
        .env("RR3_UART_EXCL_PROBE", "holder")
        .env("RR3_UART_EXCL_PATH", &slave_path)
        .env("RR3_UART_EXCL_READY", &ready)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn 自身测试二进制失败");
    // 等就绪文件（holder 持锁成功的信号），10s 上限。
    let mut ready_seen = false;
    for _ in 0..200 {
        if ready.exists() {
            ready_seen = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if matches!(child.try_wait(), Ok(Some(_))) || !ready_seen {
        let _ = std::fs::remove_file(&ready);
        panic!("holder 子进程 10s 内未就绪（pty 路径：{slave_path:?}）");
    }
    match Rr3UartConn::open(&slave_path) {
        Ok(_) => {
            drop(child.stdin.take());
            let _ = child.wait();
            let _ = std::fs::remove_file(&ready);
            panic!("跨进程二次 open 必须被拒（flock 排他）");
        }
        Err(err) => {
            let msg = format!("{err}");
            assert_eq!(
                err.kind(),
                std::io::ErrorKind::Other,
                "跨进程被拒必须 Other+文案点名（实得 {msg}）"
            );
            assert!(
                msg.contains("flock 排他"),
                "错误文案必须点名 flock 排他：{msg}"
            );
        }
    }
    drop(child.stdin.take()); // 关 stdin 收兵
    let _ = child.wait();
    let _ = std::fs::remove_file(&ready);
}
