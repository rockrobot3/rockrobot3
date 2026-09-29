//! 单连接服务：本线程 reader + 一条 writer 线程。
//!
//! 协议面（spec §5.0 + 偏离 4）：
//! - 第一条帧必须是 `Request` + `dst_task = RR3_TASK_RRD` 的 `rr3.register`，否则按协议错误断开；
//! - `src_task` 由 rrd 分配并在应答 result 里回带；注册后上行帧的 `src_task` 必须等于分配值
//!   （AOS-STD-IPC-024：用户态不得自设）——伪造的那一帧被拒，但连接不断（一次笔误不该让整机掉线）；
//! - 注册完成前不收心跳、不判活（legacy 教训 76c0ad8）；未注册连接由读超时收掉；
//! - `rr3.unregister` 是模块主动退场，**不走** restart_policy（那是崩溃语义）。
//!
//! 写面（本任务的并发形状）：reader 线程写应答、writer 线程写广播，两者持有同一 socket 的
//! `try_clone` 句柄、共用一把 `Mutex<UnixStream>`、且一帧一次 `write_all` → **整帧原子**；
//! 但同一 socket 上"应答 vs 广播"的**相对顺序不保证**，所以测试按类型/字段挑帧，不按序号。

use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rr_bus::RR3_TASK_RRD;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame,
    Rr3FrameError, Rr3Header, Rr3PayloadType, Rr3StatusCode,
};
use rr_registry::{rr3_registry_validate_manifest_text, Rr3TopicSet};
use serde_json::{json, Value};

use crate::daemon::{rr3_now_ms, rr3_now_ns, rr3_state_frame};
use crate::manifest::{rr3_manifest_entry_is_absolute, rr3_manifest_facts_from_text};
use crate::registry::RrdRegistry;
use crate::rpc::{
    rr3_rpc_build_error, rr3_rpc_build_result, rr3_rpc_manifest_param, rr3_rpc_parse_request,
    rr3_rpc_status_for_register, Rr3RpcMethod,
};
use crate::rr3_log;

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
}

impl Rr3ConnCtx {
    pub fn rr3_ctx_now_ms(&self) -> u64 {
        rr3_now_ms(self.t0)
    }
}

/// 写侧锁中毒只可能来自 panic 的 writer；继续用比弃连接好
fn rr3_lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// 唯一的写出口：锁覆盖整帧 → 与并发 writer 的字节永不交错
fn rr3_write(out: &Arc<Mutex<UnixStream>>, frame: &Rr3Frame) {
    if let Err(e) = rr3_frame_write(&mut *rr3_lock(out), frame) {
        rr3_log(&format!("写帧失败 dst={}: {e:?}", frame.header.dst_task));
    }
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
        },
        payload,
    }
}

/// 成功/失败应答的两个写法（合起来只有一处构造帧）
fn rr3_reply_ok(out: &Arc<Mutex<UnixStream>>, me: &Rr3Peer, id: u64, result: &Value) {
    rr3_write(out, &rr3_out_frame(Rr3PayloadType::Response, me.task, &rr3_rpc_build_result(id, result)));
}

