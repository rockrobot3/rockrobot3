// SPDX-License-Identifier: Apache-2.0
//! espidf-sentinel —— 0.0.7 批D1 层③ 哨兵（esp-rs std，机制核 `rr-core::runtime`
//! 的真板消费者）。
//!
//! 职责（0.0.7 方案 §2.1 修订版 9 口径 + §9.3 层③ runbook）：**内跑
//! `rr-core::runtime`**——连接态转移 [`Rr3LinkState`]、心跳拍点
//! [`Rr3ModuleRuntime`]、收帧分类 [`rr3_dispatch_class`]——**不另写状态机**；
//! 传输下底只写窄缝 [`Rr3ByteStream`] 两方法（esp-idf-hal UART2 半分体，经
//! rr-ipc std 单向 adapter + blanket conn 免费获得与 uds/uart 宿主侧同源的
//! 帧语义）。本文件只做平台事：UART 装配、注册、线程拉起、堆采样；reader/心跳
//! 两线程循环与入站分发在 `worker.rs`（std 驱动器 rr-bus client 同构：reader 线程
//! + 心跳线程 + 一次一答 pending 表）。
//!
//! 双证之一（编译面）：`cargo +esp build` 在 xtensa-esp32-espidf 上编译
//! rr-core + rr-ipc + 本哨兵（专用门 tools/ci/platform/espidf-sentinel-check.sh）；
//! 双证之二（板上真跑）：注册→心跳→round-trip + 三点堆采样（README runbook）。
//! D1→D2 触发判据：实测运行峰值剩余堆 < 20% 总堆（§9.3 ④）。
//!
//! 批G 扩面（⑤ 列安全核逐拍用例的板上腿，载体裁定=扩本哨兵不另立 crate）：
//! manifest exposes 增 `rr3-sentinel/diag/parity`，点名应答跑
//! `rr_core::control::parity::rr3_parity_run()` 的**真实原语**（限速门/低电
//! 边沿/饱和检测/节拍方案/窗结算/速率折算/钳位），输出经 serde_json 手工展开
//! 回带——**不回显 RR3_PARITY_EXPECTED**（对拍的有效性前提：板上输出与期望
//! 向量相互独立，两腿汇合在宿主 harness）。输入序列与语义出处全部单源在
//! rr-core::control::parity，本文件零向量复制。
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
//!   `flush`，错误面 `EspError`），std::io 的 Read/Write 桥在 `uart.rs` 手写；
//! - 上游只给全双工 `UartDriver` 标了 `unsafe Send + Sync`，半体驱动漏标——
//!   `uart.rs` 以两个 `unsafe impl Send` 补齐（安全性论证见各结构体注记）；
//! - xtensa LX6 无 64 位原子，`AtomicU64` 不可用，id 发号/task 号/计数全部
//!   `AtomicU32`。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_hal::gpio::{AnyInputPin, AnyOutputPin};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::uart::{UartConfig, UartDriver};
use esp_idf_hal::units::Hertz;
use esp_idf_sys::{esp_get_free_heap_size, esp_get_minimum_free_heap_size, link_patches};
use serde_json::{json, Value};

use rr_core::runtime::{Rr3GuardBuf, Rr3GuardEvent, Rr3ModuleRuntime};
use rr_core::{rr3_frame_compose, RR3_TASK_RRD, RR3_TRACE_ID_DIRECT};
use rr_ipc::transport::Rr3TransportConn;
use rr_ipc::{opcode, rr3_body_encode, Rr3FrameBuf, Rr3PayloadType};

mod uart;
mod worker;
use uart::*;
use worker::*;

