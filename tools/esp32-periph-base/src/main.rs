//! esp32-periph-base —— 0.0.5 增补轮五 批B 外设底座档 + 批C 帧执行面（espidf std，不依赖桥）。
//!
//! 职责（全案 §4 批B + 批C）：**SSD1306 I2C 驱动出字 + LEDC PWM 云台两轴逐度转向 +
//! UART2 双面指令接入**。批B 是独立固件档（行协议面）；批C 在本 crate 上加**帧
//! 执行面**（rr-ipc 能力帧，不进城——路线二裁定：板不注册总线身份，宿主 HAL 桥
//! 后端 rr-hal::esp32_uart 直连发 `rr3.call` 帧，本固件只是被点名的执行面）；
//! 批D 加 I2S——同一固件 crate 分批合入（全案 §5 板上固件形态）。
//!
//! 双面分流（批C）：指令循环逐字节收，首字节 = `0x31`（帧头 magic "1ERA" 的 LE
//! 首字节）走帧面（`rr3_framebuf_read` 整帧收 → rr3.call 分发 → Response 应答）；
//! 其余走批B 行面（调试与舵机五点 SCAN 判据沿用）。行面首字节集全为字母，
//! 分流无歧义。
//!
//! 引脚预算（全案 §5 约束面，与 0.0.7 实测接线零冲突）：
//! - UART2 = GPIO17→TX / GPIO16←RX（115200，CP2102 指令面；批B 行协议临时面，
//!   批C 换能力帧——接线与波特率不变，为进城铺路）
//! - I2C0  = GPIO21 SDA / GPIO22 SCL（400kHz，SSD1306 0x3C）
//! - LEDC  = GPIO32 水平轴（pan）/ GPIO33 俯仰轴（tilt）——避开 strapping 面
//!   （0/2/5/12/15）、UART2（16/17）、I2C（21/22）、I2S 候选（25/26/27，批D）、
//!   VSPI 缺省组（18/19/23，点阵预留不占用不使能）
//! - UART0 = 控制台（monitor 观测面，板载 USB cu.usbserial-1410）
//!
//! 供电铁律（全案 §5）：舵机由 UBEC 独立 5V 轨供电（堵转 ~700mA/只，禁接板载
//! 5V），UBEC/板/舵机单点共地；信号线 3.3V 逻辑直驱 SG90 级兼容。
//!
//! esp-idf-hal 0.44.1 API 对表实录（vendored 源码核对，批B）：
//! - `I2cDriver::new(i2c, sda, scl, &I2cConfig::new().baudrate(Hertz))`（i2c.rs
//!   L207；`I2cConfig = config::Config` alias L66）；事务面 `write(addr, bytes,
//!   timeout: TickType_t)` 每调一次 START-addr-bytes-STOP（SSD1306 每命令/每
//!   数据块一传输）；探测用单字节 0x00 写（控制字节后 STOP，芯片无害）。
//! - `LedcTimerDriver::new(timer, &TimerConfig::new().frequency(Hertz(50))
//!   .resolution(Resolution::Bits14))`（ledc.rs L108）；`LedcDriver::new(channel,
//!   &timer, pin)`（L201，B: Borrow<LedcTimerDriver>——同一 timer 借用两次驱动
//!   两通道，timer 未 move）；`set_duty(u32)`（L289）。50Hz×2^14=819200Hz，
//!   LEDC 源 80MHz 分频 97.66 → 实际 ≈50.0Hz（±0.3%），SG90 容差内。
//!   peripherals.ledc 的 timer0–3/channel0–7 为低速组（L625），Bits14 上限
//!   同样适用。
//! - I2cDriver/LedcDriver 上游均未标 `Send`（grep 0.44.1 全源无 unsafe impl
//!   Send for I2cDriver/LedcDriver）——本文件以 [`Periph`] 单 wrapper 手补
//!   （安全性论证见其注记），与哨兵 UartTxInner/UartRxHalf 同款纪律。
//! - UART2 整装 `UartDriver::new(uart, tx, rx, cts, rts, config)`（cts/rts 传
//!   None；同口二次 install 必败的教训，哨兵 API 对表实录在案，照抄不赘）。
//!   不 split：指令线程单线程读写同驱动，owned move 即可。
//! - xtensa LX6 无 64 位原子——本档状态面由指令线程单线程独占，角度用 u32
//!   直传，无需原子。
//!
//! 线程模型（哨兵同构纪律）：main 线程只做初始化（默认栈 3584B，println 少量，
//! 哨兵同款安全），指令循环放自起 16KB 线程（8KB→16KB 运行下限钉死的教训在案，
//! 勿再调低）；I2C/LEDC/UART 驱动 move 进指令线程独占，无并发访问面。

use std::thread;
use std::time::Duration;

use esp_idf_hal::gpio::{AnyInputPin, AnyOutputPin};
use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::ledc::{config::TimerConfig, LedcDriver, LedcTimerDriver, Resolution};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::uart::{UartConfig, UartDriver};
use esp_idf_hal::units::Hertz;
use esp_idf_sys::{
    esp_get_free_heap_size, esp_get_minimum_free_heap_size, esp_random, esp_timer_get_time,
    link_patches, TickType_t,
};
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, Rr3ByteStream, Rr3Frame, Rr3FrameBuf, Rr3Header,
    Rr3PayloadType, Rr3TransportConn,
};
use serde_json::{json, Value};

/// I2C 事务超时（TickType_t；portMAX_DELAY 同义 u32::MAX——阻塞至传输完成）。
const I2C_TIMEOUT: TickType_t = u32::MAX;
/// SSD1306 I2C 从机地址（0.96" 128x64 模块缺省）。
const OLED_ADDR: u8 = 0x3C;
/// OLED 每行字符数：5px 字模 + 1px 间隔 = 6px/字符，128px/6 = 21.3 → 21 列。
const OLED_COLS: usize = 21;
/// OLED 文本行数：4 行 = 文本 3 行（页 0/2/4，`OLED` 命令按 21 列硬切）+ 固定
/// 角度行（页 6，P/T 随舵机命令刷新）——Mac 实时数据面（温度/内存/IP/时间）
/// 同屏三行布局，批B 增量批裁定（2→4 扩行，`OLED <text>` 形状不变向后兼容）。
const OLED_ROWS: usize = 4;
/// 指令行缓冲上限（超出整行丢弃回 ERR，fail-closed 不截半执行）。
const LINE_CAP: usize = 128;
/// 指令/心跳类线程的栈面（板上实测实锤 16KB 运行下限，哨兵同款，勿再调低）。
const THREAD_STACK: usize = 16384;

/// 帧面本板 task 位形（帧头 dst 门）：外设执行面占首模块池位形 2——与宿主
/// rr-hal::esp32_uart::RR3_PERIPH_TASK 同值（本 crate 不依赖 rr-core，此位形是
/// 两端帧头约定，宿主侧有同值锚定测试防静默分叉）。
const RR3_PERIPH_TASK: u64 = 2;
/// 帧头 magic "1ERA" 的 LE 首字节（0x31='1'）——双面分流窥视判据。行面首字节
/// 集全为字母（HELP/OLED/PAN/...），与 0x31 无交集，分流无歧义。
const RR3_MAGIC_FIRST: u8 = 0x31;
/// 舵机点名主题（在册 request L1；形状事实源
/// contracts/events/act-servo-command.schema.json，与 topics.toml 同串）。
const RR3_SERVO_TOPIC: &str = "rr3-act/servo/command";
/// 表情点名主题（在册 request L1；形状事实源
/// contracts/events/interact-face-expression.schema.json，与 topics.toml 同串）。
const RR3_FACE_TOPIC: &str = "rr3-interact/face/expression";
/// 表情白名单（rr3-interact 模块面同款 7 态；帧面执行面同步执行该裁定）。
const RR3_FACE_EXPRS: [&str; 7] = ["idle", "blink", "talk", "think", "happy", "listen", "turn"];

