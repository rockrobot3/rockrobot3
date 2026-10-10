//! espidf-sentinel —— 0.0.7 批D1 层③ 哨兵（esp-rs std，机制核 `rr-core::runtime`
//! 的真板消费者）。
//!
//! 职责（0.0.7 方案 §2.1 修订版 9 口径 + §9.3 层③ runbook）：**内跑
//! `rr-core::runtime`**——连接态转移 [`Rr3LinkState`]、心跳拍点
//! [`Rr3ModuleRuntime`]、收帧分类 [`rr3_dispatch_class`]——**不另写状态机**；
//! 传输下底只写窄缝 [`Rr3ByteStream`] 两方法（esp-idf-hal UART2 半分体，经
//! rr-ipc std 单向 adapter + blanket conn 免费获得与 uds/uart 宿主侧同源的
//! 帧语义）。本文件只做平台事：组帧/发帧/线程/堆采样——与 std 驱动器
//! （rr-bus client）同构：reader 线程 + 心跳线程 + 一次一答 pending 表。
//!
//! 双证之一（编译面）：`cargo +esp build` 在 xtensa-esp32-espidf 上编译
//! rr-core + rr-ipc + 本哨兵（专用门 tools/ci/espidf-sentinel-check.sh）；
//! 双证之二（板上真跑）：注册→心跳→round-trip + 三点堆采样（README runbook）。
//! D1→D2 触发判据：实测运行峰值剩余堆 < 20% 总堆（§9.3 ④）。
//!
//! 板侧接线（批C 载体同款硬约束）：UART2 = GPIO17→适配器 RXD / GPIO16←适配器
//! TXD，115200，3.3V TTL，共地；UART0 = 控制台（monitor 观测面）。
//!
//! esp-idf-hal 0.44.1 API 对表实录（vendored 源码核对，批D1）：
//! - 构造走 `UartDriver::new(uart, tx, rx, cts: Option, rts: Option, &config)`
//!   ——TX/RX 半体的两个独立 `new()` 会对同一 UART 口各做一次
//!   `uart_driver_install`（IDF 一口一装，第二装必败），故整装后
//!   `Box::leak` + `split()` 出 `'static` 半体；
//! - 半体驱动的 io 面是 `embedded_io`（固有 `read(buf, delay)` / `write` /
//!   `flush`，错误面 `EspError`），std::io 的 Read/Write 桥在本文件手写；
//! - 上游只给全双工 `UartDriver` 标了 `unsafe Send + Sync`，半体驱动漏标——
//!   本文件以两个 `unsafe impl Send` 补齐（安全性论证见各结构体注记）；
//! - xtensa LX6 无 64 位原子，`AtomicU64` 不可用，id 发号/task 号/计数全部
//!   `AtomicU32`。

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_hal::gpio::{AnyInputPin, AnyOutputPin};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::uart::{UartConfig, UartDriver, UartRxDriver, UartTxDriver};
use esp_idf_hal::units::Hertz;
use esp_idf_sys::{esp_get_free_heap_size, esp_get_minimum_free_heap_size, link_patches};
use serde_json::{json, Value};

use rr_core::runtime::{
    rr3_dispatch_class, Rr3Dispatch, Rr3GuardBuf, Rr3GuardEvent, Rr3ModuleRuntime,
};
use rr_core::{rr3_frame_compose, RR3_TASK_RRD, RR3_TRACE_ID_DIRECT};
use rr_ipc::transport::Rr3TransportConn;
use rr_ipc::{opcode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3FrameBuf, Rr3PayloadType};

/// 模块名（manifest.name 唯一真值——线上注册面）。
const NAME: &str = "rr3-espidf-sentinel";
/// 心跳间隔（manifest [health].heartbeat_ms 同值；机制核保底 ≥1ms 的输入）。
const HEARTBEAT_MS: u64 = 200;
/// 线上 manifest（rr3.register 的 params.manifest）：entry 绝对路径是 rrd 门禁
/// 硬项——哨兵是 ESP32 固件、不被 rrd spawn，entry 仅过"绝对路径"门，真实执行体
/// 是本固件；restart_policy = "never"（进程面监督对本模块不适用，watchdog 判死
/// 走在册表记账）。`[rpc].exposes` 是 M3 `rr3.call` 的路由表来源（0.0.7 §9.4 ⑤
/// e2e round-trip：sim_peer 点名 probe 主题 → 本哨兵应答），不声明 = 不接点名。
const MANIFEST_TEXT: &str = "[manifest]\nschema_version = 1\nname = \"rr3-espidf-sentinel\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/opt/rr3/bin/rr3-espidf-sentinel\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[resources]\nrestart_policy = \"never\"\n\n[health]\nheartbeat_ms = 200\nwatchdog_timeout_ms = 2000\n\n[rpc]\nexposes = [\"rr3-espidf-sentinel/probe\"]\n";

