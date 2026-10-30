//! 监听、accept、连接线程编排（spec §4）。
//!
//! Task 10 加了**唯一的** watchdog 线程（`rr3_spawn_watchdog`）；Task 11b 加了**唯一的**
//! supervisor 线程（`supervisor::rr3_spawn_supervisor`）与 boot 序。除这两条外本文件不养
//! 其它常驻线程——收尸、退避、重启全部串行在那一条上（E-5）。例外（0.0.5 增补轮四
//! 批W3，R26）：`counters-term-flush` 一条——SIGTERM/SIGINT 强制 flush 的 sleep-and-wake
//! 线程（self-pipe，见 [`rr3_install_term_counters_flush`]），平时不醒，不算常驻负载，
//! 进程账按 +1 线程 +1 fd 记。
//! 本任务做的：清残留套接字 → bind → 起 supervisor → 起监视线程 → 投 boot 队列 → accept 循环。
//! 连接账（M-1）：每连接 **2 条线程 + 2 个 fd**，不是一条——accept 处 spawn 的 reader 线程，
//! 加上 `conn.rs` 里为同一条连接 spawn 的 writer 线程与它那份 `try_clone`（评审资源探针实测
//! 一条活跃连接 +2 线程 +2 fd）。Task 10/11 算并发与文件描述符账按这个数。
//! 单次 accept 失败只记日志不带走守护进程；协议错误只结束那一条连接。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rr_bus::{RR3_BROADCAST_DST_TASK, RR3_TASK_RRD};
use rr_ipc::{
    opcode, rr3_body_encode, Rr3Frame, Rr3Header, Rr3TransportEndpoint, Rr3TransportListener,
    Rr3UdsListener,
};
use rr_registry::{rr3_registry_plan_boot, Rr3DependsFacts, Rr3TopicSet};
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
                // ADR-0009 裁定 3：提供者离开 Running（到期判死这一路，死亡已在
                // `rr3_reg_expire` 里落账）→ 降级传播。只置位与广播，不产计划、
                // 不经 supervisor（裁定 4：降级不是崩溃）。
                let degraded = rr3_lock(&shared).rr3_reg_degrade_after(&ex.name);
                for (dep, task) in degraded {
                    // 排除项是 `RR3_TASK_RRD`（等于不排除，E-9 的同一理据）：依赖者自己
                    // 恰恰是最需要知道"我已降级"的一方——它没有别的途径查自己的相位。
                    let n = rr3_lock(&shared).rr3_reg_fanout(
                        &rr3_state_frame(&dep, Rr3Phase::Degraded.as_str(), task),
                        RR3_TASK_RRD,
                    );
                    rr3_log(&format!(
                        "{dep} 依赖断供 → degraded（提供者 {} 离开 Running）广播到 {n} 个连接",
                        ex.name
                    ));
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
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// `rr3-runtime/system/power` 下行广播帧：低电兜底点名（0.0.2 方案 Q4 裁定，
/// D3 随批 append 的内建命名空间主题）。owner 是内建命名空间，只许 rrd 发；
/// `reason` 由调用方给词（"battery-low" 置旗 / "battery-ok" 解除，批N5 件二笔3），
/// percent/voltage_mv 从触发它的 battery 事件体透传（取不到按 0 回缺省）。
pub fn rr3_system_power_frame(reason: &str, percent: u64, voltage_mv: u64) -> Rr3Frame {
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
    // 装配链（0.0.2 方案 D2，Q3 裁定）：--profile 是装机正门——enabled_slots 决定起谁，
    // 装载/查册/缺格判定/manifest 解析全部在 bind 之前完成（fatal 即非 0 退出，
    // 套接字不留残迹）；--manifest 调试后门维持原路径。两路径**共用**下面的
    // plan_boot 仲裁与 supervisor spawn，不双真值。缺格不是致命项（方案 P2：
    // 缺格只降点名模块、缺格≠拒 boot），逐条点名后由 boot 队列跳过。
    let (boot_manifests, degraded, hal_bindings, axis_state, predegraded) = match &cfg.profile {
        Some(p) => {
            // 格位/角色/器件契约与主题契约同目录（--contracts 运行期落点，T5；roles.toml
            // 是校验①②判据源、devices.toml 是校验⑦-d 的 mate 集判定源、platform/ 是
            // 校验③④⑤⑧的判据前件选册目录，增补轮六 §4.1/§4.1b/§4.6）；接线档按装机
            // 布局由装配单推导（hardware/<形态>/wiring.toml，§4.3——与 modules-root
            // 同一条 <root> 纪律，校验⑦ 判据面）
            let contracts = crate::topics_load::rr3_contracts_dir(cfg.contracts.as_deref());
            let slots = contracts.join("slots.toml");
            let roles = contracts.join("roles.toml");
            let devices = contracts.join("devices.toml");
            let platform = contracts.join("platform");
            let wiring = crate::wiring::rr3_wiring_path_for_profile(p)?;
            let boot = crate::profile::rr3_profile_boot_inputs(
                p, &slots, &roles, &devices, &wiring, &platform,
            )?;
            // 日志环容量一次定格（批N5 件二 F-30）：装配单 [runtime].log_ring_cap——
            // 装载期缺键已在 parse 层拒（缺即红），到这里必是合法正整数。首份胜出
            // （OnceLock，与装配快照同语义）；--manifest 调试路径不读装配单，环按
            // lib.rs 的 512 兜底。点名一行进环本身：运维在 rr3.ctl.logs 里看得到
            // 本机容量是多少。
            if crate::rr3_log_ring_cap_set(boot.log_ring_cap) {
                rr3_log(&format!(
                    "日志环容量定格 {} 行（装配单 [runtime].log_ring_cap）",
                    boot.log_ring_cap
                ));
            }
            for (module, slot) in &boot.missing {
                rr3_log(&format!(
                    "拒绝启动 {module}：缺格 {slot}（装配单 bindings 无此格或探测不到，fail-closed 绝不带病起）"
                ));
            }
            // 校验②（增补轮六 §5，后果 = 降）：缺角色与缺格分开点名——缺格是
            // "bindings 无此格"，缺角色是"格位在位却无 [[pin]] 绑定"，两件事不可混写。
            for (slot, role) in &boot.role_missing {
                rr3_log(&format!(
                    "缺角色 {slot}：角色 {role} 无 [pins] 绑定（校验②——该格缺能力，修装配单 [pins] 后重启解除）"
                ));
            }
            // G4（ADR-0019 修订 1 ③）：装配缺项点名到件，模块照常 spawn 但在册即 Degraded
            for (module, msg) in &boot.assemble_broken {
                rr3_log(&format!(
                    "装配缺项 {module}：{msg}（该模块在册即 Degraded，不致命——修数据后重启 rrd 解除）"
                ));
            }
            // 执行轴状态（批N3 件三）：档案装载点名（坏件逐条 / 多份合法）与缺格/
            // 缺角色分开——注入文本空串 = 不注入，模块侧折保守缺省（§6 复位语义④）
            for note in &boot.axis_notes {
                rr3_log(note);
            }
            let names: BTreeSet<String> = boot.missing.iter().map(|(m, _)| m.clone()).collect();
            let predegraded: BTreeSet<String> = boot
                .assemble_broken
                .iter()
                .map(|(m, _)| m.clone())
                .collect();
            // 装配快照定格（批N4 件九笔4）：boot 链验收通过的底账一次 set 进
            // OnceLock（assembly.rs 单点，日志环同款进程级静态），`rr3.ctl.wiring`
            // 的渲染源。首份胜出语义下重复 boot 不覆盖；--manifest 调试路径
            // （None 臂）无快照，ctl.wiring 如实回不可得。
            if let Some(s) = boot.snapshot.clone() {
                crate::assembly::rr3_assembly_snapshot_set(s);
            }
            // 契约记账快照定格（批N5 件五笔4，Z-4；批N8 件一②扩面，基-40）：boot
            // 链验收通过后对装载消费真值集记账（contracts.rs 单点）——契约四册
            // sha256 记账（Z-4 判据）＋真值集七成员记账（基-40 成员完整性判据，
            // 定位由装载面 rr3_runtime_truth_set_locations 派生），`rr3.ctl.contracts`
            // 的渲染源 + 自检"齐全且 sha 对"的对比锚。此刻七册刚被装载面读过一遍，
            // 记账重读失败 = 文件系统级异常，fail-closed 带因拒 boot（不带病定格）。
            let cs = crate::contracts::rr3_contracts_snapshot_capture(
                p,
                &contracts,
                &wiring,
                &boot.platform_chip,
            )?;
            if crate::contracts::rr3_contracts_snapshot_set(cs) {
                rr3_log("真值集记账定格：契约四册 Z-4 账＋真值集七成员基-40 账（rr3.ctl.contracts 自检判据锚）");
            }
            (
                boot.manifests,
                names,
                boot.hal_bindings,
                boot.axis_state,
                predegraded,
            )
        }
        // None 臂 = **仅 --manifest 调试后门**（基-23）：裸起（既无 --profile 又无
        // --manifest）在 cli parse 处已非 0 退出，走不到这里；后门路径无装配单，
        // hal_bindings/axis_state 空串（spawn 不注入，fail-closed + 应急 env）、
        // 无缺格点名、无装配快照（ctl.wiring 如实回不可得）。
        None => (
            cfg.manifests.clone(),
            BTreeSet::new(),
            String::new(),
            String::new(),
            BTreeSet::new(),
        ),
    };
    // rrd 自身日志落盘（批N5 件二笔2，§17 运维留痕同轨判据）：`<socket>.log/rrd.log`
    // append——与 per-unit 文件同目录同派生单点。每行同步 append、不轮转；下一行
    // 的启动回填把崩溃前的文件尾置回环头，`rr3.ctl.logs` 重启后仍取得到崩溃前的
    // 最后若干行（模块侧批N4 件八已同轨成立，rrd 自身自此同权）。打不开 = 点名
    // 退纯内存，观测通道故障降级、不放大成可用性故障（supervisor stderr 回退
    // inherit 同一条纪律）。位置钉死在装配链 match **之后**：--profile 路径此刻
    // 环容量已定格（回填行数按真容量收）；--manifest 调试路径按 512 兜底收。
    if let Err(e) = crate::rr3_log_sink_set(&cfg.socket) {
        rr3_log(&format!(
            "rrd 日志落盘不可用: {e} —— 运维留痕退纯内存（rr3.ctl.logs 仅本次启动的环）"
        ));
    }
    crate::rr3_log_backfill();
    // bind/cleanup/accept 走 trait 工厂（ADR-0008）：端点与监听语义上提到 rr-ipc，
    // Conn 类型仍是 UnixStream——conn.rs 的读写循环与 socket 专有门（读超时/shutdown）不动。
    Rr3UdsListener::rr3_cleanup(&cfg.socket)
        .map_err(|e| format!("清理残留套接字 {}: {e}", cfg.socket.display()))?;
    let listener = Rr3UdsListener::rr3_bind(&cfg.socket)
        .map_err(|e| format!("绑定 {} 失败: {e}", cfg.socket.display()))?;
    // 访问控制放行（批F5b R12，ADR-0010 增补）：chmod 0o660 + chgrp 属主组，
    // 尽力而为不拒启——详见函数 doc
    rr3_socket_acl_grant(&cfg.socket);
    rr3_log(&format!("监听 {}", cfg.socket.display()));

    let shared = Arc::new(Mutex::new(RrdRegistry::new(cfg.restart_base_ms)));
    // G4：装配缺项的模块预降级——注册入表即 Degraded，且不被解除传播捞回
    if !predegraded.is_empty() {
        shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_reg_predegrade_mark(predegraded);
    }
    let topics = Arc::new(topics);

    // 运维计数持久化（0.0.4 批B，Q1/Q2 裁定）：启动加载全表 → `boots` 自增（rrd 自有键，
    // 不走心跳）→ 立即回写一次。路径判定一处（counters.rs）：env RR3_DATA_DIR 优先，
    // cfg(linux) 缺省 /data/rr3，皆无 = 不持久化（日志一行点名，不报错——sim 直跑就是
    // 没有数据分区的，运维账只在内存里也照常用）。
    let counters_path = crate::counters::rr3_counters_path();
    match &counters_path {
        Some(path) => {
            let (mut table, note) = crate::counters::rr3_counters_load(path);
            if let Some(n) = note {
                rr3_log(&format!("counters 启动加载按空表起账: {n}"));
            }
            *table
                .entry(crate::counters::RR3_COUNTERS_BOOTS_KEY.to_string())
                .or_insert(0) += 1;
            rr3_lock(&shared).rr3_reg_counters_replace(table.clone());
            match crate::counters::rr3_counters_store(path, &table) {
                Ok(()) => rr3_log(&format!(
                    "counters 持久化已启用: {}（boots 自增已回写）",
                    path.display()
                )),
                Err(e) => rr3_log(&format!("counters 回写失败（心跳拍会自然重试）: {e}")),
            }
        }
        None => rr3_log(
            "counters 不持久化（无 RR3_DATA_DIR 且非 linux 缺省面）: 计数仅内存账，断电不保",
        ),
    }

    // R26 心跳合并写闸门（0.0.5 增补轮四 批W3）：env 解析一次，全连接共享一份；
    // SIGTERM/SIGINT 强制 flush 在此一并装上——合并写窗口内未落盘的尾部计数
    // 靠关停路径兜底，缺了它 R26 的"最多丢 M 秒"只在断电时成立、正常关停会白丢。
    let counters_gate = Arc::new(Mutex::new(crate::counters::Rr3CountersGate::rr3_new(
        crate::counters::rr3_counters_flush_interval_ms(),
        crate::counters::rr3_counters_flush_max_pending(),
    )));
    rr3_install_term_counters_flush(shared.clone(), counters_path.clone());

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
    // 再交给 rr-registry 仲裁（ADR-0009），最后按裁定投递。分成几段而不是边读边投：
    // `拒绝启动` 走的是 CLI 顺序、`spawn` 走的是仲裁序，两种序混在日志里就没有可读性
    // 可言（先装载后投递让这两段各自连续）。
    let mut jobs: Vec<Rr3SpawnJob> = Vec::new();
    for manifest in &boot_manifests {
        if let Some(mut job) = rr3_boot_job_from_manifest(manifest) {
            // 物理绑定描述符（ADR-0018 裁定 4、批D④）：装配链解析出的 [hardware]
            // 文本随每个 job 透传，supervisor spawn 时注入 RR3_HAL_BINDINGS；
            // --manifest 调试后门无装配单 → 空串（不注入，fail-closed + 应急 env）。
            // 执行轴状态（批N3 件三，件五消歧）同位透传：instance 与轴读数
            // （RR3_HAL_PERIPH_AXIS，缺省契约常量）相等的恰一份合法档案的投影（含
            // 行程三键），spawn 时注入 RR3_ACT_AXIS_STATE；空串不注入（模块侧折保守
            // 缺省）。
            job.hal_bindings = hal_bindings.clone();
            job.axis_state = axis_state.clone();
            jobs.push(job);
        }
    }
    // ADR-0009 裁定 1/2：装载全集交给 rr-registry 仲裁——rrd 只**执行**裁定，不 reimplement：
    // 环 / 名重复 → `fatal` 非空，拒整个 boot（先逐条点名到 stdout 日志，再以 Err 退出，
    // main 据此非 0 退出）；不可满足 → `rejected` 点名模块照既有口径 `拒绝启动`（不吃预算），
    // 其余模块照常；`order` = 拓扑序为主键、层内 (start_priority, name) 确定性次键——
    // 旧的"start_priority 全局稳定升序"从主键降级为次键，不再直接排 boot 队列。
    let arb: Vec<Rr3DependsFacts> = jobs
        .iter()
        .map(|j| Rr3DependsFacts {
            name: j.facts.name.clone(),
            provides: j.facts.provides.clone(),
            depends_required: j.facts.depends_required.clone(),
            start_priority: j.facts.start_priority,
        })
        .collect();
    let plan = rr3_registry_plan_boot(&arb);
    if !plan.fatal.is_empty() {
        for e in &plan.fatal {
            rr3_log(&format!("拒绝 boot：{e}"));
        }
        return Err(format!(
            "拒绝进入 boot 阶段（ADR-0009 裁定 2，环拒整个 boot）: {}",
            plan.fatal.join("; ")
        ));
    }
    // 提供者表（裁定 1/3 的运行期账本）：boot 全集一次性写入，**含被拒模块**——
    // 它们的 provides 仍计入可满足性宇宙（`plan_boot` 的同一口径），运行期不重排。
    // 降级传播只认这张表：不在表里的能力按 base 内建处理（永远在位）。
    let mut providers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for j in &jobs {
        for cap in &j.facts.provides {
            providers
                .entry(cap.clone())
                .or_default()
                .insert(j.facts.name.clone());
        }
    }
    rr3_lock(&shared).rr3_reg_set_providers(providers);
    for name in &plan.rejected {
        rr3_log(&format!(
            "拒绝启动 {name}：required 能力不可满足（base 内建与 boot 全集 provides 均无，ADR-0009 裁定 2）"
        ));
    }
    let mut by_name: BTreeMap<String, Rr3SpawnJob> = jobs
        .into_iter()
        .map(|j| (j.facts.name.clone(), j))
        .collect();
    for name in &plan.order {
        // 缺格模块不进 boot 队列（拒绝启动 已由装配链在 bind 前点名）；它的 provides
        // 仍在上面的 providers 表里——可满足性宇宙完整，下游不连坐（ADR-0009 同款语义）。
        if degraded.contains(name) {
            continue;
        }
        if let Some(job) = by_name.remove(name) {
            sup.rr3_boot(job);
        }
    }

    loop {
        let stream = match listener.rr3_accept() {
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
            counters_path: counters_path.clone(),
            counters_gate: counters_gate.clone(),
        };
        thread::spawn(move || {
            if let Err(e) = rr3_conn_serve(stream, ctx) {
                rr3_log(&format!("连接收尾: {e}"));
            }
        });
    }
}

/// ctl socket 的属主组（0.0.6 批F5b R12）：postinst 建系统组，运维用户由
/// 装机现场入组（deb 不该动用户）——socket 权限位因此成为 ADR-0010 裁定 2
/// "套接字文件权限就是访问控制"的落地值。
const RR3_CTL_SOCKET_GROUP: &str = "rockrobot3";

// ── R26 关停强制 flush（0.0.5 增补轮四 批W3）────────────────────────────────
//
// self-pipe 信号面：`signal(2)` 的处理器里只做 async-signal-safe 的一件事——
// 往管道写一个字节；快照 / 写盘 / 日志全部落在 `counters-term-flush` 普通线程
// （信号处理器里碰 Mutex/File 都是未定义行为，不能省这一层）。两个 libc 函数
// 手写 extern 壳不引依赖（同 rr-hal chmod/getgrnam 先例；原 rr-ipc shm 六函数
// 先例已随 L-32 摘面移除，0.0.8 批N19）。

extern "C" {
    fn signal(signum: i32, handler: usize) -> usize;
    // buf 用 POSIX 原型 `const void *`（c_void）：与 std 运行时同名符号签名一致，
    // 否则触发 rustc suspicious_runtime_symbol_definitions 警告（CI -D warnings 红）
    fn write(fd: i32, buf: *const std::ffi::c_void, count: usize) -> isize;
}

/// POSIX 标准信号号：SIGINT=2、SIGTERM=15
const RR3_SIGINT: i32 = 2;
const RR3_SIGTERM: i32 = 15;

/// self-pipe 发送端裸 fd（处理器读的唯一状态；-1 = 未安装）
static RR3_TERM_PIPE_FD: AtomicI32 = AtomicI32::new(-1);

/// 信号处理器：写一字节即返回。 errno 不保存（best-effort 唤醒，失手也只影响
/// 这一拍 flush，进程随后的默认退出路径照走）。
extern "C" fn rr3_term_pipe_wake(_sig: i32) {
    let fd = RR3_TERM_PIPE_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let one: u8 = 1;
        unsafe {
            write(fd, &one as *const u8 as *const std::ffi::c_void, 1);
        }
    }
}