// ---------------------------------------------------------------------------
// 舵机换算（50Hz / 14bit：1 tick = 20ms / 16384 = 1.2207µs）
// ---------------------------------------------------------------------------

/// 0° 对应 duty ticks（0.5ms = 500µs / 1.2207µs ≈ 409.6 → 410）。
const DUTY_MIN: u32 = 410;
/// 180° 对应 duty ticks（2.5ms = 2048）。
const DUTY_MAX: u32 = 2048;

/// 角度 → duty ticks（线性 0–180 ↦ 410–2048；入参已由调用方 clamp 0–180）。
fn deg_to_duty(deg: u32) -> u32 {
    DUTY_MIN + (deg * (DUTY_MAX - DUTY_MIN)) / 180
}

/// duty ticks → 角度（回读换算，与 deg_to_duty 互逆的整数近似，±1° 内）。
fn duty_to_deg(duty: u32) -> u32 {
    ((duty.saturating_sub(DUTY_MIN)) * 180 + (DUTY_MAX - DUTY_MIN) / 2) / (DUTY_MAX - DUTY_MIN)
}

// ---------------------------------------------------------------------------
// SSD1306 5x7 字模（ASCII 32–126，经典公共域 glcdfont 表；每字符 5 列 LSB 在左）
// ---------------------------------------------------------------------------

const FONT5X7: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x00, 0x00, 0x5F, 0x00, 0x00], // !
    [0x00, 0x07, 0x00, 0x07, 0x00], // "
    [0x14, 0x7F, 0x14, 0x7F, 0x14], // #
    [0x24, 0x2A, 0x7F, 0x2A, 0x12], // $
    [0x23, 0x13, 0x08, 0x64, 0x62], // %
    [0x36, 0x49, 0x55, 0x22, 0x50], // &
    [0x00, 0x05, 0x03, 0x00, 0x00], // '
    [0x00, 0x1C, 0x22, 0x41, 0x00], // (
    [0x00, 0x41, 0x22, 0x1C, 0x00], // )
    [0x14, 0x08, 0x3E, 0x08, 0x14], // *
    [0x08, 0x08, 0x3E, 0x08, 0x08], // +
    [0x00, 0x50, 0x30, 0x00, 0x00], // ,
    [0x08, 0x08, 0x08, 0x08, 0x08], // -
    [0x00, 0x60, 0x60, 0x00, 0x00], // .
    [0x20, 0x10, 0x08, 0x04, 0x02], // /
    [0x3E, 0x51, 0x49, 0x45, 0x3E], // 0
    [0x00, 0x42, 0x7F, 0x40, 0x00], // 1
    [0x42, 0x61, 0x51, 0x49, 0x46], // 2
    [0x21, 0x41, 0x45, 0x4B, 0x31], // 3
    [0x18, 0x14, 0x12, 0x7F, 0x10], // 4
    [0x27, 0x45, 0x45, 0x45, 0x39], // 5
    [0x3C, 0x4A, 0x49, 0x49, 0x30], // 6
    [0x01, 0x71, 0x09, 0x05, 0x03], // 7
    [0x36, 0x49, 0x49, 0x49, 0x36], // 8
    [0x06, 0x49, 0x49, 0x29, 0x1E], // 9
    [0x00, 0x36, 0x36, 0x00, 0x00], // :
    [0x00, 0x56, 0x36, 0x00, 0x00], // ;
    [0x08, 0x14, 0x22, 0x41, 0x00], // <
    [0x14, 0x14, 0x14, 0x14, 0x14], // =
    [0x00, 0x41, 0x22, 0x14, 0x08], // >
    [0x02, 0x01, 0x51, 0x09, 0x06], // ?
    [0x32, 0x49, 0x79, 0x41, 0x3E], // @
    [0x7E, 0x11, 0x11, 0x11, 0x7E], // A
    [0x7F, 0x49, 0x49, 0x49, 0x36], // B
    [0x3E, 0x41, 0x41, 0x41, 0x22], // C
    [0x7F, 0x41, 0x41, 0x22, 0x1C], // D
    [0x7F, 0x49, 0x49, 0x49, 0x41], // E
    [0x7F, 0x09, 0x09, 0x09, 0x01], // F
    [0x3E, 0x41, 0x49, 0x49, 0x7A], // G
    [0x7F, 0x08, 0x08, 0x08, 0x7F], // H
    [0x00, 0x41, 0x7F, 0x41, 0x00], // I
    [0x20, 0x40, 0x41, 0x3F, 0x01], // J
    [0x7F, 0x08, 0x14, 0x22, 0x41], // K
    [0x7F, 0x40, 0x40, 0x40, 0x40], // L
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], // M
    [0x7F, 0x04, 0x08, 0x10, 0x7F], // N
    [0x3E, 0x41, 0x41, 0x41, 0x3E], // O
    [0x7F, 0x09, 0x09, 0x09, 0x06], // P
    [0x3E, 0x41, 0x51, 0x21, 0x5E], // Q
    [0x7F, 0x09, 0x19, 0x29, 0x46], // R
    [0x46, 0x49, 0x49, 0x49, 0x31], // S
    [0x01, 0x01, 0x7F, 0x01, 0x01], // T
    [0x3F, 0x40, 0x40, 0x40, 0x3F], // U
    [0x1F, 0x20, 0x40, 0x20, 0x1F], // V
    [0x3F, 0x40, 0x38, 0x40, 0x3F], // W
    [0x63, 0x14, 0x08, 0x14, 0x63], // X
    [0x07, 0x08, 0x70, 0x08, 0x07], // Y
    [0x61, 0x51, 0x49, 0x45, 0x43], // Z
    [0x00, 0x7F, 0x41, 0x41, 0x00], // [
    [0x02, 0x04, 0x08, 0x10, 0x20], // '\'
    [0x00, 0x41, 0x41, 0x7F, 0x00], // ]
    [0x04, 0x02, 0x01, 0x02, 0x04], // ^
    [0x40, 0x40, 0x40, 0x40, 0x40], // _
    [0x00, 0x01, 0x02, 0x04, 0x00], // `
    [0x20, 0x54, 0x54, 0x54, 0x78], // a
    [0x7F, 0x48, 0x44, 0x44, 0x38], // b
    [0x38, 0x44, 0x44, 0x44, 0x20], // c
    [0x38, 0x44, 0x44, 0x48, 0x7F], // d
    [0x38, 0x54, 0x54, 0x54, 0x18], // e
    [0x08, 0x7E, 0x09, 0x01, 0x02], // f
    [0x0C, 0x52, 0x52, 0x52, 0x3E], // g
    [0x7F, 0x08, 0x04, 0x04, 0x78], // h
    [0x00, 0x44, 0x7D, 0x40, 0x00], // i
    [0x20, 0x40, 0x44, 0x3D, 0x00], // j
    [0x7F, 0x10, 0x28, 0x44, 0x00], // k
    [0x00, 0x41, 0x7F, 0x40, 0x00], // l
    [0x7C, 0x04, 0x18, 0x04, 0x78], // m
    [0x7C, 0x08, 0x04, 0x04, 0x78], // n
    [0x38, 0x44, 0x44, 0x44, 0x38], // o
    [0x7C, 0x14, 0x14, 0x14, 0x08], // p
    [0x08, 0x14, 0x14, 0x18, 0x7C], // q
    [0x7C, 0x08, 0x04, 0x04, 0x08], // r
    [0x48, 0x54, 0x54, 0x54, 0x20], // s
    [0x04, 0x3F, 0x44, 0x40, 0x20], // t
    [0x3C, 0x40, 0x40, 0x20, 0x7C], // u
    [0x1C, 0x20, 0x40, 0x20, 0x1C], // v
    [0x3C, 0x40, 0x30, 0x40, 0x3C], // w
    [0x44, 0x28, 0x10, 0x28, 0x44], // x
    [0x0C, 0x50, 0x50, 0x50, 0x3C], // y
    [0x44, 0x64, 0x54, 0x4C, 0x44], // z
    [0x00, 0x08, 0x36, 0x41, 0x00], // {
    [0x00, 0x00, 0x7F, 0x00, 0x00], // |
    [0x00, 0x41, 0x36, 0x08, 0x00], // }
    [0x08, 0x08, 0x2A, 0x1C, 0x08], // ~
];

