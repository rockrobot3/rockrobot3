// SPDX-License-Identifier: Apache-2.0
//! 单连接服务：本线程 reader + 一条 writer 线程。
//!
//! 本文件留共享件（帧构造/应答/写出口/`Rr3Flow`/拒绝出口）、入站路由 `rr3_serve_frame`、
//! 上行方法表 `rr3_serve_request`、收尾与主流程；协议分面各自成子模块：`conn/handshake.rs`
//! （注册）、`ctl.rs`（控制旁支）、`publish.rs`（业务发布+低电兜底）、`beat.rs`（心跳与
//! 升级）、`route.rs`（`rr3.call` 与应答回程）、`read.rs`（读侧循环与 ADR-0025 静默上界）。
//! 协议面：首帧必须是 `dst=RR3_TASK_RRD` 的 `rr3.register`，否则断开；`src_task` 由 rrd
//! 分配并回带，其后上行帧必须相等（用户态不得自设），伪造帧只拒该帧不断连；注册前不收
//! 心跳、不判活，未注册连接由读超时收掉，注册后由 2× 看门狗窗口静默上界回收（判死权归
//! 看门狗）。`Event` 过三层归属检查（命名空间冒充 → 前缀/owner 越权 → 主题未登记）后转发
//! 给除发布者外的在册连接，头改写为发布者 task + `dst=0`；`Response` 是 `rr3.call` 回程：
//! trace_id = 调用方 task，body 原样、dst 定向回投。
//! 写面：reader 写应答、writer 写广播，同一 socket 的 `try_clone` + 一把
//! `Mutex<UnixStream>`；一帧一次 `write_all` → 整帧原子，但应答与广播顺序不保证。

use std::io::ErrorKind;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rr_bus::RR3_TASK_RRD;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3Header, Rr3PayloadType, Rr3StatusCode,
};
use rr_registry::{rr3_manifest_chk, Rr3TopicSet};
use serde_json::{json, Value};

use crate::daemon::{rr3_now_ms, rr3_now_ns, rr3_state_frame};
use crate::manifest::{rr3_manifest_abs, rr3_manifest_parse, Rr3ManifestFacts};
use crate::registry::{
    rr3_phase_for_exit, Rr3Beat, Rr3Death, Rr3FanFrame, Rr3Phase, Rr3RestartPlan, RrdRegistry,
    RR3_FAN_CAP,
};
use crate::rpc::{
    rr3_rpc_build_error, rr3_rpc_build_result, rr3_rpc_call_params, rr3_rpc_counters,
    rr3_rpc_deny_status, rr3_rpc_manifest, rr3_rpc_parse, rr3_rpc_reg_status, rr3_rpc_topic_ref,
    Rr3RpcMethod, Rr3RpcRequest,
};
use crate::supervisor::Rr3Supervisor;
use crate::{rr3_lock, rr3_log, rr3_log_query};

mod beat;
mod ctl;
mod handshake;
mod publish;
mod read;
mod reply;
mod route;
use reply::{
    rr3_best_effort_id, rr3_out_frame, rr3_refuse, rr3_reply_err, rr3_reply_ok, rr3_write, Rr3Flow,
};

/// writer 线程单帧写超时（0.0.8b 审计 B-5）：对端不收时 5s 内必须显形——
/// 超时即写失败，writer 退出、通道 drop、fanout 走 Disconnected 回收。
/// 取 5s 与面板/总线读超时同档：正常 Unix socket 写是 µs 级，5s 不可达 = 真卡死。
const RR3_FAN_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// 一条已注册连接的身份证
struct Rr3Peer {
    task: u64,
    name: String,
}

