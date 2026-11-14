//! UART 行面/帧面收发与主循环。

use std::time::Duration;

use rr_ipc::{Rr3ByteStream, Rr3UartConn};

use crate::app::Board;
use crate::board::{
    FIRST_BYTE_TIMEOUT_MS, FRAME_DRAIN_MAX, FRAME_GAP_BUDGET, LINE_BYTE_TIMEOUT_MS, LINE_MAX,
};

/// 帧面控制面适配（`pre` = 双面分流时已消耗的首字节；帧首读时为 Some(0x31)）。
pub(crate) struct UartIo<'a> {
    pub(crate) uart: &'a mut Rr3UartConn,
    pub(crate) pre: Option<u8>,
}

impl Rr3ByteStream for UartIo<'_> {
    fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
        if let Some(b) = self.pre.take() {
            if buf.is_empty() {
                return 0;
            }
            buf[0] = b;
            return 1;
        }
        // 帧内读与行面同一纪律：**字节间超时 + 无进展预算**。旧版直接走阻塞
        // `Read`（VMIN=1 无 VTIME），半帧断线时控制面永久停在 128B 头里——宿主
        // 进程被杀/误码丢字节后再不会有字节到达，只能整机重启。预算用尽折叠为
        // 0（缝上无错误通道，Closed 形状）→ `TornHeader/TornPayload` → 调用方
        // 残尾吞净后复位失步，与 ESP 板侧同名实现同一语义。
        let mut idle = 0u32;
        loop {
            match self.uart.rr3_read_timeout(buf, LINE_BYTE_TIMEOUT_MS) {
                Ok(0) => {
                    idle += 1;
                    if idle >= FRAME_GAP_BUDGET {
                        eprintln!("[periph] 帧内读超时（{FRAME_GAP_BUDGET} 拍无进展，判半帧断线）");
                        return 0;
                    }
                }
                Ok(n) => return n,
                Err(e) => {
                    eprintln!("[periph] 帧面 UART 读失败: {e}");
                    return 0;
                }
            }
        }
    }

    fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
        self.uart.rr3_write_all_bytes(buf)
    }
}

impl Board {
    pub(crate) fn run(&mut self) -> ! {
        loop {
            self.maybe_sample_rail();
            let mut first = [0u8; 1];
            match self
                .uart
                .rr3_read_timeout(&mut first, FIRST_BYTE_TIMEOUT_MS)
            {
                Ok(0) => continue,
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[periph] UART 读失败，退避后重试: {e}");
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }
            if first[0] == rr_periph_proto::RR3_MAGIC_FIRST {
                self.frame_round(first[0]);
            } else {
                match self.read_line(first[0]) {
                    Ok(line) => self.line_round(&line),
                    Err(e) => {
                        eprintln!("[periph] 收行失败: {e}");
                        self.reply(&format!("ERR {e}"));
                    }
                }
            }
        }
    }

    /// 帧面一轮：收帧 → 协议门 → 分发 → 应答；坏帧丢弃 + 残尾吞净。
    fn frame_round(&mut self, first: u8) {
        let recv = {
            let mut io = UartIo {
                uart: &mut self.uart,
                pre: Some(first),
            };
            periph_mech::frame::frame_recv(&mut io)
        };
        let req = match recv {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[periph] 帧面丢弃: {e}");
                self.drain_uart();
                return;
            }
        };
        println!("[periph] 帧面 id={} topic=\"{}\"", req.id, req.topic);
        let result = self.dispatch(&req);
        if let Err(e) = &result {
            eprintln!("[periph] 帧面业务失败 id={}: {e}", req.id);
        }
        let mut io = UartIo {
            uart: &mut self.uart,
            pre: None,
        };
        let clock = &self.clock;
        if let Err(e) = periph_mech::frame::frame_send_response(&mut io, &req, result, clock) {
            eprintln!("[periph] 应答发送失败: {e}");
        }
    }

    fn drain_uart(&mut self) {
        for _ in 0..FRAME_DRAIN_MAX {
            let mut b = [0u8; 1];
            match self.uart.rr3_read_timeout(&mut b, 2) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }

    /// 收一行（半行超时丢弃；'\\r' 剥离；超长吞尾）。
    fn read_line(&mut self, first: u8) -> Result<String, String> {
        let mut buf: Vec<u8> = Vec::with_capacity(LINE_MAX);
        if first == b'\n' {
            return Ok(String::new());
        }
        buf.push(first);
        let mut idle = 0u32;
        loop {
            let mut byte = [0u8; 1];
            match self.uart.rr3_read_timeout(&mut byte, LINE_BYTE_TIMEOUT_MS) {
                Ok(0) => {
                    idle += 1;
                    if idle >= 2 {
                        return Err("line timeout（半行无进展丢弃）".into());
                    }
                    continue;
                }
                Ok(_) => {
                    idle = 0;
                    match byte[0] {
                        b'\n' => {
                            while matches!(buf.last(), Some(b'\r')) {
                                buf.pop();
                            }
                            return Ok(String::from_utf8_lossy(&buf).into_owned());
                        }
                        b => {
                            if buf.len() >= LINE_MAX {
                                self.drain_line_tail();
                                return Err("line too long".into());
                            }
                            buf.push(b);
                        }
                    }
                }
                Err(e) => return Err(format!("uart read 失败: {e}")),
            }
        }
    }

    fn drain_line_tail(&mut self) {
        for _ in 0..(LINE_MAX * 4) {
            let mut b = [0u8; 1];
            match self.uart.rr3_read_timeout(&mut b, LINE_BYTE_TIMEOUT_MS) {
                Ok(0) => return,
                Ok(_) => {
                    if b[0] == b'\n' {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }

    pub(crate) fn reply(&mut self, line: &str) {
        let bytes = format!("{line}\r\n");
        let n = self.uart.rr3_write_all_bytes(bytes.as_bytes());
        if n < bytes.len() {
            eprintln!("[periph] 应答丢字节 {n}/{}: {line}", bytes.len());
        }
    }
}
