//! 时钟、心跳与看门狗节拍：到期判死、广播与降级传播。

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rr_bus::{RR3_BROADCAST_DST_TASK, RR3_TASK_RRD};
use rr_ipc::{opcode, rr3_body_encode, Rr3Frame, Rr3Header};
use serde_json::json;

use crate::registry::{Rr3Expiry, Rr3Phase, RrdRegistry};
use crate::supervisor::Rr3Supervisor;
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
pub(super) fn rr3_spawn_watchdog(
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
        rr3_watchdog_loop(&shared, t0, tick, &sup);
    });
}

/// 看门狗节拍本体：每 tick 判死一拍，逐条到期交广播/重启/降级。
fn rr3_watchdog_loop(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    tick: Duration,
    sup: &Rr3Supervisor,
) {
    loop {
        thread::sleep(tick);
        let expiries = rr3_lock(shared).rr3_reg_expire(rr3_now_ms(t0));
        for ex in expiries {
            rr3_expiry_fan(shared, sup, ex);
        }
    }
}

/// 单条到期判死：点名、广播 failed、交重启计划、降级传播。
fn rr3_expiry_fan(shared: &Arc<Mutex<RrdRegistry>>, sup: &Rr3Supervisor, ex: Rr3Expiry) {
    rr3_log(&format!(
        "watchdog 判死 {} task={} 距上次心跳 {}ms ≥ 超时 {}ms",
        ex.name, ex.src_task, ex.elapsed_ms, ex.watchdog_ms
    ));
    let frame = rr3_state_frame(&ex.name, Rr3Phase::Failed.as_str(), ex.src_task);
    // 阶段名取自 `Rr3Phase::as_str()`，不写字面量 "failed"：那是第二个真值
    // （as_str 改了而广播没改，`rr3_read_state(.., "failed")` 会替我们抓到，
    // 但抓到的是一条本该不必存在的失败）。
    let n = rr3_lock(shared).rr3_reg_fanout(&frame, ex.src_task);
    rr3_log(&format!("watchdog 广播 failed 到 {n} 个连接"));
    // D-8 的落点（11b）：到期那一路的记账结果交给 supervisor。
    // `rr3_restart` 只做一次 send，绝不在这条线程里 spawn/睡眠。
    if let Some(p) = ex.restart {
        sup.rr3_restart(p);
    }
    // ADR-0009 裁定 3：提供者离开 Running（到期判死这一路，死亡已在
    // `rr3_reg_expire` 里落账）→ 降级传播。只置位与广播，不产计划、
    // 不经 supervisor（裁定 4：降级不是崩溃）。
    let degraded = rr3_lock(shared).rr3_reg_degrade(&ex.name);
    rr3_fan_degraded(shared, &ex.name, degraded);
}

/// 降级传播：逐个依赖者广播 degraded（排除项 RR3_TASK_RRD = 不排除，E-9 同一理据）。
fn rr3_fan_degraded(
    shared: &Arc<Mutex<RrdRegistry>>,
    provider: &str,
    degraded: Vec<(String, u64)>,
) {
    for (dep, task) in degraded {
        let n = rr3_lock(shared).rr3_reg_fanout(
            &rr3_state_frame(&dep, Rr3Phase::Degraded.as_str(), task),
            RR3_TASK_RRD,
        );
        rr3_log(&format!(
            "{dep} 依赖断供 → degraded（提供者 {provider} 离开 Running）广播到 {n} 个连接"
        ));
    }
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
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// `rr3-runtime/system/power` 下行广播帧：低电兜底点名（0.0.2 方案 Q4 裁定，
/// D3 随批 append 的内建命名空间主题）。owner 是内建命名空间，只许 rrd 发；
/// `reason` 由调用方给词（"battery-low" 置旗 / "battery-ok" 解除，批N5 件二笔3），
/// percent/voltage_mv 从触发它的 battery 事件体透传（取不到按 0 回缺省）。
pub fn rr3_power_frame(reason: &str, percent: u64, voltage_mv: u64) -> Rr3Frame {
    let body = json!({
        "topic": "rr3-runtime/system/power",
        "reason": reason,
        "percent": percent,
        "voltage_mv": voltage_mv,
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
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}
