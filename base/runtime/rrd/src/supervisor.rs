// SPDX-License-Identifier: Apache-2.0
//! spawn/重启唯一执行者：单执行线程串行收尸与起进程，全 daemon 只此一条 + watchdog。
//!
//! 边界（不许被"崩溃自愈已完成"盖过）：只重启本守护起的进程（无 SO_PEERCRED，
//! 认不出外来者）；不持 SIGCHLD、不用 waitpid——`try_wait` + 扫尸节拍，死后至多
//! 一拍才知；depends/degraded 由 daemon/registry 裁定，本文件只交出收尸事实。
//!
//! 纪律：`rr3_lock(...).rr3_reg_*()` 先绑定再 `match`（写进匹配式会把 guard 延寿，
//! 臂内重入即自死锁）；退避进 `due`，**绝不** sleep——一条退避不能停摆整机收尸；
//! 子进程 stdout 必须 null（测试 needle 读 rrd 的 stdout），stderr 进
//! `<socket>.log/<unit>.log`；`spawn ` 只用于成功起进程，失败一律 `起进程失败`。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::registry::{Rr3RestartPlan, RrdRegistry};
use crate::rr3_log;

use reap::{rr3_fire_due, rr3_reap};
use schedule::rr3_schedule_due;
use spawn::rr3_boot_spawn;

mod job;
mod reap;
mod schedule;
mod spawn;
mod unit_log;

pub use job::{rr3_boot_job_parse, Rr3SpawnJob, Rr3SuperMsg, Rr3Supervisor};

/// 低电旗期间的启动重试间隔：计划保留、延后重问（旗解除后即放行）。
/// 5s 与运维观察节奏同档；低电段本身应短于一次充电恢复。
const RR3_LOW_POWER_RETRY_MS: u64 = 5_000;

/// 起唯一那条 supervisor 线程。`sweep` 复用 `--watchdog-tick-ms`（E-5：生产 200ms / 测试 20ms），
/// 不新增 CLI 旋钮。
pub fn rr3_spawn_supervisor(
    rx: Receiver<Rr3SuperMsg>,
    shared: Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    sweep: Duration,
    bus_path: PathBuf,
    boot_role: String,
) -> JoinHandle<()> {
    thread::spawn(move || {
        // 与 watchdog 同一口径：日志打在**线程内**，它证明的是"线程真的跑起来过"，
        // 而不是"走到了 spawn 那一行"。
        rr3_log(&format!("supervisor 启动 sweep={}ms", sweep.as_millis()));
        let mut jobs: BTreeMap<String, Rr3SpawnJob> = BTreeMap::new();
        let mut children: BTreeMap<String, Child> = BTreeMap::new();
        let mut due: Vec<(Instant, Rr3RestartPlan)> = Vec::new();
        while rr3_super_tick(
            &rx,
            sweep,
            &shared,
            t0,
            &mut jobs,
            &mut children,
            &mut due,
            &bus_path,
            &boot_role,
        ) {}
    })
}

/// 一拍等多久：`sweep` 与"最近一个到期点"取小者。为什么不能只用 `sweep`：
/// `recv_timeout` 一收到消息就立刻醒，醒完这一轮又 park 回 `sweep` —— 而既有 e2e
/// 线级用例的 tick 是 1 小时（`RR3_QUIET_WATCHDOG_TICK_MS`），那样"20ms 后到期的
/// 重启"永远等不到 `rr3_fire_due` 被叫到，E-1 那句 `跳过重启` 也就永远打不出来
/// （`e2e_sim_peer` 那条对照用例正面等的就是它）。取小者同时把 E-4 补全：退避表是
/// **到期时刻**队列，到点了就得有人看表；"没东西到期时这一拍仍等于
/// `--watchdog-tick-ms`"这句话没被改动（E-5 的接线照旧）。
fn rr3_sweep_wait(due: &[(Instant, Rr3RestartPlan)], sweep: Duration) -> Duration {
    let Some(at) = due.iter().map(|(at, _)| *at).min() else {
        return sweep;
    };
    sweep.min(at.saturating_duration_since(Instant::now()))
}

