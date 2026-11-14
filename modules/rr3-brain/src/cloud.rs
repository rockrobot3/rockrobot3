//! rr3-brain 云端共享原语：云档/密钥、HTTP 传输与有界重试、三家客户端与应答体。

mod cfg;
mod client;
mod payload;
mod retry;

pub(crate) use cfg::{rr3_key_set_of, RR3_KEY_REF_ENV_PREFIX};
pub(crate) use client::{Rr3AsrClient, Rr3LlmClient, Rr3TtsClient};

/// 测试互斥柄（仅测试编译）：RR3_BRAIN_CLOUD 等键是进程级全局，动它的测试须全 crate 串行。
#[cfg(test)]
pub(crate) static RR3_TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) use cfg::{
    rr3_cloud_mode, rr3_secrets_get, Rr3CloudMode, RR3_API_KEY_ENV, RR3_CLOUD_MODE_ENV,
    RR3_MOCK_TEXT_ENV, RR3_SECRETS_FILE_ENV, RR3_TTS_BACKEND_ENV, RR3_TTS_BACKEND_MAC_SAY,
};
#[cfg(test)]
pub(crate) use client::{rr3_boundary_hex, rr3_http_post, rr3_read_capped_body};
#[cfg(test)]
pub(crate) use retry::rr3_cloud_retryable;

#[cfg(test)]
pub(crate) mod testkit;

#[cfg(test)]
mod tests;