fn rr3_reply_err(out: &Arc<Mutex<UnixStream>>, me: &Rr3Peer, id: u64, code: Rr3StatusCode, msg: &str) {
    rr3_log(&format!("{} 应答错误 {}: {msg}", me.name, code as i32));
    rr3_write(out, &rr3_out_frame(Rr3PayloadType::Response, me.task, &rr3_rpc_build_error(id, code, msg)));
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
fn rr3_refuse(out: &Arc<Mutex<UnixStream>>, dst_task: u64, id: u64, code: Rr3StatusCode, msg: &str) -> String {
    rr3_log(&format!("注册被拒（dst={dst_task}）: {msg}"));
    rr3_write(out, &rr3_out_frame(Rr3PayloadType::Response, dst_task, &rr3_rpc_build_error(id, code, msg)));
    msg.to_string()
}

/// 握手：首帧必须是 rr3.register；成功后把出站 Sender 交给在册表并分配 task
///
/// 注册期每一次拒绝都**先回错误帧再断开**（只断连不回码等于让客户端挂在超时上）。
/// 状态码：首帧类型/dst_task/RPC 畸形/manifest 文本非法/`entry` 非绝对 = `Invalid`；
/// 重复名与超额 = `Busy`；保留名 = `Permission`（映射只在 `rr3_rpc_status_for_register` 一处）。
fn rr3_handshake(
    reader: &mut UnixStream,
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    fan_tx: Sender<Rr3Frame>,
) -> Result<Rr3Peer, String> {
    let frame = match rr3_frame_read(&mut *reader) {
        Ok(f) => f,
        // 连首帧都没读到：无从应答，只把原因交回上层记一条
        Err(e) => return Err(format!("首帧读取失败: {e:?}")),
    };
    // 注册前 rrd 还不知道对端的 task（还没分配），应答只能按对端**自报**的 src_task 回
    let reported_task = frame.header.src_task;
    let (ty, body) = match rr3_body_split(&frame.payload) {
        Ok(v) => v,
        Err(e) => {
            return Err(rr3_refuse(
                out,
                reported_task,
                0,
                Rr3StatusCode::Invalid,
                &format!("首帧 payload 前缀非法: {e:?}"),
            ))
        }
    };
    if frame.header.dst_task != RR3_TASK_RRD {
        return Err(rr3_refuse(
            out,
            reported_task,
            rr3_best_effort_id(&body),
            Rr3StatusCode::Invalid,
            &format!("首帧 dst_task 必须为 {RR3_TASK_RRD}，收到 {}", frame.header.dst_task),
        ));
    }
    if ty != Rr3PayloadType::Request {
        return Err(rr3_refuse(
            out,
            reported_task,
            rr3_best_effort_id(&body),
            Rr3StatusCode::Invalid,
            &format!("首帧 payload 类型必须是 Request，收到 {ty:?}"),
        ));
    }
    let req = match rr3_rpc_parse_request(&body) {
        Ok(r) => r,
        Err((_, msg)) => {
            return Err(rr3_refuse(
                out,
                reported_task,
                rr3_best_effort_id(&body),
                Rr3StatusCode::Invalid,
                &format!("首帧 RPC 畸形: {msg}"),
            ))
        }
    };
    if req.method != Rr3RpcMethod::Register {
        return Err(rr3_refuse(
            out,
            reported_task,
            req.id,
            Rr3StatusCode::Invalid,
            &format!("注册前第一条 RPC 必须是 rr3.register，收到 {}", req.method.as_str()),
        ));
    }
    let text = match rr3_rpc_manifest_param(&req.params) {
        Ok(t) => t,
        Err((_, msg)) => return Err(rr3_refuse(out, reported_task, req.id, Rr3StatusCode::Invalid, &msg)),
    };
    let facts = match rr3_manifest_facts_from_text(&text) {
        Ok(f) => f,
        Err(errs) => {
            return Err(rr3_refuse(out, reported_task, req.id, Rr3StatusCode::Invalid, &errs.join("; ")))
        }
    };
    let report = rr3_registry_validate_manifest_text(&text, &ctx.topics);
    if !report.ok() {
        return Err(rr3_refuse(out, reported_task, req.id, Rr3StatusCode::Invalid, &report.errors.join("; ")));
    }
    // M2 的 manifest 从线上（wire）来，没有"所在目录"可依据 → entry 必须是绝对路径。
    // 在仓的 modules/*/module.toml 是**契约工件**而非注册输入：装机时由安装步骤写入绝对
    // entry（见 modules/README.md），所以这条门禁不吃在仓夹具。
    if !rr3_manifest_entry_is_absolute(&facts.entry) {
        return Err(rr3_refuse(
            out,
            reported_task,
            req.id,
            Rr3StatusCode::Invalid,
            &format!("manifest.entry 必须是绝对路径，收到 {}", facts.entry),
        ));
    }
    let now = ctx.rr3_ctx_now_ms();
    // 一份 Sender 进在册表（此后它是这条连接唯一的广播入口），一份留在这里给"后到者补快照"用
    let replay_tx = fan_tx.clone();
    let task = match rr3_lock(&ctx.shared).rr3_reg_add(&facts, fan_tx, now) {
        Ok(t) => t,
        Err(e) => {
            // 状态码映射只在这一处（Task 6 的 rr3_rpc_status_for_register），别在 conn 里重写一遍
            let code = rr3_rpc_status_for_register(&e);
            return Err(rr3_refuse(out, reported_task, req.id, code, &e.to_string()));
        }
    };
    rr3_log(&format!(
        "注册 {} task={task} hb={}ms wd={}ms policy={}",
        facts.name, facts.heartbeat_ms, facts.watchdog_ms, facts.restart_policy
    ));
    // 应答与广播的先后不确定（两条线程两个写点），所以测试按类型挑帧，不按序号
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            task,
            &rr3_rpc_build_result(req.id, &json!({"src_task": task})),
        ),
    );
    let peer = Rr3Peer { task, name: facts.name.clone() };
    let n = rr3_lock(&ctx.shared).rr3_reg_fanout(&rr3_state_frame(&peer.name, "registered", peer.task), RR3_TASK_RRD);
    rr3_log(&format!("广播 registered 到 {n} 个连接"));
    // 后到者的状态快照：state 是"当前生命周期"的唯一可见面，只发转移不发快照的话，
    // 按 start_priority 后起的模块永远看不到先起模块上线过（Task 9 的依赖判定就没有输入）。
    // 注意这里说的只是**入队次序**，不构成上线次序：writer 线程可能在 reader 取锁前就把别的
    // 广播落盘了。所以这一条连接的帧序不做任何保证，读侧一律按类型/字段挑帧（夹具的按连接
    // 待处理队列正是为此而存在，见 `rrd/tests/support/mod.rs` 的文件头）。
    let snapshot = rr3_lock(&ctx.shared).rr3_reg_snapshot();
    let mut replayed = 0usize;
    for (name, t, phase) in snapshot.iter().filter(|(name, _, _)| *name != peer.name) {
        if replay_tx.send(rr3_state_frame(name, phase.as_str(), *t)).is_ok() {
            replayed += 1;
        }
    }
    rr3_log(&format!("向 {} 补发在册状态 {replayed} 条", peer.name));
    Ok(peer)
}

