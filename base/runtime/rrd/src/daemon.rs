//! 监听、accept、连接线程编排（spec §4）。
//!
//! Task 10 加了**唯一的** watchdog 线程（`rr3_spawn_watchdog`）；Task 11b 加了**唯一的**
//! supervisor 线程（`supervisor::rr3_spawn_supervisor`）与 boot 序。除这两条外本文件不养
//! 其它常驻线程——收尸、退避、重启全部串行在那一条上（E-5）。
//! 本任务做的：清残留套接字 → bind → 起 supervisor → 起监视线程 → 投 boot 队列 → accept 循环。
//! 连接账（M-1）：每连接 **2 条线程 + 2 个 fd**，不是一条——accept 处 spawn 的 reader 线程，
//! 加上 `conn.rs` 里为同一条连接 spawn 的 writer 线程与它那份 `try_clone`（评审资源探针实测
//! 一条活跃连接 +2 线程 +2 fd）。Task 10/11 算并发与文件描述符账按这个数。
//! 单次 accept 失败只记日志不带走守护进程；协议错误只结束那一条连接。

use std::os::unix::net::UnixListener;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rr_bus::{RR3_BROADCAST_DST_TASK, RR3_TASK_RRD};
use rr_ipc::{opcode, rr3_body_encode, Rr3Frame, Rr3Header};
use rr_registry::Rr3TopicSet;
use serde_json::json;

use crate::cli::RrdConfig;
use crate::conn::{rr3_conn_serve, Rr3ConnCtx};
use crate::registry::{Rr3Phase, RrdRegistry};
use crate::supervisor::{
    rr3_boot_job_from_manifest, rr3_spawn_supervisor, Rr3SpawnJob, Rr3SuperMsg, Rr3Supervisor,
};
use crate::{rr3_lock, rr3_log};

/// 相对进程起点的单调毫秒：在册表的时钟单位（Task 5"时钟约定"）
pub fn rr3_now_ms(t0: Instant) -> u64 {
    t0.elapsed().as_millis() as u64
}

