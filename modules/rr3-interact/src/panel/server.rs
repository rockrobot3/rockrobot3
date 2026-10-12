//! 面板 HTTP 服务（0.0.4 批C，D3/Q3 裁定落地）：std 手写 HTTP/1.1 子集，
//! **零新 crates.io 依赖**——GET/POST + Content-Length + JSON body 就是全部，
//! 不做 keep-alive、不做 chunked、不做 TLS（局域网可信面声明落 README，
//! 不承诺公网暴露）。
//!
//! 架构边界（ADR-0006 裁定 3 的面板条款）：
//! - **面板不当发布者**——硬件控制只走 `rr3.call` RPC 归属门。这不是口号而是
//!   类型面：[`Rr3PanelDispatch`] 里根本没有 Publish 变体，路由器想越权也造不出
//!   发布动作（"越权拒"的静态形态）。
//! - **ctl 查询走短连**——`rr3.ctl.*` 只对未注册连接开放（ADR-0010），面板在
//!   本进程内另开一条裸 UDS 一问一答即断，与外部运维探针同权同拒。
//! - **beep/face 不出进程**——播放面与表情机归主循环所有，面板经动作通道
//!   （mpsc）请求主循环代执行，串行无锁，与 TTS 点名天然互斥。
//!
//! 端点白名单见 [`RR3_PANEL_ROUTES`]（legacy 四页签语义搬入，"面板：状态与版本/
//! 硬件自检/实时交互流/运行数据/重启"逐项有承接；WiFi 配网页随 0.0.4 批D 挂上）：
//! `GET /`（静态页）、`GET /api/status`（health+link 缓存）、`GET /api/data`
//! （counters）、`GET /api/logs`（rrd 日志环）、`POST /api/relay`（act RPC）、
//! `POST /api/beep`（进程内）、`POST /api/face`（进程内）、`POST /api/selftest`
//! （并列三调，各答各展示）、`POST /api/reboot`（ctl.reboot，双重门缺省拒）、
//! `GET /api/wifi/scan` + `POST /api/wifi/connect` + `POST /api/wifi/forget`
//! （0.0.4 批D：BusCall 透传 connect 三 RPC，参数裁决在 owner 模块）。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rr_bus::Rr3BusClient;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_frame_read, rr3_frame_write, Rr3Frame, Rr3Header, Rr3PayloadType,
};
use serde_json::{json, Value};

use super::RR3_PANEL_INDEX_HTML;

/// 面板监听面：env `RR3_PANEL_ADDR` 优先，缺省只绑回环（Q3 裁定）。
/// 板上 `0.0.0.0` 随装配单注入，真机验证后置（不 claim）。
pub const RR3_PANEL_ADDR_ENV: &str = "RR3_PANEL_ADDR";
pub const RR3_PANEL_ADDR_DEFAULT: &str = "127.0.0.1:8080";

/// 并发连接上限：面板是单人调试面，8 个并发连接已是慷慨上限；
/// 超限直接 503 拒——慢客户端不能把 accept 队列占穿。
pub const RR3_PANEL_MAX_CONNS: usize = 8;
/// 单请求读超时：半包/慢客户端的耐心上界（P3 风险缓解）
pub const RR3_PANEL_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// 头区上界：请求行 + 头，8KB 足够面板这种乖客户端
const RR3_PANEL_HEADER_LIMIT: usize = 8 * 1024;
/// body 上界：最大的是 selftest 的空 body，4KB 只是防滥用
const RR3_PANEL_BODY_LIMIT: usize = 4 * 1024;
/// 模块间 RPC 的应答耐心：relay 经 rrd 路由到 act，正常毫秒级，5s 是 10 倍余量
const RR3_PANEL_CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// 连接线程 spawn 失败的运维计数键（批M M-β I-4）：失败点账后续连，
/// `rr3.ctl.counters` 读面键 `rr3-interact/panel_spawn_fail`。
const RR3_PANEL_SPAWN_FAIL_KEY: &str = "panel_spawn_fail";

/// 并发槽位 RAII 守卫（批M M-β I-4）：acquire 即 +1、Drop 即 -1——连接线程
/// 正常返回与 panic 展开都归还槽位；spawn 失败时闭包（连同守卫）被丢弃，
/// 走同一 Drop 路径归还。旧实现 fetch_add 在 spawn 前、归还只在闭包尾手动
/// fetch_sub，panic/失败双路泄漏。
struct Rr3ConnSlot {
    live: Arc<AtomicUsize>,
}