const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);
const RPC_TIMEOUT: Duration = Duration::from_secs(5);
/// reader/心跳线程的栈面（板上实测实锤：8KB 不够 serde_json 解析 + json! 构造 +
/// FrameBuf 的峰值——溢出踩坏堆 → Arc 指针踩成垃圾 → std Mutex 惰性 pthread 初始化
/// 崩溃（EXCVADDR=0xb）；16KB 后板上三点实测全程无崩溃。IDF pthread 缺省栈偏小，
/// 此值即运行下限，勿再调低）
const THREAD_STACK: usize = 16384;
/// 断开后的复位延迟（让 monitor 把日志冲完再 restart，避免紧复位刷屏）。
const RESTART_DELAY: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// UART2 半分体 → 窄缝（板侧接总线的全部：两个 Read/Write 半体 + rr-ipc adapter）
// ---------------------------------------------------------------------------

/// TX 半体内体：多发送方共享（注册 / 心跳线程 / round-trip 应答），互斥在
/// 外挂 `Mutex` 锁内。写面真、读面故意不存在（Err = Unsupported）：conn 只用
/// rr3_send_buf。
struct UartTxInner(UartTxDriver<'static>);

// SAFETY：esp-idf-hal 0.44.1 只给全双工 `UartDriver` 标了 `unsafe Send + Sync`
// （其状态同为 port + Owner + queue），半体驱动是同源状态的拆面、上游漏标属
// 保守留白。IDF uart 驱动上下文本就线程安全（uart_write_bytes/uart_read_bytes
// 按 IDF 契约可并发调用），且本仓对 TX 半体的全部访问都在外挂 `Mutex` 锁内
// ——`Send` 的主张与上游 `UartDriver` 同据。
unsafe impl Send for UartTxInner {}

impl Write for UartTxInner {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // 固有 write（IDF 阻塞写满 tx FIFO/缓冲），错误面 EspError → io::Error
        self.0
            .write(buf)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
    fn flush(&mut self) -> io::Result<()> {
        // flush 已弃用（0.44.1），语义等价替换为 wait_done（等 TX FIFO 排空；
        // timeout = portMAX_DELAY 与阻塞 flush 同语义）
        self.0
            .wait_done(u32::MAX)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

impl Read for UartTxInner {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "tx 半体无读面（conn 只用于 send）",
        ))
    }
}

