//! 服务循环：监听、accept、连接线程与派生失败不退循环的守护。

use super::*;

/// 面板服务线程的共享面：总线句柄（[`BusHandle`]，clone = 同一连接的另一句柄，
/// 0.0.8 W-A 批4 换型——面板的 rr3.call 与主循环应答互不干扰）、总线路径
/// （ctl 短连用）、动作通道（beep/face 递主循环）、link 订阅缓存、五限额
/// （I-18②：config 数据化贯穿）与自检期望表情（I-17/M-10：产线取向可配）。
pub struct Rr3PanelShared {
    pub bus: BusHandle,
    pub bus_path: String,
    pub actions: mpsc::Sender<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>,
    pub link: Arc<Mutex<Option<Value>>>,
    /// 面板五限额/超时（批N4 件五 I-18②：装配期自 config 构造）
    pub limits: Rr3PanelLimits,
    /// 自检 face 腿的期望表情（批N4 件五 I-17/M-10：在册白名单值，'static
    /// 来自表情册 lookup，跨线程安全）
    pub selftest_face_expect: &'static str,
    /// 自检预期表（批N5 件五 F-31）：固定腿逐名期望三态 + wiring 组预期，
    /// verdict = 结果集与预期集等值（装配期 parse 已对词表，'static 值）
    pub selftest_expect: Rr3SelftestExpect,
}

/// 面板入口：绑 [`rr3_panel_addr`] 起 accept 循环（阻塞，调用方起线程收容）。
/// 每连接一线程，超 [`Rr3PanelLimits::max_conns`] 直接 503。
pub fn rr3_panel_serve(shared: Arc<Rr3PanelShared>) -> Result<(), String> {
    let addr = rr3_panel_addr();
    let listener = TcpListener::bind(&addr).map_err(|e| format!("绑面板 {addr} 失败: {e}"))?;
    eprintln!(
        "面板就绪: http://{addr}（{}端点，控制全走 RPC 归属门）",
        RR3_PANEL_ROUTES.len()
    );
    rr3_panel_warn_addr(&addr);
    rr3_accept_loop(listener, shared, Arc::new(AtomicUsize::new(0)), || {
        std::thread::Builder::new().name("rr3-panel-conn".into())
    })
}

/// 安全面自述：面板写口无令牌鉴权，非回环监听是"可信局域网"决策，启动即点名。
fn rr3_panel_warn_addr(addr: &str) {
    if !rr3_panel_loopback(addr) {
        eprintln!(
            "面板警告: 监听 {addr} 非回环——写口无令牌鉴权（继电器/密钥/配网/重启），仅限可信局域网"
        );
    }
}

/// 监听是否只绑回环：可解析 IP 才判 loopback；主机名/非法形状按非回环处理
/// （保守告警，不猜解析结果）。
fn rr3_panel_loopback(addr: &str) -> bool {
    addr.parse::<std::net::SocketAddr>()
        .map(|a| a.ip().is_loopback())
        .unwrap_or(false)
}

/// 面板动作入队通道：动作 + 代执行回执发送端（主循环 on_tick 排空）
pub type Rr3PanelActRx = mpsc::Receiver<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>;

/// 面板状态页 link 槽（主循环 on_event 写、面板线程读）
pub type Rr3PanelLink = Arc<Mutex<Option<Value>>>;

/// [`spawn_host`] 的装配返回：动作入队端 + 状态页 link 槽
pub type Rr3PanelHost = (Rr3PanelActRx, Rr3PanelLink);