impl Rr3ConnSlot {
    fn rr3_acquire(live: &Arc<AtomicUsize>) -> Self {
        live.fetch_add(1, Ordering::Relaxed);
        Self { live: live.clone() }
    }
}

impl Drop for Rr3ConnSlot {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 端点总表（I-9/I-12 唯一真值，0.0.5 批M MC-8③）：`(方法, 路径)` 二元组，
/// 自述文案与 serve 行的端点数由本表长度推导（文案不手写数字），match 分派与
/// 本表的漂移由 `route_table_matches_dispatch` 单测钉死——新增端点 = 加表 +
/// 过漂移门，不再改第三处文案。
pub const RR3_PANEL_ROUTES: &[(&str, &str)] = &[
    ("GET", "/"),
    ("GET", "/api/status"),
    ("GET", "/api/data"),
    ("GET", "/api/logs"),
    ("POST", "/api/relay"),
    ("POST", "/api/beep"),
    ("POST", "/api/face"),
    ("POST", "/api/selftest"),
    ("POST", "/api/reboot"),
    ("GET", "/api/wifi/scan"),
    ("POST", "/api/wifi/connect"),
    ("POST", "/api/wifi/forget"),
];

/// 启动自述（端点数由 [`RR3_PANEL_ROUTES`] 长度推导——I-9/I-12 同一真值）。
pub fn rr3_panel_surface() -> Vec<String> {
    vec![format!(
        "panel: http {}端点（缺省 127.0.0.1:8080，RR3_PANEL_ADDR 可改）",
        RR3_PANEL_ROUTES.len()
    )]
}

/// 主循环代执行的面板动作（beep/face），HTTP 线程经 mpsc 递给主循环。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3PanelAction {
    /// 喇叭测试音：进程内直调播放面（占位正弦 burst，真音随后续里程碑）
    Beep,
    /// 表情切换：进程内直调表情机（名字合法性由 Rr3Face 白名单裁决）
    Face { expression: String },
}

/// 路由器的产物。**没有 Publish 变体**——"面板不当发布者"由类型面钉死，
/// 越权发布在路由器里就无从构造（ADR-0006 裁定 3）。
#[derive(Debug, Clone, PartialEq)]
pub enum Rr3PanelDispatch {
    /// `GET /`：单文件内嵌静态页
    Html,
    /// `GET /api/status`：ctl.health + net/link 订阅缓存
    StatusQuery,
    /// `GET /api/data`：ctl.counters（运维计数账）
    CountersQuery,
    /// `GET /api/logs`：ctl.logs（rrd 日志环尾部）
    LogsQuery,
    /// 硬件控制：`rr3.call` 经 rrd 归属门转发（relay → `rr3-act/relay/set`）
    BusCall {
        topic: String,
        args: Value,
    },
    /// 进程内动作：主循环代执行（beep/face）
    PanelAct(Rr3PanelAction),
    /// `POST /api/selftest`：并列三调（beep + relay + face），各答各展示，
    /// 单项失败不连锁、无依赖链无状态机——**不算编排**（brain 唯一编排者，
    /// ADR-0007 边界注记）
    SelfTest,
    /// `POST /api/reboot`：ctl.reboot（rrd 侧双重门，缺省恒拒）
    Reboot,
    NotFound,
    MethodNotAllowed,
    BadRequest(String),
}

/// 解析出的一个 HTTP 请求（HTTP/1.1 子集：方法/路径/body）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3HttpRequest {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

/// 从连接上读一个请求：请求行 → 头（找 Content-Length）→ 读满 body。
/// 半包/慢客户端按 [`RR3_PANEL_READ_TIMEOUT`] 折 Err，不 panic 不占线程。
pub fn rr3_http_read_request(stream: &TcpStream) -> Result<Rr3HttpRequest, String> {
    stream
        .set_read_timeout(Some(RR3_PANEL_READ_TIMEOUT))
        .map_err(|e| format!("设读超时失败: {e}"))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|e| format!("try_clone 失败: {e}"))?,
    );

    // 请求行："METHOD /path HTTP/1.1"
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("读请求行失败: {e}"))?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| format!("请求行不完整: {line:?}"))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| format!("请求行缺路径: {line:?}"))?
        .to_string();

    // 头区：只认 Content-Length，其余忽略；空行收尾，总量超界即断
    let mut content_length = 0usize;
    let mut header_bytes = 0usize;
    loop {
        let mut head = String::new();
        reader
            .read_line(&mut head)
            .map_err(|e| format!("读头失败: {e}"))?;
        header_bytes += head.len();
        if header_bytes > RR3_PANEL_HEADER_LIMIT {
            return Err(format!("头区超界（>{RR3_PANEL_HEADER_LIMIT}B）"));
        }
        let head = head.trim_end();
        if head.is_empty() {
            break;
        }
        if let Some(v) = head
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(str::trim)
        {
            content_length = v
                .parse()
                .map_err(|_| format!("Content-Length 不是数字: {v:?}"))?;
        }
    }
    if content_length > RR3_PANEL_BODY_LIMIT {
        return Err(format!("body 超界（>{RR3_PANEL_BODY_LIMIT}B）"));
    }

    // body 按声明的 Content-Length 读满——半包在这里等满或超时
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("读 body 失败（半包/超时）: {e}"))?;
    Ok(Rr3HttpRequest { method, path, body })
}