/// 头里的 `timestamp_ns`：契约规定 CLOCK_REALTIME 纳秒（envelope.schema.json offset 16）
pub fn rr3_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// 唯一的 watchdog 监视线程（spec §5.0"心跳"段）。
///
/// 为什么是一条、不是每连接一条：到期判定要比全表 `last_beat_ms`，而连接线程只看得见
/// 自己那条；每连接一条既分不清"对端不喂狗"与"对端刚注册完还没喂第一颗"，又会把
/// failed 广播放大成 O(连接数)。tick 由 `--watchdog-tick-ms` 决定（生产 200，
/// 集成测试要么 20 要么 1 小时，见 `RR3_QUIET_WATCHDOG_TICK_MS`）。
///
/// 先睡后扫：注册完成那一刻不必立刻扫一遍，第一拍在 t+tick。
/// 每拍为扫描只锁一次（每个到期回报再锁一次做广播）：`rr3_reg_expire` 持锁期间 accept 线程与
/// conn 线程都在等同一把锁，所以锁的粒度就是全表扫描本身（≤ `RR3_REG_MAX_MODULES` = 16 条，有硬上界）。
/// 广播在锁外做（`rr3_reg_fanout` 自己再锁一次）：判死与广播之间不持锁，
/// 避免"持锁 sleep"那种把整个守护卡住的写法。
///
/// 放锁与广播之间有一个窗口：同一身份可在 `Failed` 之后被重新注册（`rr3_reg_add` 允许），
/// 新连接的 `task` 与 `ex.src_task` 不同，于是 `exclude_task` 挡不住它——它会收到一条
/// 关于**自己上一世**的 failed。11b 把重启落地之后这条窗口**确实有人踩**（重启正是"同身份
/// 新 task"的量产现场），但要点是它窄到需要"新实例恰好在判死与 fanout 之间完成注册"：
/// 到期那一路的 fanout 在同一次循环里紧随判死，而重启还要过退避表（E-4）与 `rr3_reg_live`
/// 检查，所以新实例的注册永远排在这条广播之后。真正的修法要给 task 加"世代号"并让
/// `exclude_task` 按世代排除 —— 那是契约级改动，本轮**不做**，只在 `supervisor.rs` 的
/// 已知边界之外把它记成一条未收的账（见实施报告）。
///
/// panic 面：循环体里唯一会 panic 的是 `rr3_lock` 的毒锁 unwrap，与 conn 线程同一口径，
/// 本任务不额外加 catch_unwind。**11b 没有改变这个决定**：重启不在这条线程里做，
/// `sup.rr3_restart(plan)` 只做一次 `mpsc::send`（不 spawn、不睡眠、不 wait），
/// 起进程与退避全部串行在 `supervisor.rs` 那一条线程上（E-5）。
///
/// 到期判死**走记账**（Task 11a 的 registry 收口）：`rr3_reg_expire` 在判死那一拍调
/// `rr3_charge` 给出一条重启计划（`Rr3Expiry.restart`）。11b 起这一路**交出去**：计划投给
/// supervisor，由它按"是不是我起的"决定起还是跳过（E-1）。于是 ledger R-A4 那条"同一身份被记
/// 两次死"的双计隐患**在表上已结清**——被 watchdog 判死后，它的连接后来真断开时走 `conn.rs` 的
/// `rr3_conn_finish` → `rr3_reg_death`，那条有 skip-failed 护栏（返回 `Rr3Death::Silent`），
/// 既不重复广播也不重复记账；它收尸那一路同样有护栏（`rr3_reg_child_exit` 对 failed 条目回
/// `Silent`）。
///
/// **为什么这一路不打"重启计划"日志**：计划的内容（attempt/延迟/entry）在 `conn.rs` 收尾那一路
/// 已经点名过，watchdog 这一路重复打只是把同一件事说两遍；而 A-5③ 的 needle 口径要求
/// "重启计划"这四个字**只有一个来源**，否则 `跳过重启`/`spawn ` 这些判据就要先分辨是哪一路打的。
fn rr3_spawn_watchdog(
    shared: Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    tick_ms: u64,
    sup: Rr3Supervisor,
) {
    thread::spawn(move || {
        // 日志在线程内打：harness 每个用例都等这一行（`support` 的
        // `rr3_spawn_with_overlay`），打在 spawn 之前它就只能证明"走到了这一行"，
        // 打在这里它证明的是"线程真的跑起来过"。文案与格式一字不改（needle 依赖它）。
        rr3_log(&format!("watchdog 启动 tick={tick_ms}ms"));
        let tick = Duration::from_millis(tick_ms);
        loop {
            thread::sleep(tick);
            let expiries = rr3_lock(&shared).rr3_reg_expire(rr3_now_ms(t0));
            for ex in expiries {
                rr3_log(&format!(
                    "watchdog 判死 {} task={} 距上次心跳 {}ms ≥ 超时 {}ms",
                    ex.name, ex.src_task, ex.elapsed_ms, ex.watchdog_ms
                ));
                let frame = rr3_state_frame(&ex.name, Rr3Phase::Failed.as_str(), ex.src_task);
                // 阶段名取自 `Rr3Phase::as_str()`，不写字面量 "failed"：那是第二个真值
                // （as_str 改了而广播没改，`rr3_read_state(.., "failed")` 会替我们抓到，
                // 但抓到的是一条本该不必存在的失败）。
                let n = rr3_lock(&shared).rr3_reg_fanout(&frame, ex.src_task);
                rr3_log(&format!("watchdog 广播 failed 到 {n} 个连接"));
                // D-8 的落点（11b）：到期那一路的记账结果交给 supervisor。
                // `rr3_restart` 只做一次 send，绝不在这条线程里 spawn/睡眠。
                if let Some(p) = ex.restart {
                    sup.rr3_restart(p);
                }
            }
        }
    });
}

