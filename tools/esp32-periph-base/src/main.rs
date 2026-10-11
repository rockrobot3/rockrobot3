//! esp32-periph-base —— 0.0.5 增补轮五 批B 外设底座档（espidf std，不依赖桥）。
//!
//! 职责（全案 §4 批B）：**SSD1306 I2C 驱动出字 + LEDC PWM 云台两轴逐度转向 +
//! 行指令解析面**。批B 是独立固件档（指令面走 UART2 行协议，不进城）；批C 在
//! 本 crate 上加哨兵面（rr-core::runtime + rr-ipc）换能力帧，批D 加 I2S——
//! 同一固件 crate 分批合入（全案 §5 板上固件形态）。
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
    esp_get_free_heap_size, esp_get_minimum_free_heap_size, link_patches, TickType_t,
};

/// I2C 事务超时（TickType_t；portMAX_DELAY 同义 u32::MAX——阻塞至传输完成）。
const I2C_TIMEOUT: TickType_t = u32::MAX;
/// SSD1306 I2C 从机地址（0.96" 128x64 模块缺省）。
const OLED_ADDR: u8 = 0x3C;
/// OLED 每行字符数：5px 字模 + 1px 间隔 = 6px/字符，128px/6 = 21.3 → 21 列。
const OLED_COLS: usize = 21;
/// OLED 文本行数（页 0 与页 2 各一行，页 1/3 留空隙；其余页清屏）。
const OLED_ROWS: usize = 2;
/// 指令行缓冲上限（超出整行丢弃回 ERR，fail-closed 不截半执行）。
const LINE_CAP: usize = 128;
/// 指令/心跳类线程的栈面（板上实测实锤 16KB 运行下限，哨兵同款，勿再调低）。
const THREAD_STACK: usize = 16384;

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

    /// OLED 文本行 + 角度行两行刷新。
    fn oled_show(&mut self, text: &str, pan_deg: u32, tilt_deg: u32) -> Result<(), String> {
        oled_text(&mut self.i2c, 0, text.as_bytes())?;
        let angle_line = format!("P:{pan_deg:03} T:{tilt_deg:03}");
        oled_text(&mut self.i2c, 1, angle_line.as_bytes())
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
fn read_line(uart: &mut UartDriver) -> Result<String, &'static str> {
    let mut buf = Vec::new();
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
// 指令循环（批B 行协议临时面；批C 换能力帧时本段整体退役）
// ---------------------------------------------------------------------------

/// 指令线程主循环：收行 → 解析 → 执行 → 应答。未知指令 ERR（fail-closed，
/// 不猜不拖——全案批B 指令解析面判据）。
fn cmd_loop(mut ph: Periph) {
    let mut last_text = String::from("(boot)");
    let mut pan_deg = 90u32;
    let mut tilt_deg = 90u32;
    reply(
        &mut ph.uart,
        "READY periph-base v0.1.0 (type HELP for commands)",
    );
    loop {
        let line = match read_line(&mut ph.uart) {
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
                "CMDS: OLED <text> | PAN <deg> | TILT <deg> | SERVO <pan> <tilt> | SCAN | INFO | HELP"
                    .into(),
            ),
            "OLED" => {
                // 参数取原始行去指令头，保留内嵌空格（≤42 字符 = 2 行 × 21 列）
                let rest = line[head.len()..].trim_start();
                if rest.is_empty() {
                    Err("OLED 缺文本参数".into())
                } else if rest.len() > OLED_COLS * OLED_ROWS {
                    Err(format!("OLED 文本超长（≤{} 字符）", OLED_COLS * OLED_ROWS))
                } else {
                    let owned = rest.to_string();
                    match ph.oled_show(&owned, pan_deg, tilt_deg) {
                        Ok(()) => {
                            last_text = owned;
                            Ok(format!("OK oled \"{last_text}\""))
                        }
                        Err(e) => Err(e),
                    }
                }
            }
            "PAN" => parse_deg(parts.next()).and_then(|deg| match ph.servo_set(Axis::Pan, deg) {
                Ok(d) => {
                    pan_deg = d;
                    if let Err(e) = ph.oled_show(&last_text, pan_deg, tilt_deg) {
                        eprintln!("[periph] 角度行刷新失败: {e}");
                    }
                    Ok(format!("OK pan={d}"))
                }
                Err(e) => Err(e),
            }),
            "TILT" => parse_deg(parts.next()).and_then(|deg| match ph.servo_set(Axis::Tilt, deg) {
                Ok(d) => {
                    tilt_deg = d;
                    if let Err(e) = ph.oled_show(&last_text, pan_deg, tilt_deg) {
                        eprintln!("[periph] 角度行刷新失败: {e}");
                    }
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
                        pan_deg = p;
                        tilt_deg = t;
                        if let Err(e) = ph.oled_show(&last_text, pan_deg, tilt_deg) {
                            eprintln!("[periph] 角度行刷新失败: {e}");
                        }
                        Ok(format!("OK pan={p} tilt={t}"))
                    }
                    (Err(e), _) | (_, Err(e)) => Err(e),
                },
                _ => Err("SERVO 需两个 0-180 角度参数".into()),
            },
            "SCAN" => {
                // 判据②：两轴各 0/45/90/135/180 五点，逐点串口回读设定角
                let mut ok = true;
                for (i, deg) in [0u32, 45, 90, 135, 180].into_iter().enumerate() {
                    let p = ph.servo_set(Axis::Pan, deg);
                    let t = ph.servo_set(Axis::Tilt, deg);
                    match (p, t) {
                        (Ok(p), Ok(t)) => {
                            pan_deg = p;
                            tilt_deg = t;
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
                            let _ = ph.oled_show(&last_text, pan_deg, tilt_deg);
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
                "INFO pan={pan_deg} tilt={tilt_deg} oled_last=\"{last_text}\" free={}B min_free={}B",
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
        "[periph] surface=esp-rs-std crate=esp32-periph-base oled=i2c0@21/22 servo=ledc@32/33 cmd=uart2@115200"
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
        let r0 = oled_text(&mut i2c, 0, b"ROCK3 PERIPH BASE");
        let r1 = oled_text(&mut i2c, 1, b"P:090 T:090");
        match (r0, r1) {
            (Ok(()), Ok(())) => println!("[periph] OLED ✓ boot 画面已出"),
            (r0, r1) => println!("[periph] boot 画面写失败: 行0={r0:?} 行1={r1:?}"),
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