/// 写一个 HTTP 应答并关连接（Connection: close：一问一答，与 ctl 短连同口径）
pub fn rr3_http_respond(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|e| format!("写应答失败: {e}"))
}

/// 请求体 JSON 解析：面板的 POST body 必须是 JSON 对象（空 body 视为 `{}`——
/// beep/reboot 这类无参动作允许裸 POST）
fn rr3_panel_body_json(body: &[u8]) -> Result<Value, String> {
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(json!({}));
    }
    let text = std::str::from_utf8(body).map_err(|e| format!("body 非 UTF-8: {e}"))?;
    serde_json::from_str(text).map_err(|e| format!("body 不是合法 JSON: {e}"))
}

/// 路由纯函数：方法 + 路径 + body → 分派。**端点白名单 = [`RR3_PANEL_ROUTES`]**，
/// 未知路径 404、方法不对 405、body 坏 400——面板不是攻击面兜底者，一切都显式。
pub fn rr3_panel_route(method: &str, path: &str, body: &[u8]) -> Rr3PanelDispatch {
    match path {
        "/" => match method {
            "GET" => Rr3PanelDispatch::Html,
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        "/api/status" => match method {
            "GET" => Rr3PanelDispatch::StatusQuery,
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        "/api/data" => match method {
            "GET" => Rr3PanelDispatch::CountersQuery,
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        "/api/logs" => match method {
            "GET" => Rr3PanelDispatch::LogsQuery,
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        "/api/relay" => {
            if method != "POST" {
                return Rr3PanelDispatch::MethodNotAllowed;
            }
            let args = match rr3_panel_body_json(body) {
                Ok(v) => v,
                Err(e) => return Rr3PanelDispatch::BadRequest(e),
            };
            // 参数在这里裁决（relay 只认布尔 on），归属门在 rrd（topic 必须在
            // act 的 exposes 里——它是既有 exposes，零新增，Q3 裁定）
            match args.get("on").and_then(Value::as_bool) {
                Some(on) => Rr3PanelDispatch::BusCall {
                    topic: "rr3-act/relay/set".to_string(),
                    args: json!({ "on": on }),
                },
                None => Rr3PanelDispatch::BadRequest("缺布尔参数 on".to_string()),
            }
        }
        "/api/beep" => match method {
            "POST" => Rr3PanelDispatch::PanelAct(Rr3PanelAction::Beep),
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        "/api/face" => {
            if method != "POST" {
                return Rr3PanelDispatch::MethodNotAllowed;
            }
            let args = match rr3_panel_body_json(body) {
                Ok(v) => v,
                Err(e) => return Rr3PanelDispatch::BadRequest(e),
            };
            match args.get("expression").and_then(Value::as_str) {
                Some(expression) => Rr3PanelDispatch::PanelAct(Rr3PanelAction::Face {
                    expression: expression.to_string(),
                }),
                None => Rr3PanelDispatch::BadRequest("缺字符串参数 expression".to_string()),
            }
        }
        "/api/selftest" => match method {
            "POST" => Rr3PanelDispatch::SelfTest,
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        "/api/reboot" => match method {
            "POST" => Rr3PanelDispatch::Reboot,
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        // 配网面（0.0.4 批D）：三端点全部 BusCall 透传 connect 三 RPC——
        // 参数裁决在 owner（connect 模块 parse_args 点名拒），面板只解析 JSON；
        // connect 的 body 原样透传（ssid/pass legacy 字段名，零翻译）
        "/api/wifi/scan" => match method {
            "GET" => Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/scan".to_string(),
                args: json!({}),
            },
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        "/api/wifi/connect" => {
            if method != "POST" {
                return Rr3PanelDispatch::MethodNotAllowed;
            }
            let args = match rr3_panel_body_json(body) {
                Ok(v) => v,
                Err(e) => return Rr3PanelDispatch::BadRequest(e),
            };
            Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/connect".to_string(),
                args,
            }
        }
        "/api/wifi/forget" => match method {
            "POST" => Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/forget".to_string(),
                args: json!({}),
            },
            _ => Rr3PanelDispatch::MethodNotAllowed,
        },
        _ => Rr3PanelDispatch::NotFound,
    }
}

/// 帧头时间戳：与本仓其余进程内副本同式（rr-bus client / rrd conn 各持一份私有实现）
fn rr3_now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// ctl 短连一问一答（ADR-0010 的面板侧执行面）：裸 UDS 连 rrd，发一条
/// `rr3.ctl.*` 请求（src=0，与外部运维探针同权），收一条应答即断。
/// 错误信封折 `Err`（HTTP 层转 502 语义的 JSON），成功回 result 对象。
pub fn rr3_ctl_query(bus_path: &str, method: &str, id: u64) -> Result<Value, String> {
    let mut stream = std::os::unix::net::UnixStream::connect(bus_path)
        .map_err(|e| format!("连总线失败: {e}"))?;
    stream
        .set_read_timeout(Some(RR3_PANEL_READ_TIMEOUT))
        .map_err(|e| format!("设读超时失败: {e}"))?;
    let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": {}});
    let payload = rr3_body_encode(Rr3PayloadType::Request, body.to_string().as_bytes());
    let frame = Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: 0, // ctl 连接没有 task，与外部运维探针同一写法
            dst_task: rr_bus::RR3_TASK_RRD,
            payload_len: payload.len() as u32,
        },
        payload,
    };
    rr3_frame_write(&mut stream, &frame).map_err(|e| format!("写 ctl 请求失败: {e:?}"))?;
    let frame = rr3_frame_read(&mut stream).map_err(|e| format!("读 ctl 应答失败: {e:?}"))?;
    let (ty, payload) = Rr3BusClient::rr3_frame_body(&frame)?;
    if ty != Rr3PayloadType::Response {
        return Err(format!("ctl 应答类型错误: {ty:?}"));
    }
    if let Some(err) = payload.get("error") {
        return Err(format!(
            "ctl {method} 被拒: {}",
            err.get("message").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    payload
        .get("result")
        .cloned()
        .ok_or_else(|| format!("ctl {method} 应答缺 result"))
}

/// 面板服务线程的共享面：总线句柄（clone = 同一连接的另一句柄）、总线路径
/// （ctl 短连用）、动作通道（beep/face 递主循环）、link 订阅缓存。
pub struct Rr3PanelShared {
    pub bus: Rr3BusClient,
    pub bus_path: String,
    pub actions: mpsc::Sender<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>,
    pub link: Arc<Mutex<Option<Value>>>,
}

/// 面板入口：绑 [`rr3_panel_addr`] 起 accept 循环（阻塞，调用方起线程收容）。
/// 每连接一线程，超 [`RR3_PANEL_MAX_CONNS`] 直接 503。
pub fn rr3_panel_serve(shared: Arc<Rr3PanelShared>) -> Result<(), String> {
    let addr = rr3_panel_addr();
    let listener = TcpListener::bind(&addr).map_err(|e| format!("绑面板 {addr} 失败: {e}"))?;
    println!(
        "面板就绪: http://{addr}（{}端点，控制全走 RPC 归属门）",
        RR3_PANEL_ROUTES.len()
    );
    rr3_panel_accept_loop(
        listener,
        shared,
        Arc::new(AtomicUsize::new(0)),
        || std::thread::Builder::new().name("rr3-panel-conn".into()),
    )
}

/// accept 循环本体（批M M-β I-4 抽缝）：listener、槽位账与线程工厂显式注入——
/// 测试用 `make_builder` 第一次返回必败 builder（`stack_size(usize::MAX)`
/// 触发 pthread_attr_setstacksize EINVAL，实测确定性；小值如 1 会被 macOS
/// 静默 clamp 成功，不可作注入）注入 spawn 失败，钉"起不来一个连接后面板
/// 仍可访问"（工单回归判据）。生产路径工厂返回普通 builder，行为与旧 serve
/// 零漂移。
fn rr3_panel_accept_loop(
    listener: TcpListener,
    shared: Arc<Rr3PanelShared>,
    live: Arc<AtomicUsize>,
    mut make_builder: impl FnMut() -> std::thread::Builder,
) -> Result<(), String> {
    loop {
        let (mut stream, _) = listener.accept().map_err(|e| format!("accept 失败: {e}"))?;
        if live.load(Ordering::Relaxed) >= RR3_PANEL_MAX_CONNS {
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
            shared_bump
                .bus
                .rr3_counter_bump(RR3_PANEL_SPAWN_FAIL_KEY, 1);
            eprintln!("面板连接线程起失败（连接弃置，服务继续）: {e}");
        }
    }
}

/// 监听面判定：env 优先，缺省回环（Q3）。独立成函数供测试钉形状。
pub fn rr3_panel_addr() -> String {
    std::env::var(RR3_PANEL_ADDR_ENV).unwrap_or_else(|_| RR3_PANEL_ADDR_DEFAULT.to_string())
}

/// 面板应答载体：状态码 + 原因 + 内容类型 + body。execute 一次定形，
/// handle 只管写出——没有"执行产物"与"编码"两层错位。
struct Rr3PanelReply {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Rr3PanelReply {
    fn json(status: u16, reason: &'static str, v: Value) -> Self {
        Self {
            status,
            reason,
            content_type: "application/json",
            body: v.to_string().into_bytes(),
        }
    }

    /// `GET /`：单文件内嵌静态页（text/html）
    fn html() -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type: "text/html; charset=utf-8",
            body: RR3_PANEL_INDEX_HTML.as_bytes().to_vec(),
        }
    }
}

/// 单连接处理：读请求 → 路由 → 执行 → 应答。任何错误都折 JSON 应答——
/// 面板连接没有"协议违规掉线"语义，坏请求回 400 就是它的全部惩罚。
fn rr3_panel_handle(stream: &mut TcpStream, shared: &Rr3PanelShared) {
    let req = match rr3_http_read_request(stream) {
        Ok(r) => r,
        Err(e) => {
            let reply = Rr3PanelReply::json(400, "Bad Request", json!({ "error": e }));
            let _ = rr3_http_respond(
                stream,
                reply.status,
                reply.reason,
                reply.content_type,
                &reply.body,
            );
            return;
        }
    };
    let reply = rr3_panel_execute(&req, shared);
    let _ = rr3_http_respond(
        stream,
        reply.status,
        reply.reason,
        reply.content_type,
        &reply.body,
    );
}

/// 路由产物的执行面：状态码三元组在返回处一次定形。
fn rr3_panel_execute(req: &Rr3HttpRequest, shared: &Rr3PanelShared) -> Rr3PanelReply {
    match rr3_panel_route(&req.method, &req.path, &req.body) {
        Rr3PanelDispatch::Html => Rr3PanelReply::html(),
        Rr3PanelDispatch::NotFound => {
            Rr3PanelReply::json(404, "Not Found", json!({ "error": "not found" }))
        }
        Rr3PanelDispatch::MethodNotAllowed => Rr3PanelReply::json(
            405,
            "Method Not Allowed",
            json!({ "error": "method not allowed" }),
        ),
        Rr3PanelDispatch::BadRequest(e) => {
            Rr3PanelReply::json(400, "Bad Request", json!({ "error": e }))
        }
        Rr3PanelDispatch::StatusQuery => {
            // health 是 ctl 查询；link 是订阅缓存（主循环从总线收 Event 存的）
            let health = rr3_ctl_query(&shared.bus_path, "rr3.ctl.health", 1);
            let link = shared.link.lock().map(|g| g.clone()).unwrap_or(None);
            match health {
                Ok(h) => Rr3PanelReply::json(200, "OK", json!({ "health": h, "link": link })),
                Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
            }
        }
        Rr3PanelDispatch::CountersQuery => {
            match rr3_ctl_query(&shared.bus_path, "rr3.ctl.counters", 2) {
                Ok(v) => Rr3PanelReply::json(200, "OK", v),
                Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
            }
        }
        Rr3PanelDispatch::LogsQuery => match rr3_ctl_query(&shared.bus_path, "rr3.ctl.logs", 3) {
            Ok(v) => Rr3PanelReply::json(200, "OK", v),
            Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "error": e })),
        },
        Rr3PanelDispatch::BusCall { topic, args } => {
            match shared
                .bus
                .rr3_call_topic(&topic, args, RR3_PANEL_CALL_TIMEOUT)
            {
                Ok(result) => {
                    Rr3PanelReply::json(200, "OK", json!({ "ok": true, "result": result }))
                }
                Err(e) => {
                    Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e }))
                }
            }
        }
        Rr3PanelDispatch::PanelAct(action) => match rr3_panel_act(shared, action) {
            Ok(result) => Rr3PanelReply::json(200, "OK", json!({ "ok": true, "result": result })),
            Err(e) => Rr3PanelReply::json(502, "Bad Gateway", json!({ "ok": false, "error": e })),
        },
        Rr3PanelDispatch::SelfTest => {
            // 并列三调（Q3）：各答各展示、无依赖链、单项失败不连锁——
            // 这不是编排（无状态机无裁决），brain 才是唯一编排者（ADR-0007）
            let mut out = serde_json::Map::new();
            let beep = rr3_panel_act(shared, Rr3PanelAction::Beep);
            out.insert(
                "speaker".into(),
                match beep {
                    Ok(v) => json!({ "ok": true, "result": v }),
                    Err(e) => json!({ "ok": false, "error": e }),
                },
            );
            let relay_on = shared.bus.rr3_call_topic(
                "rr3-act/relay/set",
                json!({ "on": true }),
                RR3_PANEL_CALL_TIMEOUT,
            );
            let relay_off = if relay_on.is_ok() {
                shared
                    .bus
                    .rr3_call_topic(
                        "rr3-act/relay/set",
                        json!({ "on": false }),
                        RR3_PANEL_CALL_TIMEOUT,
                    )
                    .is_ok()
            } else {
                false
            };
            out.insert(
                "relay".into(),
                json!({ "on": relay_on.is_ok(), "off": relay_off }),
            );
            let face = rr3_panel_act(
                shared,
                Rr3PanelAction::Face {
                    expression: "happy".into(),
                },
            );
            out.insert(
                "face".into(),
                match face {
                    Ok(v) => json!({ "ok": true, "result": v }),
                    Err(e) => json!({ "ok": false, "error": e }),
                },
            );
            Rr3PanelReply::json(200, "OK", Value::Object(out))
        }
        Rr3PanelDispatch::Reboot => match rr3_ctl_query(&shared.bus_path, "rr3.ctl.reboot", 4) {
            Ok(v) => Rr3PanelReply::json(200, "OK", v),
            // rrd 侧双重门缺省恒拒：面板如实转达拒因（403 语义用 502 会误导，
            // 直接回 403 + rrd 原文案——"解锁旋钮"是运维要看见的字）
            Err(e) => Rr3PanelReply::json(403, "Forbidden", json!({ "error": e })),
        },
    }
}

