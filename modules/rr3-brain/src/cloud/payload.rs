//! 云应答/请求体的共享纯函数 re-export（W2-1）+ mock 文本。
//!
//! 请求体字节与应答解析的真值迁至 `rr-cloud-proto`（Pi/MCU 共用）；本层只保留
//! mock 文本的环境读取（测试注入点，属策略/IO 面）。

use super::cfg::{RR3_MOCK_TEXT, RR3_MOCK_TEXT_ENV};

pub(super) use rr_cloud_proto::{rr3_asr_text, rr3_chat_body, rr3_ping_body, rr3_reply_text};

/// mock 档识别文本（`RR3_BRAIN_MOCK_ASR_TEXT` 注入覆盖，空白回落固定文本）。
pub(super) fn rr3_mock_text() -> String {
    std::env::var(RR3_MOCK_TEXT_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| RR3_MOCK_TEXT.to_string())
}
