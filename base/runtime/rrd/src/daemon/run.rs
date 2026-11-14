//! 守护主循环骨架：装配、绑定、线程起序、boot 投递与 accept 交棒。

use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use rr_registry::Rr3TopicSet;

use crate::cli::RrdConfig;
use crate::registry::RrdRegistry;
use crate::supervisor::{rr3_spawn_supervisor, Rr3SuperMsg, Rr3Supervisor};
use crate::{rr3_lock, rr3_log};

use super::boot::{
    rr3_backend_summary, rr3_boot_summary, rr3_daemon_bind, rr3_daemon_boot, rr3_daemon_counters,
    rr3_daemon_logs, rr3_degrade_json,
};
use super::ctl::rr3_flush_on_term;
use super::task::{rr3_daemon_accept, rr3_daemon_queue, Rr3AcceptCtx};
use super::tick::rr3_spawn_watchdog;

/// 守护主循环：永不返回（除非 bind 失败）。集成测试靠父进程 kill 收尾。
///
/// 线程与投递的次序**有意义**，别重排（三件事各不相同）：
/// 1. 起 supervisor 线程 —— 它是唯一的执行线程，boot 与重启都从它身上过；
/// 2. 起 watchdog —— spec §5.0 要"注册完成前不收心跳也不判活"，而 boot 的第一个子进程
///    可能在同一毫秒内注册完，所以**监视线程必须已在跑**才有东西扫它（brief Step 2 的
///    "先起 watchdog、再起 boot 队列"）；
/// 3. 投 boot 队列 —— 放在 accept 循环之前，否则第一条消息永远没人取。
pub fn rr3_daemon_run(cfg: &RrdConfig, topics: Rr3TopicSet) -> Result<(), String> {
    // panic 留痕（可观测性）：线程 panic 默认只到 stderr，不进 rrd 日志环/落盘；
    // 先落一行再交默认钩子，ctl.logs 与 rrd.log 都能看到"哪个线程/哪一行"出事，
    // 不改变 panic 语义。rr3_log 已不 panic（写失败降级），钩子内无重入风险。
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        rr3_log(&format!("线程 panic: {info}"));
        default_hook(info);
    }));
    let t0 = Instant::now();
    let boot = rr3_daemon_boot(cfg)?;
    // rrd 自身日志落盘（批N5 件二笔2，§17 运维留痕同轨判据）：`<socket>.log/rrd.log`
    // append——与 per-unit 文件同目录同派生单点。每行同步 append、不轮转；下一行
    // 的启动回填把崩溃前的文件尾置回环头，`rr3.ctl.logs` 重启后仍取得到崩溃前的
    // 最后若干行（模块侧批N4 件八已同轨成立，rrd 自身自此同权）。打不开 = 点名
    // 退纯内存，观测通道故障降级、不放大成可用性故障（supervisor stderr 回退
    // inherit 同一条纪律）。位置钉死在装配链 match **之后**：--profile 路径此刻
    // 环容量已定格（回填行数按真容量收）；--manifest 调试路径按 512 兜底收。
    rr3_daemon_logs(cfg);
    // 启动摘要（可观测性）：一行钉住本次装配的输入面与规模，排障先看这一行。
    // 后门（--manifest）无装配单：tier/chip 显示 `-`，不造假值。
    let profile = cfg
        .profile
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "--manifest".to_string());
    let backends = rr3_backend_summary(
        &boot.hal_backend,
        boot.audio_backend.as_deref(),
        boot.servo_backend.as_deref(),
        boot.display_backend.as_deref(),
    );
    // 缺格数按 (模块, 格位) 条目计——一格一条，不是"缺格模块数"：同一模块缺两格
    // 是两个缺格，报文只数模块会把严重度报小。
    let missing_slots = boot
        .degrade
        .iter()
        .filter(|(_, kind, _)| kind == "missing_slot")
        .count();
    let summary = rr3_boot_summary(
        &boot.tier,
        &boot.chip,
        &profile,
        boot.manifests.len(),
        boot.predegraded.len(),
        missing_slots,
        &backends,
    );
    rr3_log(&summary);
    // 同一行定格给 ctl.status：远程诊断读一次应答即可看到装配面，不必翻日志。
    let _ = crate::rr3_boot_summary_set(summary);
    // 降级原因同刻定格给 ctl.health：卡在哪一格、为何不 spawn，一条应答看全。
    // 只在装配单正门定格——`--manifest` 后门未跑装配链，"无降级"是未知而非事实，
    // 读面须为 null（[] 会被误读成"确认无降级"）。
    if cfg.profile.is_some() {
        let _ = crate::rr3_degrade_set(rr3_degrade_json(&boot.degrade));
    }
    // bind/cleanup/accept 走 trait 工厂（ADR-0008）：端点与监听语义上提到 rr-ipc，
    // Conn 类型仍是 UnixStream——conn.rs 的读写循环与 socket 专有门（读超时/shutdown）不动。
    // bind 独占探测（0.0.8d 审计 B-9）：存活实例占用 = 拒绝启动（裂脑禁绝）；
    // 残留死文件 = 探测 connect 失败后清掉再绑，探测与清理合并进 rr3_bind_exclusive。
    let listener = rr3_daemon_bind(cfg)?;

    let shared = Arc::new(Mutex::new(RrdRegistry::new(cfg.restart_base_ms)));
    // G4：装配缺项的模块预降级——注册入表即 Degraded，且不被解除传播捞回
    if !boot.predegraded.is_empty() {
        rr3_lock(&shared).rr3_reg_predegrade(boot.predegraded.clone());
    }
    let topics = Arc::new(topics);

    // 运维计数持久化（0.0.4 批B，Q1/Q2 裁定）：启动加载全表 → `boots` 自增（rrd 自有键，
    // 不走心跳）→ 立即回写一次。路径判定一处（counters.rs）：env RR3_DATA_DIR 优先，
    // cfg(linux) 缺省 /data/rr3，皆无 = 不持久化（日志一行点名，不报错——sim 直跑就是
    // 没有数据分区的，运维账只在内存里也照常用）。
    let counters_path = rr3_daemon_counters(&shared);

    // R26 心跳合并写闸门（0.0.5 增补轮四 批W3）：env 解析一次，全连接共享一份；
    // SIGTERM/SIGINT 强制 flush 在此一并装上——合并写窗口内未落盘的尾部计数
    // 靠关停路径兜底，缺了它 R26 的"最多丢 M 秒"只在断电时成立、正常关停会白丢。
    let counters_gate = Arc::new(Mutex::new(crate::counters::Rr3CountersGate::rr3_new(
        crate::counters::rr3_flush_ms(),
        crate::counters::rr3_flush_max(),
    )));
    rr3_flush_on_term(shared.clone(), counters_path.clone());

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
            counters_path,
            counters_gate,
        },
    )
}
