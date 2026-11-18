//! 到期表与终态落点：退避计划、低电重试与收尸后的广播记账。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::RR3_LOW_POWER_RETRY_MS;
use crate::daemon::rr3_state_frame;
use crate::registry::{rr3_phase_for_exit, Rr3ChildExit, Rr3Phase, Rr3RestartPlan, RrdRegistry};
use crate::{rr3_lock, rr3_log};

/// 到期表（E-4）：`(起它的时刻, 计划)`。`Instant::now() + Duration::from_millis(delay_ms)` 在
/// `delay_ms == u64::MAX`（`rr3_charge` 的饱和值，语义"永不重启"）时会溢出 → 必须 `checked_add`，
/// 取不到就打 E-4 那句并丢弃——**不许**在这里改回 `sleep`，也不许 `unwrap`。
pub(super) fn rr3_schedule_due(due: &mut Vec<(Instant, Rr3RestartPlan)>, plan: Rr3RestartPlan) {
    let Some(at) = Instant::now().checked_add(Duration::from_millis(plan.delay_ms)) else {
        rr3_log(&format!("退避不可表示，视为永久停止 {}", plan.name));
        return;
    };
    due.push((at, plan));
}

/// 低电拒绝时重建的到期计划：名字/第几次/入口原样，延迟换成固定重试间隔。
/// 只改"什么时候再问"，不动重启预算（被拒的一次不记账）。
pub(super) fn rr3_low_power_retry(name: &str, attempt: u32, entry: &str) -> Rr3RestartPlan {
    Rr3RestartPlan {
        name: name.to_string(),
        attempt,
        delay_ms: RR3_LOW_POWER_RETRY_MS,
        entry: entry.to_string(),
    }
}

/// 一个终态的**唯一**落点：广播（若该广播）、点名理由的日志、计划进到期表。
/// 收尸那一路与"起进程失败"那一路共用它（两条路只是 `status` 的来源不同）。
///
/// 锁纪律就在下面第一行：`verdict` 由调用方 `let` 绑好后传进来，本函数体内每次
/// `rr3_lock` 都单独成句，臂内不再取第二把锁。
pub(super) fn rr3_absorb_exit(
    shared: &Arc<Mutex<RrdRegistry>>,
    name: &str,
    status: &str,
    verdict: Rr3ChildExit,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
) {
    let plan = match verdict {
        Rr3ChildExit::Crashed {
            src_task,
            clean_exit,
            plan,
        } => {
            let phase = rr3_phase_for_exit(clean_exit);
            // E-9：这里的排除项是 `crashed.src_task`（它的 tx 已是 None，排除属形式，
            // 与 expire 那一路同形）；conn 收尾那一路排的是 `RR3_TASK_RRD`（等于不排除）。
            // 两种口径是**已知**的第三/第四种口径，归 Task 13 的 A-3 文档化，此处不"统一"——
            // 统一成哪一种都没有判据支持。
            let n =
                rr3_lock(shared).rr3_reg_fanout(&rr3_state_frame(name, phase, src_task), src_task);
            rr3_log(&format!(
                "{name} 子进程退出 status={status} → 广播 {phase} 到 {n} 个连接"
            ));
            // ADR-0009 裁定 3：本路刚记完收尸账（`rr3_reg_child_exit` 已置 Failed），
            // 死者若是某能力的提供者且再无人在位 → 下游逐个置 Degraded 并广播。
            // EarlyExit / Silent 两路不传播：前者从未注册过（没有"离开 Running"的相位
            // 事件），后者的死亡已由别人记过、传播也归那一拍。
            let degraded = rr3_lock(shared).rr3_reg_degrade(name);
            rr3_degrade_fanout(shared, name, degraded);
            plan
        }
        Rr3ChildExit::EarlyExit { plan } => {
            rr3_log(&format!(
                "{name} 未注册即退出 status={status}（无 state 可广播）"
            ));
            plan
        }
        Rr3ChildExit::Silent => {
            rr3_log(&format!(
                "{name} 子进程退出 status={status}，终态已有人记过"
            ));
            None
        }
    };
    if let Some(p) = plan {
        rr3_schedule_due(due, p);
    } else {
        // 硬规矩：`rr3_reg_hit_cap` 先 `let` 绑，**绝不写进 match 守卫**（守卫里的 `MutexGuard`
        // 会延寿到整个 match 语句结束，而调用方随后还要再取同一把锁）。
        let capped = rr3_lock(shared).rr3_reg_hit_cap(name);
        if capped {
            rr3_log(&format!(
                "停止重启 {name}：连续崩溃达上限，运行时降级（spec §5.3）"
            ));
        }
    }
}

/// 降级传播广播：逐个依赖者点名并记数（排除项 `RR3_TASK_RRD`，E-9 同一理据）。
fn rr3_degrade_fanout(shared: &Arc<Mutex<RrdRegistry>>, name: &str, degraded: Vec<(String, u64)>) {
    for (dep, task) in degraded {
        let n = rr3_lock(shared).rr3_reg_fanout(
            &rr3_state_frame(&dep, Rr3Phase::Degraded.as_str(), task),
            rr_bus::RR3_TASK_RRD,
        );
        rr3_log(&format!(
            "{dep} 依赖断供 → degraded（提供者 {name} 离开 Running）广播到 {n} 个连接"
        ));
    }
}
