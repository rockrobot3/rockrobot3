//! 单连接服务：本线程 reader + 一条 writer 线程。
//!
//! 分面（批B12）：本文件留共享件（帧构造/应答/写出口/`Rr3Flow`/拒绝出口）、
//! 入站路由 `rr3_serve_frame`、上行方法表 `rr3_serve_request`、收尾与主循环；
//! 四个协议分面各自成子模块——`conn/handshake.rs`（注册握手）、`conn/ctl.rs`
//! （控制旁支）、`conn/publish.rs`（业务发布 + 低电兜底）、`conn/beat.rs`
//! （心跳与升级）、`conn/route.rs`（`rr3.call` 与应答回程）。
//!
//! 协议面（spec §5.0 + 偏离 4）：
//! - 第一条帧必须是 `Request` + `dst_task = RR3_TASK_RRD` 的 `rr3.register`，否则按协议错误断开；
//! - `src_task` 由 rrd 分配并在应答 result 里回带；注册后上行帧的 `src_task` 必须等于分配值
//!   （AOS-STD-IPC-024：用户态不得自设）——伪造的那一帧被拒，但连接不断（一次笔误不该让整机掉线）；
//! - 注册完成前不收心跳、不判活（legacy 教训 76c0ad8）；未注册连接由读超时收掉；
//!   注册后由静默上界（2 × 该模块看门狗窗口，ADR-0025）回收连接，判死权仍归看门狗；
//! - `rr3.unregister` 是模块主动退场，**不走** restart_policy（那是崩溃语义）；
//! - 上行 `Event` 是业务发布：过三层归属检查（冒充内建命名空间 → 前缀/在册 owner 越权 →
//!   主题未登记）后原样转发给**除发布者外**的在册连接，头改写成发布者 task + `dst=0`；
//!   任何一种拒法都只回错误帧 + 计数，不断连（spec §6、ADR-0006 决策 2/4）；
//! - 上行 `Response` 是 M3 `rr3.call` 的应答回路：trace_id = 调用方 task（转发时写入），
//!   body 原样、dst 改写为 trace_id 定向回投；trace_id=0 或调用方已不在册都只回错误帧
//!   不断连（M3 定型，见 `route::rr3_route_call` / `route::rr3_route_response`）。
//!
//! 写面（本任务的并发形状）：reader 线程写应答、writer 线程写广播，两者持有同一 socket 的
//! `try_clone` 句柄、共用一把 `Mutex<UnixStream>`、且一帧一次 `write_all` → **整帧原子**；
//! 但同一 socket 上"应答 vs 广播"的**相对顺序不保证**，所以测试按类型/字段挑帧，不按序号。

use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rr_bus::RR3_TASK_RRD;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read_cap, rr3_frame_write, Rr3Frame,
    Rr3FrameError, Rr3Header, Rr3PayloadType, Rr3StatusCode, RR3_FRAMING_PLEN_CAP_DEFAULT,
};
use rr_registry::{rr3_manifest_chk, Rr3TopicSet};
use serde_json::{json, Value};

use crate::daemon::{rr3_now_ms, rr3_now_ns, rr3_state_frame};
use crate::manifest::{rr3_manifest_abs, rr3_manifest_parse, Rr3ManifestFacts};
use crate::registry::{rr3_phase_for_exit, Rr3Beat, Rr3Death, Rr3Phase, RrdRegistry, RR3_FAN_CAP};
use crate::rpc::{
    rr3_rpc_build_error, rr3_rpc_build_result, rr3_rpc_call_params, rr3_rpc_counters,
    rr3_rpc_deny_status, rr3_rpc_manifest, rr3_rpc_parse, rr3_rpc_reg_status, rr3_rpc_topic_param,
    Rr3RpcMethod, Rr3RpcRequest,
};
use crate::supervisor::Rr3Supervisor;
use crate::{rr3_lock, rr3_log, rr3_log_query};