/// 可见 ASCII 取字模（其余一律空格字形——fail-closed，不越表下标）。
fn glyph(c: u8) -> [u8; 5] {
    let idx = match c {
        0x20..=0x7E => (c - 0x20) as usize,
        _ => 0,
    };
    FONT5X7[idx]
}

// ---------------------------------------------------------------------------
// SSD1306 原语驱动（命令流 0x00 控制字节 / 数据流 0x40 控制字节；页寻址模式）
// ---------------------------------------------------------------------------

/// 发命令（控制字节 0x00 + 命令字节序列）。SSD1306 控制字节 Co=0/D/C#=0 表示
/// 其后全部为命令——流式命令面单笔 I2C 写合法（数据手册 §8.1.5.1；init 全序列
/// 25B 一笔发，b7 真板实锤：8B 缓冲护栏误拒 init 致"命令序列超长"假故障）。
fn oled_cmd(i2c: &mut I2cDriver, cmds: &[u8]) -> Result<(), String> {
    let mut buf = [0u8; 32];
    buf[0] = 0x00;
    if cmds.len() > 31 {
        return Err("命令序列超长".into());
    }
    buf[1..=cmds.len()].copy_from_slice(cmds);
    i2c.write(OLED_ADDR, &buf[..=cmds.len()], I2C_TIMEOUT)
        .map_err(|e| format!("I2C 命令写失败: {e:?}"))
}

/// 写一页 GDDRAM（128 列；控制字节 0x40 + 128 数据字节一次传输）。
fn oled_page(i2c: &mut I2cDriver, page: u8, data: &[u8; 128]) -> Result<(), String> {
    let mut buf = [0u8; 129];
    buf[0] = 0x40;
    buf[1..].copy_from_slice(data);
    oled_cmd(i2c, &[0xB0 | page, 0x00, 0x10])?;
    i2c.write(OLED_ADDR, &buf, I2C_TIMEOUT)
        .map_err(|e| format!("I2C 页数据写失败: {e:?}"))
}

/// 初始化（页寻址模式 + 电荷泵开 + 全清屏；序列为 SSD1306 数据手册标准序）。
fn oled_init(i2c: &mut I2cDriver) -> Result<(), String> {
    oled_cmd(
        i2c,
        &[
            0xAE, // display off
            0xD5, 0x80, // clock div
            0xA8, 0x3F, // multiplex 64
            0xD3, 0x00, // display offset
            0x40, // start line 0
            0x8D, 0x14, // charge pump on
            0x20, 0x02, // page addressing mode
            0xA1, // segment remap
            0xC8, // COM scan dec
            0xDA, 0x12, // COM pins
            0x81, 0xCF, // contrast
            0xD9, 0xF1, // precharge
            0xDB, 0x40, // VCOM detect
            0xA4, // resume from RAM
            0xA6, // normal display
            0xAF, // display on
        ],
    )?;
    oled_clear(i2c)
}

/// 全清屏（8 页 × 128B = 1KB 全零）。
fn oled_clear(i2c: &mut I2cDriver) -> Result<(), String> {
    for page in 0u8..8 {
        oled_page(i2c, page, &[0u8; 128])?;
    }
    Ok(())
}

/// 画一行文本（row 0/1 ↦ 页 0/2；≤21 可见字符，超出截断——行协议侧同样约束）。
fn oled_text(i2c: &mut I2cDriver, row: usize, text: &[u8]) -> Result<(), String> {
    if row >= OLED_ROWS {
        return Err(format!("文本行号越界: {row}"));
    }
    let page = (row * 2) as u8;
    let mut buf = [0u8; 128];
    let mut col = 0usize;
    for &c in text.iter().take(OLED_COLS) {
        if col + 6 > 128 {
            break;
        }
        buf[col..col + 5].copy_from_slice(&glyph(c));
        col += 6; // 5px 字模 + 1px 空列
    }
    oled_page(i2c, page, &buf)
}

/// I2C 总线扫描（0x03–0x77 逐地址单字节 0x00 探测，返回应答地址表）。
/// 判据①：0x3C 必在表内（全案 §4 批B ① 预期输出）。
fn i2c_scan(i2c: &mut I2cDriver) -> Vec<u8> {
    let mut found = Vec::new();
    for addr in 0x03u8..=0x77 {
        if i2c.write(addr, &[0x00], I2C_TIMEOUT).is_ok() {
            found.push(addr);
        }
    }
    found
}

// ---------------------------------------------------------------------------
// 外设底座 wrapper（move 进指令线程独占）
// ---------------------------------------------------------------------------

/// 指令线程独占的外设面：I2C（OLED）+ 两轴 LEDC 舵机 + UART2 整装。
/// 驱动 'static（外设 token 即 'static 无借用绑定；move 进线程后 esp_restart
/// 前永不 drop）。
struct Periph {
    i2c: I2cDriver<'static>,
    pan: LedcDriver<'static>,
    tilt: LedcDriver<'static>,
    uart: UartDriver<'static>,
}

// SAFETY：esp-idf-hal 0.44.1 未给 I2cDriver/LedcDriver 标 Send（上游保守留白，
// 全源 grep 无 unsafe impl Send for 二者；哨兵 UartTxInner 同款先例论证）。
// IDF 契约面：i2c_master_write_to_device / ledc_set_duty+ledc_update_duty /
// uart_write_bytes+uart_read_bytes 均为驱动内部加锁或无共享可变状态的线程安全
// 调用；本仓的全部访问在 move 后由指令线程**单线程独占**（无任何并发面），
// `Send` 主张与上游已标 Send 的 UartDriver 同据。
unsafe impl Send for Periph {}

impl Periph {
    /// 舵机转角（clamp 0–180 → duty → set+update；返回实际设定角）。
    fn servo_set(&mut self, axis: Axis, deg: u32) -> Result<u32, String> {
        let deg = deg.clamp(0, 180);
        let duty = deg_to_duty(deg);
        let driver = match axis {
            Axis::Pan => &mut self.pan,
            Axis::Tilt => &mut self.tilt,
        };
        driver
            .set_duty(duty)
            .map_err(|e| format!("LEDC set_duty 失败: {e:?}"))?;
        Ok(deg)
    }