/// 处理注册后的入站帧。`Err` = 协议违规，该连接按崩溃收尾。
fn rr3_serve_frame(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    frame: &Rr3Frame,
) -> Result<Rr3Flow, String> {
    if frame.header.dst_task != RR3_TASK_RRD {
        return Err(format!("上行 dst_task 必须为 {RR3_TASK_RRD}，收到 {}", frame.header.dst_task));
    }
    let (ty, body) = rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if frame.header.src_task != me.task {
        // AOS-STD-IPC-024：src_task 由 rrd 分配，用户态不得自设。
        // 拒这一帧但不断连——一次笔误不该让整个进程掉线（spec §4"rrd 不崩"的连接版）。
        rr3_reply_err(
            out,
            me,
            rr3_best_effort_id(&body),
            Rr3StatusCode::Invalid,
            &format!("src_task 由 rrd 分配，本连接分配值 {}，收到 {}", me.task, frame.header.src_task),
        );
        return Ok(Rr3Flow::Keep);
    }
    match ty {
        Rr3PayloadType::Request => rr3_serve_request(out, ctx, me, &body),
        // Task 8 在这里插 `Rr3PayloadType::Event => rr3_publish(...)` 一支；
        // 现在先把所有非 Request 上行拒掉——拒是默认值，放行才需要理由。
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
    let req = match rr3_rpc_parse_request(body) {
        Ok(r) => r,
        Err((_, msg)) => {
            rr3_reply_err(out, me, rr3_best_effort_id(body), Rr3StatusCode::Invalid, &msg);
            return Ok(Rr3Flow::Keep);
        }
    };
    match req.method {
        Rr3RpcMethod::Register => {
            rr3_reply_err(out, me, req.id, Rr3StatusCode::Invalid, "该连接已注册，拒绝重复 rr3.register");
            Ok(Rr3Flow::Keep)
        }
        Rr3RpcMethod::Heartbeat => {
            let accepted = rr3_lock(&ctx.shared).rr3_reg_beat(me.task, ctx.rr3_ctx_now_ms());
            if accepted {
                rr3_reply_ok(out, me, req.id, &json!({"beat": "accepted"}));
            } else {
                rr3_reply_err(out, me, req.id, Rr3StatusCode::TimedOut, "该 task 已过期或不在册，需重新注册");
            }
            Ok(Rr3Flow::Keep)
        }
        Rr3RpcMethod::Unregister => {
            // 主动退场 ≠ 崩溃：直接移出在册表，不走 restart_policy。
            // 广播留给收尾处，保证"一次退出只有一条 state"。
            let removed = rr3_lock(&ctx.shared).rr3_reg_remove(&me.name);
            rr3_reply_ok(out, me, req.id, &json!({"removed": removed}));
            Ok(Rr3Flow::QuitClean)
        }
    }
}

/// 收尾：死亡记账（决定要不要重启）+ 恰好一条 state 广播。全进程只有这一处发 state。
fn rr3_conn_finish(ctx: &Rr3ConnCtx, me: &Rr3Peer, clean_exit: bool, reason: &str) {
    let plan = rr3_lock(&ctx.shared).rr3_reg_death(&me.name, clean_exit, ctx.rr3_ctx_now_ms());
    let phase = if clean_exit { "unregistered" } else { "failed" };
    let n = rr3_lock(&ctx.shared)
        .rr3_reg_fanout(&rr3_state_frame(&me.name, phase, me.task), RR3_TASK_RRD);
    match plan {
        Some(p) => rr3_log(&format!(
            "{} 退出（{reason}）→ 重启计划 attempt={} delay_ms={} entry={}（spawn 见 Task 11）",
            p.name, p.attempt, p.delay_ms, p.entry
        )),
        None => rr3_log(&format!("{} 退出（{reason}），无重启，广播 {phase} 到 {n} 个连接", me.name)),
    }
}

/// 服务一条连接。返回 `Err` 只描述**这一条**连接的断开原因；守护进程与别的连接不受影响。
pub fn rr3_conn_serve(stream: UnixStream, ctx: Rr3ConnCtx) -> Result<(), String> {
    let out: Arc<Mutex<UnixStream>> = Arc::new(Mutex::new(
        stream.try_clone().map_err(|e| format!("try_clone 失败: {e}"))?,
    ));
    let (fan_tx, fan_rx) = mpsc::channel::<Rr3Frame>();
    let writer: JoinHandle<()> = {
        let out = out.clone();
        thread::spawn(move || {
            for frame in fan_rx.iter() {
                rr3_write(&out, &frame);
            }
        })
    };

    // 注册前设读超时：连上却不注册的客户端不能永久占着线程（spec §5.0）
    stream
        .set_read_timeout(Some(Duration::from_millis(ctx.pre_register_timeout_ms.max(1))))
        .map_err(|e| format!("set_read_timeout 失败: {e}"))?;
    let mut reader = stream;
    let me = rr3_handshake(&mut reader, &out, &ctx, fan_tx)?;
    reader
        .set_read_timeout(None)
        .map_err(|e| format!("清读超时失败: {e}"))?;

    let outcome: Result<Rr3Flow, String> = loop {
        match rr3_frame_read(&mut reader) {
            Ok(frame) => match rr3_serve_frame(&out, &ctx, &me, &frame) {
                Ok(Rr3Flow::Keep) => {}
                Ok(Rr3Flow::QuitClean) => break Ok(Rr3Flow::QuitClean),
                Err(reason) => break Err(reason),
            },
            Err(Rr3FrameError::Closed) => break Ok(Rr3Flow::QuitClean),
            Err(e) => break Err(format!("帧读取失败: {e:?}")),
        }
    };

    let clean_exit = outcome.is_ok();
    let reason = match &outcome {
        Ok(_) => "干净收尾".to_string(),
        Err(e) => e.clone(),
    };
    rr3_conn_finish(&ctx, &me, clean_exit, &reason);
    // 所有 Sender 都已随注册表条目释放 → writer 的 iter() 自然结束。
    // 成立前提（F5/I-5）：这句只在**条目被移除时**成立。僵尸在册条目——注册之后不再读 socket
    // 的模块——条目仍留在表里、Sender 不会被释放，而 `mpsc::channel` 是无界的：writer 一阻塞在
    // write_all，那条连接的积压就无限增长。有界化 + 丢弃最旧 + 断连是 Task 10 的行为改动（已记
    // carry，watchdog 心跳过期正是现成的回收路径），本轮只把前提写清、不改通道形状
    // （brief 29-31 行约定的是 `mpsc::Receiver<Rr3Frame>` 这个类型，`sync_channel` 同样满足）。
    let _ = writer.join();
    let _ = reader.shutdown(std::net::Shutdown::Both);
    outcome.map(|_| ())
}