/// RX 半体：reader 线程独占。读面真（阻塞读，delay = portMAX_DELAY）。
/// 写面故意不存在（Err = Unsupported）：conn 只用 rr3_recv_buf。
struct UartRxHalf(UartRxDriver<'static>);

// SAFETY：同 [`UartTxInner`] 的论证——半体是全双工驱动同源状态的拆面，IDF
// 读面上游本就允许跨任务调用；本仓 RX 半体由 reader 线程独占（move 语义），
// 无并发访问面。
unsafe impl Send for UartRxHalf {}

impl Read for UartRxHalf {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // 固有 read 带 delay 参数（portMAX_DELAY = u32::MAX，TickType_t=u32），
        // 阻塞至有字节或驱动错误；错误面 EspError → io::Error
        self.0
            .read(buf, u32::MAX)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

impl Write for UartRxHalf {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "rx 半体无写面（conn 只用于 recv）",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 共享状态（与 rr-bus client 的 Rr3Inner 同构，字段只留哨兵用到的）
// ---------------------------------------------------------------------------

struct Shared {
    /// 发送互斥体（TX 半体经 adapter + blanket impl 的 conn 面，锁内只调
    /// rr3_send_buf——&mut 语义在锁内成立）
    tx: Arc<Mutex<UartTxInner>>,
    /// 机制核状态机：连接态 + 拍点（真值唯一处；reader 与心跳线程各持锁短段）
    rt: Mutex<Rr3ModuleRuntime>,
    /// rrd 分配的 task 号（注册前 0，"用户态不得自设"的客户端侧写法；
    /// xtensa LX6 无 64 位原子，u32 域对 task 号/发号器/计数充足）
    task: AtomicU32,
    /// 断开旗标：Down 后心跳线程退出、主循环复位（"不装活着"→ 运行环复位）
    stop: AtomicBool,
    /// JSON-RPC id 发号器（自 1 起）
    next_id: AtomicU32,
    /// 一次一答 pending 表：id → 应答投递口
    pending: Mutex<HashMap<u32, mpsc::SyncSender<Value>>>,
    /// 单调时基锚点（批I A-2 口径同源：拍点全链同一 Instant 锚，espidf 的
    /// Instant 走 esp_timer，单调）
    epoch: Instant,
    /// 本地守护行缓冲（rr-core::runtime H6 行文法的真板消费者：boot/feed 两类）
    guard: Mutex<Rr3GuardBuf>,
    /// round-trip 应答计数（首答触发堆采样③）
    round_trips: AtomicU32,
}

impl Shared {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    fn task(&self) -> u64 {
        self.task.load(Ordering::Relaxed) as u64
    }

    /// 本地守护行落控制台（行文法真源在 rr-core::runtime，此处只产出事件）。
    fn guard_line(&self, ev: Rr3GuardEvent) {
        let mut g = self.guard.lock().unwrap_or_else(|p| p.into_inner());
        println!("{}", g.rr3_guard_format(&ev));
    }

    /// 上行 Request 一次一答（rr-bus client `rr3_call_inner` 同构：先占邮箱再发帧，
    /// 应答不可能落在邮箱就位前）。
    fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (stx, srx) = mpsc::sync_channel(1);
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, stx);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let frame = rr3_frame_compose(
            opcode::SEND,
            0,
            RR3_TRACE_ID_DIRECT,
            self.now_ns(),
            self.task(),
            RR3_TASK_RRD,
            rr3_body_encode(Rr3PayloadType::Request, body.to_string().as_bytes()),
        );
        let sent = self
            .tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_send_buf(Rr3FrameBuf::from(&frame));
        if let Err(e) = sent {
            self.pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id);
            return Err(format!("写帧失败: {e:?}"));
        }
        match srx.recv_timeout(timeout) {
            Ok(v) => Ok(v),
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&id);
                Err(format!("RPC {method} 等应答超时（{timeout:?}）"))
            }
        }
    }

    /// 断开转移 + 复位（Rr3LinkState::Down 的裸机面语义：运行环复位）。
    fn mark_down_and_restart(&self, why: &str) -> ! {
        eprintln!("[sentinel] 断开: {why}");
        self.rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_mark_down();
        self.guard_line(Rr3GuardEvent::Boot(rr_core::runtime::Rr3LinkState::Down));
        self.stop.store(true, Ordering::Relaxed);
        thread::sleep(RESTART_DELAY);
        unsafe { esp_idf_sys::esp_restart() }
    }
}

/// 三点堆采样（§9.3 ③：free + min_free 双面——min_free 即运行峰值剩余堆水位，
/// D1→D2 触发判据按它对 20% 总堆口径）。
fn ram_point(point: &str) {
    let free = unsafe { esp_get_free_heap_size() };
    let min_free = unsafe { esp_get_minimum_free_heap_size() };
    println!("[D1-RAM] point={point} free={free}B min_free={min_free}B");
}