    /// OLED 文本 3 行（21 列硬切）+ 角度行（row 3）刷新。
    fn oled_show(&mut self, text: &str, pan_deg: u32, tilt_deg: u32) -> Result<(), String> {
        // 先过滤成可见 ASCII（其余转空格，与 glyph 行为一致）再按 21 列硬切
        // 3 行——字节切片不劈多字节字符（char boundary 安全），行对齐不漂移。
        let ascii: String = text
            .chars()
            .map(|c| {
                if (0x20..=0x7E).contains(&(c as u32)) {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let bytes = ascii.as_bytes();
        for row in 0..(OLED_ROWS - 1) {
            let start = row * OLED_COLS;
            let chunk = if start < bytes.len() {
                let end = (start + OLED_COLS).min(bytes.len());
                &bytes[start..end]
            } else {
                b"" as &[u8]
            };
            oled_text(&mut self.i2c, row, chunk)?;
        }
        let angle_line = format!("P:{pan_deg:03} T:{tilt_deg:03}");
        oled_text(&mut self.i2c, OLED_ROWS - 1, angle_line.as_bytes())
    }
}

#[derive(Clone, Copy)]
enum Axis {
    Pan,
    Tilt,
}

/// 应答行回写（UART2；\r\n 结尾，终端与 pyserial 同读友好）。
fn reply(uart: &mut UartDriver, line: &str) {
    let _ = uart.write(format!("{line}\r\n").as_bytes());
    // 整装 UartDriver 未实现 std::io::Write（vendored 面只有 embedded_io::Write），
    // 用固有 wait_tx_done(portMAX_DELAY) 等移位完成——语义即"应答发完再返回"。
    let _ = uart.wait_tx_done(u32::MAX);
}

/// 收一行（阻塞逐字节读直到 '\n'；'\r' 剥离；超 [LINE_CAP] 丢弃整行回 Err）。
/// `first` 是 cmd_loop 双面分流时已消耗的首字节（非 0x31 的行面回流字节）。
fn read_line(uart: &mut UartDriver, first: u8) -> Result<String, &'static str> {
    let mut buf = Vec::new();
    if first == b'\n' {
        // 空行保持批B 行为：静默空串（cmd_loop trim 后跳过）
        return Ok(String::new());
    }
    buf.push(first);
    let mut byte = [0u8; 1];
    loop {
        let n = uart
            .read(&mut byte, u32::MAX)
            .map_err(|_| "uart read 失败")?;
        if n == 0 {
            continue;
        }
        match byte[0] {
            b'\n' => {
                while matches!(buf.last(), Some(b'\r')) {
                    buf.pop();
                }
                return Ok(String::from_utf8_lossy(&buf).into_owned());
            }
            b => {
                buf.push(b);
                if buf.len() > LINE_CAP {
                    return Err("line too long");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 帧面（批C）：rr3.call 能力帧执行——宿主 rr-hal::esp32_uart 桥后端直连点名
// ---------------------------------------------------------------------------

/// UART2 的帧面 io 窄缝：驱动可变借用 + cmd_loop 已窥视消耗的首字节。实现
/// rr-ipc [`Rr3ByteStream`] 两 `usize` 方法（板侧接帧语义的全部——transport
/// mod.rs 形状用例为证），经 blanket impl 免费获得 rr3_send_buf/rr3_recv_buf
/// ——与宿主/桥/哨兵同源帧语义，零第二份帧层。行面继续直用 UartDriver 固有
/// 方法（逐字节语义），两面互不侵扰。
struct UartIo<'a> {
    uart: &'a mut UartDriver<'static>,
    /// 窥视消耗的帧 magic 首字节（0x31）：Read 首次先吐它，128B 头的 magic
    /// 窗口才完整；发 Response 时此位恒 None（无窥视）。
    pre: Option<u8>,
}

impl Rr3ByteStream for UartIo<'_> {
    fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
        if let Some(b) = self.pre.take() {
            if !buf.is_empty() {
                buf[0] = b;
                return 1;
            }
            return 0;
        }
        // 固有 read（阻塞，delay = portMAX_DELAY）；错误折叠为 0（缝上无错误
        // 通道，Closed 形状）——细节 eprintln 当场可见（批J G5 口径）。
        match self.uart.read(buf, u32::MAX) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[periph] UART 读失败: {e:?}");
                0
            }
        }
    }

    fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
        let mut done = 0usize;
        while done < buf.len() {
            match self.uart.write(&buf[done..]) {
                Ok(0) => break, // 无进展（WriteZero 形状）
                Ok(n) => done += n,
                Err(e) => {
                    eprintln!("[periph] UART 写失败: {e:?}");
                    break;
                }
            }
        }
        // 等移位完成再返回（reply 同款"应答发完再返回"语义）
        let _ = self.uart.wait_tx_done(u32::MAX);
        done
    }
}

/// 帧头 timestamp（esp_timer 单调时基 µs→ns；epoch 无关——时间戳只作诊断参考
/// 不参与校验，板侧取不到 epoch 墙钟，与宿主 rr3_now_ns 各自独立合法）。
fn rr3_frame_now_ns() -> u64 {
    (unsafe { esp_timer_get_time() }).unsigned_abs() * 1000
}

/// 双面共享状态：行面/帧面同一份文本与角度记账，指令线程单线程独占无锁。
struct PeriphState {
    /// 最近一次 OLED 文本（行面 OLED 命令 / 帧面 expression 出字）
    last_text: String,
    pan_deg: u32,
    tilt_deg: u32,
    /// 最近一次表情（帧面 expression 应答的 previous 字段；开机未出过表情=none）
    prev_expr: String,
    /// 显示模式：true=帧面 64x64 点阵位图占全屏（舵机角度行不刷，不凿位图）；
    /// 行面 OLED 命令 / 帧面 expression 复位 false。
    frame_mode: bool,
    /// 表情模式：true=胶囊眼 + 生命感微动作（帧面 expression 专有）；位图占屏
    /// 与行面文本模式复位 false（动画只在此模式跑）。
    expr_mode: bool,
    /// 表情基态（expr_eye_base 预设）：眼开合% + 水平/垂直扫视偏移
    base_open: u32,
    base_dx: i16,
    base_dy: i16,
    /// 微动作时钟（esp_timer ms）：眨眼/扫视的下一触发点与进行中截止点 +
    /// 上次渲染合成参数（变化才重绘，省 I2C）。
    next_blink_ms: u64,
    blink_until_ms: u64,
    next_glance_ms: u64,
    glance_until_ms: u64,
    glance_dx: i16,
    glance_dy: i16,
    drawn: (u32, i16, i16),
    /// 云台自主扫摆（WANDER 行面开关）：pan/tilt 缓速连续摆动不停顿。外部
    /// PAN/TILT/SERVO/SCAN 或帧面 servo 写到达即关（外部点名优先）。
    wander_on: bool,
    wander_pan_dir: i8,
    wander_tilt_dir: i8,
    /// 扫摆节拍：60ms 一拍的拍号（u8 回绕无妨，只用模数）+ 下拍时间。
    wander_phase: u8,
    next_wander_ms: u64,
}

impl PeriphState {
    /// 角度行刷新：文本模式全 4 行重绘（批B 行为不变）；帧模式/表情模式跳过
    /// ——两者都占满 8 页，刷角度行会凿穿画面。
    fn refresh_angle_row(&self, ph: &mut Periph) {
        if self.frame_mode || self.expr_mode {
            return;
        }
        if let Err(e) = ph.oled_show(&self.last_text, self.pan_deg, self.tilt_deg) {
            eprintln!("[periph] 角度行刷新失败: {e}");
        }
    }

    /// 表情切换时重置微动作时钟：立即显基态，眨眼/扫视各按随机间隔推迟。
    fn anim_reset(&mut self) {
        let now = now_ms();
        self.blink_until_ms = 0;
        self.glance_until_ms = 0;
        self.next_blink_ms = now + 2800 + (esp_random_u32() % 2400) as u64;
        self.next_glance_ms = now + 4000 + (esp_random_u32() % 4000) as u64;
    }

    /// 表情微动作一拍（cmd_loop 窥视空档驱动，10ms 一拍）：自动眨眼（2.8–5.2s
    /// 随机间隔，闭 130ms）+ 随机微扫视（4–8s 间隔，水平 ±6px、垂直 ±2 行，
    /// 500–900ms 回位）。合成参数与上次一致则不重绘（I2C 重绘有成本）；
    /// 非表情模式一拍即回。
    fn anim_tick(&mut self, ph: &mut Periph) {
        if !self.expr_mode {
            return;
        }
        let now = now_ms();
        if self.blink_until_ms == 0 {
            if now >= self.next_blink_ms {
                self.blink_until_ms = now + 130;
                self.next_blink_ms = now + 2800 + (esp_random_u32() % 2400) as u64;
            }
        } else if now >= self.blink_until_ms {
            self.blink_until_ms = 0;
        }
        let open = if self.blink_until_ms != 0 {
            4
        } else {
            self.base_open
        };
        let mut dx = self.base_dx;
        let mut dy = self.base_dy;
        if self.glance_until_ms == 0 {
            if now >= self.next_glance_ms {
                self.glance_dx = (esp_random_u32() % 13) as i16 - 6;
                self.glance_dy = match esp_random_u32() % 3 {
                    0 => -2,
                    1 => 2,
                    _ => 0,
                };
                self.glance_until_ms = now + 500 + (esp_random_u32() % 400) as u64;
                self.next_glance_ms = now + 4000 + (esp_random_u32() % 4000) as u64;
            }
        } else if now >= self.glance_until_ms {
            self.glance_until_ms = 0;
        } else {
            dx += self.glance_dx;
            dy += self.glance_dy;
        }
        let key = (open, dx, dy);
        if key != self.drawn {
            self.drawn = key;
            if let Err(e) = oled_capsules(&mut ph.i2c, open, dx, dy) {
                eprintln!("[periph] 表情微动作渲染失败: {e}");
            }
        }
    }