mod beat;
mod ctl;
mod handshake;
mod publish;
mod route;

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
    /// 运维计数持久化路径（0.0.4 批B，Q2 裁定）：daemon 启动判定一次，`None` =
    /// 不持久化（心跳聚合照常进内存账，只是断电不保）。conn 心跳归并后按
    /// R26 合并写节流落盘（0.0.5 增补轮四 批W3 改判，闸门在 counters.rs）。
    pub counters_path: Option<std::path::PathBuf>,
    /// 运维计数合并写闸门（R26，批W3）：daemon 造一份、全连接共享（`Arc`）——
    /// 每条连接各持一份等于把 M 窗口放大 N 连接倍。
    pub counters_gate: Arc<Mutex<crate::counters::Rr3CountersGate>>,
}

impl Rr3ConnCtx {
    pub fn rr3_ctx_now_ms(&self) -> u64 {
        rr3_now_ms(self.t0)
    }
}

/// 唯一的写出口：锁覆盖整帧 → 与并发 writer 的字节永不交错。
/// 写一帧到 `out`（持锁写，帧级原子）。返回 false = 写失败（含写超时）：
/// writer 线程据此收摊退出（对端不收 = 这条连接写向已死），应答路径忽略返回值
/// ——失败已点名进日志，连接生死由 reader 侧的 EOF/心跳回收裁定。
fn rr3_write(out: &Arc<Mutex<UnixStream>>, frame: &Rr3Frame) -> bool {
    if let Err(e) = rr3_frame_write(&mut *rr3_lock(out), frame) {
        rr3_log(&format!("写帧失败 dst={}: {e:?}", frame.header.dst_task));
        return false;
    }
    true
}

/// 组一帧 rrd → 模块（ty=Response/Notification，dst=对端 task）
fn rr3_out_frame(ty: Rr3PayloadType, dst_task: u64, body: &[u8]) -> Rr3Frame {
    let payload = rr3_body_encode(ty, body);
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: RR3_TASK_RRD,
            dst_task,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 成功/失败应答的两个写法（合起来只有一处构造帧）
fn rr3_reply_ok(out: &Arc<Mutex<UnixStream>>, me: &Rr3Peer, id: u64, result: &Value) {
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            me.task,
            &rr3_rpc_build_result(id, result),
        ),
    );
}

fn rr3_reply_err(
    out: &Arc<Mutex<UnixStream>>,
    me: &Rr3Peer,
    id: u64,
    code: Rr3StatusCode,
    msg: &str,
) {
    rr3_log(&format!("{} 应答错误 {}: {msg}", me.name, code as i32));
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            me.task,
            &rr3_rpc_build_error(id, code, msg),
        ),
    );
}

/// 解析失败时尽力取 JSON-RPC 的 id；取不到用 0（客户端只按 code 判定）
fn rr3_best_effort_id(body: &[u8]) -> u64 {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("id").and_then(Value::as_u64))
        .unwrap_or(0)
}

/// 入站帧处理结论
enum Rr3Flow {
    /// 继续读下一帧
    Keep,
    /// 干净收尾（EOF / rr3.unregister）：不做崩溃记账
    QuitClean,
}

/// 注册期拒绝的统一出口：按对端自报的 src_task 回错误帧，再把原因交回上层断开连接
fn rr3_refuse(
    out: &Arc<Mutex<UnixStream>>,
    dst_task: u64,
    id: u64,
    code: Rr3StatusCode,
    msg: &str,
) -> String {
    rr3_log(&format!("注册被拒（dst={dst_task}）: {msg}"));
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            dst_task,
            &rr3_rpc_build_error(id, code, msg),
        ),
    );
    msg.to_string()
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
    match ty {
        Rr3PayloadType::Request => rr3_serve_request(out, ctx, me, &body),
        Rr3PayloadType::Event => publish::rr3_publish(out, ctx, me, &body),
        // M3：上行 Response 是 rr3.call 的应答回路（trace_id 改道回调用方）
        Rr3PayloadType::Response => route::rr3_route_response(out, ctx, me, frame, &body),
        // 拒是默认值，放行才需要理由：Request/Event/Response 之外的上行（Notification
        // 冒充上行）仍然一律拒帧不断连。
        other => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(&body),
                Rr3StatusCode::Invalid,
                &format!("上行不支持 payload 类型 {other:?}（业务发布请用 Event）"),
            );
            Ok(Rr3Flow::Keep)
        }
    }
}

