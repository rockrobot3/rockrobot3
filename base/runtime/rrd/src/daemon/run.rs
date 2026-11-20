//! 守护主循环骨架：装配、绑定、线程起序、boot 投递与 accept 交棒。

use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use rr_registry::Rr3TopicSet;

use crate::cli::RrdConfig;
use crate::registry::RrdRegistry;
use crate::supervisor::{rr3_spawn_supervisor, Rr3SuperMsg, Rr3Supervisor};
use crate::{rr3_lock, rr3_log};

use super::boot::{
    rr3_boot_announce, rr3_ctr_wire, rr3_daemon_bind, rr3_daemon_boot, rr3_daemon_logs,
};
use super::task::{rr3_daemon_accept, rr3_daemon_queue, Rr3AcceptCtx};
use super::tick::rr3_spawn_watchdog;

/// panic 留痕（可观测性）：线程 panic 默认只到 stderr，不进 rrd 日志环/落盘；
/// 先落一行再交默认钩子，ctl.logs 与 rrd.log 都能看到"哪个线程/哪一行"出事，
/// 不改变 panic 语义。rr3_log 已不 panic（写失败降级），钩子内无重入风险。
fn rr3_panic_log() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        rr3_log(&format!("线程 panic: {info}"));
        default_hook(info);
    }));
}

/// 守护主循环：永不返回（除非 bind 失败）。集成测试靠父进程 kill 收尾。
///
/// 线程与投递的次序**有意义**，别重排（三件事各不相同）：
/// 1. 起 supervisor 线程 —— 它是唯一的执行线程，boot 与重启都从它身上过；
/// 2. 起 watchdog —— spec §5.0 要"注册完成前不收心跳也不判活"，而 boot 的第一个子进程
///    可能在同一毫秒内注册完，所以**监视线程必须已在跑**才有东西扫它（brief Step 2 的
///    "先起 watchdog、再起 boot 队列"）；
/// 3. 投 boot 队列 —— 放在 accept 循环之前，否则第一条消息永远没人取。
///
/// 装配链（boot 产物、启动摘要、日志落盘、计数持久化）按依赖序在此串起；
/// 各段的内部次序见 [`super::boot`]。
pub fn rr3_daemon_run(cfg: &RrdConfig, topics: Rr3TopicSet) -> Result<(), String> {
    rr3_panic_log();
    let t0 = Instant::now();
    let boot = rr3_daemon_boot(cfg)?;
    rr3_daemon_logs(cfg);
    rr3_boot_announce(cfg, &boot);
    // bind/cleanup/accept 走 trait 工厂（ADR-0008）：端点与监听语义上提到 rr-ipc，
    // Conn 类型仍是 UnixStream——conn.rs 的读写循环与 socket 专有门（读超时/shutdown）不动。
    // bind 独占探测（0.0.8d 审计 B-9）：存活实例占用 = 拒绝启动（裂脑禁绝）；
    // 残留死文件 = 探测 connect 失败后清掉再绑，探测与清理合并进 rr3_bind_exclusive。
    let listener = rr3_daemon_bind(cfg)?;

    let shared = Arc::new(Mutex::new(RrdRegistry::new(cfg.restart_base_ms)));
    // 冷启动基准定格（ctl.health 的 boot_ms 列）：在册表的时刻轴本就是
    // `rr3_now_ms(t0)`（相对 daemon 入口），故基准取 0——与注册行日志同一个起点。
    rr3_lock(&shared).rr3_reg_boot_set(0);
    // G4：装配缺项的模块预降级——注册入表即 Degraded，且不被解除传播捞回
    if !boot.predegraded.is_empty() {
        rr3_lock(&shared).rr3_reg_predegrade(boot.predegraded.clone());
    }
    let topics = Arc::new(topics);
    let (ctr_store, counters_gate) = rr3_ctr_wire(&shared, t0);

    // `Rr3Supervisor` 是 `Sender` 的一层薄壳（可 clone）：一份进 conn 收尾、一份进 watchdog，
    // 两条路交出的重启计划在这里汇成一条队列。
    let (sup_tx, sup_rx) = mpsc::channel::<Rr3SuperMsg>();
    let sup = Rr3Supervisor::rr3_new(sup_tx);
    // E-5：扫尸节拍复用 `--watchdog-tick-ms`，不新增 CLI 旋钮。
    // E-6 的 `RR3_BUS_PATH` 用 `cfg.socket` —— 它在 `main.rs` 里已按 CLI 原词**绝对化**过
    // （横幅回显、bind、注入给子进程的必须是同一个值，否则子进程会按自己的 CWD 去连一个
    // 不存在的路径）。
    // 句柄不 join：daemon 永不 shutdown（`incoming()` 不返回），join 它等于把主循环卡死。
    let _ = rr3_spawn_supervisor(
        sup_rx,
        shared.clone(),
        t0,
        Duration::from_millis(cfg.watchdog_tick_ms),
        cfg.socket.clone(),
        cfg.boot_role.clone(),
    );

    rr3_spawn_watchdog(shared.clone(), t0, cfg.watchdog_tick_ms, sup.clone());

    // E-7 启动序：先全部装载，再交 rr-registry 仲裁，最后按裁定投递
    rr3_daemon_queue(&boot, &shared, &sup)?;

    rr3_daemon_accept(
        listener,
        Rr3AcceptCtx {
            shared,
            topics,
            t0,
            pre_register_timeout_ms: cfg.pre_register_timeout_ms,
            sup,
            ctr_store,
            counters_gate,
        },
    )
}
