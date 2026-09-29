//! 监听、accept、连接线程编排（spec §4）。
//!
//! Task 10 已加**唯一的** watchdog 线程（`rr3_spawn_watchdog`）；Task 11 在这里加 spawn/重启。
//! 本任务做的：清残留套接字 → bind → 起监视线程 → accept 循环。
//! 连接账（M-1）：每连接 **2 条线程 + 2 个 fd**，不是一条——accept 处 spawn 的 reader 线程，
//! 加上 `conn.rs` 里为同一条连接 spawn 的 writer 线程与它那份 `try_clone`（评审资源探针实测
//! 一条活跃连接 +2 线程 +2 fd）。Task 10/11 算并发与文件描述符账按这个数。
//! 单次 accept 失败只记日志不带走守护进程；协议错误只结束那一条连接。

use std::os::unix::net::UnixListener;
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
/// 关于**自己上一世**的 failed。今天无害（多一条陈旧广播），Task 11 做重启时必须处理
/// （重启正是"同身份新 task"的量产现场）。
///
/// panic 面：循环体里唯一会 panic 的是 `rr3_lock` 的毒锁 unwrap，与 conn 线程同一口径，
/// 本任务不额外加 catch_unwind。Task 11 若把重启做进这条线程，必须重估这个决定
/// （那时循环体里会多出 spawn 与退避睡眠）。
///
/// 到期判死**现在也走记账**（Task 11a 的 registry 收口）：`rr3_reg_expire` 在判死那一拍调
/// `rr3_charge` 给出一条重启计划（`Rr3Expiry.restart`）。但 11a 的 daemon 侧**只收下不使用**
/// （下面那行 `let _ = ex.restart;`）：真正接管它、据它派生重启进程的是 11b 的 supervisor。
/// 于是 ledger R-A4 那条"同一身份被记两次死"的双计隐患**在表上已结清**——被 watchdog 判死后，
/// 它的连接后来真断开时走 `conn.rs` 的 `rr3_conn_finish` → `rr3_reg_death`，那条现在有 skip-failed
/// 护栏（返回 `Rr3Death::Silent`），既不重复广播也不重复记账。
///
/// **为什么 11a 不在这里打"重启计划"日志**：任何含"重启计划"四字的新日志行都会把
/// `e2e_watchdog` 的 W4 当场打死（W4 判据是窗口内 rrd stdout 不出现那四字），而 W4 的改写与
/// "重启计划"上线必须同一次提交成对发生——那是 11b 的活（见 11b 的 A-5③ 裁定）。
fn rr3_spawn_watchdog(shared: Arc<Mutex<RrdRegistry>>, t0: Instant, tick_ms: u64) {
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
                // D-8：到期那一路的记账结果在 11a 只收下不使用——11b 的 supervisor 接管它，
                // 并据它派生重启。此刻绝不打"重启计划"日志（会打死 e2e_watchdog 的 W4）。
                let _ = ex.restart;
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

    rr3_spawn_watchdog(shared.clone(), t0, cfg.watchdog_tick_ms);

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
        };
        thread::spawn(move || {
            if let Err(e) = rr3_conn_serve(stream, ctx) {
                rr3_log(&format!("连接收尾: {e}"));
            }
        });
    }
    Ok(())
}