/// 模块名（manifest.name 唯一真值——线上注册面）。词干纯小写是命名契约硬规则
/// （^rr3-[a-z]+$，module.schema.toml rule=module_name）；工程名/目录名
/// espidf-sentinel 是 crate 名非总线身份，不受此约束。
const NAME: &str = "rr3-sentinel";
/// 心跳间隔（manifest [health].heartbeat_ms 同值；机制核保底 ≥1ms 的输入）。
const HEARTBEAT_MS: u64 = 200;
/// 线上 manifest（rr3.register 的 params.manifest）：entry 绝对路径是 rrd 门禁
/// 硬项——哨兵是 ESP32 固件、不被 rrd spawn，entry 仅过"绝对路径"门，真实执行体
/// 是本固件；restart_policy = "never"（进程面监督对本模块不适用，watchdog 判死
/// 走在册表记账）。`[rpc].exposes` 是 M3 `rr3.call` 的路由表来源（0.0.7 §9.4 ⑤
/// e2e round-trip：sim_peer 点名 probe 主题 → 本哨兵应答），不声明 = 不接点名。
const MANIFEST_TEXT: &str = "[manifest]\nschema_version = 1\nname = \"rr3-sentinel\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/opt/rr3/bin/rr3-espidf-sentinel\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[resources]\nrestart_policy = \"never\"\n\n[health]\nheartbeat_ms = 200\nwatchdog_timeout_ms = 2000\n\n[rpc]\nexposes = [\"rr3-sentinel/diag/probe\", \"rr3-sentinel/diag/parity\"]\n";

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
    fn rr3_mark_restart(&self, why: &str) -> ! {
        eprintln!("[sentinel] 断开: {why}");
        self.rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_mark_down();
        self.guard_line(Rr3GuardEvent::Boot(rr_core::runtime::Rr3LinkState::Down));
        self.stop.store(true, Ordering::Relaxed);
        thread::sleep(RESTART_DELAY);
        // SAFETY：esp_restart 不返回；调用前已把链路置 Down、置 stop 并留出日志
        // 窗口，重启是看门狗的终局动作，之后没有任何代码依赖本栈状态。
        unsafe { esp_idf_sys::esp_restart() }
    }
}

/// 三点堆采样（§9.3 ③：free + min_free 双面——min_free 即运行峰值剩余堆水位，
/// D1→D2 触发判据按它对 20% 总堆口径）。
fn ram_point(point: &str) {
    // SAFETY：两个堆查询都是无参只读的 IDF 系统调用，不触碰 Rust 内存状态。
    let free = unsafe { esp_get_free_heap_size() };
    let min_free = unsafe { esp_get_minimum_free_heap_size() };
    println!("[D1-RAM] point={point} free={free}B min_free={min_free}B");
}

/// UART2 装配：GPIO17→TX / GPIO16←RX，115200（批C 载体同款接线与波特率硬约束）。
/// cts/rts 不用（None）；TX/RX 半体的独立 new() 会对同口二次
/// uart_driver_install（IDF 一口一装），故整装 UartDriver 一次。
fn open_uart(peripherals: Peripherals) -> Result<(Arc<Mutex<UartTxInner>>, UartRxHalf), String> {
    let config = UartConfig::new().baudrate(Hertz(115200));
    let driver = UartDriver::new(
        peripherals.uart2,
        peripherals.pins.gpio17,
        peripherals.pins.gpio16,
        None::<AnyInputPin>,
        None::<AnyOutputPin>,
        &config,
    )
    .map_err(|e| format!("UART2 驱动安装失败 GPIO17(TX)/GPIO16(RX) @115200: {e:?}"))?;
    // 半体 'static 化：驱动泄漏（固件常驻语义——esp_restart 前永不 drop，
    // 复位即整机复位），split 借 'static 引用得 'static 半体，进线程所需。
    let driver: &'static mut UartDriver<'static> = Box::leak(Box::new(driver));
    let (tx_driver, rx_driver) = driver.split();
    Ok((
        Arc::new(Mutex::new(UartTxInner(tx_driver))),
        UartRxHalf(rx_driver),
    ))
}

/// 共享状态装配（字段语义见 [`Shared`] 上各注记）。
fn build_shared(tx: Arc<Mutex<UartTxInner>>, epoch: Instant) -> Arc<Shared> {
    Arc::new(Shared {
        tx,
        rt: Mutex::new(Rr3ModuleRuntime::rr3_runtime_new(HEARTBEAT_MS)),
        task: AtomicU32::new(0),
        stop: AtomicBool::new(false),
        next_id: AtomicU32::new(0),
        pending: Mutex::new(HashMap::new()),
        epoch,
        guard: Mutex::new(Rr3GuardBuf::rr3_guard_buf_new()),
        round_trips: AtomicU32::new(0),
    })
}

