// SPDX-License-Identifier: Apache-2.0
//! 服务面：ctl 短连、槽位 RAII、派生失败不退循环、回环判定。

use std::path::Path;

use super::*;

#[test]
fn ctl_query_errors() {
    let dir = std::env::temp_dir().join(format!("panel-ctl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ctl.sock");

    // 成功回路：假 rrd 回 {"modules":[]}，result 原样回来
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let jh = rr3_fake_ctl_rrd(
        listener,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "modules": [] } }),
    );
    let v = rr3_ctl_query(
        path.to_str().unwrap(),
        "rr3.ctl.health",
        1,
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(v["modules"], json!([]));
    jh.join().unwrap();

    // 错误信封：假 rrd 以 error 信封应答 → 折 Err 且文案带原文
    //（UDS 文件不随 listener drop 清理，重绑前先删）
    let _ = std::fs::remove_file(&path);
    let listener2 = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let jh2 = rr3_fake_ctl_rrd(
        listener2,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "error": { "code": 3, "message": "reboot 执行面未解锁（需 cfg(linux) 且 env RR3_REBOOT_OK=1）" }
        }),
    );
    // 假 rrd 回的是 error 信封，rr3_ctl_query 读到 error 字段 → Err
    let got = rr3_ctl_query(
        path.to_str().unwrap(),
        "rr3.ctl.reboot",
        4,
        Duration::from_secs(5),
    );
    assert!(got.is_err(), "error 信封必须折 Err");
    assert!(
        got.unwrap_err().contains("reboot"),
        "拒因原文要在错误里（面板如实转达 rrd 文案）"
    );
    jh2.join().unwrap();
}

/// RAII 守卫的 panic 路径：连接线程 panic 展开时槽位必须归还（旧实现
/// fetch_sub 在闭包尾，panic 即泄漏）。
#[test]
fn conn_slot_unwind() {
    let live = Arc::new(AtomicUsize::new(0));
    let r = std::panic::catch_unwind(|| {
        let _slot = Rr3ConnSlot::rr3_acquire(&live);
        assert_eq!(live.load(Ordering::Relaxed), 1, "acquire 记 +1");
        panic!("boom（展开即走 Drop 归还）");
    });
    assert!(r.is_err(), "panic 应被 catch_unwind 捕获");
    assert_eq!(
        live.load(Ordering::Relaxed),
        0,
        "panic 展开必须归还槽位（守卫 Drop）"
    );
}

/// 假 rrd + 真总线客户端（RR3_BUS_PATH 注入，连上即撤 env）。
fn fake_bus(dir: &Path) -> (BusHandle, Arc<Mutex<Vec<Value>>>, String) {
    let bus_path = dir.join("i4.sock");
    let beats = Arc::new(Mutex::new(Vec::new()));
    let listener = std::os::unix::net::UnixListener::bind(&bus_path).unwrap();
    {
        let beats = beats.clone();
        std::thread::spawn(move || rr3_i4_fake_rrd(listener, beats, json!({}), json!({})));
    }
    let _env = rr3_env_lock();
    std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
    let bus = BusHandle::wrap(Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功"));
    std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);
    (bus, beats, bus_path.to_string_lossy().into_owned())
}

/// 起 accept 循环：第 1 条连接的 spawn 必败注入，其余走正常 builder。
fn start_panel(
    bus: BusHandle,
    bus_path: String,
    tcp: TcpListener,
    live: Arc<AtomicUsize>,
) -> std::thread::JoinHandle<Result<(), String>> {
    let (actions_tx, _rx) = mpsc::channel();
    let shared = panel_shared(bus, bus_path, actions_tx);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = calls.clone();
    let shared2 = shared.clone();
    let live2 = live.clone();
    std::thread::spawn(move || {
        rr3_accept_loop(tcp, shared2, live2, move || {
            if calls2.fetch_add(1, Ordering::Relaxed) == 0 {
                std::thread::Builder::new().stack_size(usize::MAX) // 第 1 次：必败注入
            } else {
                std::thread::Builder::new().name("rr3-panel-conn".into())
            }
        })
    })
}