/// 三个上行方法。RPC 解析失败一律回 Invalid——Task 6 已保证解析器不产别的码，
/// 这里不重建 id→code 的映射，避免第二套真值。
fn rr3_serve_request(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    let req = match rr3_rpc_parse(body) {
        Ok(r) => r,
        Err((_, msg)) => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(body),
                Rr3StatusCode::Invalid,
                &msg,
            );
            return Ok(Rr3Flow::Keep);
        }
    };
    match req.method {
        Rr3RpcMethod::Register => {
            rr3_reply_err(
                out,
                me,
                req.id,
                Rr3StatusCode::Invalid,
                "该连接已注册，拒绝重复 rr3.register",
            );
            Ok(Rr3Flow::Keep)
        }
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
        | Rr3RpcMethod::CtlDomainSet => {
            rr3_reply_err(
                out,
                me,
                req.id,
                Rr3StatusCode::Permission,
                "rr3.ctl.* 只对未注册的控制连接开放，模块请走 rr3.register / rr3.heartbeat",
            );
            Ok(Rr3Flow::Keep)
        }
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
/// `watchdog_broadcasts_exactly_one_failed_state` 与 `e2e_spawn_restart` 的 S2 各自钉住一半）。
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
            // D-7：死因→phase 的字面量只在 registry 一处，conn 不再各写各的。
            let phase = rr3_phase_for_exit(clean_exit);
            // E-9：这里排 `RR3_TASK_RRD` 等于不排除（发起者自己也收到关于自己的终态）；
            // supervisor 收尸那一路排的是 `crashed.src_task`。两种口径是**已知**的第三/第四种
            // 口径，归 Task 13 的 A-3 文档化，本任务不"统一"——统一成哪一种都没有判据支持。
            let n = rr3_lock(&ctx.shared)
                .rr3_reg_fanout(&rr3_state_frame(&me.name, phase, me.task), RR3_TASK_RRD);
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
                let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
                    &rr3_state_frame(&dep, Rr3Phase::Degraded.as_str(), task),
                    task,
                );
                rr3_log(&format!(
                    "{dep} 依赖断供 → degraded（提供者 {} 离开 Running）广播到 {n} 个连接",
                    me.name
                ));
            }
        }
    }
}

/// 注册后读侧静默上界就位（ADR-0025 裁定 1/4），返回上界毫秒；0 = 不设上界。
///
/// 上界 = 该模块自己申报的 `watchdog_ms × 2`：2 倍是归属裁定——保证看门狗先到期，
/// 连接收尾走 `Silent`，判死/广播/重启预算只归看门狗一拍。健康模块按 heartbeat_ms
/// 喂狗（< watchdog_ms），正常业务永远触不到这条界。在册事实缺失或看门狗窗口为 0
/// 时不设上界并留痕——宁可退化为无上界，也不拿错的上界误杀健康连接。
fn rr3_conn_idle_set(
    stream: &UnixStream,
    shared: &Arc<Mutex<RrdRegistry>>,
    name: &str,
) -> Result<u64, String> {
    let idle_ms = rr3_lock(shared)
        .rr3_reg_facts(name)
        .map(|f| f.watchdog_ms.saturating_mul(2))
        .unwrap_or(0);
    if idle_ms > 0 {
        stream
            .set_read_timeout(Some(Duration::from_millis(idle_ms)))
            .map_err(|e| format!("set_read_timeout 失败: {e}"))?;
    } else {
        rr3_log(&format!(
            "{name} 无可用看门狗窗口（在册事实缺失或为 0），读侧不设静默上界——\
             连接回收退化为依赖对端断开"
        ));
        stream
            .set_read_timeout(None)
            .map_err(|e| format!("清读超时失败: {e}"))?;
    }
    Ok(idle_ms)
}

