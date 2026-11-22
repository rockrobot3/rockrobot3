//! 有界重试与错误分类（429/5xx/传输层可重试，其余不重试）。

use std::time::Duration;

/// 云端调用有界重试：总尝试次数（首调 + 1 次重试，2026-10-31；TTS 同轨同批——
/// 实机唤醒 Ack 命中后 CosyVoice 读响应超时一次即弃轮，用户侧表现为"喊了没
/// 应答"，与 ASR 是同一类瞬时态）。只对**瞬时**失败重试——HTTP 429/5xx 与
/// 传输层（连接拒绝/超时/读断）；鉴权/请求形状 4xx、坏应答、缺密钥一律不重试
/// （重试只会重复同一结局，还把 401 拖成 2 倍时延）。同步调用发生在对话轮内，
/// 上限 = 首调 `timeout_s` + 重试 [`rr3_retry_budget`]——有界，不无限重试。
const RR3_CLOUD_ATTEMPTS: u32 = 2;
/// 两次尝试之间的固定短歇（对话轮内同步调用，不能长睡）。
const RR3_CLOUD_RETRY_PAUSE: Duration = Duration::from_millis(500);
/// 重试预算下限（秒）：再短连 TLS 握手都不够，重试没有意义。
const RR3_RETRY_MIN_S: f64 = 2.0;

/// 单次尝试的超时预算（策略）：首调用配置全额，重试减半、下限
/// [`RR3_RETRY_MIN_S`]。依据是"重试时连接池已热、DNS 已解析"——不需要再为
/// 冷启动留全额；实测一次 ASR 超时把整轮拖到 16.8 s（8+0.5+8），重试减半后
/// 同场景 12.5 s，降级播报明显更早。
pub(super) fn rr3_retry_budget(timeout_s: f64, attempt: u32) -> f64 {
    if attempt <= 1 {
        timeout_s
    } else {
        (timeout_s / 2.0).max(RR3_RETRY_MIN_S)
    }
}

/// 可重试判据的真值迁至 `rr-cloud-proto`（W2-1，Pi/MCU 共用）；本层只
/// re-export，供 [`rr3_http_try`] 与云端用例消费。
pub(crate) use rr_cloud_proto::rr3_cloud_retryable;

/// 200 应答体直通，其余按接口前缀点名（业务失败；重试判据见 [`rr3_cloud_retryable`]）。
pub(super) fn rr3_ok_resp(interface: &str, status: u16, resp: Vec<u8>) -> Result<Vec<u8>, String> {
    if status == 200 {
        Ok(resp)
    } else {
        Err(format!("{interface} HTTP {status}"))
    }
}

/// 有界重试机制（策略由调用方注入：接口前缀词＋台账标签＋单次尝试闭包）。
/// 闭包收**尝试序号**（1 起）：调用方据此选单次预算（[`rr3_retry_budget`]），
/// 机制不替调用方决定超时。三家客户端共用同一循环，重试判据只有
/// [`rr3_cloud_retryable`] 一处——重复率与圈复杂度的唯一落点。闭包每次调用
/// 重发同一请求体（体在闭包外装配一次）。
pub(super) fn rr3_http_try<T>(
    interface: &str,
    label: &str,
    mut attempt_fn: impl FnMut(u32) -> Result<T, String>,
) -> Result<T, String> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match attempt_fn(attempt) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_budget_halves_after_first_try() {
        assert_eq!(rr3_retry_budget(8.0, 1), 8.0, "首调用配置全额");
        assert_eq!(rr3_retry_budget(8.0, 2), 4.0, "重试减半");
        assert_eq!(rr3_retry_budget(8.0, 3), 4.0, "之后保持减半");
        assert_eq!(rr3_retry_budget(1.0, 2), RR3_RETRY_MIN_S, "下限兜底");
    }
}
