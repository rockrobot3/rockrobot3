//! 注册握手面（自 `conn.rs` 抽出，批B12 件二）：首帧读取 → 四道注册门 →
//! 入册与应答。原型面（连接主循环/路由/发布/控制旁支）留 `conn.rs`。
//!
//! 判据原样搬移，不改语义：首帧必须 `Request` 且 `dst_task = RR3_TASK_RRD`，
//! 方法必须 `rr3.register`（ctl 首帧走控制旁支，不注册、一问一答即断），
//! manifest 过解析/形状/绝对 entry 三道，入册失败按 `rr3_rpc_reg_status`
//! 单点映射回状态码。

use std::os::unix::net::UnixStream;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

use super::*;

/// 注册握手的唯一入口：读首帧 → 过门 → 入册，失败一律回错误帧并回原因。
///
/// 重复名与超额 = `Busy`；保留名 = `Permission`（映射只在 `rr3_rpc_reg_status` 一处）。
pub(super) fn rr3_handshake(
    reader: &mut UnixStream,
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    fan_tx: SyncSender<Rr3Frame>,
) -> Result<Rr3Peer, String> {
    let (reported_task, req) = rr3_hs_first(reader, out)?;
    // ADR-0010 裁定 3：首帧是 rr3.ctl.* → 控制旁支，不注册、一问一答即断
    if req.method.is_ctl() {
        return Err(super::ctl::rr3_serve_ctl(out, ctx, reported_task, req));
    }
    let facts = rr3_hs_facts(&req, out, reported_task, ctx)?;
    rr3_hs_admit(ctx, &facts, fan_tx, out, reported_task, req.id)
}

/// 首帧读取 + 解析；畸形按对端自报 task 回错误帧并交回原因。
fn rr3_hs_first(
    reader: &mut UnixStream,
    out: &Arc<Mutex<UnixStream>>,
) -> Result<(u64, Rr3RpcRequest), String> {
    let (reported_task, body) = rr3_hs_read(reader, out)?;
    match rr3_rpc_parse(&body) {
        Ok(r) => Ok((reported_task, r)),
        Err((_, msg)) => Err(rr3_refuse(
            out,
            reported_task,
            rr3_best_effort_id(&body),
            Rr3StatusCode::Invalid,
            &format!("首帧 RPC 畸形: {msg}"),
        )),
    }
}

/// 握手首段：读首帧并过 dst/类型两道门（注册前应答按对端自报 src_task 回）
fn rr3_hs_read(
    reader: &mut UnixStream,
    out: &Arc<Mutex<UnixStream>>,
) -> Result<(u64, Vec<u8>), String> {
    let frame = match rr3_frame_read_cap(&mut *reader, RR3_FRAMING_PLEN_CAP_DEFAULT) {
        Ok(f) => f,
        // 连首帧都没读到：无从应答，只把原因交回上层记一条
        Err(e) => return Err(format!("首帧读取失败: {e:?}")),
    };
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
    rr3_hs_gates(&frame, &body, ty, reported_task, out)?;
    Ok((reported_task, body))
}

/// 首帧两道形状门：dst_task 必须是 rrd、payload 类型必须是 Request。
fn rr3_hs_gates(
    frame: &Rr3Frame,
    body: &[u8],
    ty: Rr3PayloadType,
    reported_task: u64,
    out: &Arc<Mutex<UnixStream>>,
) -> Result<(), String> {
    if frame.header.dst_task != RR3_TASK_RRD {
        return Err(rr3_refuse(
            out,
            reported_task,
            rr3_best_effort_id(body),
            Rr3StatusCode::Invalid,
            &format!(
                "首帧 dst_task 必须为 {RR3_TASK_RRD}，收到 {}",
                frame.header.dst_task
            ),
        ));
    }
    if ty != Rr3PayloadType::Request {
        return Err(rr3_refuse(
            out,
            reported_task,
            rr3_best_effort_id(body),
            Rr3StatusCode::Invalid,
            &format!("首帧 payload 类型必须是 Request，收到 {ty:?}"),
        ));
    }
    Ok(())
}

/// 注册门：方法/参数/形状/entry 绝对路径四道，全 fail-closed 回错误帧
fn rr3_hs_facts(
    req: &Rr3RpcRequest,
    out: &Arc<Mutex<UnixStream>>,
    reported_task: u64,
    ctx: &Rr3ConnCtx,
) -> Result<Rr3ManifestFacts, String> {
    let (text, facts) = rr3_hs_facts_read(req, out, reported_task)?;
    let report = rr3_manifest_chk(&text, &ctx.topics);
    if !report.ok() {
        return Err(rr3_refuse(
            out,
            reported_task,
            req.id,
            Rr3StatusCode::Invalid,
            &report.errors.join("; "),
        ));
    }
    // M2 的 manifest 从线上来，没有"所在目录"可依据 → entry 必须是绝对路径。
    // 在仓 modules/*/module.toml 是契约工件而非注册输入（装机时写入绝对 entry）。
    if !rr3_manifest_abs(&facts.entry) {
        return Err(rr3_refuse(
            out,
            reported_task,
            req.id,
            Rr3StatusCode::Invalid,
            &format!("manifest.entry 必须是绝对路径，收到 {}", facts.entry),
        ));
    }
    Ok(facts)
}