/// supervisor 一拍：收件 → 扫尸 → 到期执行。返回 false = 投递方已断，收摊退出。
#[allow(clippy::too_many_arguments)]
fn rr3_super_tick(
    rx: &Receiver<Rr3SuperMsg>,
    sweep: Duration,
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    jobs: &mut BTreeMap<String, Rr3SpawnJob>,
    children: &mut BTreeMap<String, Child>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
    bus_path: &Path,
    boot_role: &str,
) -> bool {
    let wait = rr3_sweep_wait(due, sweep);
    // 一次收件即一拍：`Timeout` 就是"这一拍没信"（节拍本身）。
    let mut keep_running = true;
    match rx.recv_timeout(wait) {
        Ok(Rr3SuperMsg::Boot(job)) => {
            // 先登记"我能起谁"再起第一世：`rr3_reap` 里那句 `expect("children 里的 name
            // 必有 job")` 就建立在这个顺序上（`children` 的键只可能来自这里）。
            // 这里不打日志：可见证据是 `rr3_boot_spawn` 的 `spawn …` 那一行，
            // 多打一条"登记了"只是把同一件事说两遍（日志面每多一行就要多审一次 needle）。
            let job = *job;
            let for_spawn = job.clone();
            jobs.insert(job.facts.name.clone(), job);
            rr3_boot_spawn(
                shared, t0, children, due, &for_spawn, 0, bus_path, boot_role,
            );
        }
        Ok(Rr3SuperMsg::Restart(plan)) => rr3_schedule_due(due, plan),
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => {
            // brief 把 Disconnected 也当"这一拍没信"，但它是**永久**状态：
            // `recv_timeout` 在已断开的通道上立即返回 Err，于是"每拍等 sweep"退化成
            // 100% 忙等（正是本仓拒 `--watchdog-tick-ms 0` 的那条理由）。
            // 现值：daemon 永不 shutdown（`incoming()` 不返回），而 watchdog 与每条 conn
            // 各持一份 Sender，所以这一支在真路径上不可达；真到了那一刻说明进程正在收尾，
            // 剩下的子进程由 rrd 退出与 `e2e_spawn_restart` 的 S5（孤儿卫生）负责。
            // 冲突已写进实施报告：这一支按"退出而不忙等"实现，语义与 brief 相同
            //（都不再处理新请求），只是不烧核。
            rr3_log("supervisor 退出：已无投递方（不再忙等）");
            keep_running = false;
        }
    }
    rr3_reap(shared, t0, jobs, children, due);
    rr3_fire_due(shared, t0, jobs, children, due, bus_path, boot_role);
    keep_running
}

#[cfg(test)]
mod tests {
    use super::schedule::rr3_low_power_retry;
    use super::unit_log::rr3_open_unit_log;
    use super::*;
    use std::io::Write;

    /// 门 r 机制面（批N4 件八）：per-unit 日志三断言——打开即建、append 不截断
    /// （"一次重启后仍能取到崩溃前的行"的载体语义）、socket 无父段时 fail-closed
    /// 报错（绝不落到 CWD 相对路径上悄悄建目录）。
    #[test]
    fn unit_log_fail_closed() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("时钟在 UNIX_EPOCH 之前")
            .as_nanos();
        let sock =
            std::env::temp_dir().join(format!("rr3-unit-log-{}-{nanos}.sock", std::process::id()));

        // 两世 spawn 各写一行，两行都在——append 不 truncate
        let f1 = rr3_open_unit_log(&sock, "rr3-x").expect("首开即建");
        drop(f1);
        let f2 = rr3_open_unit_log(&sock, "rr3-x").expect("重开不拒");
        writeln!(&f2, "first").expect("写首行");
        drop(f2);
        let f3 = rr3_open_unit_log(&sock, "rr3-x").expect("第三世仍 append");
        writeln!(&f3, "second").expect("写次行");
        drop(f3);
        let mut dir_os = sock.clone().into_os_string();
        dir_os.push(".log");
        let text = std::fs::read_to_string(PathBuf::from(dir_os.clone()).join("rr3-x.log"))
            .expect("日志文件可读");
        assert!(
            text.contains("first") && text.contains("second"),
            "append 语义被破: {text:?}"
        );
        let _ = std::fs::remove_dir_all(PathBuf::from(dir_os));

        // socket 无父段（空路径）必须报错，而不是相对 CWD 悄悄建目录
        assert!(
            rr3_open_unit_log(Path::new(""), "rr3-x").is_err(),
            "空 socket 路径应 fail-closed"
        );
    }

    /// 低电拒绝只延后、不丢计划：身份/第几次/入口必须原样带过，延迟足够大
    /// （远大于扫尸节拍），避免低电段内紧循环重试。
    #[test]
    fn low_keeps_delays() {
        let p = rr3_low_power_retry("rr3-power", 2, "bin/rr3-power");
        assert_eq!(p.name, "rr3-power");
        assert_eq!(p.attempt, 2, "重启预算的计数不因被拒而归零");
        assert_eq!(p.entry, "bin/rr3-power");
        assert_eq!(p.delay_ms, RR3_LOW_POWER_RETRY_MS);
        assert!(
            p.delay_ms >= 1_000,
            "重试间隔必须远大于扫尸节拍，低电段内不得紧循环"
        );
    }
}