/// 注册后的读循环：逐帧读、逐帧处理，直到干净收尾或带原因断开。
/// EOF（对端关连接）与 `rr3.unregister` 都是 `QuitClean`；其余读错/处理错带原因返回。
///
/// `idle_ms` 是注册后静默上界（ADR-0025：2 × 该模块看门狗窗口，0 = 不设上界）。
/// 触发时文案点名空闲时长与归属——判死、广播、重启预算仍归看门狗那一拍，本循环
/// 只回收连接本身。
fn rr3_read_loop(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    reader: &mut UnixStream,
    idle_ms: u64,
) -> Result<Rr3Flow, String> {
    let mut last_frame = Instant::now();
    loop {
        // 逐帧读：`Some` = 该收尾了（干净或带原因），`None` = 继续下一帧。
        if let Some(done) = rr3_read_step(out, ctx, me, reader, &mut last_frame, idle_ms) {
            break done;
        }
    }
}

/// 读一帧并处理；`Some(结论)` = 收尾，`None` = 本帧已消化、继续读。
fn rr3_read_step(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    reader: &mut UnixStream,
    last_frame: &mut Instant,
    idle_ms: u64,
) -> Option<Result<Rr3Flow, String>> {
    match rr3_frame_read_cap(&mut *reader, RR3_FRAMING_PLEN_CAP_DEFAULT) {
        Ok(frame) => {
            *last_frame = Instant::now();
            match rr3_serve_frame(out, ctx, me, &frame) {
                Ok(Rr3Flow::Keep) => None,
                Ok(Rr3Flow::QuitClean) => Some(Ok(Rr3Flow::QuitClean)),
                Err(reason) => Some(Err(reason)),
            }
        }
        Err(Rr3FrameError::Closed) => Some(Ok(Rr3Flow::QuitClean)),
        Err(e) => Some(Err(rr3_read_err(&e, *last_frame, idle_ms))),
    }
}

/// 读错文案：静默超时（上界 = 2 × 看门狗）与普通读错分流。
fn rr3_read_err(e: &Rr3FrameError, last_frame: Instant, idle_ms: u64) -> String {
    let idle = last_frame.elapsed().as_millis() as u64;
    if idle_ms > 0 && idle >= idle_ms {
        format!(
            "静默超时：{idle}ms 无完整帧（上界 = 2 × 看门狗 {idle_ms}ms），\
             本连接回收；判死与重启归看门狗一拍"
        )
    } else {
        format!("帧读取失败: {e:?}")
    }
}

/// 广播写线程：从有界队列取帧，写失败即退出（连接回收随通道 drop 走 Disconnected）。
fn rr3_conn_writer(
    out: &Arc<Mutex<UnixStream>>,
    fan_rx: mpsc::Receiver<Rr3Frame>,
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
    let (fan_tx, fan_rx) = mpsc::sync_channel::<Rr3Frame>(RR3_FAN_CAP);
    let writer: JoinHandle<()> = rr3_conn_writer(&out, fan_rx);

    // 注册前设读超时：连上却不注册的客户端不能永久占着线程（spec §5.0）
    stream
        .set_read_timeout(Some(Duration::from_millis(
            ctx.pre_register_timeout_ms.max(1),
        )))
        .map_err(|e| format!("set_read_timeout 失败: {e}"))?;
    let mut reader = stream;
    let me = handshake::rr3_handshake(&mut reader, &out, &ctx, fan_tx)?;
    let idle_ms = rr3_conn_idle_set(&reader, &ctx.shared, &me.name)?;

    let outcome = rr3_read_loop(&out, &ctx, &me, &mut reader, idle_ms);

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
