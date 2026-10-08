//! esp32-uart-echo —— 0.0.7 批C 层② UART 板级 round-trip 夹具（ESP32 裸金属）。
//!
//! 职责（方案 §9.2 ④⑤）：
//! 1. **xtensa 运行期 dump（第二重证据）**：启动时经 UART0 控制台打印与宿主
//!    `ipct003_dump` 逐字节同构的七行（header + 五种 payload 类型整帧 + version），
//!    宿主抓取后与 `contracts/golden/event-header-128.hex` 对拍——批B 第④路是
//!    编译期 const 求值，本 dump 是真机运行期编码线上字节的物理复核。
//! 2. **板级 echo**：UART2（GPIO16/17，115200）收帧回发——合法帧（128B 头 +
//!    payload）整帧原样回发；坏 magic / reserved 非零 / 超长 payload / 半帧静默
//!    一律拒收并打日志，注入后不死锁，后续帧经滑动重同步照常收发。
//!
//! 契约字节面单源（方案 §5）：编解码直接复用 rr-ipc——`Rr3Header::from_bytes`
//! 校验、`Rr3Header::to_bytes`（const 化，批B 第④路已证明任意目标编译期成立）
//! 出 dump 线上字节、`rr3_body_encode` 出类型前缀体。固件内没有任何"ESP32 版
//! 协议"代码；本文件只写 IO 形状（滑窗重同步、静默超时、回发）。
//!
//! 落点说明：跑在裸金属（xtensa-esp32-none-elf），不走 espidf std——层③ 哨兵
//! 的 std 运行时（Q2 裁定 a）归批D1；本夹具只承担层② 字节面验证，std 面的
//! transport blanket impl 复用由批D1 的 espidf 语境补验。

#![no_std]
#![no_main]

extern crate alloc;

use esp_hal::{time::Duration, time::Instant, uart::UartTx, Blocking};

use rr_ipc::{
    opcode, rr3_body_encode, Rr3Header, Rr3PayloadType, RR3_CONTRACT_SCHEMA_VERSION,
    RR3_IPC_HDR_SIZE,
};

/// 夹具 payload 回发上限：先验头后分配，防 64 MiB 声明值吃穿 64 KiB 堆
/// （超限按契约头层合法帧处理：排空线路保持对齐，不回发不崩溃）。
const PAYLOAD_CAP: usize = 8192;

/// 帧内静默超时：帧中途停流即判半帧拒收（runbook ⑤ "short frame"）。
/// 115200 下一字节间隙 ~87µs，50ms 有 500+ 倍余量；注入命令之间的线路静默
/// 远大于它，不会误伤连续流。
const IDLE_TIMEOUT: Duration = Duration::from_millis(50);

// ---------------------------------------------------------------------------
// 最小 panic handler：夹具无 backtrace 依赖，死循环即可（板级调试看 reject 日志）。
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// 控制台日志（UART0，devkit USB ↔ CP2102 ↔ 宿主 monitor）与链路（UART2，runbook ①）
// ---------------------------------------------------------------------------

fn write_all(u: &mut UartTx<'_, Blocking>, mut data: &[u8]) -> Result<(), ()> {
    while !data.is_empty() {
        match u.write(data) {
            Ok(0) => return Err(()),
            Ok(n) => data = &data[n..],
            Err(_) => return Err(()),
        }
    }
    u.flush().map_err(|_| ())
}

/// 写入期引用与 UART 自身生命周期解绑（&mut 对 UartTx<'u> 不变，强绑同名
/// 生命周期会让调用方 reborrow 失败——报错现场实证）。
struct UartWriter<'w, 'u> {
    u: &'w mut UartTx<'u, Blocking>,
}

impl core::fmt::Write for UartWriter<'_, '_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_all(self.u, s.as_bytes()).map_err(|_| core::fmt::Error)
    }
}

fn logk(u: &mut UartTx<'_, Blocking>, args: core::fmt::Arguments) {
    let mut w = UartWriter { u };
    let _ = core::fmt::write(&mut w, args);
    let _ = core::fmt::write(&mut w, format_args!("\r\n"));
}

/// 调用点统一传 `&mut console` 形式：拥有值取可变借用、`&mut` 参数绑定为
/// `mut console`（参数模式可加 mut），两条路径都落同一写法。
macro_rules! log {
    ($u:expr, $($arg:tt)*) => { logk($u, format_args!($($arg)*)) };
}

/// hex 小写显示（dump 行与对拍脚本都吃小写 hex）
struct Hex<'a>(&'a [u8]);