/// 面板线程宿主（0.0.8 W-A 批4 函数化，方案 §4.6）：link Arc＋动作通道＋
/// 五限额毫秒→Duration 换算＋[`Rr3PanelShared`] 构造＋线程 spawn 收进一处，
/// main.rs 一行装配。返回 `(act_rx, link)` 给 Dispatcher——act_rx 主循环
/// on_tick 排空（beep/face 代执行回执）、link 主循环 on_event 只写/面板线程
/// 只读（状态页卡片数据源，Mutex 语义不变）。线程名 rr3-panel；spawn 失败
/// Err 上抛（fail-closed，装配段显式回错）。
pub fn spawn_host(
    bus: &BusHandle,
    panel: &crate::boot::Rr3PanelCfg,
    selftest: &crate::boot::Rr3SelftestCfg,
) -> Result<Rr3PanelHost, String> {
    let link = Arc::new(Mutex::new(None::<Value>));
    let (act_tx, act_rx) = mpsc::channel();
    // 面板五限额/超时（批N4 件五 I-18②）：config 毫秒值转 Duration，装配期
    // 一次换算，server 侧机制面只见 Duration 不再携带策略
    let limits = Rr3PanelLimits {
        max_conns: panel.max_conns as usize,
        read_timeout: Duration::from_millis(panel.read_timeout_ms),
        header_limit: panel.header_limit as usize,
        body_limit: panel.body_limit as usize,
        call_timeout: Duration::from_millis(panel.call_timeout_ms),
    };
    let shared = Arc::new(Rr3PanelShared {
        bus: bus.clone(),
        bus_path: std::env::var(RR3_BUS_PATH_ENV)
            .unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string()),
        actions: act_tx,
        link: Arc::clone(&link),
        limits,
        selftest_face_expect: selftest.face_expect,
        selftest_expect: selftest.expect,
    });
    rr3_panel_spawn(shared)?;
    Ok((act_rx, link))
}

/// 面板线程起转：绑核失败点名为装配期错（服务退出只落日志，不拉主循环）。
fn rr3_panel_spawn(shared: Arc<Rr3PanelShared>) -> Result<(), String> {
    std::thread::Builder::new()
        .name("rr3-panel".into())
        .spawn(move || rr3_panel_guard(shared))
        .map_err(|e| format!("起面板线程失败: {e}"))?;
    Ok(())
}

/// 面板线程体：serve 退出即点名（面板故障不带走主循环）。
fn rr3_panel_guard(shared: Arc<Rr3PanelShared>) {
    if let Err(e) = rr3_panel_serve(shared) {
        eprintln!("面板服务退出: {e}");
    }
}

/// accept 循环本体（批M M-β I-4 抽缝）：listener、槽位账与线程工厂显式注入——
/// 测试用 `make_builder` 第一次返回必败 builder（`stack_size(usize::MAX)`
/// 触发 pthread_attr_setstacksize EINVAL，实测确定性；小值如 1 会被 macOS
/// 静默 clamp 成功，不可作注入）注入 spawn 失败，钉"起不来一个连接后面板
/// 仍可访问"（工单回归判据）。生产路径工厂返回普通 builder，行为与旧 serve
/// 零漂移。
fn rr3_accept_loop(
    listener: TcpListener,
    shared: Arc<Rr3PanelShared>,
    live: Arc<AtomicUsize>,
    mut make_builder: impl FnMut() -> std::thread::Builder,
) -> Result<(), String> {
    // accept 失败退避（与 rrd 守护 accept 同款策略，200ms）：单次失败不许带走
    // 服务面——面板是运维的最后一扇门，EMFILE/ENFILE 等瞬态错误后必须还能接客。
    const RR3_ACCEPT_BACKOFF: Duration = Duration::from_millis(200);
    loop {
        let (mut stream, _) = match listener.accept() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("面板 accept 失败（继续服务）: {e}");
                std::thread::sleep(RR3_ACCEPT_BACKOFF);
                continue;
            }
        };
        // 连接 IO 耐心（读侧已在 rr3_http_read_req 设，写侧在此按同一预算补）：
        // 对端只连不读时 write_all 会一直堵在“发送缓冲已满”，8 个槽位被钉死＝
        // 面板整体失联（运维最后一扇门自锁，静态审计 B16）。
        if let Err(e) = rr3_http_patience(&stream, shared.limits.read_timeout) {
            eprintln!("面板连接写耐心未设上（连接仍按既有语义处理）: {e}");
        }
        if live.load(Ordering::Relaxed) >= shared.limits.max_conns {
            // 拒连必须留痕（信息面）：面板"打得开但一直 503"时，只有这行能指出
            // 是对端洪水还是自家槽位泄漏——对端地址是最小充分证据。
            let peer = stream
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|_| "?".to_string());
            eprintln!(
                "面板并发已达上限 {}（在服 {}），拒连对端 {peer}",
                shared.limits.max_conns,
                live.load(Ordering::Relaxed)
            );
            let body = br#"{"error":"conn limit"}"#;
            let _ = rr3_http_respond(
                &mut stream,
                503,
                "Service Unavailable",
                "application/json",
                body,
            );
            continue;
        }
        let slot = Rr3ConnSlot::rr3_acquire(&live);
        let shared = shared.clone();
        let shared_bump = shared.clone(); // Err 分支记账用（闭包 move 走主句柄）
                                          // spawn 失败（批M M-β I-4）：闭包（连同 stream 与守卫）被丢弃——守卫
                                          // Drop 归还槽位、连接自然关闭。**不退循环**：面板是运维的最后一扇门，
                                          // 一条连接起不来不能把整个面板带走（工单判据"起不来一个连接后面板仍
                                          // 可访问"）；点账 counters + stderr 点名，服务继续。
        if let Err(e) = make_builder().spawn(move || {
            rr3_panel_handle(&mut stream, &shared);
            drop(slot); // RAII 守卫：正常返回或 panic 展开都在此归还槽位
        }) {
            shared_bump.bus.counter_bump(RR3_PANEL_SPAWN_FAIL_KEY, 1);
            eprintln!("面板连接线程起失败（连接弃置，服务继续）: {e}");
        }
    }
}

