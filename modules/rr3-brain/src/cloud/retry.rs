//! 有界重试与错误分类（429/5xx/传输层可重试，其余不重试）。

use std::time::Duration;

/// 云端调用有界重试：总尝试次数（首调 + 1 次重试，2026-10-31；TTS 同轨同批——
/// 实机唤醒 Ack 命中后 CosyVoice 读响应超时一次即弃轮，用户侧表现为"喊了没
/// 应答"，与 ASR 是同一类瞬时态）。只对**瞬时**失败重试——HTTP 429/5xx 与
/// 传输层（连接拒绝/超时/读断）；鉴权/请求形状 4xx、坏应答、缺密钥一律不重试
/// （重试只会重复同一结局，还把 401 拖成 2 倍时延）。同步调用发生在对话轮内，
/// 上限 2×`timeout_s`（当前配置 8s）——有界，不无限重试。
const RR3_CLOUD_ATTEMPTS: u32 = 2;
/// 两次尝试之间的固定短歇（对话轮内同步调用，不能长睡）。
const RR3_CLOUD_RETRY_PAUSE: Duration = Duration::from_millis(500);

/// 可重试判据（纯函数，单测钉域）：传输层「云端不可达 …」与 HTTP 429/5xx 重试；
/// 其余（4xx/坏应答/缺密钥/缺字段）不重试。`interface` 是各云端错误文案的接口
/// 前缀词（"识别接口"/"合成接口"/"大模型接口"），用于解析
/// `"{interface} HTTP {code}"`。
pub(crate) fn rr3_cloud_retryable(interface: &str, err: &str) -> bool {
    if err.starts_with("云端不可达") {
        return true;
    }
    rr3_http_code_of(interface, err).is_some_and(rr3_retry_code)
}

/// 从 `"{interface} HTTP {code}"` 解析状态码（形状不符即 None）。
fn rr3_http_code_of(interface: &str, err: &str) -> Option<u16> {
    let prefix = format!("{interface} HTTP ");
    err.strip_prefix(&prefix)
        .and_then(|s| s.parse::<u16>().ok())
}

/// 200 应答体直通，其余按接口前缀点名（业务失败；重试判据见 [`rr3_cloud_retryable`]）。
pub(super) fn rr3_ok_resp(interface: &str, status: u16, resp: Vec<u8>) -> Result<Vec<u8>, String> {
    if status == 200 {
        Ok(resp)
    } else {
        Err(format!("{interface} HTTP {status}"))
    }
}

/// 可重试状态码：限流（429）与服务端（5xx）。
fn rr3_retry_code(code: u16) -> bool {
    code == 429 || (500..=599).contains(&code)
}

/// 有界重试机制（策略由调用方注入：接口前缀词＋台账标签＋单次尝试闭包）。三家
/// 客户端共用同一循环，重试判据只有 [`rr3_cloud_retryable`] 一处——重复率与
/// 圈复杂度的唯一落点。闭包每次调用重发同一请求体（体在闭包外装配一次）。
pub(super) fn rr3_http_try<T>(
    interface: &str,
    label: &str,
    mut attempt_fn: impl FnMut() -> Result<T, String>,
) -> Result<T, String> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match attempt_fn() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if rr3_cloud_retryable(interface, &e) && attempt < RR3_CLOUD_ATTEMPTS {
                    eprintln!(
                        "rr3-brain: {label} 第 {attempt}/{RR3_CLOUD_ATTEMPTS} 次失败（{e}），{}ms 后重试",
                        RR3_CLOUD_RETRY_PAUSE.as_millis()
                    );
                    std::thread::sleep(RR3_CLOUD_RETRY_PAUSE);
                    continue;
                }
                // 重试过的最终失败补尝试数（首调失败文案逐字不变——台账/单测依赖）
                return if attempt > 1 {
                    Err(format!("{e}（{label} 共尝试 {attempt} 次）"))
                } else {
                    Err(e)
                };
            }
        }
    }
}
