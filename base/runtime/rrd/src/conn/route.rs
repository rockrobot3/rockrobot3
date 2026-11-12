//! 模块间请求面（自 `conn.rs` 抽出，批B12 件六）：`rr3.call` 的转发与上行
//! `Response` 的回程。
//!
//! 判据原样搬移：rrd 是转发器不是代理，没有同步应答；调用凭据是帧头
//! `trace_id = 调用方 task`，回程按它定向投递。查表 + 构帧 + 投递在同一把
//! 锁内完成（不留"查到 callee、锁一放它就死了"的窗口）；畸形参数 / 无人在册 /
//! 自呼 / 被调方已死 / trace_id=0 一律回错误帧不断连。

use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use super::*;

/// `rr3.call` 的路由（M3，architecture.md §6"模块间请求"）：按 params.topic 查
/// `[rpc].exposes` 它的在册模块，把**原始 body 原样**转发成 Request 帧——
/// src = 调用方 task、dst = 被调方 task、**trace_id = 调用方 task**（应答回路的
/// 唯一凭据，被调方应答时必须原样带回头上的 trace_id，见 `rr3_route_response`）。
///
/// 这里**没有同步应答**：rrd 是转发器不是代理，调用方在 `rr3_call_topic` 上等的
/// 就是被调方沿 Response 回路送回来的那一帧。拒法（畸形参数 / 无人在册 / 自呼 /
/// 被调方已死）都回错误帧 + 不断连，口径与 publish 的形状错一致；无人在册是
/// `Invalid` 不是 `Busy`——"没人接"是路由事实，不是容量问题。
pub(super) fn rr3_route_call(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    id: u64,
    params: &Value,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    // 参数先在锁外验（畸形是调用方的笔误，用不着查表）
    let topic = match rr3_rpc_call_params(params) {
        Ok((t, _)) => t,
        Err((_, msg)) => {
            // 解析器只产 Invalid（Task 6 的口径），这里不重建 id→code 映射
            rr3_reply_err(out, me, id, Rr3StatusCode::Invalid, &msg);
            return Ok(Rr3Flow::Keep);
        }
    };
    let outcome = rr3_call_fwd(ctx, me, &topic, body);
    rr3_call_reply(out, me, id, &topic, outcome);
    Ok(Rr3Flow::Keep)
}

/// 查表 + 构帧 + 投递在同一把锁内完成：避免"查到 callee、锁一放它就死了"的窗口里
/// 构出一帧注定无处投递的转发。锁内 send 与 `rr3_reg_fanout` 同一口径（无界 mpsc 不阻塞）。
/// `Ok(bool)` = 投递结果，`Err` = 拒法文案（无人在册 / 自呼）。
fn rr3_call_fwd(ctx: &Rr3ConnCtx, me: &Rr3Peer, topic: &str, body: &[u8]) -> Result<bool, String> {
    let g = rr3_lock(&ctx.shared);
    match g.rr3_reg_callee_for(topic) {
        None => Err(format!(
            "没有在册模块 exposes 主题 {topic}（被调方须在 manifest 的 [rpc].exposes 里声明）"
        )),
        Some((_, task)) if task == me.task => {
            Err(format!("主题 {topic} 的唯一被调方就是调用方自己，拒绝自呼"))
        }
        Some((name, task)) => {
            let frame = rr3_req_frame(me, task, body);
            rr3_log(&format!(
                "rr3.call {topic}: {}(task={}) → {name}(task={task})",
                me.name, me.task
            ));
            Ok(g.rr3_reg_send_to(task, &frame))
        }
    }
}

/// 投递结果 → 调用方应答（拒法与投递前退场都回 Invalid 错误帧，不断连）。
fn rr3_call_reply(
    out: &Arc<Mutex<UnixStream>>,
    me: &Rr3Peer,
    id: u64,
    topic: &str,
    outcome: Result<bool, String>,
) {
    match outcome {
        Ok(true) => {}
        // 查表时还活着、投递时收件人没了：对调用方而言就是"这一拍没人接"
        Ok(false) => rr3_fwd_gone(out, me, id, topic),
        Err(msg) => rr3_reply_err(out, me, id, Rr3StatusCode::Invalid, &msg),
    }
}

/// 投递前退场：回 Invalid 错误帧（不断连），点名被调方已走。
fn rr3_fwd_gone(out: &Arc<Mutex<UnixStream>>, me: &Rr3Peer, id: u64, topic: &str) {
    rr3_reply_err(
        out,
        me,
        id,
        Rr3StatusCode::Invalid,
        &format!("主题 {topic} 的被调方在投递前退场"),
    );
}

/// `rr3.call` 的转发帧：src/trace = 调用方 task，dst = 被调方 task，body 原样。
fn rr3_req_frame(me: &Rr3Peer, task: u64, body: &[u8]) -> Rr3Frame {
    let payload = rr3_body_encode(Rr3PayloadType::Request, body);
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: me.task,
            timestamp_ns: rr3_now_ns(),
            src_task: me.task,
            dst_task: task,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 上行应答的回程帧：dst/trace 改写成调用方 task，body 原样。
fn rr3_fwd_frame(me: &Rr3Peer, trace: u64, body: &[u8]) -> Rr3Frame {
    let payload = rr3_body_encode(Rr3PayloadType::Response, body);
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: trace,
            timestamp_ns: rr3_now_ns(),
            src_task: me.task,
            dst_task: trace,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 上行 Response 的回路（M3）：rr3.call 的应答去路。头上的 `trace_id` 是**调用方
/// task**（`rr3_route_call` 转发时写入，被调方须原样带回）；body 原样转发，只把
/// dst 改写成 trace_id。trace_id = 0 无从投递，回 Invalid；调用方已不在册（已退场 /
/// 已过期）同样无处投递——两种都是"拒帧不断连"，与 src_task 伪造同一口径。
pub(super) fn rr3_route_response(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    frame: &Rr3Frame,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    let trace = frame.header.trace_id;
    if trace == 0 {
        rr3_reply_err(
            out,
            me,
            rr3_best_effort_id(body),
            Rr3StatusCode::Invalid,
            "上行 Response 缺回路：trace_id 必须为调用方 task（0 = 无从投递）",
        );
        return Ok(Rr3Flow::Keep);
    }
    let fwd = rr3_fwd_frame(me, trace, body);
    if !rr3_lock(&ctx.shared).rr3_reg_send_to(trace, &fwd) {
        rr3_reply_err(
            out,
            me,
            rr3_best_effort_id(body),
            Rr3StatusCode::Invalid,
            &format!("应答的调用方 task={trace} 已不在册，无处投递"),
        );
    }
    Ok(Rr3Flow::Keep)
}