/// daemon 每接受一个连接构造一份
pub struct Rr3ConnCtx {
    pub shared: Arc<Mutex<RrdRegistry>>,
    pub topics: Arc<Rr3TopicSet>,
    pub t0: Instant,
    pub pre_register_timeout_ms: u64,
    /// 重启计划的**唯一去路**（`supervisor.rs`）。conn 只负责"要不要投"，
    /// "起不起得来"由 supervisor 按所有权裁定（E-1）——收尾这一路绝不自己 `Command::spawn`。
    pub sup: Rr3Supervisor,
    /// 运维计数落盘投递端（0.0.4 批B Q2；续轮十三改为执行面分离）：daemon 启动
    /// 判定一次，`None` = 未启用持久化或落盘线程起不来（心跳聚合照常进内存账，
    /// 只是断电不保）。心跳归并后按 R26 闸门**只投非阻塞信号**，快照与 fsync
    /// 在 `rrd-ctr-store` 线程——reader 线程不再被磁盘抖动阻塞。
    pub ctr_store: Option<std::sync::mpsc::SyncSender<()>>,
    /// 运维计数合并写闸门（R26，批W3）：daemon 造一份、全连接共享（`Arc`）——
    /// 每条连接各持一份等于把 M 窗口放大 N 连接倍。
    pub counters_gate: Arc<Mutex<crate::counters::Rr3CountersGate>>,
}

impl Rr3ConnCtx {
    pub fn rr3_ctx_now_ms(&self) -> u64 {
        rr3_now_ms(self.t0)
    }
}

/// 写超时（对端 5s 不收）累计：慢消费者点名的一半——满队列丢帧记 `fan_dropped`，
/// 写不动记这里。两者都在稳态下应为 0（续轮二十五判据）。
static RR3_WRITE_TIMEOUTS: AtomicU64 = AtomicU64::new(0);

/// 写超时累计读口（`ctl.status.fan_write_timeout`）
pub fn rr3_wr_timeouts() -> u64 {
    RR3_WRITE_TIMEOUTS.load(Ordering::Relaxed)
}

/// 处理注册后的入站帧。`Err` = 协议违规，该连接按崩溃收尾。
fn rr3_serve_frame(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    frame: &Rr3Frame,
) -> Result<Rr3Flow, String> {
    if frame.header.dst_task != RR3_TASK_RRD {
        return Err(format!(
            "上行 dst_task 必须为 {RR3_TASK_RRD}，收到 {}",
            frame.header.dst_task
        ));
    }
    let (ty, body) =
        rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if frame.header.src_task != me.task {
        // AOS-STD-IPC-024：src_task 由 rrd 分配，用户态不得自设。
        // 拒这一帧但不断连——一次笔误不该让整个进程掉线（spec §4"rrd 不崩"的连接版）。
        rr3_reply_err(
            out,
            me,
            rr3_best_effort_id(&body),
            Rr3StatusCode::Invalid,
            &format!(
                "src_task 由 rrd 分配，本连接分配值 {}，收到 {}",
                me.task, frame.header.src_task
            ),
        );
        return Ok(Rr3Flow::Keep);
    }
    rr3_serve_payload(out, ctx, me, frame, ty, &body)
}

/// 上行 payload 类型分派（校验已过）：Request 走 RPC、Event 走发布、
/// Response 走 `rr3.call` 应答回路；其余一律拒帧不断连。
fn rr3_serve_payload(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    frame: &Rr3Frame,
    ty: Rr3PayloadType,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    match ty {
        Rr3PayloadType::Request => rr3_serve_request(out, ctx, me, body),
        Rr3PayloadType::Event => publish::rr3_publish(out, ctx, me, body),
        // M3：上行 Response 是 rr3.call 的应答回路（trace_id 改道回调用方）
        Rr3PayloadType::Response => route::rr3_route_response(out, ctx, me, frame, body),
        // 拒是默认值，放行才需要理由：Request/Event/Response 之外的上行（Notification
        // 冒充上行）仍然一律拒帧不断连。
        other => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(body),
                Rr3StatusCode::Invalid,
                &format!("上行不支持 payload 类型 {other:?}（业务发布请用 Event）"),
            );
            Ok(Rr3Flow::Keep)
        }
    }
}

/// 三个上行方法。RPC 解析失败一律回 Invalid——Task 6 已保证解析器不产别的码，
/// 这里不重建 id→code 的映射，避免第二套真值。
/// 上行解析（自 [`rr3_serve_request`] 抽出）：坏帧回 Invalid 并 Keep。
/// None = 已应答，调用方直接按 Keep 收尾。
fn rr3_req_or_reply(
    out: &Arc<Mutex<UnixStream>>,
    me: &Rr3Peer,
    body: &[u8],
) -> Option<Rr3RpcRequest> {
    rr3_rpc_parse(body)
        .map_err(|(_, msg)| {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(body),
                Rr3StatusCode::Invalid,
                &msg,
            )
        })
        .ok()
}

