//! 错型与定长文本：`Rr3HalError` / `Rr3ErrText` 及 char 边界截断助手。

/// 退到 char 边界：返回不劈多字节字符的最大前缀字节数（定长文本共用）。
pub(crate) fn rr3_text_trim(s: &str, mut len: usize) -> usize {
    while len > 0 && s.get(..len).is_none() {
        len -= 1;
    }
    len
}

/// 按容量抄字节并退到 char 边界（定长文本构造函数共用）。
pub(crate) fn rr3_copy_trim(s: &str, bytes: &mut [u8]) -> usize {
    let mut len = 0usize;
    for (i, b) in s.as_bytes().iter().enumerate() {
        if i >= bytes.len() {
            break;
        }
        bytes[i] = *b;
        len = i + 1;
    }
    rr3_text_trim(s, len)
}

/// 定长错误文案（[`Rr3HalError::InvalidParam`] 载荷）。no_std 面没有 `String`，
/// 但实现层（Linux 后端）的错误文案是动态的（io error、路径、数值插值）——
/// 实现层 `format!` 后经 [`Rr3ErrText::rr3_err_text_new`] 截断入缓冲，按 char
/// 边界截（保证 `as_str` 恒合法），文案语义不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ErrText {
    bytes: [u8; 128],
    len: u8,
}

impl Rr3ErrText {
    /// 缓冲容量（字节数）
    pub const fn rr3_err_text_cap() -> usize {
        128
    }

    /// 从字符串构造，按 char 边界截断到容量（截断不报错：错误文案是给人看的，
    /// 截断的尾巴不带语义——判据面靠变体与静态前缀，不靠尾巴）
    pub fn rr3_err_text_new(s: &str) -> Self {
        let mut bytes = [0u8; 128];
        let len = rr3_copy_trim(s, &mut bytes);
        // 退到 char 边界（UTF-8 多字节序列不劈半）
        Self {
            bytes,
            len: len as u8,
        }
    }

    /// 文本视图（构造时已按 char 边界截断，恒合法）
    pub fn rr3_err_text_as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

/// HAL 错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3HalError {
    /// 后端未实现该能力
    Unsupported(&'static str),
    /// 设备忙 / 已被其他模块持有
    Busy,
    /// 参数越界（定长文案，见 [`Rr3ErrText`]）
    InvalidParam(Rr3ErrText),
}

impl core::fmt::Display for Rr3HalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported(b) => write!(f, "HAL 后端不支持: {b}"),
            Self::Busy => write!(f, "HAL 设备忙"),
            Self::InvalidParam(s) => write!(f, "HAL 参数非法: {}", s.rr3_err_text_as_str()),
        }
    }
}