    /// 云台自主扫摆一拍（cmd_loop 窥视空档驱动，60ms 一拍）：pan 每 2 拍走
    /// 2°（≈16.7°/s，全程 10.8s）、tilt 每 5 拍走 2°（≈6.7°/s，全程 27s），
    /// 端点折返持续不停——两轴节奏错开避免机械同步感。2° 步进越过 SG90 死区
    /// （脉宽 ~5µs ≈ 4 duty ticks，1° ≈ 9.1 ticks 在死区边缘可能静默不动）。
    /// 外部命令到达即 wander_on=false，本函数一拍即回。角度行每 10 步刷一次
    /// （600ms，省 I2C；expr/frame 模式由 refresh_angle_row 守卫跳过）。
    fn servo_tick(&mut self, ph: &mut Periph) {
        if !self.wander_on || now_ms() < self.next_wander_ms {
            return;
        }
        self.next_wander_ms = now_ms() + 60;
        self.wander_phase = self.wander_phase.wrapping_add(1);
        if self.wander_phase % 2 == 0 {
            let next = wander_step(self.pan_deg, &mut self.wander_pan_dir);
            if let Ok(d) = ph.servo_set(Axis::Pan, next) {
                self.pan_deg = d;
            }
        }
        if self.wander_phase % 5 == 0 {
            let next = wander_step(self.tilt_deg, &mut self.wander_tilt_dir);
            if let Ok(d) = ph.servo_set(Axis::Tilt, next) {
                self.tilt_deg = d;
            }
        }
        if self.wander_phase % 20 == 0 {
            self.refresh_angle_row(ph);
        }
    }
}

/// 扫摆步进：deg ± 2°，端点（0/180）折返方向后返回新角度。dir 应为 ±1
/// （WANDER 开关随机起步；0 防御上不会出现——0 时原地不动）。
fn wander_step(deg: u32, dir: &mut i8) -> u32 {
    let mut next = deg as i32 + *dir as i32 * 2;
    if next >= 180 {
        next = 180;
        *dir = -1;
    } else if next <= 0 {
        next = 0;
        *dir = 1;
    }
    next as u32
}

/// 帧面请求（协议门全过后的视图）：id/method/params 摘出，帧头位形随行携带
/// （应答回带 trace_id / dst=src_task 用）。
struct FrameReq {
    id: u64,
    method: String,
    topic: String,
    args: Value,
    src_task: u64,
    trace_id: u64,
}

/// 收帧 + 协议门（全过才返回 Ok）：`rr3_framebuf_read` 整帧收 → 帧还原（magic/
/// 长度校验在 rr-ipc）→ dst 门（非本板位形丢弃）→ payload 前缀门（非 Request
/// 丢弃）→ JSON 门 → id 门。任一门失败回 Err = **丢弃不回帧**（fail-closed：
/// 帧级不可信的请求不作确定性应答；宿主侧重开连接即恢复）。半帧失败后流上可能
/// 残留尾巴字节——下一轮 magic 窥视大概率落行面 ERR，自愈路径在案。
fn frame_recv(io: &mut UartIo) -> Result<FrameReq, String> {
    let buf = io.rr3_recv_buf().map_err(|e| format!("收帧失败: {e:?}"))?;
    let frame = Rr3Frame::try_from(buf).map_err(|e| format!("帧还原失败: {e:?}"))?;
    if frame.header.dst_task != RR3_PERIPH_TASK {
        return Err(format!(
            "dst_task={} 非本板位形 {RR3_PERIPH_TASK}",
            frame.header.dst_task
        ));
    }
    let (ty, body) =
        rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if ty != Rr3PayloadType::Request {
        return Err(format!("payload 类型非 Request: {ty:?}"));
    }
    let v: Value = serde_json::from_slice(&body).map_err(|e| format!("请求体非 JSON: {e}"))?;
    let id = v.get("id").and_then(Value::as_u64).ok_or("请求缺 id")?;
    let method = v
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let (topic, args) = match v.get("params") {
        Some(p) => (
            p.get("topic")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            p.get("args").cloned().unwrap_or(Value::Null),
        ),
        None => (String::new(), Value::Null),
    };
    Ok(FrameReq {
        id,
        method,
        topic,
        args,
        src_task: frame.header.src_task,
        trace_id: frame.header.trace_id,
    })
}

/// 主题分发：method 恒 rr3.call（rrd 转发形状，宿主桥同款）；主题只认两条在册
/// 能力面。method/主题/args 层面的失败是**业务性失败**——折 result.error 点名
/// 回帧（模块面同款形状，宿主 rr3_bridge_parse_response 的解析契约）。
fn frame_dispatch(ph: &mut Periph, st: &mut PeriphState, req: &FrameReq) -> Result<Value, String> {
    if req.method != "rr3.call" {
        return Err(format!("method 非法: \"{}\"（恒 rr3.call）", req.method));
    }
    match req.topic.as_str() {
        RR3_SERVO_TOPIC => frame_handle_servo(ph, st, &req.args),
        RR3_FACE_TOPIC => frame_handle_face(ph, st, &req.args),
        other => Err(format!("未知主题: \"{other}\"")),
    }
}

/// 舵机执行（rr3-act/servo/command）：args 带 angle_deg = 写（执行 + 生效角回
/// 显），不带 = 读（LEDC 寄存器 duty 回读换算）。axis 可选缺省 pan（宿主单轴
/// trait ↔ 板上双轴的映射面，RR3_HAL_PERIPH_AXIS env + 本参数两级）；写路径
/// 状态记账 + 角度行刷新（帧模式跳过，不凿位图）。
fn frame_handle_servo(
    ph: &mut Periph,
    st: &mut PeriphState,
    args: &Value,
) -> Result<Value, String> {
    let axis = match args.get("axis") {
        None | Some(Value::Null) => Axis::Pan,
        Some(v) => match v.as_str() {
            Some("pan") => Axis::Pan,
            Some("tilt") => Axis::Tilt,
            _ => return Err(format!("axis 非法: {v}（pan|tilt）")),
        },
    };
    match args.get("angle_deg") {
        None | Some(Value::Null) => {
            let duty = match axis {
                Axis::Pan => ph.pan.get_duty(),
                Axis::Tilt => ph.tilt.get_duty(),
            };
            Ok(json!({ "angle_deg": duty_to_deg(duty), "move_ms": 0 }))
        }
        Some(v) => {
            let deg = v.as_u64().ok_or_else(|| format!("angle_deg 非法: {v}"))?;
            if deg > 180 {
                return Err(format!("角度超范围 0-180: {deg}"));
            }
            let d = ph.servo_set(axis, deg as u32)?;
            st.wander_on = false; // 外部点名优先，自主扫摆让位
            match axis {
                Axis::Pan => st.pan_deg = d,
                Axis::Tilt => st.tilt_deg = d,
            }
            st.refresh_angle_row(ph);
            Ok(json!({ "angle_deg": d, "move_ms": 0 }))
        }
    }
}

/// 表情执行（rr3-interact/face/expression）：expression 变体出胶囊眼（基态
/// 预设 + 微动作动画，rr-hal-api Rr3FaceFrame 契约之外的表情名面），
/// frame 变体 8x8 点阵 → OLED 中央 64x64 放大渲染（HAL 渲染变体：行主序
/// 8 字节、每字节 bit7 为该行最左点）。
fn frame_handle_face(ph: &mut Periph, st: &mut PeriphState, args: &Value) -> Result<Value, String> {
    if let Some(v) = args.get("expression") {
        let expr = v
            .as_str()
            .ok_or_else(|| format!("expression 非法: {v}（须字符串）"))?;
        if !RR3_FACE_EXPRS.contains(&expr) {
            return Err(format!("expression 不在白名单: \"{expr}\""));
        }
        let (base_open, base_dx, base_dy) = expr_eye_base(expr);
        st.expr_mode = true;
        st.frame_mode = false;
        st.base_open = base_open;
        st.base_dx = base_dx;
        st.base_dy = base_dy;
        let previous = std::mem::replace(&mut st.prev_expr, expr.to_string());
        st.last_text = format!("EXPR {expr}");
        st.anim_reset();
        oled_capsules(&mut ph.i2c, base_open, base_dx, base_dy)?;
        st.drawn = (base_open, base_dx, base_dy);
        Ok(json!({ "expression": expr, "previous": previous }))
    } else if let Some(v) = args.get("frame") {
        let arr = v
            .as_array()
            .ok_or_else(|| format!("frame 非法: {v}（须 8 字节数组）"))?;
        if arr.len() != 8 {
            return Err(format!("frame 长度非法: {}（须 8）", arr.len()));
        }
        let mut f = [0u8; 8];
        for (i, item) in arr.iter().enumerate() {
            let b = item
                .as_u64()
                .ok_or_else(|| format!("frame[{i}] 非法: {item}"))?;
            if b > 255 {
                return Err(format!("frame[{i}] 超 u8: {b}"));
            }
            f[i] = b as u8;
        }
        oled_frame(&mut ph.i2c, &f)?;
        st.frame_mode = true;
        st.expr_mode = false;
        Ok(json!({ "applied": true }))
    } else {
        Err("缺 expression/frame 之一".into())
    }
}

/// 8x8 点阵帧 → OLED 中央 64x64 放大渲染：源帧每像素横向铺 8 列、纵向铺 8 行，
/// 落在列 32..96 × 全 8 页；页 p 全部 8 个位行同显源行 p（sy = (p*8+by)/8 = p）。
/// 位序映射：MAX7219 字节 bit7=行最左 ↔ 目标列 x=32 处（c=0 取 bit7）——方向
/// 不镜像；SSD1306 页内列字节 bit0=页顶。两侧留白清零（居中不残影）。
fn oled_frame(i2c: &mut I2cDriver, frame: &[u8; 8]) -> Result<(), String> {
    for page in 0u8..8 {
        let src = frame[page as usize];
        let mut buf = [0u8; 128];
        for (x, cell) in buf.iter_mut().enumerate().skip(32).take(64) {
            let c = (x - 32) / 8;
            *cell = if (src >> (7 - c)) & 1 == 1 { 0xFF } else { 0 };
        }
        oled_page(i2c, page, &buf)?;
    }
    Ok(())
}

/// esp_timer 毫秒（单调时基，微动作时钟源；u64 毫秒 58 万年不回绕）。
fn now_ms() -> u64 {
    (unsafe { esp_timer_get_time() }).unsigned_abs() / 1000
}

/// 硬件 RNG（WIFI/蓝牙未启也能用——ESP32 硬件熵源独立于无线栈）。
fn esp_random_u32() -> u32 {
    unsafe { esp_random() }
}

/// 胶囊眼渲染（表情模式画面）：两只竖直胶囊眼，开合 open_pct% 与扫视偏移
/// (dx,dy) 由表情基态 + 微动作动画合成。形状=圆角竖矩形（圆角半径取
/// min(半宽, 半高)：满高 26px 时即纯胶囊，眯缝低压扁成 18px 宽横缝不退化成
/// 圆盘）；行内实心段解析求宽（直段区全宽、圆角区 sqrt 余量），逐页写
/// GDDRAM。扫视出界自然裁剪（0..127 列钳制）不回卷。
fn oled_capsules(i2c: &mut I2cDriver, open_pct: u32, dx: i16, dy: i16) -> Result<(), String> {
    const W2: i32 = 9; // 眼半宽（眼宽 18px）
    const H_FULL: i32 = 26; // 满开眼高
    const CY: i32 = 32; // 眼中心行（64 行画布居中）
    const EYES: [i32; 2] = [42, 86]; // 眼中心列（128 列画布左右分置）
    let h = ((H_FULL * open_pct.clamp(4, 100) as i32) / 100).max(4);
    let half_h = h / 2;
    let rc = W2.min(half_h); // 圆角半径
    let straight = half_h - rc; // 垂直直段半长
    for page in 0u8..8 {
        let mut buf = [0u8; 128];
        let y0 = page as i32 * 8;
        for y in y0..y0 + 8 {
            let py = (y - (CY + dy as i32)).abs();
            if py > half_h {
                continue;
            }
            for &ecx in &EYES {
                let cx = ecx + dx as i32;
                let hw = if py <= straight {
                    W2
                } else {
                    let rem = rc * rc - (py - straight) * (py - straight);
                    if rem <= 0 {
                        continue;
                    }
                    (W2 - rc + (rem as f32).sqrt() as i32).min(W2)
                };
                for x in (cx - hw).max(0)..=(cx + hw).min(127) {
                    buf[x as usize] |= 1 << (y % 8);
                }
            }
        }
        oled_page(i2c, page, &buf)?;
    }
    Ok(())
}

/// 表情→胶囊眼基态预设（开合% / 水平扫视 / 垂直扫视）：七态白名单各有面目，
/// 微动作动画在此基态上叠加（眨眼/扫视见 anim_tick）。
fn expr_eye_base(expr: &str) -> (u32, i16, i16) {
    match expr {
        "blink" => (4, 0, 0),     // 眯缝
        "happy" => (55, 0, 2),    // 眯眼微垂
        "talk" => (70, 0, 0),     // 说话态半开
        "think" => (100, 6, -3),  // 抬眼右上
        "listen" => (100, -6, 0), // 侧耳向左
        "turn" => (100, 8, 0),    // 回望右方
        _ => (100, 0, 0),         // idle 与兜底：正目全开
    }
}

/// Response 回帧：result 在位（业务失败折 {"error": 文案}——宿主解析契约），
/// trace_id 原样回带、src=本板位形 2、dst=调用方、timestamp 单调时基。发送失败
/// eprintln 点名不重试（宿主 io 失败自会重开连接重发）。
fn frame_send_response(io: &mut UartIo, req: &FrameReq, result: Result<Value, String>) {
    let result = match result {
        Ok(v) => v,
        Err(e) => json!({ "error": e }),
    };
    let body = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
    let payload = rr3_body_encode(Rr3PayloadType::Response, body.to_string().as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = req.trace_id;
    hdr.timestamp_ns = rr3_frame_now_ns();
    hdr.src_task = RR3_PERIPH_TASK;
    hdr.dst_task = req.src_task;
    let resp = Rr3Frame {
        header: hdr,
        payload,
    };
    if let Err(e) = io.rr3_send_buf(Rr3FrameBuf::from(&resp)) {
        eprintln!("[periph] Response 发送失败: {e:?}");
    }
}

/// 帧面一轮（cmd_loop 双面分流的 0x31 分支）：收帧+协议门（失败丢弃不回）→
/// 分发执行（io 已还，独占驱动面）→ Response 回帧。三段式借用：UartIo 只在
/// 收/发两段活着，执行段 ph.i2c/pan/tilt 自由可用。
fn frame_round(ph: &mut Periph, st: &mut PeriphState, first: u8) {
    let req = {
        let mut io = UartIo {
            uart: &mut ph.uart,
            pre: Some(first),
        };
        match frame_recv(&mut io) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[periph] 帧面丢弃: {e}");
                return;
            }
        }
    };
    println!("[periph] 帧面 id={} topic=\"{}\"", req.id, req.topic);
    let result = frame_dispatch(ph, st, &req);
    if let Err(e) = &result {
        eprintln!("[periph] 帧面业务失败 id={}: {e}", req.id);
    }
    let mut io = UartIo {
        uart: &mut ph.uart,
        pre: None,
    };
    frame_send_response(&mut io, &req, result);
}