/// 安装 SIGTERM/SIGINT 强制 flush（R26）：合并写窗口内未落盘的尾部计数在关停
/// 路径一次性写全（systemctl stop / reboot 的 cgroup SIGTERM、Ctrl-C 都走这里）。
/// flush 完成 `exit(0)`——装了处理器信号就不再杀进程，必须自己退；flush 失败
/// 只落日志不挡关停（运维账不许挡产品路，与启动装载同一口径）。未装上（pipe
/// 失败/线程起不来）留痕跳过，SIGTERM 回落默认杀进程语义，不致命。
fn rr3_install_term_counters_flush(shared: Arc<Mutex<RrdRegistry>>, path: Option<PathBuf>) {
    use std::io::Read;
    use std::os::unix::io::IntoRawFd;
    let (tx, rx) = match std::os::unix::net::UnixStream::pair() {
        Ok(pair) => pair,
        Err(e) => {
            rr3_log(&format!(
                "counters 关停 flush 未装上（self-pipe 失败）: {e}"
            ));
            return;
        }
    };
    // 发送端交给处理器：取裸 fd 后刻意不 close（进程生命期一根管，泄漏即设计）
    RR3_TERM_PIPE_FD.store(tx.into_raw_fd(), Ordering::Relaxed);
    unsafe {
        signal(
            RR3_SIGTERM,
            rr3_term_pipe_wake as extern "C" fn(i32) as usize,
        );
        signal(
            RR3_SIGINT,
            rr3_term_pipe_wake as extern "C" fn(i32) as usize,
        );
    }
    let spawned = thread::Builder::new()
        .name("counters-term-flush".into())
        .spawn(move || {
            let mut rx = rx;
            let mut one = [0u8; 1];
            // 平时睡在 read；醒来一次 = 一个 TERM/INT，flush 后即关停（exit 不返回，
            // 故不是循环——clippy never_loop）。EOF = 发送端随进程没了，线程自然收场
            if matches!(rx.read(&mut one), Ok(1)) {
                let snap = rr3_lock(&shared).rr3_reg_counters_snapshot();
                match path
                    .as_deref()
                    .map(|p| crate::counters::rr3_counters_store(p, &snap))
                {
                    Some(Ok(())) => rr3_log("counters 关停 flush 已落盘（SIGTERM/SIGINT）"),
                    Some(Err(e)) => rr3_log(&format!("counters 关停 flush 失败（不挡关停）: {e}")),
                    None => rr3_log("counters 关停 flush 跳过（未启用持久化）"),
                }
                std::process::exit(0);
            }
        });
    if let Err(e) = spawned {
        rr3_log(&format!("counters 关停 flush 未装上（线程起不来）: {e}"));
    }
}

