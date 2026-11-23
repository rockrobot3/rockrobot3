// SPDX-License-Identifier: Apache-2.0
//! 任务与连接管理：boot 队列仲裁投递、accept 循环与并发槽守卫。

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rr_ipc::{Rr3TransportListener, Rr3UdsListener};
use rr_registry::{rr3_plan_boot, Rr3BootPlan, Rr3DependsFacts, Rr3TopicSet};

use crate::conn::{rr3_conn_serve, Rr3ConnCtx};
use crate::registry::RrdRegistry;
use crate::supervisor::{rr3_boot_job_parse, Rr3SpawnJob, Rr3Supervisor};
use crate::{rr3_lock, rr3_log};

use super::boot::Rr3DaemonBoot;

/// 装载全集 → 依赖事实表 → 仲裁计划（拓扑序 + 层内确定性次键）。
fn rr3_queue_arb(jobs: &[Rr3SpawnJob]) -> Rr3BootPlan {
    let arb: Vec<Rr3DependsFacts> = jobs
        .iter()
        .map(|j| Rr3DependsFacts {
            name: j.facts.name.clone(),
            provides: j.facts.provides.clone(),
            depends_required: j.facts.depends_required.clone(),
            start_priority: j.facts.start_priority,
        })
        .collect();
    rr3_plan_boot(&arb)
}

/// boot 队列：装载 → 仲裁 → 提供者表 → 按 order 投递（缺格模块跳过）
pub(super) fn rr3_daemon_queue(
    boot: &Rr3DaemonBoot,
    shared: &Arc<Mutex<RrdRegistry>>,
    sup: &Rr3Supervisor,
) -> Result<(), String> {
    let jobs = rr3_jobs_load(boot);
    // ADR-0009 裁定 1/2：装载全集交 rr-registry 仲裁——rrd 只执行裁定：
    // 环/名重复 → fatal 拒整个 boot；不可满足 → rejected 点名；order = 拓扑序 +
    // 层内 (start_priority, name) 确定性次键。
    let plan = rr3_queue_arb(&jobs);
    rr3_queue_fatal(&plan)?;
    rr3_queue_note(&plan, boot);
    // 提供者表：boot 全集一次性写入，含被拒模块——provides 仍计入可满足性宇宙
    rr3_queue_providers(&jobs, shared);
    rr3_queue_rejects(&plan);
    let by_name: BTreeMap<String, Rr3SpawnJob> = jobs
        .into_iter()
        .map(|j| (j.facts.name.clone(), j))
        .collect();
    rr3_queue_dispatch(boot, &plan, by_name, sup);
    Ok(())
}

/// 裁定序一行定格（缺格跳过项已单独点名，不混入）：spawn 行给时刻，本行给顺序与
/// 全集——boot 卡住时先看这行，按序对照下一条 spawn 即可定位停在哪一项。
fn rr3_queue_note(plan: &rr_registry::Rr3BootPlan, boot: &Rr3DaemonBoot) {
    let order: Vec<&str> = plan
        .order
        .iter()
        .map(String::as_str)
        .filter(|n| !boot.degraded.contains(*n))
        .collect();
    if order.is_empty() {
        rr3_log("boot 序: 空（无可投递模块）");
    } else {
        rr3_log(&format!(
            "boot 序（{} 项）: {}",
            order.len(),
            order.join(" → ")
        ));
    }
}

/// 被仲裁拒的模块逐个点名（不可满足，但不连坐下游）。
fn rr3_queue_rejects(plan: &rr_registry::Rr3BootPlan) {
    for name in &plan.rejected {
        rr3_log(&format!(
            "拒绝启动 {name}：required 能力不可满足（base 内建与 boot 全集 provides 均无，ADR-0009 裁定 2）"
        ));
    }
}

/// fatal 裁定点名：非空即拒整个 boot（环/名重复）。
fn rr3_queue_fatal(plan: &rr_registry::Rr3BootPlan) -> Result<(), String> {
    if plan.fatal.is_empty() {
        return Ok(());
    }
    for e in &plan.fatal {
        rr3_log(&format!("拒绝 boot：{e}"));
    }
    Err(format!(
        "拒绝进入 boot 阶段（ADR-0009 裁定 2，环拒整个 boot）: {}",
        plan.fatal.join("; ")
    ))
}

/// 提供者表写入：能力 → 声明模块名集合（含被拒模块）。
fn rr3_queue_providers(jobs: &[Rr3SpawnJob], shared: &Arc<Mutex<RrdRegistry>>) {
    let mut providers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for j in jobs {
        for cap in &j.facts.provides {
            providers
                .entry(cap.clone())
                .or_default()
                .insert(j.facts.name.clone());
        }
    }
    rr3_lock(shared).rr3_reg_providers(providers);
}