/// reader 必须先于注册帧上路：注册应答也走 pending 路由（client.rs 同序）。
fn spawn_reader(rx: UartRxHalf, shared: &Arc<Shared>) {
    let sh = shared.clone();
    if let Err(e) = thread::Builder::new()
        .name("sentinel-reader".into())
        .stack_size(THREAD_STACK)
        .spawn(move || reader_loop(rx, sh))
    {
        halt(format!("起 reader 线程失败（stack={THREAD_STACK}）: {e:?}"));
    }
}

/// 注册（注册前没有 task，按契约 src=0）→ 机制核状态机 Registering → Online，
/// 首拍 = 此刻 + heartbeat_ms；被拒/失败/缺 `src_task` 一律复位（不装活着）。
fn register_module(shared: &Shared) {
    let reply = shared.call(
        "rr3.register",
        json!({"name": NAME, "manifest": MANIFEST_TEXT}),
        REGISTER_TIMEOUT,
    );
    let reply = match reply {
        Ok(v) if v.get("error").is_none() => v,
        Ok(v) => shared.rr3_mark_restart(&format!("注册被拒: {v}")),
        Err(e) => shared.rr3_mark_restart(&format!("注册失败: {e}")),
    };
    let task = reply
        .pointer("/result/src_task")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| shared.rr3_mark_restart("注册应答缺 result.src_task"));
    shared.task.store(task as u32, Ordering::Relaxed);
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
}

/// 心跳线程：拍点真值在机制核状态机（相对节拍）；worker 面只做平台事。
fn spawn_heartbeat(shared: &Arc<Shared>) {
    let sh = shared.clone();
    if let Err(e) = thread::Builder::new()
        .name("sentinel-heartbeat".into())
        .stack_size(THREAD_STACK)
        .spawn(move || heartbeat_loop(sh))
    {
        halt(format!("起心跳线程失败（stack={THREAD_STACK}）: {e:?}"));
    }
}

/// 主循环：周期状态行（观测面），Down 由各线程复位。
fn status_loop(shared: &Shared) {
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
        // SAFETY：无参只读的 IDF 堆查询，不触碰 Rust 内存状态。
        let free = unsafe { esp_get_free_heap_size() };
        println!(
            "[sentinel] status online={online} round_trips={} free={free}B",
            shared.round_trips.load(Ordering::Relaxed)
        );
    }
}

/// 启动失败的统一停机：先打印诊断，再 abort——`panic=immediate-abort` 档下
/// panic 文案不落地，先打印现场才有一条可查。
fn halt(e: String) -> ! {
    eprintln!("[sentinel] 启动失败：{e}");
    std::process::abort()
}

fn main() {
    // IDF 绑定面补丁（esp-idf-hal 工程惯例起手）
    link_patches();
    let epoch = Instant::now();

    // 堆采样①：启动后（runbook §9.3 ③ 第一点）
    ram_point("1-boot");

    let Ok(peripherals) = Peripherals::take() else {
        halt("Peripherals::take 失败：外设已被取走（启动期不应发生，查是否有二次取）".into());
    };
    let (tx, rx) = match open_uart(peripherals) {
        Ok(pair) => pair,
        Err(e) => halt(e),
    };
    let shared = build_shared(tx, epoch);

    // SURFACE 自述行（runbook §9.3 ② 的观测面预期）
    println!(
        "[sentinel] surface=esp-rs-std runtime=rr-core::runtime transport=uart2@115200 name={NAME}"
    );
    shared.guard_line(Rr3GuardEvent::Boot(
        rr_core::runtime::Rr3LinkState::Registering,
    ));

    spawn_reader(rx, &shared);
    register_module(&shared);
    spawn_heartbeat(&shared);
    status_loop(&shared);
}
