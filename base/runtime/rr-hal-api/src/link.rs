//! 链路契约：链路状态摘要、SSID 定长文本与链路读数面。

use crate::err::{rr3_copy_trim, Rr3HalError};

/// SSID 定长文本（[`Rr3LinkState`] 载荷）。IEEE 802.11 SSID 上限 32 octets，
/// 定长 + 长度承载，`⊙` 跨片不再过 `String`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SsidText {
    bytes: [u8; 32],
    len: u8,
}

impl Rr3SsidText {
    /// 缓冲容量（octets，IEEE 802.11 SSID 上限）
    pub const fn rr3_ssid_text_cap() -> usize {
        32
    }

    /// 从字符串构造，按 char 边界截断到 32 octets（SSID 是观测读数不是指令，
    /// 超长截断进摘要；真 SSID 本就不该超限）
    pub fn rr3_ssid_text_new(s: &str) -> Self {
        let mut bytes = [0u8; 32];
        let len = rr3_copy_trim(s, &mut bytes);
        Self {
            bytes,
            len: len as u8,
        }
    }

    /// 文本视图
    pub fn rr3_ssid_text_as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

/// 链路状态摘要
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3LinkState {
    pub up: bool,
    pub ssid: Option<Rr3SsidText>,
    pub rssi_dbm: Option<i8>,
}

/// 网络链路读数
pub trait Rr3Link {
    fn state(&self) -> Result<Rr3LinkState, Rr3HalError>;
}