/// 第 1 条连接：spawn 失败 → 连接被弃置（无应答；平台差异只影响 RST/FIN）。
fn first_conn_dropped(addr: std::net::SocketAddr) {
    let mut c = TcpStream::connect(addr).unwrap();
    c.write_all(b"GET / HTTP/1.1\r\nHost: t\r\n\r\n").unwrap();
    let _ = c.shutdown(std::net::Shutdown::Write);
    let mut dropped = Vec::new();
    let _ = c.read_to_end(&mut dropped);
    assert!(
        dropped.is_empty(),
        "spawn 失败的连接必须被弃置（无应答）: {dropped:?}"
    );
}

/// 第 2 条连接：正常 builder → 200 OK（工单判据：面板仍可访问）。
fn second_conn_200(addr: std::net::SocketAddr) {
    let mut c = TcpStream::connect(addr).unwrap();
    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
    c.write_all(b"GET / HTTP/1.1\r\nHost: t\r\n\r\n").unwrap();
    let mut rbuf = [0u8; 64];
    let n = c.read(&mut rbuf).expect("spawn 失败后面板必须仍有应答");
    let head = String::from_utf8_lossy(&rbuf[..n]).to_string();
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "spawn 失败后面板必须仍可访问: {head:?}"
    );
}

/// 槽位账平：失败连接（闭包连守卫被丢）与正常连接（守卫 Drop）都归还。
/// 读到 200 不等于 handler 线程已退出，故有界等待（1s 上界）。
fn wait_slots_released(live: &Arc<AtomicUsize>) {
    let settled = (0..100).any(|_| {
        if live.load(Ordering::Relaxed) == 0 {
            true
        } else {
            std::thread::sleep(Duration::from_millis(10));
            false
        }
    });
    assert!(
        settled,
        "失败与正常连接都必须归还槽位（RAII 账平）: live={}",
        live.load(Ordering::Relaxed)
    );
}

/// counters 记账：心跳快照是累计值，多拍取 max（心跳周期 50ms，4s 窗口足量）。
fn wait_spawn_counter(beats: &Arc<Mutex<Vec<Value>>>) {
    let mut seen = 0u64;
    for _ in 0..40 {
        seen = beats
            .lock()
            .unwrap()
            .iter()
            .filter_map(|p| p.get("counters"))
            .filter_map(|c| c.get(RR3_PANEL_SPAWN_FAIL_KEY))
            .filter_map(Value::as_u64)
            .max()
            .unwrap_or(0);
        if seen >= 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        seen >= 1,
        "spawn 失败必须进 counters 心跳自报（panel_spawn_fail）: {seen}"
    );
}

/// 工单回归判据：起不来一个连接后面板仍可访问。spawn 必败注入
/// （stack_size(usize::MAX)）打头一条连接，后续连接照常 200；槽位账平；
/// 失败进 counters（panel_spawn_fail 随心跳自报）。
#[test]
fn panel_fail_counter() {
    // 前置自证：stack_size(usize::MAX) 的 spawn 在本平台确定性失败
    //（attr_setstacksize EINVAL；实测小值如 1 会被 macOS 静默 clamp，
    // 不可作注入，故取上溢值）
    assert!(
        std::thread::Builder::new()
            .stack_size(usize::MAX)
            .spawn(|| {})
            .is_err(),
        "stack_size(usize::MAX) 必须起不来线程（注入有效性自证）"
    );

    let dir = std::env::temp_dir().join(format!("panel-i4-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (bus, beats, bus_path) = fake_bus(&dir);
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = tcp.local_addr().unwrap();
    let live = Arc::new(AtomicUsize::new(0));
    let loop_jh = start_panel(bus, bus_path, tcp, live.clone());
    first_conn_dropped(addr);
    second_conn_200(addr);
    wait_slots_released(&live);
    wait_spawn_counter(&beats);
    drop(loop_jh); // accept 循环不退出（判据本身），随测试进程收尾
}

/// 回环判定：IP 才判 loopback；主机名/非法形状按非回环（保守告警）。
#[test]
fn panel_ok_ips() {
    assert!(rr3_panel_loopback("127.0.0.1:8080"));
    assert!(rr3_panel_loopback("[::1]:8080"));
    assert!(!rr3_panel_loopback("0.0.0.0:8080"));
    assert!(!rr3_panel_loopback("192.168.1.10:8080"));
    assert!(!rr3_panel_loopback("localhost:8080"));
    assert!(!rr3_panel_loopback("not-an-addr"));
}