fn main() {
    // IDF 绑定面补丁（esp-idf-hal 工程惯例起手）
    link_patches();
    let epoch = Instant::now();

    // 堆采样①：启动后（runbook §9.3 ③ 第一点）
    ram_point("1-boot");

    let peripherals = Peripherals::take().unwrap();
    // UART2：GPIO17→TX / GPIO16←RX，115200（批C 载体同款接线与波特率硬约束）。
    // cts/rts 不用（None）；TX/RX 半体的独立 new() 会对同口二次
    // uart_driver_install（IDF 一口一装），故整装 UartDriver 一次。
    let config = UartConfig::new().baudrate(Hertz(115200));
    let driver = UartDriver::new(
        peripherals.uart2,
        peripherals.pins.gpio17,
        peripherals.pins.gpio16,
        None::<AnyInputPin>,
        None::<AnyOutputPin>,
        &config,
    )
    .expect("UART2 驱动安装失败");
    // 半体 'static 化：驱动泄漏（固件常驻语义——esp_restart 前永不 drop，
    // 复位即整机复位），split 借 'static 引用得 'static 半体，进线程所需。
    let driver: &'static mut UartDriver<'static> = Box::leak(Box::new(driver));
    let (tx_driver, rx_driver) = driver.split();

    let tx = Arc::new(Mutex::new(UartTxInner(tx_driver)));
    let rx = UartRxHalf(rx_driver);

    let shared = Arc::new(Shared {
        tx,
        rt: Mutex::new(Rr3ModuleRuntime::rr3_runtime_new(HEARTBEAT_MS)),
        task: AtomicU32::new(0),
        stop: AtomicBool::new(false),
        next_id: AtomicU32::new(0),
        pending: Mutex::new(HashMap::new()),
        epoch,
        guard: Mutex::new(Rr3GuardBuf::rr3_guard_buf_new()),
        round_trips: AtomicU32::new(0),
    });

    // SURFACE 自述行（runbook §9.3 ② 的观测面预期）
    println!(
        "[sentinel] surface=esp-rs-std runtime=rr-core::runtime transport=uart2@115200 name={NAME}"
    );
    shared.guard_line(Rr3GuardEvent::Boot(
        rr_core::runtime::Rr3LinkState::Registering,
    ));

    // reader 必须先于注册帧上路：注册应答也走 pending 路由（client.rs 同序）
    {
        let sh = shared.clone();
        thread::Builder::new()
            .name("sentinel-reader".into())
            .stack_size(THREAD_STACK)
            .spawn(move || reader_loop(rx, sh))
            .expect("起 reader 线程失败");
    }

    // 注册（注册前没有 task，按契约 src=0）
    let reply = shared.call(
        "rr3.register",
        json!({"name": NAME, "manifest": MANIFEST_TEXT}),
        REGISTER_TIMEOUT,
    );
    let reply = match reply {
        Ok(v) if v.get("error").is_none() => v,
        Ok(v) => shared.mark_down_and_restart(&format!("注册被拒: {v}")),
        Err(e) => shared.mark_down_and_restart(&format!("注册失败: {e}")),
    };
    let task = reply
        .pointer("/result/src_task")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| shared.mark_down_and_restart("注册应答缺 result.src_task"));
    shared.task.store(task as u32, Ordering::Relaxed);
    // 机制核状态机转移 Registering → Online，首拍 = 此刻 + heartbeat_ms
    let now_ms = shared.now_ms();
    shared
        .rt
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .rr3_mark_registered(task, now_ms);
    shared.guard_line(Rr3GuardEvent::Boot(
        rr_core::runtime::Rr3LinkState::Online { task },
    ));
    println!("[sentinel] 注册成功 task={task} hb={HEARTBEAT_MS}ms");
    // 堆采样②：注册帧应答后（runbook §9.3 ③ 第二点）
    ram_point("2-registered");

    // 心跳线程：拍点真值在机制核状态机（相对节拍）；std 面只做平台事（client.rs 同构）
    {
        let sh = shared.clone();
        thread::Builder::new()
            .name("sentinel-heartbeat".into())
            .stack_size(THREAD_STACK)
            .spawn(move || heartbeat_loop(sh))
            .expect("起心跳线程失败");
    }

    // 主循环：周期状态行（观测面），Down 由各线程复位
    loop {
        thread::sleep(Duration::from_secs(30));
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        let online = shared
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_link_online();
        println!(
            "[sentinel] status online={online} round_trips={} free={}B",
            shared.round_trips.load(Ordering::Relaxed),
            unsafe { esp_get_free_heap_size() }
        );
    }
}

/// 心跳线程：sleep 等拍点 → due 判定（机制核）→ 发拍 → sent 记账（相对节拍）。
fn heartbeat_loop(sh: Arc<Shared>) {
    let mut seq: u64 = 0;
    loop {
        let Some(wait_ms) = sh
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_next_beat_wait_ms(sh.now_ms())
        else {
            return; // Down 即停拍——"不装活着"同一裁定
        };
        thread::sleep(Duration::from_millis(wait_ms));
        if sh.stop.load(Ordering::Relaxed) {
            return;
        }
        let now_ms = sh.now_ms();
        let due = sh
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_heartbeat_due(now_ms);
        if !due {
            continue; // 提前醒，回睡
        }
        match sh.call("rr3.heartbeat", json!({}), RPC_TIMEOUT) {
            Ok(v) if v.get("error").is_none() => {
                sh.rt
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .rr3_heartbeat_sent(now_ms);
                seq += 1;
                sh.guard_line(Rr3GuardEvent::Feed { seq });
            }
            other => sh.mark_down_and_restart(&format!("心跳失败: {other:?}")),
        }
    }
}