// ---------------------------------------------------------------------------
// 指令循环（批B 行面 + 批C 帧面双面并存：行面保留调试与舵机五点 SCAN 判据）
// ---------------------------------------------------------------------------

/// 指令线程主循环（批C 双面）：逐字节窥视首字节——0x31（帧 magic LE 首字节）
/// 走帧面 round（收帧 → rr3.call 分发 → Response 应答），其余作行首字节继续收
/// 行走批B 行面。未知指令 ERR（fail-closed，不猜不拖——全案批B 指令解析面判据）。
fn cmd_loop(mut ph: Periph) {
    let mut st = PeriphState {
        last_text: String::from("(boot)"),
        pan_deg: 90,
        tilt_deg: 90,
        prev_expr: String::from("none"),
        frame_mode: false,
        expr_mode: false,
        base_open: 100,
        base_dx: 0,
        base_dy: 0,
        next_blink_ms: 0,
        blink_until_ms: 0,
        next_glance_ms: 0,
        glance_until_ms: 0,
        glance_dx: 0,
        glance_dy: 0,
        drawn: (100, 0, 0),
        wander_on: false,
        wander_pan_dir: 0,
        wander_tilt_dir: 0,
        wander_phase: 0,
        next_wander_ms: 0,
    };
    reply(
        &mut ph.uart,
        "READY periph-base v0.3.1 line+frame+anim+wander (type HELP for commands)",
    );
    loop {
        let mut first = [0u8; 1];
        // 窥视带 1-tick（10ms）超时：空口返回 Ok(0) 驱动表情微动作一拍——
        // 指令线程单线程独占 I2C/OLED，动画只能搭窥视空档执行；有字节
        // （Ok(n≥1)）照旧双面分流，帧/行路径的后续读仍 portMAX_DELAY 阻塞
        // 不受影响。Err 才是真读失败，退避重试。
        match ph.uart.read(&mut first, 1) {
            Ok(0) => {
                st.anim_tick(&mut ph);
                st.servo_tick(&mut ph);
                continue;
            }
            Ok(_) => {}
            Err(_) => {
                eprintln!("[periph] UART 读失败，退避后重试");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
        if first[0] == RR3_MAGIC_FIRST {
            frame_round(&mut ph, &mut st, first[0]);
            continue;
        }
        let line = match read_line(&mut ph.uart, first[0]) {
            Ok(l) => l,
            Err(why) => {
                eprintln!("[periph] 收行失败: {why}");
                continue;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let head = parts.next().unwrap_or("");
        let result: Result<String, String> = match head {
            "HELP" => Ok(
                "CMDS: OLED <text> | PAN <deg> | TILT <deg> | SERVO <pan> <tilt> | SCAN | WANDER [STOP] | INFO | HELP"
                    .into(),
            ),
            "OLED" => {
                // 参数取原始行去指令头，保留内嵌空格（≤63 字符 = 3 行 × 21 列，row3 角度行）
                let rest = line[head.len()..].trim_start();
                if rest.is_empty() {
                    Err("OLED 缺文本参数".into())
                } else if rest.len() > OLED_COLS * (OLED_ROWS - 1) {
                    Err(format!(
                        "OLED 文本超长（≤{} 字符 = 3 行 × 21 列，第 4 行为角度行）",
                        OLED_COLS * (OLED_ROWS - 1)
                    ))
                } else {
                    let owned = rest.to_string();
                    match ph.oled_show(&owned, st.pan_deg, st.tilt_deg) {
                        Ok(()) => {
                            st.last_text = owned;
                            st.frame_mode = false;
                            st.expr_mode = false;
                            Ok(format!("OK oled \"{}\"", st.last_text))
                        }
                        Err(e) => Err(e),
                    }
                }
            }
            "PAN" => parse_deg(parts.next()).and_then(|deg| match ph.servo_set(Axis::Pan, deg) {
                Ok(d) => {
                    st.wander_on = false; // 外部点名优先，自主扫摆让位
                    st.pan_deg = d;
                    st.refresh_angle_row(&mut ph);
                    Ok(format!("OK pan={d}"))
                }
                Err(e) => Err(e),
            }),
            "TILT" => parse_deg(parts.next()).and_then(|deg| match ph.servo_set(Axis::Tilt, deg) {
                Ok(d) => {
                    st.wander_on = false;
                    st.tilt_deg = d;
                    st.refresh_angle_row(&mut ph);
                    Ok(format!("OK tilt={d}"))
                }
                Err(e) => Err(e),
            }),
            "SERVO" => match (parse_deg(parts.next()), parse_deg(parts.next())) {
                (Ok(p), Ok(t)) => match (
                    ph.servo_set(Axis::Pan, p),
                    ph.servo_set(Axis::Tilt, t),
                ) {
                    (Ok(p), Ok(t)) => {
                        st.wander_on = false;
                        st.pan_deg = p;
                        st.tilt_deg = t;
                        st.refresh_angle_row(&mut ph);
                        Ok(format!("OK pan={p} tilt={t}"))
                    }
                    (Err(e), _) | (_, Err(e)) => Err(e),
                },
                _ => Err("SERVO 需两个 0-180 角度参数".into()),
            },
            "WANDER" => match parts.next() {
                Some("STOP") => {
                    st.wander_on = false;
                    Ok(format!("OK wander off pan={} tilt={}", st.pan_deg, st.tilt_deg))
                }
                None => {
                    st.wander_on = true;
                    // 两轴方向独立随机起步，立即起摆
                    st.wander_pan_dir = if esp_random_u32() % 2 == 0 { 1 } else { -1 };
                    st.wander_tilt_dir = if esp_random_u32() % 2 == 0 { 1 } else { -1 };
                    st.next_wander_ms = 0;
                    Ok(format!(
                        "OK wander on pan={} tilt={}",
                        st.pan_deg, st.tilt_deg
                    ))
                }
                Some(other) => Err(format!("WANDER 参数非法: {other}（无参=开，STOP=停）")),
            },
            "SCAN" => {
                // 判据②：两轴各 0/45/90/135/180 五点，逐点串口回读设定角
                let mut ok = true;
                st.wander_on = false; // 判据测试不被自主扫摆扰动
                for (i, deg) in [0u32, 45, 90, 135, 180].into_iter().enumerate() {
                    let p = ph.servo_set(Axis::Pan, deg);
                    let t = ph.servo_set(Axis::Tilt, deg);
                    match (p, t) {
                        (Ok(p), Ok(t)) => {
                            st.pan_deg = p;
                            st.tilt_deg = t;
                            // 回读闭环：LEDC 寄存器实际 duty 换算回角度（duty_to_deg），
                            // 与设定角并打——判据②"目视+串口回读一致"的硬件侧证据
                            let pr = duty_to_deg(ph.pan.get_duty());
                            let tr = duty_to_deg(ph.tilt.get_duty());
                            println!(
                                "[periph] SCAN point={} pan={}(read {pr}) tilt={}(read {tr})",
                                i + 1,
                                p,
                                t
                            );
                            st.refresh_angle_row(&mut ph);
                            reply(
                                &mut ph.uart,
                                &format!("SCAN point={} pan={p} read={pr} tilt={t} read={tr}", i + 1),
                            );
                            thread::sleep(Duration::from_millis(600));
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            ok = false;
                            eprintln!("[periph] SCAN 失败: {e}");
                            break;
                        }
                    }
                }
                if ok {
                    Ok("OK scan done".into())
                } else {
                    Err("SCAN 中途失败（见 monitor 日志）".into())
                }
            }
            "INFO" => Ok(format!(
                "INFO pan={} tilt={} oled_last=\"{}\" free={}B min_free={}B",
                st.pan_deg,
                st.tilt_deg,
                st.last_text,
                unsafe { esp_get_free_heap_size() },
                unsafe { esp_get_minimum_free_heap_size() }
            )),
            "FONT" => {
                // 字模自检护栏：打印 "ABC123" 的 5x7 字模 hex，供人工比对
                let hex: Vec<String> = "ABC123"
                    .bytes()
                    .map(|c| {
                        glyph(c)
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<String>()
                    })
                    .collect();
                Ok(format!("FONT ABC123 = {}", hex.join(",")))
            }
            other => Err(format!("unknown cmd \"{other}\"（HELP 看指令表）")),
        };
        match result {
            Ok(line) => reply(&mut ph.uart, &line),
            Err(e) => reply(&mut ph.uart, &format!("ERR {e}")),
        }
    }
}

/// 角度参数解析（0–180 之外一律 Err——调用方不再 clamp，fail-closed）。
fn parse_deg(s: Option<&str>) -> Result<u32, String> {
    let Some(s) = s else {
        return Err("缺角度参数".into());
    };
    let deg: u32 = s.parse().map_err(|_| format!("角度参数非法: \"{s}\""))?;
    if deg > 180 {
        return Err(format!("角度超范围 0-180: {deg}"));
    }
    Ok(deg)
}

// ---------------------------------------------------------------------------

/// 三点堆采样观测行（批D 三点 RAM 复测沿用本法，批B 先落观测面）。
fn ram_point(point: &str) {
    let free = unsafe { esp_get_free_heap_size() };
    let min_free = unsafe { esp_get_minimum_free_heap_size() };
    println!("[periph] RAM point={point} free={free}B min_free={min_free}B");
}

fn main() {
    // IDF 绑定面补丁（esp-idf-hal 工程惯例起手，哨兵同款）
    link_patches();

    println!(
        "[periph] surface=esp-rs-std crate=esp32-periph-base oled=i2c0@21/22 servo=ledc@32/33 cmd=uart2@115200 line+frame"
    );
    ram_point("1-boot");

    let peripherals = Peripherals::take().unwrap();

    // ---- I2C0 @ GPIO21/22 400kHz（判据①：扫描 0x3C 应答）----
    let i2c_config = I2cConfig::new()
        .baudrate(Hertz(400_000))
        .sda_enable_pullup(true)
        .scl_enable_pullup(true);
    let mut i2c = I2cDriver::new(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
        &i2c_config,
    )
    .expect("I2C0 驱动安装失败");
    let found = i2c_scan(&mut i2c);
    if found.contains(&OLED_ADDR) {
        println!(
            "[periph] I2C 扫描应答: {:02X?} —— OLED 0x{OLED_ADDR:02X} 在列 ✓",
            found
        );
    } else {
        println!(
            "[periph] I2C 扫描应答: {found:02X?} —— 0x{OLED_ADDR:02X} 不在列 ✗（查接线/上拉）"
        );
    }
    // 判据①失败处置链第一步：换 I2C 频率档（100k）重试 init（全案 §4 批B ①）。
    // 实现走 i2c_set_period 运行中重配 SCL 分频（APB 80MHz/(high+low=800)=100kHz，
    // 14bit 值域内 400/400 对称占空）——外设 token 单次 move 语义下同 token 无法
    // 二次 move 重装驱动（E0382 实录），且 IDF param_config 与 driver_install
    // 解耦，已装驱动运行中改周期合法（IDF v5.2.2 driver/i2c.h L465 在案）。
    // 真板诊断面（b7 实录）：init 失败打印 Err 明细不 panic——panic_abort 下
    // expect 无消息直接 Guru 复位循环，排障拿不到证据；终败继续跑指令面。
    let mut oled_ok = false;
    match oled_init(&mut i2c) {
        Ok(()) => {
            oled_ok = true;
            println!("[periph] OLED init ✓（400k 档一次过）");
        }
        Err(e) => {
            println!("[periph] OLED 400k init 失败: {e}");
            println!("[periph] 降 100k 重试（失败处置链）");
            if let Err(e) = esp_idf_sys::esp!(unsafe { esp_idf_sys::i2c_set_period(0, 400, 400) }) {
                println!("[periph] i2c_set_period 降频失败: {e:?}");
            }
            for attempt in 1..=5u32 {
                thread::sleep(Duration::from_secs(2));
                match oled_init(&mut i2c) {
                    Ok(()) => {
                        oled_ok = true;
                        println!("[periph] OLED init ✓（100k 档第 {attempt} 次重试过）");
                        break;
                    }
                    Err(e) => println!("[periph] OLED 100k 重试 {attempt}/5 失败: {e}"),
                }
            }
        }
    }
    if oled_ok {
        // boot 画面 4 行全出（4 行扩行能力的 boot 直证；row3 由 oled_show 常刷，
        // boot 期先摆初始角度占位）
        let boot_rows: [(&[u8], &str); 4] = [
            (b"ROCK3 PERIPH BASE", "boot 行0写失败"),
            (b"4-ROW OLED READY", "boot 行1写失败"),
            (b"UART2 CMD @115200", "boot 行2写失败"),
            (b"P:090 T:090", "boot 角度行写失败"),
        ];
        let mut boot_ok = true;
        for (row, (text, msg)) in boot_rows.iter().enumerate() {
            if let Err(e) = oled_text(&mut i2c, row, text) {
                println!("[periph] {msg}: {e}");
                boot_ok = false;
            }
        }
        if boot_ok {
            println!("[periph] OLED ✓ boot 画面已出（4 行）");
        }
    } else {
        println!(
            "[periph] OLED init 终败（400k/100k 双档）——不 panic 继续跑指令面（查上拉/接线/供电）"
        );
    }

    // ---- LEDC timer0 50Hz 14bit：channel0→GPIO32 pan / channel1→GPIO33 tilt ----
    let timer_config = TimerConfig::new()
        .frequency(Hertz(50))
        .resolution(Resolution::Bits14);
    let timer =
        LedcTimerDriver::new(peripherals.ledc.timer0, &timer_config).expect("LEDC timer0 配置失败");
    let mut pan = LedcDriver::new(peripherals.ledc.channel0, &timer, peripherals.pins.gpio32)
        .expect("LEDC channel0(PAN, GPIO32) 安装失败");
    let mut tilt = LedcDriver::new(peripherals.ledc.channel1, &timer, peripherals.pins.gpio33)
        .expect("LEDC channel1(TILT, GPIO33) 安装失败");
    // 上电先置中位 90°（0.5–2.5ms 量程中点），避免初始化期停在 0 duty
    pan.set_duty(deg_to_duty(90)).expect("pan 中位设定失败");
    tilt.set_duty(deg_to_duty(90)).expect("tilt 中位设定失败");
    println!(
        "[periph] LEDC ✓ pan=GPIO32 tilt=GPIO33 中位90° duty pan={} tilt={}（max={})",
        pan.get_duty(),
        tilt.get_duty(),
        pan.get_max_duty()
    );

    // ---- UART2 @ 115200（指令面；整装 UartDriver::new 不 split，单线程读写）----
    let uart_config = UartConfig::new().baudrate(Hertz(115_200));
    let uart = UartDriver::new(
        peripherals.uart2,
        peripherals.pins.gpio17,
        peripherals.pins.gpio16,
        None::<AnyInputPin>,
        None::<AnyOutputPin>,
        &uart_config,
    )
    .expect("UART2 驱动安装失败");

    // 外设 'static 化 + move 进指令线程（16KB 运行下限，哨兵纪律）
    let ph = Periph {
        i2c,
        pan,
        tilt,
        uart,
    };
    ram_point("2-periph-ready");
    thread::Builder::new()
        .name("periph-cmd".into())
        .stack_size(THREAD_STACK)
        .spawn(move || cmd_loop(ph))
        .expect("起指令线程失败");

    // main 循环：周期状态行（观测面）
    loop {
        thread::sleep(Duration::from_secs(30));
        println!(
            "[periph] status free={}B min_free={}B",
            unsafe { esp_get_free_heap_size() },
            unsafe { esp_get_minimum_free_heap_size() }
        );
    }
}