/// 按 order 投递：缺格模块跳过（provides 已在表里，下游不连坐）。
fn rr3_queue_dispatch(
    boot: &Rr3DaemonBoot,
    plan: &rr_registry::Rr3BootPlan,
    mut by_name: BTreeMap<String, Rr3SpawnJob>,
    sup: &Rr3Supervisor,
) {
    for name in &plan.order {
        rr3_dispatch_one(name, boot, &mut by_name, sup);
    }
}

fn rr3_dispatch_one(
    name: &str,
    boot: &Rr3DaemonBoot,
    by_name: &mut BTreeMap<String, Rr3SpawnJob>,
    sup: &Rr3Supervisor,
) {
    if boot.degraded.contains(name) {
        return;
    }
    if let Some(job) = by_name.remove(name) {
        sup.rr3_boot(job);
    }
}

/// 装配链事实透传给 job（空串 = 不注入）。
fn rr3_job_attach(job: &mut Rr3SpawnJob, boot: &Rr3DaemonBoot) {
    // 物理绑定/轴状态/HAL 后端随 job 透传给子进程（空串 = 不注入）
    job.hal_bindings = boot.hal_bindings.clone();
    job.axis_state = boot.axis_state.clone();
    job.hal_backend = boot.hal_backend.clone();
    job.audio_backend = boot.audio_backend.clone();
    job.servo_backend = boot.servo_backend.clone();
    job.display_backend = boot.display_backend.clone();
}

/// 装载期解析全部 manifest 并为每个 job 注入装配链事实
fn rr3_jobs_load(boot: &Rr3DaemonBoot) -> Vec<Rr3SpawnJob> {
    let mut jobs = Vec::new();
    for manifest in &boot.manifests {
        if let Some(mut job) = rr3_boot_job_parse(manifest) {
            rr3_job_attach(&mut job, boot);
            jobs.push(job);
        }
    }
    jobs
}

/// accept 循环的共享面（一次构造，逐连接 clone）
pub(super) struct Rr3AcceptCtx {
    pub(super) shared: Arc<Mutex<RrdRegistry>>,
    pub(super) topics: Arc<Rr3TopicSet>,
    pub(super) t0: Instant,
    pub(super) pre_register_timeout_ms: u64,
    pub(super) sup: Rr3Supervisor,
    pub(super) ctr_store: Option<std::sync::mpsc::SyncSender<()>>,
    pub(super) counters_gate: Arc<Mutex<crate::counters::Rr3CountersGate>>,
}

/// accept 主循环（永不返回）：持续性错误退避 200ms，满员退避 50ms，并发上限 64。
pub(super) fn rr3_daemon_accept(listener: Rr3UdsListener, ctx: Rr3AcceptCtx) -> ! {
    const RR3_MAX_CONNS: usize = 64;
    let live = Arc::new(AtomicUsize::new(0));
    let mut gate = Rr3RefuseGate::default();
    loop {
        match rr3_accept_pair(&listener, &live, RR3_MAX_CONNS) {
            Rr3AcceptGain::Conn(stream, slot) => rr3_spawn_conn(stream, ctx.rr3_conn_ctx(), slot),
            // 满员不是错误，但绝不能空转：无退避时重连风暴会以"accept→拒→记日志"
            // 的速度打满一个核并把 rrd 日志写穿（日志写入是同步 append）。50ms 的
            // 代价是容量释放后新连接最多多等一拍；收益是尝试率封顶 20 次/秒。
            Rr3AcceptGain::Full => {
                let live_n = live.load(Ordering::Relaxed);
                // 账与日志分开：计数键逐次累加（ctl.counters 可读），日志按窗口
                // 汇总——节流只压打印，不压真实次数。
                rr3_lock(&ctx.shared)
                    .rr3_reg_ctr_bump(crate::counters::RR3_COUNTERS_REFUSED_KEY, 1);
                if let Some(line) = gate.rr3_refuse_note(Instant::now(), live_n, RR3_MAX_CONNS) {
                    rr3_log(&line);
                }
                thread::sleep(RR3_FULL_BACKOFF);
            }
            // accept 失败已在 rr3_accept_pair 内退避，这里直接进入下一拍。
            Rr3AcceptGain::Retry => {}
        }
    }
}

/// 满员拒连退避（见 `rr3_daemon_accept` 的注释）。
const RR3_FULL_BACKOFF: Duration = Duration::from_millis(50);

/// 满员拒连日志的汇总窗口：首条即时打，窗口内的后续拒连只计数。
const RR3_REFUSE_LOG_EVERY: Duration = Duration::from_secs(5);

