//! 源/汇共享件：入参门禁、锁与有界等待助手、跨线程共享态。

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::{Rr3AudioCfg, Rr3ErrText, Rr3HalError};

/// open 参数门禁：与 ALSA/sim 后端同一套要求（16bit 采样且 rate/channels 非零）。
pub(super) fn rr3_mac_cfg_check(cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
    if [
        cfg.sample_rate_hz != 0,
        cfg.channels != 0,
        cfg.bit_depth == 16,
    ]
    .iter()
    .all(|ok| *ok)
    {
        return Ok(());
    }
    Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
        &format!("mac 后端要求 16bit 采样且 rate/channels 非零，收到 {cfg:?}"),
    )))
}

/// 锁的毒化恢复（linux.rs 同款：锁内无 panic 传播，毒锁照用）。
pub(super) fn rr3_mac_lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// 跨线程交接的有界等（读/写共用同一个超时契约）。无超时 `cv.wait` 是真实挂死面：
/// 输入/输出设备掉线或回调停摆时模块线程永久挂起且零日志；又因 read/write 持
/// `inner` 锁等待，关停路径的 Drop 会与之互等。上界取 2s（16k mono 一块 128ms，
/// 连续 2s 凑不满 = 设备侧确实不供数，不是抖动）。
pub(super) const RR3_MAC_WAIT_MS: u64 = 2_000;

/// 条件等：条件成立或到点即还守卫，返回 `(守卫, 是否就绪)`。超时由调用方带因
/// 点名上抛——观测面降级/设备掉线必须可见，不静默续等。
pub(super) fn rr3_mac_wait<'a, T, F>(
    cv: &Condvar,
    mut guard: std::sync::MutexGuard<'a, T>,
    ready: F,
    timeout: Duration,
) -> (std::sync::MutexGuard<'a, T>, bool)
where
    F: Fn(&T) -> bool,
{
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if ready(&guard) {
            return (guard, true);
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return (guard, false);
        }
        let (g, _) = cv
            .wait_timeout(guard, deadline - now)
            .unwrap_or_else(|p| p.into_inner());
        guard = g;
    }
}

/// 拾音共享态：回调（AudioQueue 内部线程）与 `read`（模块线程）只经此交接。
/// `cap` 是 deque 样点上限（open 时按采样规格算好存住）——超限丢最旧的背压策略
/// 在回调侧执行，须随 Shared 携带。
pub(super) struct SourceShared {
    pub(super) state: Mutex<VecDeque<i16>>,
    pub(super) cv: Condvar,
    pub(super) cap: usize,
}

/// 回放共享态：`write`（模块线程）入队，输出回调（AudioQueue 内部线程）排水。
pub(super) struct SinkShared {
    pub(super) state: Mutex<SinkState>,
    pub(super) cv: Condvar,
}

pub(super) struct SinkState {
    pub(super) deque: VecDeque<i16>,
    /// 高水位（样点数），open 时按采样规格算好存住
    pub(super) watermark: usize,
}