/// 已注册连接的错向请求（重复注册 / rr3.ctl.*）：回错误帧、连接保持。
/// 拒绝口径按方法显式选择，不随调用点漂移。
fn rr3_reply_misroute(
    out: &Arc<Mutex<UnixStream>>,
    me: &Rr3Peer,
    id: u64,
    method: Rr3RpcMethod,
) -> Rr3Flow {
    let (status, msg) = match method {
        Rr3RpcMethod::Register => (
            Rr3StatusCode::Invalid,
            "该连接已注册，拒绝重复 rr3.register",
        ),
        _ => (
            Rr3StatusCode::Permission,
            "rr3.ctl.* 只对未注册的控制连接开放，模块请走 rr3.register / rr3.heartbeat",
        ),
    };
    rr3_reply_err(out, me, id, status, msg);
    Rr3Flow::Keep
}

fn rr3_serve_request(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    let Some(req) = rr3_req_or_reply(out, me, body) else {
        return Ok(Rr3Flow::Keep);
    };
    match req.method {
        Rr3RpcMethod::Register => Ok(rr3_reply_misroute(out, me, req.id, req.method)),
        // ADR-0010：rr3.ctl.* 只对未注册的控制连接开放（握手分流）；已注册模块连接
        // 上行一律拒——回错误帧不断连，与 src_task 伪造同一口径（一次笔误不掉线）。
        // 显式 OR 臂而非 is_ctl 守卫：带守卫的臂不计入穷尽性，新增方法时这里必须表态。
        Rr3RpcMethod::CtlStatus
        | Rr3RpcMethod::CtlHealth
        | Rr3RpcMethod::CtlLogs
        | Rr3RpcMethod::CtlOta
        | Rr3RpcMethod::CtlCounters
        | Rr3RpcMethod::CtlReboot
        | Rr3RpcMethod::CtlWiring
        | Rr3RpcMethod::CtlContracts
        | Rr3RpcMethod::CtlDomainSet => Ok(rr3_reply_misroute(out, me, req.id, req.method)),
        Rr3RpcMethod::Heartbeat => beat::rr3_beat_handle(out, ctx, me, req.id, &req.params),
        Rr3RpcMethod::Unregister => {
            // 主动退场 ≠ 崩溃：直接移出在册表，不走 restart_policy。
            // 广播留给收尾处，保证"一次退出只有一条 state"。
            let removed = rr3_lock(&ctx.shared).rr3_reg_remove(&me.name);
            rr3_reply_ok(out, me, req.id, &json!({"removed": removed}));
            Ok(Rr3Flow::QuitClean)
        }
        Rr3RpcMethod::Call => route::rr3_route_call(out, ctx, me, req.id, &req.params, body),
    }
}

