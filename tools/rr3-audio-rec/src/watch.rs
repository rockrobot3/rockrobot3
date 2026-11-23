// SPDX-License-Identifier: Apache-2.0
//! 看门狗：跨线程共享账 + 武装后停滞判据（补恢复头退 3）。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::exit::exit_with_recovery;
use crate::state::WatchState;
use crate::{NOMINAL_RATE_FALLBACK_HZ, STALL};

/// 长流楔死看门狗（收流端策略面，d4 600s 实锤的修复）。
///
/// 机理：宿主/CP2102 驱动层丢字节后帧层等幻影帧尾永不返回（第一轮 600s 稳态
/// 跑：板面 seq=9182 sent=9182 drop=0 全程健康，宿主 `rr3_recv_buf` 楔死 8
/// 分钟无退出）。本线程每 500ms 查推进：武装后 [`STALL`] 无新分片字节 → 补
/// wav 恢复头（data_len=盘侧写透字节）→ exit(3)。fail-honest：已收数据保得
/// 住，停滞事实打得响。
pub(crate) fn watchdog_run(w: Arc<WatchState>, out: String) {
    let mut last_seen = 0u64;
    let mut last_move = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if rr3_watch_idle(&w, &mut last_seen, &mut last_move) {
            continue;
        }
        rr3_watch_fail(&w, last_seen, &out);
    }
}

/// 停滞收口：点名入队/盘侧字节后补恢复头退出 3。
fn rr3_watch_fail(w: &WatchState, last_seen: u64, out: &str) -> ! {
    let disk = w.disk_bytes.load(Ordering::Acquire);
    eprintln!(
        "[rec] ✗ 收流停滞 ≥{STALL:?} 无新分片字节（楔死防护触发）：入队 {last_seen}B/盘侧 {disk}B，补恢复头后退出"
    );
    exit_with_recovery(std::path::Path::new(out), w, 3);
}

/// 一拍看门狗记账：推进/未武装/未到死线都返回 `true`（继续循环）。
fn rr3_watch_idle(w: &WatchState, last_seen: &mut u64, last_move: &mut Instant) -> bool {
    let nb = w.ingested.load(Ordering::Acquire);
    if nb != *last_seen {
        *last_seen = nb;
        *last_move = Instant::now();
        return true;
    }
    if !w.armed.load(Ordering::Acquire) {
        *last_move = Instant::now(); // 未武装期不计时
        return true;
    }
    last_move.elapsed() < STALL
}

/// 跨线程共享账的初值（三线程按需克隆）。
pub(crate) fn rr3_watch_state() -> Arc<WatchState> {
    Arc::new(WatchState {
        ingested: AtomicU64::new(0),
        disk_bytes: AtomicU64::new(0),
        reconnects: AtomicU64::new(0),
        rate: AtomicU32::new(NOMINAL_RATE_FALLBACK_HZ),
        armed: AtomicBool::new(false),
    })
}