/// `rr3-runtime/module/state` 下行广播帧：owner 是内建命名空间，只许 rrd 发（spec §5.0）
pub fn rr3_state_frame(name: &str, phase: &str, src_task: u64) -> Rr3Frame {
    let body = json!({
        "topic": "rr3-runtime/module/state",
        "module": name,
        "phase": phase,
        "src_task": src_task,
    })
    .to_string()
    .into_bytes();
    let payload = rr3_body_encode(rr_ipc::Rr3PayloadType::Notification, &body);
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: RR3_TASK_RRD,
            dst_task: RR3_BROADCAST_DST_TASK,
            payload_len: payload.len() as u32,
        },
        payload,
    }
}

/// 守护主循环：永不返回（除非 bind 失败）。集成测试靠父进程 kill 收尾。
///
/// 线程与投递的次序**有意义**，别重排（三件事各不相同）：
/// 1. 起 supervisor 线程 —— 它是唯一的执行线程，boot 与重启都从它身上过；
/// 2. 起 watchdog —— spec §5.0 要"注册完成前不收心跳也不判活"，而 boot 的第一个子进程
///    可能在同一毫秒内注册完，所以**监视线程必须已在跑**才有东西扫它（brief Step 2 的
///    "先起 watchdog、再起 boot 队列"）；
/// 3. 投 boot 队列 —— 放在 accept 循环之前，否则第一条消息永远没人取。
pub fn rr3_daemon_run(cfg: &RrdConfig, topics: Rr3TopicSet) -> Result<(), String> {
    let t0 = Instant::now();
    if cfg.socket.exists() {
        std::fs::remove_file(&cfg.socket)
            .map_err(|e| format!("清理残留套接字 {}: {e}", cfg.socket.display()))?;
    }
    let listener = UnixListener::bind(&cfg.socket)
        .map_err(|e| format!("绑定 {} 失败: {e}", cfg.socket.display()))?;
    rr3_log(&format!("监听 {}", cfg.socket.display()));

    let shared = Arc::new(Mutex::new(RrdRegistry::new(cfg.restart_base_ms)));
    let topics = Arc::new(topics);

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

    // E-7 启动序：先**全部装载**（含 E-3 的存在性/可执行位门禁，被拒的当场打 `拒绝启动`），
    // 再排序投递。分成两段而不是边读边投：`拒绝启动` 走的是 CLI 顺序、`spawn` 走的是
    // start_priority 顺序，两种序混在日志里就没有可读性可言（先装载后投递让这两段各自连续）。
    // `sort_by_key` 是**稳定**排序：同 `start_priority` 保持 CLI 顺序（E-7 明令不许用
    // `sort_unstable_by_key`，否则"同优先级按 CLI 序"这句话不成立）。
    let mut jobs: Vec<Rr3SpawnJob> = Vec::new();
    for manifest in &cfg.manifests {
        if let Some(job) = rr3_boot_job_from_manifest(manifest) {
            jobs.push(job);
        }
    }
    jobs.sort_by_key(|j| j.facts.start_priority);
    for job in jobs {
        sup.rr3_boot(job);
    }

    for accepted in listener.incoming() {
        let stream = match accepted {
            Ok(s) => s,
            Err(e) => {
                rr3_log(&format!("accept 失败（继续服务）: {e}"));
                continue;
            }
        };
        let ctx = Rr3ConnCtx {
            shared: shared.clone(),
            topics: topics.clone(),
            t0,
            pre_register_timeout_ms: cfg.pre_register_timeout_ms,
            sup: sup.clone(),
        };
        thread::spawn(move || {
            if let Err(e) = rr3_conn_serve(stream, ctx) {
                rr3_log(&format!("连接收尾: {e}"));
            }
        });
    }
    Ok(())
}
