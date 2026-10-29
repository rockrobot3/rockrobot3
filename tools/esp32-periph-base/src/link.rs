use esp_idf_hal::uart::UartDriver;
use esp_idf_sys::esp_timer_get_time;
use rr_ipc::Rr3ByteStream;

use crate::board::{LINE_BYTE_TICKS, LINE_CAP, LINE_GAP_BUDGET};

/// 应答行回写（UART2；\r\n 结尾，终端与 pyserial 同读友好）。
/// 返回是否**整行发完**：旧版全用 `let _ =` 吞掉写结果，TX 环满时 `write` 回
/// `Ok(0)`（零进展）而 `wait_tx_done` 立即"成功"，应答就这样无声消失——宿主侧
/// 只表现为"命令发了没回音"，日志零线索。此处零进展 = 先等移位腾空再一次重试，
/// 仍不全则 eprintln + 回 false（调用方计 `reply_drop` 观测）。
pub(crate) fn reply(uart: &mut UartDriver, line: &str) -> bool {
    let bytes = format!("{line}\r\n");
    let bytes = bytes.as_bytes();
    let mut done = 0usize;
    let mut stuck = 0u8;
    while done < bytes.len() {
        match uart.write(&bytes[done..]) {
            Ok(0) => {
                stuck += 1;
                if stuck > 2 {
                    break; // 连续零进展：放弃（下面点名，不谎报成功）
                }
                let _ = uart.wait_tx_done(u32::MAX); // 等移位腾空后重试
            }
            Ok(n) => done += n,
            Err(e) => {
                eprintln!("[periph] 应答写失败: {e:?}");
                break;
            }
        }
    }
    // 整装 UartDriver 未实现 std::io::Write（vendored 面只有 embedded_io::Write），
    // 用固有 wait_tx_done(portMAX_DELAY) 等移位完成——语义即"应答发完再返回"。
    let _ = uart.wait_tx_done(u32::MAX);
    if done < bytes.len() {
        eprintln!("[periph] 应答丢字节 {done}/{}: {line}", bytes.len());
        return false;
    }
    true
}

/// 收一行（阻塞逐字节读直到 '\n'；'\r' 剥离；超 [LINE_CAP] 丢弃整行回 Err）。
/// `first` 是 cmd_loop 双面分流时已消耗的首字节（非 0x31 的行面回流字节）。
///
/// **字节间超时 + 半行预算**（稳健性修复）：逐字节读带 [LINE_BYTE_TICKS] 超时，
/// 连续 [LINE_GAP_BUDGET] 拍无进展即判半行断线丢弃（回 Err）。旧版用
/// portMAX_DELAY 阻塞，主机发半行后断线/复位就让控制面永久卡死（outbox/audio/
/// INFO 全停，只能硬件复位）。
pub(crate) fn read_line(uart: &mut UartDriver, first: u8) -> Result<String, &'static str> {
    let mut buf = Vec::with_capacity(LINE_CAP);
    if first == b'\n' {
        // 空行保持批B 行为：静默空串（cmd_loop trim 后跳过）
        return Ok(String::new());
    }
    buf.push(first);
    let mut byte = [0u8; 1];
    let mut idle = 0u32;
    loop {
        let n = uart
            .read(&mut byte, LINE_BYTE_TICKS)
            .map_err(|_| "uart read 失败")?;
        if n == 0 {
            idle += 1;
            if idle >= LINE_GAP_BUDGET {
                return Err("line timeout（半行无进展丢弃）");
            }
            continue;
        }
        idle = 0;
        match byte[0] {
            b'\n' => {
                while matches!(buf.last(), Some(b'\r')) {
                    buf.pop();
                }
                return Ok(String::from_utf8_lossy(&buf).into_owned());
            }
            b => {
                if buf.len() >= LINE_CAP {
                    // 超长行：**必须把本行余下字节吞掉**再返回——旧版直接 return，
                    // 行尾残留在 UART FIFO 里被下一轮当成新命令解析（垃圾指令执行
                    // + 一串没人请求的 ERR 行）。
                    drain_line_tail(uart);
                    return Err("line too long");
                }
                buf.push(b);
            }
        }
    }
}

/// 吞掉当前行剩余字节（超长行处置）：读到 '\n' 为止；字节带超时 + 行进总预算，
/// 故线上噪声也不会让这里挂住（最多 [LINE_CAP]*4 字节或半行预算用尽即返回）。
fn drain_line_tail(uart: &mut UartDriver) {
    let mut byte = [0u8; 1];
    let mut idle = 0u32;
    for _ in 0..(LINE_CAP * 4) {
        match uart.read(&mut byte, LINE_BYTE_TICKS) {
            Ok(0) => {
                idle += 1;
                if idle >= LINE_GAP_BUDGET {
                    return;
                }
            }
            Ok(_) => {
                idle = 0;
                if byte[0] == b'\n' {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

/// 帧面失败后的**残尾吞净**（[`crate::board::FRAME_DRAIN_MAX`] 上限，读到线静默一拍为止）。
///
/// 旧版 `frame_round` 在 `frame_recv` 失败时直接 return：失败前已消费的字节不可知
/// （`TornHeader{got}` / `Header(..)` 都不给出"还剩多少"），剩余字节就留在 UART
/// FIFO 里，下一轮控制面把它们当**行命令**解析——垃圾指令被执行，或刷出一串
/// 没人请求的 `ERR`，控制面被残尾污染。
///
/// 唯一可靠的边界是"线静默认一拍"：帧内字节间隔是一字节时间（921600 下 10µs），
/// 100ms 无字节即证明发送方已停手。代价是失败时最多停顿一拍；换来的是失败后
/// 流面回到干净边界（连残尾之后紧跟的那帧一并丢弃——失败后字节流本就不可信，
/// fail-closed 比猜帧边界安全）。
pub(crate) fn drain_uart_quiet(uart: &mut UartDriver, max_bytes: usize) {
    let mut scratch = [0u8; 64];
    let mut done = 0usize;
    while done < max_bytes {
        let want = (max_bytes - done).min(scratch.len());
        match uart.read(&mut scratch[..want], LINE_BYTE_TICKS) {
            Ok(0) => return, // 静默一拍：残尾吞净
            Ok(n) => done += n,
            Err(_) => return,
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
pub(crate) struct UartIo<'a> {
    pub(crate) uart: &'a mut UartDriver<'static>,
    /// 窥视消耗的帧 magic 首字节（0x31）：Read 首次先吐它，128B 头的 magic
    /// 窗口才完整；发 Response 时此位恒 None（无窥视）。
    pub(crate) pre: Option<u8>,
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
pub(crate) fn rr3_frame_now_ns() -> u64 {
    (unsafe { esp_timer_get_time() }).unsigned_abs() * 1000
}