/// 收尾：死亡记账（决定要不要重启）+ **至多**一条 state 广播 + 把计划交出去。
///
/// 11b 之后"三条公开路"的分工是：本函数（conn 见 EOF/unregister）、watchdog 到期
/// （`daemon.rs`）、子进程退出码（`supervisor.rs` 的 `rr3_reap`）。三条各自只发**自己那一条**
/// state，护栏都在 registry：`rr3_reg_death` 对已 failed 的条目回 `Silent`、对 owned 的回
/// `DeferredToExitStatus`，`rr3_reg_child_exit` 对已 failed 的同样回 `Silent`——
/// 所以"一次死亡被记两次 / 广播两次"在表上不可能（ledger R-A4 结清点，线上由
/// `watchdog_fail_state` 与 `e2e_spawn_restart` 的 S2 各自钉住一半）。
///
/// 另两处 state 广播点（`registered` 与 `running`）不走本函数：它们与死亡无关。
fn rr3_conn_finish(ctx: &Rr3ConnCtx, me: &Rr3Peer, clean_exit: bool, reason: &str) {
    // 死亡记账的结果必须先绑到 `let`：若把 `rr3_lock(..).rr3_reg_death(..)` 直接写进 `match` 的
    // 被匹配表达式，返回的 `MutexGuard` 临时量会活到整个 `match` 语句结束（Rust 的临时量寿命
    // 规则： scrutinee 位置的临时量延寿到所属语句收尾），于是 `Broadcast` 分支里再 `rr3_lock`
    // 同一把在册表锁就成了同线程重入自死锁（std Mutex 不可重入）——收尾永远走不到 fanout 与
    // 日志，e2e 的三条 EOF/退场用例集体 3s 超时。绑到 `let` 后守卫在 `;` 处即释放。
    let death = rr3_lock(&ctx.shared).rr3_reg_death(&me.name, clean_exit, ctx.rr3_ctx_now_ms());
    match death {
        // owned（本守护起的进程）：EOF 不改相位/不广播/不记账，三件事一起让给退出码那一路
        //（`supervisor.rs::rr3_reap`）。这里必须留一行痕迹：否则运维只看日志会以为这条连接
        // 什么都没发生，而"没发生"和"让给了别人"是两件不同的话。
        // 刻意不写"退出"两字：`rr3_assert_no_log("退出")` 那类否定 needle 测的是 EOF 收尾文案，
        // 这一路今天只在传了 `--manifest` 的用例里可达（见 A-5③ 第 3 条）。
        Rr3Death::DeferredToExitStatus => {
            rr3_log(&format!(
                "{} 连接断开（{reason}），终态交给子进程退出码那一路",
                me.name
            ));
        }
        // 已被 watchdog 判死：终态广播与重启预算都归 watchdog 那一拍，收尾不得双计（R-A4）。
        // 同样不打计划、不投 supervisor——那份计划到期时已经投过一次。
        Rr3Death::Silent => {
            rr3_log(&format!(
                "{} 连接断开（{reason}），不重复广播：终态已由到期扫描记过",
                me.name
            ));
        }
        // 刚由 live 转终态，或 unregister 后条目已摘（plan=None）：调用方广播恰好一条 state。
        Rr3Death::Broadcast { plan } => {
            rr3_finish_broadcast(ctx, me, clean_exit, reason, plan);
        }
    }
}

/// 终态广播 + 重启计划 + 依赖降级传播（`Broadcast` 分支；一次死亡恰好传播一次）。
fn rr3_finish_broadcast(
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    clean_exit: bool,
    reason: &str,
    plan: Option<Rr3RestartPlan>,
) {
    // D-7：死因→phase 的字面量只在 registry 一处，conn 不再各写各的。
    let phase = rr3_phase_for_exit(clean_exit);
    // E-9：这里排 `RR3_TASK_RRD` 等于不排除（发起者自己也收到关于自己的终态）；
    // supervisor 收尸那一路排的是 `crashed.src_task`。两种口径是**已知**的第三/第四种
    // 口径，归 Task 13 的 A-3 文档化，本任务不"统一"——统一成哪一种都没有判据支持。
    let n = crate::registry::rr3_reg_with(&ctx.shared, |g| {
        g.rr3_reg_fanout(rr3_state_frame(&me.name, phase, me.task), RR3_TASK_RRD)
    });
    match plan {
        Some(p) => {
            rr3_log(&format!(
                "{} 退出（{reason}）→ 重启计划 attempt={} delay_ms={} entry={}（非本守护起的模块不 spawn）",
                p.name, p.attempt, p.delay_ms, p.entry
            ));
            // 投出去就完事：起不起由 supervisor 按 `jobs`（"我能起谁"的全集）裁定（E-1）。
            // 顺序要紧：日志在投之前——`rr3_restart` 只做一次 send，但把"计划是什么"
            // 钉在"谁拿到了它"前面，读日志时才不会先看到结果再找原因。
            ctx.sup.rr3_restart(p);
        }
        None => rr3_log(&format!(
            "{} 退出（{reason}），无重启，广播 {phase} 到 {n} 个连接",
            me.name
        )),
    }
    // ADR-0009 裁定 3：本路刚记完一条死亡/注销（EOF 或 unregister），若死者
    // 是某能力在 boot 全集里的提供者且再无人在位 → 下游逐个置 Degraded 并广播。
    // 只置位与广播，不记账不出计划（裁定 4）。Deferred/Silent 两路不在这里传播：
    // 死亡分别由退出码那一路（supervisor）与到期那一路（watchdog）记，各自的
    // 落点负责各自的传播，一次死亡恰好传播一次。
    let degraded = rr3_lock(&ctx.shared).rr3_reg_degrade(&me.name);
    for (dep, task) in degraded {
        let n = crate::registry::rr3_reg_with(&ctx.shared, |g| {
            g.rr3_reg_fanout(
                rr3_state_frame(&dep, Rr3Phase::Degraded.as_str(), task),
                task,
            )
        });
        rr3_log(&format!(
            "{dep} 依赖断供 → degraded（提供者 {} 离开 Running）广播到 {n} 个连接",
            me.name
        ));
    }
}