/// 满员拒连的日志节流器。
///
/// 为什么需要它：拒连日志本身是同步 append，重连风暴下"每条一行"等于把
/// 可观测性通道变成写放大源——磁盘被写满后 journal/OTA/配置写全灭。
/// 首条必须即时可见（运维要知道"现在满员"），窗口内的次数不丢账（汇总时报），
/// 这样既不会被刷屏，也不会把真实规模报小。
#[derive(Default)]
pub(super) struct Rr3RefuseGate {
    last: Option<Instant>,
    suppressed: u64,
}

impl Rr3RefuseGate {
    /// 记一次拒连：应打印时返回文案，否则计入被压掉的次数并返回 `None`。
    pub(super) fn rr3_refuse_note(
        &mut self,
        now: Instant,
        live: usize,
        max: usize,
    ) -> Option<String> {
        match self.last {
            None => {
                self.last = Some(now);
                Some(format!("连接数超上限 {max}，拒绝新连接（在服 {live} 条）"))
            }
            Some(t) if now.saturating_duration_since(t) >= RR3_REFUSE_LOG_EVERY => {
                let n = std::mem::take(&mut self.suppressed);
                self.last = Some(now);
                Some(format!(
                    "连接数超上限 {max}，拒绝新连接（在服 {live} 条；过去 {}s 另有 {n} 次拒连未逐条打印）",
                    RR3_REFUSE_LOG_EVERY.as_secs()
                ))
            }
            Some(_) => {
                self.suppressed = self.suppressed.saturating_add(1);
                None
            }
        }
    }
}

/// accept 一拍的结果：成功连接 / 满员拒连 / 可重试（accept 失败已退避）。
enum Rr3AcceptGain {
    Conn(UnixStream, Rr3ConnSlot),
    Full,
    Retry,
}

impl Rr3AcceptCtx {
    /// 逐连接克隆共享面。
    fn rr3_conn_ctx(&self) -> Rr3ConnCtx {
        Rr3ConnCtx {
            shared: self.shared.clone(),
            topics: self.topics.clone(),
            t0: self.t0,
            pre_register_timeout_ms: self.pre_register_timeout_ms,
            sup: self.sup.clone(),
            ctr_store: self.ctr_store.clone(),
            counters_gate: self.counters_gate.clone(),
        }
    }
}

/// 取一条连接：accept 失败退避后回 `Retry`；并发满回 `Full`（由主循环退避与记账）。
fn rr3_accept_pair(
    listener: &Rr3UdsListener,
    live: &Arc<AtomicUsize>,
    max: usize,
) -> Rr3AcceptGain {
    const RR3_ACCEPT_ERR_BACKOFF: Duration = Duration::from_millis(200);
    let stream = match listener.rr3_accept() {
        Ok(stream) => stream,
        Err(e) => {
            rr3_log(&format!("accept 失败（继续服务）: {e}"));
            thread::sleep(RR3_ACCEPT_ERR_BACKOFF);
            return Rr3AcceptGain::Retry;
        }
    };
    match rr3_conn_acquire(live, max) {
        Some(slot) => Rr3AcceptGain::Conn(stream, slot),
        // 连接对象在此 drop：对端立刻看到 EOF，不会以为"连上了只是没应答"。
        None => Rr3AcceptGain::Full,
    }
}

/// 连接线程：线程退出（含 panic）必归还并发槽；起不来只拒这一条连接。
fn rr3_spawn_conn(stream: UnixStream, conn: Rr3ConnCtx, slot: Rr3ConnSlot) {
    let spawned = thread::Builder::new()
        .name("rrd-conn".into())
        .spawn(move || {
            let _slot = slot;
            rr3_conn_thread(stream, conn);
        });
    if let Err(e) = spawned {
        // 线程起不来（资源耗尽）只拒这一条连接：accept 主循环不能 panic——
        // 守护停摆比少一条连接严重得多（闭包已 drop，槽位随 Drop 归还）。
        rr3_log(&format!("连接线程起不来，拒连（继续服务）: {e}"));
    }
}

/// 连接线程本体：服务一条连接，退出（含 panic）由槽守卫归还并发位。
fn rr3_conn_thread(stream: UnixStream, conn: Rr3ConnCtx) {
    if let Err(e) = rr3_conn_serve(stream, conn) {
        rr3_log(&format!("连接收尾: {e}"));
    }
}

/// 连接并发槽守卫：Drop 归还计数，panic 路径也不漏账。
pub(super) struct Rr3ConnSlot(Arc<AtomicUsize>);

impl Drop for Rr3ConnSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 申请一个连接槽；满则 None（调用方拒连并点名）。`max` 显式传入便于单测小帽。
pub(super) fn rr3_conn_acquire(live: &Arc<AtomicUsize>, max: usize) -> Option<Rr3ConnSlot> {
    let n = live.fetch_add(1, Ordering::Relaxed);
    if n >= max {
        live.fetch_sub(1, Ordering::Relaxed);
        return None;
    }
    Some(Rr3ConnSlot(live.clone()))
}