/// 注册门前三道：方法必须 register、params 取 manifest 文本、文本过解析。
fn rr3_hs_facts_read(
    req: &Rr3RpcRequest,
    out: &Arc<Mutex<UnixStream>>,
    reported_task: u64,
) -> Result<(String, Rr3ManifestFacts), String> {
    if req.method != Rr3RpcMethod::Register {
        return Err(rr3_refuse(
            out,
            reported_task,
            req.id,
            Rr3StatusCode::Invalid,
            &format!(
                "注册前第一条 RPC 必须是 rr3.register，收到 {}",
                req.method.as_str()
            ),
        ));
    }
    let text = match rr3_rpc_manifest(&req.params) {
        Ok(t) => t,
        Err((_, msg)) => {
            return Err(rr3_refuse(
                out,
                reported_task,
                req.id,
                Rr3StatusCode::Invalid,
                &msg,
            ))
        }
    };
    let facts = match rr3_manifest_parse(&text) {
        Ok(f) => f,
        Err(errs) => {
            return Err(rr3_refuse(
                out,
                reported_task,
                req.id,
                Rr3StatusCode::Invalid,
                &errs.join("; "),
            ))
        }
    };
    Ok((text, facts))
}

/// 后到者补发在册状态（跳过自己）：只保证入队次序，不构成上线次序。
fn rr3_hs_replay(shared: &Arc<Mutex<RrdRegistry>>, tx: &SyncSender<Rr3Frame>, skip: &str) -> usize {
    let snapshot = rr3_lock(shared).rr3_reg_snapshot();
    let mut replayed = 0usize;
    for (name, t, phase) in snapshot.iter().filter(|(name, _, _)| *name != skip) {
        if tx
            .try_send(rr3_state_frame(name, phase.as_str(), *t))
            .is_ok()
        {
            replayed += 1;
        }
    }
    replayed
}

/// 入册与应答：注册成功 → 回 src_task → 广播 registered → 给后到者补发在册快照
fn rr3_hs_admit(
    ctx: &Rr3ConnCtx,
    facts: &Rr3ManifestFacts,
    fan_tx: SyncSender<Rr3Frame>,
    out: &Arc<Mutex<UnixStream>>,
    reported_task: u64,
    req_id: u64,
) -> Result<Rr3Peer, String> {
    // 一份 Sender 进在册表（此后它是这条连接唯一的广播入口），一份给补快照用
    let replay_tx = fan_tx.clone();
    // 硬规矩：锁结果先 `let` 绑再 `match`——Guard 临时量不得延寿到 Err 臂的
    // rr3_refuse（那边要取另一把锁），与 rr3_conn_finish 同形。
    let task = rr3_hs_register(ctx, facts, fan_tx, out, reported_task, req_id)?;
    rr3_log(&format!(
        "注册 {} task={task} hb={}ms wd={}ms policy={}",
        facts.name, facts.heartbeat_ms, facts.watchdog_ms, facts.restart_policy
    ));
    // 应答与广播的先后不确定（两条线程两个写点），测试按类型挑帧，不按序号
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            task,
            &rr3_rpc_build_result(req_id, &json!({"src_task": task})),
        ),
    );
    let peer = Rr3Peer {
        task,
        name: facts.name.clone(),
    };
    let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
        &rr3_state_frame(&peer.name, "registered", peer.task),
        RR3_TASK_RRD,
    );
    rr3_log(&format!("广播 registered 到 {n} 个连接"));
    // 后到者补发在册状态：只发转移不发快照的话，后起模块看不到先起模块上线过。
    // 这里只保证入队次序，不构成上线次序——读侧一律按类型/字段挑帧。
    let replayed = rr3_hs_replay(&ctx.shared, &replay_tx, &peer.name);
    rr3_log(&format!("向 {} 补发在册状态 {replayed} 条", peer.name));
    Ok(peer)
}

/// 注册入库一段：取现在册表 → 失败按状态码映射回拒帧。
fn rr3_hs_register(
    ctx: &Rr3ConnCtx,
    facts: &Rr3ManifestFacts,
    fan_tx: SyncSender<Rr3Frame>,
    out: &Arc<Mutex<UnixStream>>,
    reported_task: u64,
    req_id: u64,
) -> Result<u64, String> {
    let now = ctx.rr3_ctx_now_ms();
    let added = rr3_lock(&ctx.shared).rr3_reg_add(facts, fan_tx, now);
    match added {
        Ok(t) => Ok(t),
        Err(e) => {
            // 状态码映射只在这一处（rr3_rpc_reg_status），不在 conn 里重写
            let code = rr3_rpc_reg_status(&e);
            Err(rr3_refuse(out, reported_task, req_id, code, &e.to_string()))
        }
    }
}