/// 广播写线程：从有界队列取帧，写失败即退出（连接回收随通道 drop 走 Disconnected）。
fn rr3_conn_writer(
    out: &Arc<Mutex<UnixStream>>,
    fan_rx: mpsc::Receiver<Rr3FanFrame>,
) -> JoinHandle<()> {
    let out = out.clone();
    thread::spawn(move || {
        for frame in fan_rx.iter() {
            if !rr3_write(&out, &frame) {
                break;
            }
        }
    })
}

/// 服务一条连接。返回 `Err` 只描述**这一条**连接的断开原因；守护进程与别的连接不受影响。
pub fn rr3_conn_serve(stream: UnixStream, ctx: Rr3ConnCtx) -> Result<(), String> {
    let out: Arc<Mutex<UnixStream>> = Arc::new(Mutex::new(
        stream
            .try_clone()
            .map_err(|e| format!("try_clone 失败: {e}"))?,
    ));
    // 写超时（0.0.8b 审计 B-5）：对端不收时 write 不得无限期占着 writer 线程与
    // out 锁——5s 写不进 = 对端判死，writer 退出、通道 drop、fanout 走 Disconnected
    // 回收。缺了它，一个卡死的订阅者能永久占一条 writer 线程。
    rr3_lock(&out)
        .set_write_timeout(Some(RR3_FAN_WRITE_TIMEOUT))
        .map_err(|e| format!("set_write_timeout 失败: {e}"))?;
    // 有界收件队列（RR3_FAN_CAP）：满侧策略见 registry.rs fanout doc——僵尸订阅者
    // 的积压被封顶在"上界 × 帧大小"，不再无界吃到 rrd 被 OOM 杀。
    let (fan_tx, fan_rx) = mpsc::sync_channel::<Rr3FanFrame>(RR3_FAN_CAP);
    let writer: JoinHandle<()> = rr3_conn_writer(&out, fan_rx);

    // 注册前设读超时：连上却不注册的客户端不能永久占着线程（spec §5.0）
    stream
        .set_read_timeout(Some(Duration::from_millis(
            ctx.pre_register_timeout_ms.max(1),
        )))
        .map_err(|e| format!("set_read_timeout 失败: {e}"))?;
    let mut reader = stream;
    let me = handshake::rr3_handshake(&mut reader, &out, &ctx, fan_tx)?;
    let idle_ms = read::rr3_conn_idle_set(&reader, &ctx.shared, &me.name)?;

    let outcome = read::rr3_read_loop(&out, &ctx, &me, &mut reader, idle_ms);

    let clean_exit = outcome.is_ok();
    let reason = match &outcome {
        Ok(_) => "干净收尾".to_string(),
        Err(e) => e.clone(),
    };
    rr3_conn_finish(&ctx, &me, clean_exit, &reason);
    // 通道收口（0.0.8b 审计 B-5 落地，原 Task 10 carry）：条目随注册表释放 →
    // 最后一个 Sender drop → writer 的 iter() 自然结束。僵尸在册条目（注册后不再
    // 读 socket 的模块）由两路兜住：① 有界队列——writer 阻塞在写时积压封顶
    // RR3_FAN_CAP 帧，fanout 满侧**丢帧但保留连接**（本轮审计修正：置 tx=None 会
    // 让心跳仍通的模块永久失聪）；② 写超时——writer 5s 写不进即退出、通道 drop，
    // fanout 走 Disconnected 才置 tx=None。两条路都不再无界吃内存。
    let _ = writer.join();
    let _ = reader.shutdown(std::net::Shutdown::Both);
    outcome.map(|_| ())
}

#[cfg(test)]
mod tests;
