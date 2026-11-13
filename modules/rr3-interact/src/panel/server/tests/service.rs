//! 服务面：ctl 短连、槽位 RAII、派生失败不退循环、回环判定。

use super::*;

#[test]
fn ctl_query_roundtrips_and_reports_errors() {
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
fn conn_slot_guard_releases_on_panic_unwind() {
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

/// 工单回归判据：起不来一个连接后面板仍可访问。spawn 必败注入
/// （stack_size(usize::MAX)）打头一条连接，后续连接照常 200；槽位账平；
/// 失败进 counters（panel_spawn_fail 随心跳自报）。
#[test]
fn panel_survives_spawn_failure_and_bumps_counter() {
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

    // 假 rrd + 真总线客户端（RR3_BUS_PATH 注入，连上即撤 env）
    let dir = std::env::temp_dir().join(format!("panel-i4-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bus_path = dir.join("i4.sock");
    let beats: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let listener = std::os::unix::net::UnixListener::bind(&bus_path).unwrap();
    {
        let beats = beats.clone();
        std::thread::spawn(move || rr3_i4_fake_rrd(listener, beats, json!({}), json!({})));
    }
    std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
    let bus = BusHandle::wrap(Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功"));
    std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);

    let (actions_tx, _rx) = mpsc::channel();
    let shared = panel_shared(bus, bus_path.to_string_lossy().into_owned(), actions_tx);

    let live = Arc::new(AtomicUsize::new(0));
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = tcp.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = calls.clone();
    let shared2 = shared.clone();
    let live2 = live.clone();
    let loop_jh = std::thread::spawn(move || {
        rr3_accept_loop(tcp, shared2, live2, move || {
            if calls2.fetch_add(1, Ordering::Relaxed) == 0 {
                std::thread::Builder::new().stack_size(usize::MAX) // 第 1 次：必败注入
            } else {
                std::thread::Builder::new().name("rr3-panel-conn".into())
            }
        })
    });

    // 第 1 条连接：spawn 失败 → 连接被弃置（无应答；服务端未读请求即关，
    // 客户端按平台收到 RST 或 FIN——两态共同点是**没有任何应答字节**）
    let mut c1 = TcpStream::connect(addr).unwrap();
    c1.write_all(b"GET / HTTP/1.1\r\nHost: t\r\n\r\n").unwrap();
    let _ = c1.shutdown(std::net::Shutdown::Write);
    let mut dropped = Vec::new();
    let _ = c1.read_to_end(&mut dropped);
    assert!(
        dropped.is_empty(),
        "spawn 失败的连接必须被弃置（无应答）: {dropped:?}"
    );

    // 第 2 条连接：正常 builder → 200 OK（工单判据：面板仍可访问）
    let mut c2 = TcpStream::connect(addr).unwrap();
    let _ = c2.set_read_timeout(Some(Duration::from_secs(5)));
    c2.write_all(b"GET / HTTP/1.1\r\nHost: t\r\n\r\n").unwrap();
    let mut rbuf = [0u8; 64];
    let n = c2.read(&mut rbuf).expect("spawn 失败后面板必须仍有应答");
    let head = String::from_utf8_lossy(&rbuf[..n]).to_string();
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "spawn 失败后面板必须仍可访问: {head:?}"
    );

    // 槽位账平：失败连接（闭包连守卫被丢）与正常连接（守卫 Drop）都归还。
    // c2 的 handler 线程写完应答到 Drop 归还槽位之间无同步点——读到 200
    // 不等于线程已退出（门禁全量并行下撞过一回调度窗），故有界等待账平
    // （1s 上界；RAII 保证终会归还，此处不等调度器）。
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

    // counters 记账：假 rrd 收到的心跳 params.counters 带上 panel_spawn_fail
    //（心跳快照是累计值，多拍取 max；心跳周期 50ms，4s 窗口足量）
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
    drop(loop_jh); // accept 循环不退出（判据本身），随测试进程收尾
}

/// 回环判定：IP 才判 loopback；主机名/非法形状按非回环（保守告警）。
#[test]
fn panel_loopback_only_accepts_loopback_ips() {
    assert!(rr3_panel_loopback("127.0.0.1:8080"));
    assert!(rr3_panel_loopback("[::1]:8080"));
    assert!(!rr3_panel_loopback("0.0.0.0:8080"));
    assert!(!rr3_panel_loopback("192.168.1.10:8080"));
    assert!(!rr3_panel_loopback("localhost:8080"));
    assert!(!rr3_panel_loopback("not-an-addr"));
}