/// reader 线程：收帧 → 机制核分类（Response→pending 路由；Request→round-trip 应答；
/// 其余→inbox 计账）。帧语义全部来自 rr-ipc blanket conn，本函数零帧解析逻辑
/// （头/payload 拆解走 rr-ipc，分类走 rr-core）。
fn reader_loop(mut rx: UartRxHalf, sh: Arc<Shared>) {
    loop {
        // rx 半体 → std adapter → blanket conn：rr3_recv_buf 带全量拒收路径
        // （半帧/坏 magic/长度不符），错误面与宿主三实现同源
        let buf = match rx.rr3_recv_buf() {
            Ok(b) => b,
            Err(e) => sh.mark_down_and_restart(&format!("收帧失败: {e:?}")),
        };
        let frame = match Rr3Frame::try_from(buf) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[sentinel] 帧还原失败: {e:?}");
                continue;
            }
        };
        let (ty, body) = match rr3_body_split(&frame.payload) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[sentinel] payload 前缀非法: {e:?}");
                continue;
            }
        };
        match rr3_dispatch_class(ty) {
            Rr3Dispatch::Pending => {
                // 应答帧：按 JSON-RPC id 路由到等待者（std 驱动器 reader 同构）
                let v: Value = match serde_json::from_slice(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[sentinel] 应答体非 JSON: {e}");
                        continue;
                    }
                };
                let id = v.get("id").and_then(Value::as_u64).map(|i| i as u32);
                let waiter = id.and_then(|i| {
                    sh.pending
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(&i)
                });
                match waiter {
                    Some(w) => {
                        let _ = w.send(v);
                    }
                    None => eprintln!("[sentinel] 无等待者的应答帧 id={id:?}"),
                }
            }
            Rr3Dispatch::Inbox => {
                if ty == Rr3PayloadType::Request {
                    handle_request(&frame, &body, &sh);
                } else {
                    // Event / Stream / Notification：inbox 计账（哨兵无订阅主题，
                    // 广播 state 帧是唯一合法来客——按类型点到名即可）
                    eprintln!("[sentinel] inbox: {ty:?} {}B", body.len());
                }
            }
        }
    }
}

/// 点名 Request 的 round-trip 应答（层③ 判据第三腿）：Response + id 回显 +
/// result 原样回带 params。首次应答触发堆采样③。
fn handle_request(frame: &Rr3Frame, body: &[u8], sh: &Shared) {
    let v: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[sentinel] 请求体非 JSON: {e}");
            return;
        }
    };
    let Some(id) = v.get("id").and_then(Value::as_u64) else {
        eprintln!("[sentinel] 请求缺 id，不应答（不猜）");
        return;
    };
    let reply = json!({"jsonrpc": "2.0", "id": id, "result": {"echo": v.get("params").cloned().unwrap_or(Value::Null)}});
    // trace_id 带回路（M3 应答回路的唯一凭据）：原样带回 Request 头上的
    // trace_id（= 调用方 task）——rrd 的 rr3_route_response 按它投递，0 无从
    // 投递会被拒（Invalid）。直发应答（RR3_TRACE_ID_DIRECT）只对 rrd 直接
    // 应答合法，点名回路必须用调用方 task。
    let out = rr3_frame_compose(
        opcode::SEND,
        0,
        frame.header.trace_id,
        sh.now_ns(),
        sh.task(),
        frame.header.src_task,
        rr3_body_encode(Rr3PayloadType::Response, reply.to_string().as_bytes()),
    );
    let sent = sh
        .tx
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .rr3_send_buf(Rr3FrameBuf::from(&out));
    if let Err(e) = sent {
        eprintln!("[sentinel] round-trip 应答写出失败: {e:?}");
        return;
    }
    let n = sh.round_trips.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 {
        println!("[sentinel] round-trip 完成（id={id}）");
        // 堆采样③：round-trip 完成后（runbook §9.3 ③ 第三点）
        ram_point("3-roundtrip");
    }
}
