//! SSD1306 传输薄壳（Pi 档）——**编码真值在 `periph-mech::oled_codec`，面目几何在
//! `atoms/face/mech.rs`，本文件只做 I2C 事务、分块与重试**。
//!
//! 分块字节数/重试轮数/块间隔等策略量一律来自 [`crate::board`]；与经典板同口径的
//! 分块（1 控制字节 + 32 数据）保证 OLED 线上字节流逐字节相同（U5 判据）。

use periph_mech::oled_codec::{
    rr3_frame_pages, rr3_init_commands, rr3_oled_lines, rr3_page_cmd, rr3_text_page,
    RR3_OLED_ZERO_PAGE,
};
use crate::board::{
    I2C_CHUNK_GAP_MS, I2C_PAGE_ATTEMPTS, I2C_PAGE_CHUNK, I2C_RETRY, I2C_RETRY_DELAY_MS, OLED_ADDR,
    OLED_COLS, OLED_CONTRAST, OLED_ROWS,
};
use crate::face::{eye_page, EyeStyle};
use crate::ports::SharedI2c;

/// OLED 渲染器：持有共享 I2C 总线；页寻址模式，整页从确定性起点写。
pub struct OledRenderer {
    i2c: SharedI2c,
}

impl OledRenderer {
    pub fn new(i2c: SharedI2c) -> Self {
        Self { i2c }
    }

    /// 带重试的单笔写（事务失败先退避再重发；整页重试在外层）。
    fn write_retry(&self, bytes: &[u8]) -> Result<(), String> {
        let mut last = String::new();
        for attempt in 0..=I2C_RETRY {
            match self.i2c.write(OLED_ADDR, bytes) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last = e;
                    if attempt < I2C_RETRY {
                        std::thread::sleep(std::time::Duration::from_millis(I2C_RETRY_DELAY_MS));
                    }
                }
            }
        }
        Err(format!("（重试 {I2C_RETRY} 次后）{last}"))
    }

    /// 发命令（控制字节 0x00 + 命令序列；SSD1306 流式命令面单笔合法）。
    fn cmd(&self, cmds: &[u8]) -> Result<(), String> {
        let mut buf = Vec::with_capacity(1 + cmds.len());
        buf.push(0x00);
        buf.extend_from_slice(cmds);
        self.write_retry(&buf)
            .map_err(|e| format!("I2C 命令写失败: {e}"))
    }

    /// 单页单趟（页地址 + 分块数据）；失败即返回，由 [`Self::page`] 整页重写。
    fn page_once(&self, page: u8, data: &[u8; 128]) -> Result<(), String> {
        self.cmd(&rr3_page_cmd(page))?;
        let mut buf = Vec::with_capacity(1 + I2C_PAGE_CHUNK);
        let mut off = 0usize;
        while off < data.len() {
            let n = I2C_PAGE_CHUNK.min(data.len() - off);
            buf.clear();
            buf.push(0x40);
            buf.extend_from_slice(&data[off..off + n]);
            self.write_retry(&buf)
                .map_err(|e| format!("I2C 页数据写失败 page={page} off={off} n={n}: {e}"))?;
            off += n;
            if off < data.len() {
                std::thread::sleep(std::time::Duration::from_millis(I2C_CHUNK_GAP_MS));
            }
        }
        Ok(())
    }

    /// 整页重写（失败从确定性起点重写整页，至多 `I2C_PAGE_ATTEMPTS` 轮）。
    fn page(&self, page: u8, data: &[u8; 128]) -> Result<(), String> {
        let mut last = String::from("未知");
        for _ in 0..I2C_PAGE_ATTEMPTS {
            match self.page_once(page, data) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(format!("（整页重写 {I2C_PAGE_ATTEMPTS} 次后）{last}"))
    }

    /// 初始化（页寻址 + 电荷泵 + 清屏）。
    pub fn init(&mut self) -> Result<(), String> {
        self.cmd(&rr3_init_commands(OLED_CONTRAST))?;
        for page in 0u8..8 {
            self.page(page, &RR3_OLED_ZERO_PAGE)?;
        }
        Ok(())
    }

    /// 文本 3 行 + 角度行（row 0..3 ↦ 页 0/2/4/6；行号/列数按参喂编码面）。
    pub fn show_text(&mut self, text: &str, pan_deg: u32, tilt_deg: u32) -> Result<(), String> {
        let lines = rr3_oled_lines(text, pan_deg, tilt_deg, OLED_ROWS, OLED_COLS);
        for (row, line) in lines.iter().enumerate() {
            let buf = rr3_text_page(row, line, OLED_ROWS, OLED_COLS)?;
            self.page((row * 2) as u8, &buf)?;
        }
        Ok(())
    }

    /// 胶囊眼表情（面目几何在 `atoms/face/mech.rs`，本面逐页取图后只做页写）。
    pub fn capsules(&mut self, style: EyeStyle) -> Result<(), String> {
        for page in 0u8..8 {
            let buf = eye_page(page, style);
            self.page(page, &buf)?;
        }
        Ok(())
    }

    /// 8×8 点阵中央放大（编码在 `periph-mech::oled_codec`）。
    pub fn frame(&mut self, frame: &[u8; 8]) -> Result<(), String> {
        let pages = rr3_frame_pages(frame);
        for (page, buf) in pages.iter().enumerate() {
            self.page(page as u8, buf)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::ports::test_support::fake;

    use super::*;

    /// 初始化序必须是 25 字节命令（一笔 0x00 控制 + 25 命令）＋ 8 页全零清屏。
    #[test]
    fn init_sends_command_sequence_and_clears_all_pages() {
        let (bus, f) = fake();
        let mut oled = OledRenderer::new(bus);
        oled.init().expect("假件 init 必须成功");

        let writes = f.borrow().writes.clone();
        assert_eq!(writes[0].1[0], 0x00, "命令控制字节");
        assert_eq!(
            &writes[0].1[1..],
            &rr3_init_commands(OLED_CONTRAST)[..],
            "init 命令序必须与机制核同源"
        );
        // 8 页：每页 1 条页地址命令 + 4 条 32B 数据块 = 40 笔；全零页数据块为 0x40 + 32×0。
        let data_writes: Vec<&(u8, Vec<u8>)> = writes
            .iter()
            .filter(|w| w.1.first() == Some(&0x40))
            .collect();
        assert_eq!(data_writes.len(), 8 * 4, "8 页 × 4 块");
        assert!(
            data_writes.iter().all(|w| w.1[1..].iter().all(|v| *v == 0)),
            "清屏页必须全零"
        );
    }

    /// 文本页：row 0 → 页 0；'A' 字模 + 1px 空列后接零（与编码面同源）。
    #[test]
    fn show_text_writes_expected_page_zero() {
        let (bus, f) = fake();
        let mut oled = OledRenderer::new(bus);
        oled.show_text("A", 90, 90).expect("假件文本渲染必须成功");
        let writes = f.borrow().writes.clone();
        // 第一笔是页地址命令（0x00 控制 + B0/00/10），第二笔是数据块（0x40 + 32B）。
        assert_eq!(writes[0].1, vec![0x00, 0xB0, 0x00, 0x10]);
        assert_eq!(writes[1].1[0], 0x40);
        assert_eq!(
            &writes[1].1[1..6],
            &periph_mech::oled_codec::rr3_glyph(b'A'),
            "'A' 字模必须落在页首"
        );
        assert!(
            writes[1].1[6..].iter().all(|v| *v == 0),
            "字模之后必须为空列"
        );
    }
}