impl core::fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for b in self.0 {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 启动 dump：与宿主 ipct003_dump 七行逐字节同构（字段值、行名、顺序全同），
// header 行与 contracts/golden/event-header-128.hex 直接可比。
// ---------------------------------------------------------------------------

/// dump 头（payload_len=96，与 golden 同一条头）
fn golden_hdr() -> Rr3Header {
    Rr3Header {
        opcode: opcode::SEND,
        flags: 0,
        trace_id: 0x1234_5678,
        timestamp_ns: 100,
        src_task: 7,
        dst_task: 0,
        payload_len: 96,
    }
}

fn frame_bytes(ty: Rr3PayloadType, body: &[u8]) -> alloc::vec::Vec<u8> {
    let payload = rr3_body_encode(ty, body);
    let hdr = Rr3Header {
        opcode: opcode::SEND,
        flags: 0,
        trace_id: 0x1234_5678,
        timestamp_ns: 100,
        src_task: 7,
        dst_task: 0,
        payload_len: payload.len() as u32,
    };
    let mut out = alloc::vec::Vec::with_capacity(RR3_IPC_HDR_SIZE + payload.len());
    out.extend_from_slice(&hdr.to_bytes());
    out.extend_from_slice(&payload);
    out
}

fn boot_dump(mut console: &mut UartTx<'_, Blocking>) {
    log!(
        &mut console,
        "esp32-uart-echo ready (uart2 gpio16/17 115200) — xtensa runtime dump follows"
    );
    // [1] header：与 golden 同一条头（payload_len=96，不带 payload）
    log!(&mut console, "header {}", Hex(&golden_hdr().to_bytes()));
    // [2-6] 五种 payload 类型各一整帧——体为定长定值字节（字面量，无任何运行期来源）
    log!(
        &mut console,
        "request {}",
        Hex(&frame_bytes(
            Rr3PayloadType::Request,
            b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}"
        ))
    );
    log!(
        &mut console,
        "response {}",
        Hex(&frame_bytes(
            Rr3PayloadType::Response,
            b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":\"pong\"}"
        ))
    );
    log!(
        &mut console,
        "event {}",
        Hex(&frame_bytes(
            Rr3PayloadType::Event,
            b"{\"topic\":\"rr3-sim/ping\",\"n\":1}"
        ))
    );
    log!(
        &mut console,
        "stream {}",
        Hex(&frame_bytes(
            Rr3PayloadType::Stream,
            &[0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        ))
    );
    log!(
        &mut console,
        "notification {}",
        Hex(&frame_bytes(
            Rr3PayloadType::Notification,
            b"{\"topic\":\"rr3-sim/beat\",\"seq\":1}"
        ))
    );
    // [7] version：把契约 schema 版本带进对拍，防两侧各编旧版契约互相对上
    log!(&mut console, "version {RR3_CONTRACT_SCHEMA_VERSION}");
}

// ---------------------------------------------------------------------------
// echo 状态机：滑窗重同步 + 静默超时拒收
// ---------------------------------------------------------------------------

/// 头层错误的日志措辞（runbook ⑤ 样例形状：`reject: bad magic` 等）
fn hdr_err_log(e: rr_ipc::Rr3HdrError) -> &'static str {
    match e {
        rr_ipc::Rr3HdrError::BadMagic { .. } => "reject: bad magic",
        rr_ipc::Rr3HdrError::ReservedNotZero => "reject: reserved not zero",
        rr_ipc::Rr3HdrError::PayloadTooLarge { .. } => "reject: payload too large",
        rr_ipc::Rr3HdrError::TooShort { .. } => "reject: short frame",
    }
}

