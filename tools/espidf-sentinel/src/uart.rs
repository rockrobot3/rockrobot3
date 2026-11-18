//! UART2 半分体 → 窄缝适配：TX/RX 两个 Read/Write 半体。
//!
//! 半体来自全双工 `UartDriver` 的 split；`unsafe impl Send` 的安全性论证
//! 逐条留在类型注记（上游只给全双工驱动标了 Send+Sync，半体是同源状态拆面）。
//! 读/写面刻意单向：TX 无读、RX 无写（conn 只用 send_buf/recv_buf 单向面）。
//!
//! 拆分动因：`main.rs` 579 行，UART 适配面与启动/循环面分属两簇。

use std::io::{self, Read, Write};

use esp_idf_hal::uart::{UartRxDriver, UartTxDriver};

// ---------------------------------------------------------------------------
// UART2 半分体 → 窄缝（板侧接总线的全部：两个 Read/Write 半体 + rr-ipc adapter）
// ---------------------------------------------------------------------------

/// TX 半体内体：多发送方共享（注册 / 心跳线程 / round-trip 应答），互斥在
/// 外挂 `Mutex` 锁内。写面真、读面故意不存在（Err = Unsupported）：conn 只用
/// rr3_send_buf。
pub(crate) struct UartTxInner(UartTxDriver<'static>);

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
pub(crate) struct UartRxHalf(UartRxDriver<'static>);

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
