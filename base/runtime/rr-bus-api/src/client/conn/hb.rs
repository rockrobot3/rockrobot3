//! 心跳驱动器（自 conn.rs 抽出）：拍点等待、发拍、失败宽限与已发记账。
//!
//! 拍点真值在机制核状态机；本件只做 std 平台事（sleep / 组帧 / 失败收场）。

use super::*;

/// 心跳线程上路（B9 自连接体抽出）：线程体归 [`rr3_hb_loop`]，此处只做 spawn。
pub(super) fn rr3_spawn_hb(
    inner: &Arc<Rr3Inner>,
    tb: Rr3TimeBase,
) -> std::io::Result<JoinHandle<()>> {
    let hb_inner = inner.clone();
    std::thread::Builder::new()
        .name("rr3-bus-heartbeat".into())
        .spawn(move || rr3_hb_loop(hb_inner, tb))
}

/// 心跳主循环（B9 自连接体抽出，语义逐字未动）：每拍先取计数快照——非空才带
/// `counters` 字段（可选 append，旧 rrd 忽略未知字段，双向兼容；空映射不带，
/// 保持与无上报模块同一条线上形状）。被拒/超时 = 连续失败到上界才退出，交给
/// supervisor 裁定（单拍抖动由下一拍自愈，见 [`RR3_HEARTBEAT_MAX_MISSES`]）。
fn rr3_hb_loop(hb_inner: Arc<Rr3Inner>, tb: Rr3TimeBase) {
    let mut misses: u32 = 0;
    loop {
        let Some(wait_ms) = rr3_hb_wait(&hb_inner, &tb) else {
            return; // 断开（Down）即停拍——"不装活着"同一裁定
        };
        std::thread::sleep(Duration::from_millis(wait_ms));
        if hb_inner.stop.load(Ordering::Relaxed) {
            return;
        }
        let now_ms = tb.rr3_now_ms();
        let due = hb_inner
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_heartbeat_due(now_ms);
        if !due {
            continue; // 提前醒（信号打断等），回睡
        }
        if !rr3_hb_send(&hb_inner) {
            // 失败宽限：stop 已置（reader/宿主收场）时按拆分前口径落到"记已发"。
            if !hb_inner.stop.load(Ordering::Relaxed) {
                if rr3_hb_miss(&hb_inner, &mut misses) {
                    return;
                }
                continue;
            }
        }
        // 成功即清零宽限计数（失败且 stop 已置的同拍也走到这里，与拆分前一致）
        misses = 0;
        rr3_hb_mark_sent(&hb_inner, now_ms);
    }
}

/// 等到下一拍；None = 已断开（Down），心跳线程应退出。
fn rr3_hb_wait(hb_inner: &Rr3Inner, tb: &Rr3TimeBase) -> Option<u64> {
    hb_inner
        .rt
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .rr3_beat_wait_ms(tb.rr3_now_ms())
}

/// 发一拍心跳（有 counters 增量则随体上行）；真 = 调用成功。
fn rr3_hb_send(hb_inner: &Rr3Inner) -> bool {
    let delta = hb_inner.rr3_counters_delta();
    let params = if delta.is_empty() {
        json!({})
    } else {
        json!({ "counters": delta })
    };
    hb_inner
        .rr3_call_inner(
            hb_inner.rr3_task(),
            "rr3.heartbeat",
            params,
            Duration::from_secs(5),
        )
        .is_ok()
}

/// 失败宽限：真 = 已达上界（标记总线死亡、解阻塞 reader 与 pending 等待者）。
/// 未记 `rr3_heartbeat_sent` → 下一拍立即到期重试，间隔由 RPC 超时（5s）兜住。
fn rr3_hb_miss(hb_inner: &Rr3Inner, misses: &mut u32) -> bool {
    *misses += 1;
    if *misses >= RR3_HEARTBEAT_MAX_MISSES {
        eprintln!(
            "rr3-bus: 心跳连续 {} 拍失败，标记总线死亡（supervisor 按 restart_policy 裁定）",
            *misses
        );
        // 库内不 process::exit：设 stop 让宿主 serve 循环的 rr3_connected 死亡探针翻假，
        // 由宿主自行以 exit code 1 退出（库不得杀宿主进程）。关 socket 解阻塞 reader
        // 与 pending 等待者，与 Drop 同形。
        hb_inner.stop.store(true, Ordering::Relaxed);
        let _ = hb_inner
            .sock
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .shutdown(std::net::Shutdown::Both);
        return true;
    }
    eprintln!(
        "rr3-bus: 心跳失败 第 {}/{RR3_HEARTBEAT_MAX_MISSES} 拍（不退出，下拍重试）",
        *misses
    );
    false
}

/// 记已发：推进拍点（成功后立即生效；失败且 stop 已置的同拍也记）。
fn rr3_hb_mark_sent(hb_inner: &Rr3Inner, now_ms: u64) {
    hb_inner
        .rt
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .rr3_heartbeat_sent(now_ms);
}