#[esp_hal::main]
fn main() -> ! {
    // 64 KiB 内部堆：payload 回发缓冲按 8 KiB 封顶分配（先验头再分配，见
    // PAYLOAD_CAP）。堆注册是运行时动作，必须在任何分配前执行。
    esp_alloc::heap_allocator!(size: 65536);

    let peripherals = esp_hal::init(esp_hal::Config::default());

    // UART0 = 控制台（GPIO1=TX / GPIO3=RX，devkit USB 板载 CP2102）；夹具只写不读
    let mut console: UartTx<'_, Blocking> =
        esp_hal::uart::UartTx::new(peripherals.UART0, esp_hal::uart::Config::default())
            .expect("uart0 init")
            .with_tx(peripherals.GPIO1);
    // UART2 = 链路（GPIO17=TX2 / GPIO16=RX2，方案 §9.2 ① 接线表；115200）。
    // 收发拆分：echo 状态机从 rx 轮询、往 tx 回发。
    let link = esp_hal::uart::Uart::new(peripherals.UART2, esp_hal::uart::Config::default())
        .expect("uart2 init")
        .with_tx(peripherals.GPIO17)
        .with_rx(peripherals.GPIO16);
    let (mut rx, mut tx): (
        esp_hal::uart::UartRx<'_, Blocking>,
        esp_hal::uart::UartTx<'_, Blocking>,
    ) = link.split();

    boot_dump(&mut console);

    // echo 状态机：逐字节累积 128B 头；magic 不符滑动 1 字节重同步；
    // 头非法滑动 1 字节重扫；payload 按头声明收满后整帧原样回发。
    let mut hdr = [0u8; RR3_IPC_HDR_SIZE];
    let mut fill: usize = 0;
    let mut last_rx = Instant::now();
    let mut one = [0u8; 1];

    loop {
        match rx.read_buffered(&mut one) {
            Ok(0) => {
                // 线路静默：帧中途停流 = 半帧，按 runbook ⑤ 拒收并复位
                if fill > 0 && last_rx.elapsed() > IDLE_TIMEOUT {
                    log!(
                        &mut console,
                        "reject: short frame (got {fill}/{RR3_IPC_HDR_SIZE} header bytes)"
                    );
                    fill = 0;
                }
            }
            Ok(_) => {
                let b = one[0];
                last_rx = Instant::now();
                hdr[fill] = b;
                fill += 1;

                // 头前缀滑窗：前导垃圾/半帧后新帧都能对齐——fill 恰为 4 时校验 magic，
                // 不符则丢最旧 1 字节继续滑（下一字节进来又是 fill=4）
                if fill == 4 && hdr[0..4] != rr_ipc::MAGIC.to_le_bytes() {
                    hdr.copy_within(1..4, 0);
                    fill = 3;
                } else if fill == RR3_IPC_HDR_SIZE {
                    match Rr3Header::from_bytes(&hdr) {
                        Err(e) => {
                            log!(&mut console, "{}, sliding resync", hdr_err_log(e));
                            // 已缓冲的 127 字节可能是后续帧的前缀，滑动 1 字节重扫
                            hdr.copy_within(1..RR3_IPC_HDR_SIZE, 0);
                            fill = RR3_IPC_HDR_SIZE - 1;
                        }
                        Ok(h) => {
                            let want = h.payload_len as usize;
                            if want > PAYLOAD_CAP {
                                // 头层合法但超出夹具承载：排空线路保持对齐（契约头层
                                // 校验已过，这不是坏帧；只是夹具不回发超大帧）
                                log!(
                                    &mut console,
                                    "reject: payload too large (declared {want}, cap {PAYLOAD_CAP}) — draining"
                                );
                                let mut drained = 0usize;
                                while drained < want {
                                    match rx.read_buffered(&mut one) {
                                        Ok(0) => {
                                            if last_rx.elapsed() > IDLE_TIMEOUT {
                                                log!(
                                                    &mut console,
                                                    "reject: drain timeout — resync"
                                                );
                                                break;
                                            }
                                        }
                                        Ok(_) => {
                                            last_rx = Instant::now();
                                            drained += 1;
                                        }
                                        Err(_) => {
                                            log!(&mut console, "reject: rx error while draining");
                                            break;
                                        }
                                    }
                                }
                                fill = 0;
                            } else {
                                let mut payload = alloc::vec![0u8; want];
                                let mut got = 0usize;
                                let mut torn = false;
                                while got < want {
                                    match rx.read_buffered(&mut one) {
                                        Ok(0) => {
                                            if last_rx.elapsed() > IDLE_TIMEOUT {
                                                log!(
                                                    &mut console,
                                                    "reject: short frame (payload {got}/{want})"
                                                );
                                                torn = true;
                                                break;
                                            }
                                        }
                                        Ok(_) => {
                                            payload[got] = one[0];
                                            got += 1;
                                            last_rx = Instant::now();
                                        }
                                        Err(_) => {
                                            log!(&mut console, "reject: rx error in payload");
                                            torn = true;
                                            break;
                                        }
                                    }
                                }
                                if !torn {
                                    log!(
                                        &mut console,
                                        "frame received: len={} payload_len={}",
                                        RR3_IPC_HDR_SIZE + want,
                                        want
                                    );
                                    // 整帧回发（raw 字节：收到的 128B 头 + payload）
                                    let mut ok = write_all(&mut tx, &hdr).is_ok();
                                    if ok {
                                        ok = write_all(&mut tx, &payload).is_ok();
                                    }
                                    if !ok {
                                        log!(&mut console, "reject: tx error — echo failed");
                                    }
                                }
                                fill = 0;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                log!(&mut console, "reject: rx error");
            }
        }
    }
}
