use esp_idf_hal::uart::UartDriver;
use esp_idf_sys::esp_timer_get_time;
use rr_ipc::Rr3ByteStream;

use crate::board::LINE_CAP;

/// 应答行回写（UART2；\r\n 结尾，终端与 pyserial 同读友好）。
pub(crate) fn reply(uart: &mut UartDriver, line: &str) {
    let _ = uart.write(format!("{line}\r\n").as_bytes());
    // 整装 UartDriver 未实现 std::io::Write（vendored 面只有 embedded_io::Write），
    // 用固有 wait_tx_done(portMAX_DELAY) 等移位完成——语义即"应答发完再返回"。
    let _ = uart.wait_tx_done(u32::MAX);
}

/// 收一行（阻塞逐字节读直到 '\n'；'\r' 剥离；超 [LINE_CAP] 丢弃整行回 Err）。
/// `first` 是 cmd_loop 双面分流时已消耗的首字节（非 0x31 的行面回流字节）。
pub(crate) fn read_line(uart: &mut UartDriver, first: u8) -> Result<String, &'static str> {
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