/// 监听面判定：env 优先，缺省回环（Q3）。独立成函数供测试钉形状。
pub fn rr3_panel_addr() -> String {
    std::env::var(RR3_PANEL_ADDR_ENV).unwrap_or_else(|_| RR3_PANEL_ADDR_DEFAULT.to_string())
}

/// 单连接处理：读请求 → 路由 → 执行 → 应答。任何错误都折 JSON 应答——
/// 面板连接没有"协议违规掉线"语义，坏请求回 400 就是它的全部惩罚。
fn rr3_panel_handle(stream: &mut TcpStream, shared: &Rr3PanelShared) {
    let req = match rr3_http_read_req(stream, &shared.limits) {
        Ok(r) => r,
        Err(e) => {
            let reply = Rr3PanelReply::json(400, "Bad Request", json!({ "error": e }));
            rr3_reply_send(stream, &reply);
            return;
        }
    };
    let reply = rr3_panel_execute(&req, shared);
    rr3_reply_send(stream, &reply);
}

/// 应答写出（坏请求与正常应答同一出口，写失败不打断连接语义）。
fn rr3_reply_send(stream: &mut TcpStream, reply: &Rr3PanelReply) {
    let _ = rr3_http_respond(
        stream,
        reply.status,
        reply.reason,
        reply.content_type,
        &reply.body,
    );
}

/// 假 rrd 的 ctl 应答器（单测夹具）：accept 一条连接，读一帧，按预置的**完整
/// JSON-RPC 应答**（result 或 error 信封，测试自组——与 rrd 真实两态同形）回一帧。
#[cfg(test)]
fn rr3_fake_ctl_rrd(
    listener: std::os::unix::net::UnixListener,
    rpc: Value,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            if let Ok(req) = rr_ipc::rr3_frame_read(&mut stream) {
                let payload = rr3_body_encode(Rr3PayloadType::Response, rpc.to_string().as_bytes());
                let frame = Rr3Frame {
                    header: Rr3Header {
                        opcode: opcode::SEND,
                        flags: 0,
                        trace_id: 0,
                        timestamp_ns: rr3_now_ns(),
                        src_task: rr_core::RR3_TASK_RRD,
                        dst_task: req.header.src_task, // 按对端自报回（注册期 rrd 同口径）
                        payload_len: payload.len() as u32,
                        payload_crc32: rr_ipc::rr3_crc32(&payload),
                    },
                    payload,
                };
                let _ = rr3_frame_write(&mut stream, &frame);
            }
        }
    })
}