/// 进程内动作执行：递主循环、等应答（动作在主循环串行处理，与 TTS 点名互斥）。
/// 主循环回的是业务 Result——Err（不认的表情等）原样上抛给 execute 折 502 语义。
fn rr3_panel_act(shared: &Rr3PanelShared, action: Rr3PanelAction) -> Result<Value, String> {
    let (tx, rx) = mpsc::channel();
    shared
        .actions
        .send((action, tx))
        .map_err(|_| "主循环不在了（进程正在退出）".to_string())?;
    match rx.recv_timeout(RR3_PANEL_CALL_TIMEOUT) {
        Ok(v) => v,
        Err(RecvTimeoutError::Timeout) => Err("主循环处理动作超时".to_string()),
        Err(RecvTimeoutError::Disconnected) => Err("应答通道已断（主循环退出）".to_string()),
    }
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
            if let Ok(req) = rr3_frame_read(&mut stream) {
                let payload = rr3_body_encode(Rr3PayloadType::Response, rpc.to_string().as_bytes());
                let frame = Rr3Frame {
                    header: Rr3Header {
                        opcode: opcode::SEND,
                        flags: 0,
                        trace_id: 0,
                        timestamp_ns: rr3_now_ns(),
                        src_task: rr_bus::RR3_TASK_RRD,
                        dst_task: req.header.src_task, // 按对端自报回（注册期 rrd 同口径）
                        payload_len: payload.len() as u32,
                    },
                    payload,
                };
                let _ = rr3_frame_write(&mut stream, &frame);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_covers_all_table_entries() {
        // 端点逐一点名（全集 = RR3_PANEL_ROUTES，表↔分派漂移由下一条门钉）：
        assert_eq!(rr3_panel_route("GET", "/", b""), Rr3PanelDispatch::Html);
        assert_eq!(
            rr3_panel_route("GET", "/api/status", b""),
            Rr3PanelDispatch::StatusQuery
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/data", b""),
            Rr3PanelDispatch::CountersQuery
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/logs", b""),
            Rr3PanelDispatch::LogsQuery
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/relay", br#"{"on":true}"#),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-act/relay/set".into(),
                args: json!({ "on": true })
            }
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/beep", b""),
            Rr3PanelDispatch::PanelAct(Rr3PanelAction::Beep)
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/face", br#"{"expression":"happy"}"#),
            Rr3PanelDispatch::PanelAct(Rr3PanelAction::Face {
                expression: "happy".into()
            })
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/selftest", b""),
            Rr3PanelDispatch::SelfTest
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/reboot", b""),
            Rr3PanelDispatch::Reboot
        );
        // 配网三端点（0.0.4 批D）：scan/forget 无参 BusCall，connect body 透传
        assert_eq!(
            rr3_panel_route("GET", "/api/wifi/scan", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/scan".into(),
                args: json!({})
            }
        );
        assert_eq!(
            rr3_panel_route(
                "POST",
                "/api/wifi/connect",
                br#"{"ssid":"home","pass":"password8"}"#
            ),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/connect".into(),
                args: json!({ "ssid": "home", "pass": "password8" })
            }
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/wifi/forget", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/forget".into(),
                args: json!({})
            }
        );
    }

    /// 端点表 ↔ 路由分派漂移门（MC-8③/I-9）：表内每条必须分派成功（不是
    /// 404/405），表长 pin 与自述文案同源（端点数由 len() 推导）——新增端点 =
    /// 加表 + 改本 pin + 过本门，文案零改动。
    #[test]
    fn route_table_matches_dispatch() {
        assert_eq!(
            RR3_PANEL_ROUTES.len(),
            12,
            "端点表计数 pin：增删端点须同批改本 pin 与设计册 I-9 口径"
        );
        for (method, path) in RR3_PANEL_ROUTES {
            let body: &[u8] = match (*method, *path) {
                ("POST", "/api/relay") => br#"{"on":true}"#,
                ("POST", "/api/face") => br#"{"expression":"happy"}"#,
                ("POST", "/api/wifi/connect") => br#"{"ssid":"s","pass":"password8"}"#,
                _ => b"",
            };
            let d = rr3_panel_route(method, path, body);
            assert!(
                !matches!(
                    &d,
                    Rr3PanelDispatch::NotFound | Rr3PanelDispatch::MethodNotAllowed
                ),
                "{method} {path} 在端点表内但路由拒绝: {d:?}"
            );
        }
        // 反向：表外路径一律 404，不因加表而放开
        assert!(matches!(
            rr3_panel_route("GET", "/api/not-in-table", b""),
            Rr3PanelDispatch::NotFound
        ));
    }

    /// 越权拒的静态形态：路由器里没有"发布"这种产物——relay 只能以
    /// BusCall（rr3.call 经 rrd 归属门）出去。这是类型级断言，不是约定。
    #[test]
    fn hardware_control_is_bus_call_only_never_publish() {
        match rr3_panel_route("POST", "/api/relay", br#"{"on":false}"#) {
            Rr3PanelDispatch::BusCall { topic, .. } => {
                assert_eq!(topic, "rr3-act/relay/set", "面板控制走 act 既有 exposes");
            }
            other => panic!("relay 必须折 BusCall（无发布面）: {other:?}"),
        }
    }

    #[test]
    fn route_rejects_unknown_path_wrong_method_and_bad_body() {
        assert_eq!(
            rr3_panel_route("GET", "/api/none", b""),
            Rr3PanelDispatch::NotFound
        );
        assert_eq!(
            rr3_panel_route("DELETE", "/api/relay", b""),
            Rr3PanelDispatch::MethodNotAllowed
        );
        assert_eq!(
            rr3_panel_route("GET", "/", b""),
            Rr3PanelDispatch::Html,
            "GET / 不受其他端点方法约束"
        );
        // relay 缺 on / body 坏 JSON → 400 语义
        assert!(matches!(
            rr3_panel_route("POST", "/api/relay", b"{}"),
            Rr3PanelDispatch::BadRequest(_)
        ));
        assert!(matches!(
            rr3_panel_route("POST", "/api/relay", b"not-json"),
            Rr3PanelDispatch::BadRequest(_)
        ));
        assert!(matches!(
            rr3_panel_route("POST", "/api/face", b"{}"),
            Rr3PanelDispatch::BadRequest(_)
        ));
    }

    #[test]
    fn http_read_request_reads_full_body_and_rejects_half() {
        // 全量：声明 12B 读满 12B
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            rr3_http_read_request(&sock).unwrap()
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let full = b"POST /api/relay HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"on\":true}";
        c.write_all(full).unwrap();
        let req = t.join().unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/relay");
        assert_eq!(req.body, b"{\"on\":true}");

        // 半包：声明 12B 只发 4B → 读超时折 Err，不 panic 不挂死
        let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr2 = listener2.local_addr().unwrap();
        let t2 = std::thread::spawn(move || {
            let (sock, _) = listener2.accept().unwrap();
            rr3_http_read_request(&sock)
        });
        let mut c2 = TcpStream::connect(addr2).unwrap();
        c2.write_all(b"POST /api/relay HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"on")
            .unwrap();
        let got = t2.join().unwrap();
        assert!(got.is_err(), "半包必须折 Err（读超时），不能装作读完");
    }

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
        let v = rr3_ctl_query(path.to_str().unwrap(), "rr3.ctl.health", 1).unwrap();
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
        let got = rr3_ctl_query(path.to_str().unwrap(), "rr3.ctl.reboot", 4);
        assert!(got.is_err(), "error 信封必须折 Err");
        assert!(
            got.unwrap_err().contains("reboot"),
            "拒因原文要在错误里（面板如实转达 rrd 文案）"
        );
        jh2.join().unwrap();
    }

    // ---------- 批M M-β I-4：spawn 失败不退 accept 循环 + 槽位 RAII ----------

    /// 假 rrd（I-4 夹具）：注册回 src_task=7、心跳回 accepted，并把每拍心跳
    /// params 原样记账——测试轮询 params.counters 断言 panel_spawn_fail 上报。
    const I4_MANIFEST: &str = "[manifest]\nschema_version = 1\nname = \"rr3-interact\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/bin/true\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 50\n";

    #[cfg(test)]
    fn rr3_i4_fake_rrd(listener: std::os::unix::net::UnixListener, beats: Arc<Mutex<Vec<Value>>>) {
        while let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            loop {
                let Some(frame) = rr3_frame_read(&mut stream).ok() else { break };
                let Ok((ty, body)) = rr3_body_split_of(&frame) else { break };
                let Ok(v) = serde_json::from_slice::<Value>(&body) else { break };
                if ty != Rr3PayloadType::Request {
                    continue;
                }
                let method = v["method"].as_str().unwrap_or("");
                let result = match method {
                    "rr3.register" => json!({ "src_task": 7 }),
                    "rr3.heartbeat" => {
                        beats.lock().unwrap().push(v["params"].clone());
                        json!({ "beat": "accepted" })
                    }
                    _ => json!({ "noop": true }),
                };
                let id = v["id"].as_u64().unwrap_or(0);
                let payload = rr3_body_encode(
                    Rr3PayloadType::Response,
                    json!({"jsonrpc": "2.0", "id": id, "result": result})
                        .to_string()
                        .as_bytes(),
                );
                let out = Rr3Frame {
                    header: Rr3Header {
                        opcode: opcode::SEND,
                        flags: 0,
                        trace_id: 0,
                        timestamp_ns: rr3_now_ns(),
                        src_task: rr_bus::RR3_TASK_RRD,
                        dst_task: 7,
                        payload_len: payload.len() as u32,
                    },
                    payload,
                };
                if rr3_frame_write(&mut stream, &out).is_err() {
                    break;
                }
            }
        }
    }

    /// rr3_body_split 的 Result 化薄封装（夹具内让 `let Ok(..) .. else` 可用）
    #[cfg(test)]
    fn rr3_body_split_of(frame: &Rr3Frame) -> Result<(Rr3PayloadType, Vec<u8>), ()> {
        rr_ipc::rr3_body_split(&frame.payload).map_err(|_| ())
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
            std::thread::spawn(move || rr3_i4_fake_rrd(listener, beats));
        }
        std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
        let bus = Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功");
        std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);

        let (actions_tx, _rx) = mpsc::channel();
        let shared = Arc::new(Rr3PanelShared {
            bus,
            bus_path: bus_path.to_string_lossy().into_owned(),
            actions: actions_tx,
            link: Arc::new(Mutex::new(None)),
        });

        let live = Arc::new(AtomicUsize::new(0));
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = tcp.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let shared2 = shared.clone();
        let live2 = live.clone();
        let loop_jh = std::thread::spawn(move || {
            rr3_panel_accept_loop(tcp, shared2, live2, move || {
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

        // 槽位账平：失败连接（闭包连守卫被丢）与正常连接（守卫 Drop）都归还
        assert_eq!(
            live.load(Ordering::Relaxed),
            0,
            "失败与正常连接都必须归还槽位（RAII 账平）"
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
}