/// ctl socket 访问控制放行（0.0.6 批F5b R12，ADR-0010 权限模型增补）：bind 后
/// `chmod 0o660`（owner + 属主组可读写，other 无份——修真板首发现敞口
/// `srwxr-xr-x root:root` 非组用户连不上）+ `chgrp rockrobot3`（组不存在时
/// 留痕跳过，chmod 位仍生效——紧权限的敞口比守护起不来轻得多，**失败不拒启**）。
/// 放行是尽力而为的部署面语义：sim 直跑/CI 全 root 无感（root 恒可连）。
///
/// libc 仅声明壳（`chmod`/`getgrnam`/`chown`，musl 静态零额外 C 依赖，R13）；
/// errno 逐调用取出（连续调用互不覆盖）。
fn rr3_socket_acl_grant(path: &std::path::Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(cpath) = CString::new(path.as_os_str().as_bytes()) else {
        rr3_log("socket 放行跳过：路径含 NUL（不可能的缺省值）");
        return;
    };
    // 1) chmod 0o660：other 位清掉，属主组位落上
    let rc = unsafe { libc::chmod(cpath.as_ptr(), 0o660) };
    if rc != 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        rr3_log(&format!(
            "socket 放行 chmod 失败（errno {errno}，保留 umask 权限如旧）"
        ));
        return; // chgrp 无从谈起，权限面交回调用方日志
    }
    // 2) chgrp 到属主组：组不在位（装机未建组/开发机）→ 留痕跳过不视为错
    let Ok(cgroup) = CString::new(RR3_CTL_SOCKET_GROUP) else {
        return;
    };
    let gr = unsafe { libc::getgrnam(cgroup.as_ptr()) };
    if gr.is_null() {
        rr3_log(&format!(
            "socket 属主组 {RR3_CTL_SOCKET_GROUP} 不在位（postinst 未跑/开发机），保留 root:root——组内用户暂连不上"
        ));
        return;
    }
    let gid = unsafe { (*gr).gr_gid };
    let uid = libc::uid_t::MAX; // chown 不改属主（uid_t 无符号，MAX 即 POSIX 的 (uid_t)-1）
    let rc = unsafe { libc::chown(cpath.as_ptr(), uid, gid) };
    if rc != 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        rr3_log(&format!(
            "socket 放行 chgrp 失败（errno {errno}，chmod 0o660 已生效）"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_ipc::{rr3_body_split, Rr3PayloadType};

    /// 低电兜底点名帧的形状钉死（0.0.2 Q4）：Notification、src=rrd、dst=广播、
    /// 体恰四字段（topic/reason/percent/voltage_mv），值从触发事件透传。e2e 的旁观者
    /// 接入时首拍已过（兜底在模块注册后一拍内触发，e2e_power.rs 文件头的时序前提），
    /// 帧级形状只有这里能钉——e2e 钉日志侧的触发与拦截，两半拼成完整证据链。
    /// reason 两值各钉（批N5 件二笔3）：置旗 battery-low / 解除 battery-ok。
    #[test]
    fn system_power_frame_shape_is_pinned() {
        let f = rr3_system_power_frame("battery-low", 5, 3250);
        assert_eq!(f.header.src_task, RR3_TASK_RRD, "点名是 rrd 自己发的");
        assert_eq!(f.header.dst_task, RR3_BROADCAST_DST_TASK, "全量扇出");
        let (ty, body) = rr3_body_split(&f.payload).expect("前缀合法");
        assert_eq!(ty, Rr3PayloadType::Notification, "点名走通知不是应答");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("体是 JSON");
        assert_eq!(v["topic"], "rr3-runtime/system/power");
        assert_eq!(v["reason"], "battery-low");
        assert_eq!(v["percent"], 5, "透传触发事件的 percent");
        assert_eq!(v["voltage_mv"], 3250, "透传触发事件的 voltage_mv");
        assert_eq!(
            v.as_object().map(|o| o.len()),
            Some(4),
            "体恰四字段，不得私加: {v}"
        );
        // 解除点名同形（批N5 件二笔3）：reason 换词、其余字段同账
        let f = rr3_system_power_frame("battery-ok", 80, 3900);
        let (_, body) = rr3_body_split(&f.payload).expect("前缀合法");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("体是 JSON");
        assert_eq!(v["reason"], "battery-ok", "解除点名 reason=battery-ok");
        assert_eq!(v["percent"], 80);
        assert_eq!(v["voltage_mv"], 3900);
        assert_eq!(
            v.as_object().map(|o| o.len()),
            Some(4),
            "解除点名同恰四字段"
        );
    }

    /// socket 放行落位（批F5b R12）：temp 树真 bind 一个 uds → grant → 权限位
    /// 0o660 断言。chgrp 面：开发机/CI 无 rockrobot3 组 → 走"组不在位留痕"路径
    /// （属主不动），chmod 位仍成立——两环境测试都绿；板上组在位时 chown 生效，
    /// 属主组断言归板上 cargo test 面，此处不断言 gid 以免开发机红。
    #[test]
    fn socket_acl_grant_sets_group_rw_mode() {
        let dir = std::env::temp_dir().join(format!("rr3-rrd-acl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("acl.sock");
        let _ = std::fs::remove_file(&sock);
        let listener = Rr3UdsListener::rr3_bind(&sock).unwrap();
        rr3_socket_acl_grant(&sock);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&sock).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o660,
            "chmod 0o660 落位（组在位与否不影响 mode 位）"
        );
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
