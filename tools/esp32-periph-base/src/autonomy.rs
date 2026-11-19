//! 自治壳（W3-2b）：把 `rr-autonomy` 监督核接到板侧，提供 `/api/board/health` 真状态。
//!
//! 本阶段只启动监督壳与健康快照；六个任务尚未接线，统一显式标为 `degraded`
//! （`rr3_sup_degrade` = 不参与判死），避免"未接线任务被当成超时"的假红。
//! W3-3 接线后逐个 `rr3_sup_beat`，健康面自然转为 running。
//!
//! 与 HTTP 的关系：`httpd` 的 `/api/board/health` 只读本模块的静态快照；
//! 锁只护快照读写，不跨 HTTP 组包持有。

use std::sync::atomic::{AtomicI8, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use rr_autonomy::{Rr3SupCfg, Rr3SupHealth, Rr3Supervisor, Rr3Task, Rr3TaskState};

/// SNTP 状态（未知/已同步/失败）。
pub(crate) const SNTP_UNKNOWN: i8 = 0;
pub(crate) const SNTP_SYNCED: i8 = 1;
pub(crate) const SNTP_FAILED: i8 = -1;

static SNTP_STATE: AtomicI8 = AtomicI8::new(SNTP_UNKNOWN);
static SNAP: Mutex<Option<Rr3SupHealth>> = Mutex::new(None);

/// 起自治监督壳：六任务先标 degraded（未接线），按 200ms 栅格 tick。
pub(crate) fn rr3_start() {
    let now = crate::state::now_ms();
    let mut sup = Rr3Supervisor::rr3_sup_new(Rr3SupCfg::default(), now);
    for task in Rr3Task::ALL {
        sup.rr3_sup_degrade(task);
    }
    let spawned = thread::Builder::new()
        .name("autonomy".into())
        .stack_size(crate::board::THREAD_STACK)
        .spawn(move || rr3_tick_loop(sup));
    if let Err(e) = spawned {
        println!("[autonomy] 起监督壳线程失败: {e:?}");
    }
}

/// SNTP 线程回写状态（供 `/health` 读）。
pub(crate) fn rr3_sntp_set(state: i8) {
    SNTP_STATE.store(state, Ordering::Relaxed);
}

/// 监督栅格：每拍更新快照（锁窗只做 Copy，不组串）。
fn rr3_tick_loop(mut sup: Rr3Supervisor) {
    loop {
        thread::sleep(Duration::from_millis(200));
        sup.rr3_sup_tick(crate::state::now_ms());
        let health = sup.rr3_sup_health();
        if let Ok(mut slot) = SNAP.lock() {
            *slot = Some(health);
        }
    }
}

/// `/api/board/health` 的 JSON 体（无快照时 `shell=off`，不虚构任务状态）。
pub(crate) fn rr3_health_json() -> String {
    let sntp = SNTP_STATE.load(Ordering::Relaxed);
    let snap = SNAP.lock().ok().and_then(|s| *s);
    let Some(h) = snap else {
        return format!("{{\"shell\":\"off\",\"sntp\":\"{}\"}}", sntp_word(sntp));
    };
    let mut tasks = String::new();
    for task in Rr3Task::ALL {
        let i = task.index();
        if i > 0 {
            tasks.push(',');
        }
        tasks.push_str(&format!(
            "{{\"name\":\"{}\",\"state\":\"{}\",\"timeouts\":{}}}",
            task.name(),
            state_word(h.state[i]),
            h.timeouts[i]
        ));
    }
    format!(
        "{{\"shell\":\"on\",\"sntp\":\"{}\",\"ticks\":{},\"overruns\":{},\"restarts\":{},\"tasks\":[{}]}}",
        sntp_word(sntp),
        h.ticks,
        h.overruns,
        h.restarts,
        tasks
    )
}

fn sntp_word(v: i8) -> &'static str {
    match v {
        SNTP_SYNCED => "synced",
        SNTP_FAILED => "failed",
        _ => "unknown",
    }
}

fn state_word(v: Rr3TaskState) -> &'static str {
    const WORDS: [&str; 4] = ["starting", "running", "degraded", "dead"];
    WORDS[v as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_words_cover_all_variants() {
        assert_eq!(state_word(Rr3TaskState::Starting), "starting");
        assert_eq!(state_word(Rr3TaskState::Running), "running");
        assert_eq!(state_word(Rr3TaskState::Degraded), "degraded");
        assert_eq!(state_word(Rr3TaskState::Dead), "dead");
    }

    #[test]
    fn sntp_words_cover_three_states() {
        assert_eq!(sntp_word(SNTP_SYNCED), "synced");
        assert_eq!(sntp_word(SNTP_FAILED), "failed");
        assert_eq!(sntp_word(SNTP_UNKNOWN), "unknown");
    }

    #[test]
    fn health_json_without_snapshot_reports_shell_off() {
        let j = rr3_health_json();
        assert!(j.contains("\"shell\":\"off\""), "{j}");
        assert!(!j.contains("tasks"), "无快照不得虚构任务状态");
    }
}
