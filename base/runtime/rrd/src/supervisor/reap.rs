//! 扫尸与到期执行：try_wait 收集、终态记账与到期计划重起。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::job::Rr3SpawnJob;
use super::schedule::rr3_absorb_exit;
use super::spawn::rr3_boot_spawn;
use crate::daemon::rr3_now_ms;
use crate::registry::{Rr3RestartPlan, RrdRegistry};
use crate::{rr3_lock, rr3_log};

/// 扫尸（E-5 每轮的第二段）。**先收集 `(name, status)` 再处理**：`iter_mut` 里不许 `remove`。
///
/// 退出码是 owned 模块死因的**唯一权威**：`abort` 走 signal(6)、`kill -9` 走 signal(9)，
/// 两者都不可能被误读成"体面退场"——这正是 conn 那一路看不见的（见 A-6）。
pub(super) fn rr3_reap(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    jobs: &BTreeMap<String, Rr3SpawnJob>,
    children: &mut BTreeMap<String, Child>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
) {
    let exited = rr3_reap_scan(children);
    for (name, status) in exited {
        // 先把 handle 交出来：之后无论走哪条分支，这个 pid 都不再是"我的活"（绝不留僵尸，
        // 也绝不让下一轮把它当还活着的进程重复收一次）。
        let _ = children.remove(&name);
        let clean = status.success();
        let job = jobs.get(&name).expect("children 里的 name 必有 job");
        // 硬规矩：先 `let` 绑结果。记账 + 相位 + 广播**只在这一次**发生——owned 的模块，
        // conn 那一路已把相位/记账/广播三件事全让了出来（`Rr3Death::DeferredToExitStatus`）。
        let verdict = rr3_lock(shared).rr3_reg_child_exit(&job.facts, clean, rr3_now_ms(t0));
        rr3_absorb_exit(shared, &name, &status.to_string(), verdict, due);
    }
}

/// 扫尸收集：`try_wait` 非阻塞，`Ok(None)` = 还活着；`Err` 只可能是 ECHILD 这类"没得收"，跳过。
fn rr3_reap_scan(children: &mut BTreeMap<String, Child>) -> Vec<(String, ExitStatus)> {
    let mut exited: Vec<(String, ExitStatus)> = Vec::new();
    for (name, child) in children.iter_mut() {
        if let Ok(Some(status)) = child.try_wait() {
            exited.push((name.clone(), status));
        }
    }
    exited
}

/// 到期表摘取：取出已到期计划，未到期项原样保留。
fn rr3_due_ready(due: &mut Vec<(Instant, Rr3RestartPlan)>) -> Vec<Rr3RestartPlan> {
    let now = Instant::now();
    let mut ready = Vec::new();
    due.retain(|(at, p)| {
        if *at <= now {
            ready.push(p.clone());
            false
        } else {
            true
        }
    });
    ready
}

/// 到期执行。**退避不是睡眠**：这里只问"到点了没有"，从不 sleep（E-4）。
pub(super) fn rr3_fire_due(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    jobs: &BTreeMap<String, Rr3SpawnJob>,
    children: &mut BTreeMap<String, Child>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
    bus_path: &Path,
    boot_role: &str,
) {
    if due.is_empty() {
        return;
    }
    let ready = rr3_due_ready(due);
    for plan in ready {
        // 硬规矩：`rr3_reg_live` 先 `let` 绑，绝不写进被匹配表达式/守卫（文件头那段）。
        // 这一问挡住的是"外来模块自己回来了"那一格（A-5② 的同名窗口）：判死到到期之间它若
        // 重新注册并喂了狗，再起一份就是拿同名注册去撞 EBUSY。
        let live = rr3_lock(shared).rr3_reg_live(&plan.name);
        if live {
            rr3_log(&format!("取消重启 {}：已有在册实例", plan.name));
            continue;
        }
        // E-1：**只有本守护起的进程才 spawn 回来**。`jobs` 是"我能起谁"的全集，
        // 判据就是这一条查询（外来模块的计划在这里被跳过，连它的 entry 都不碰）。
        // 这里不"统一"成"照 plan.entry 硬起"：那要求 rrd 知道该给外来进程注入什么身份（E-2）。
        let Some(job) = jobs.get(&plan.name).cloned() else {
            rr3_log(&format!("跳过重启 {}：不是本守护起的进程", plan.name));
            continue;
        };
        rr3_boot_spawn(
            shared,
            t0,
            children,
            due,
            &job,
            plan.attempt,
            bus_path,
            boot_role,
        );
    }
}
