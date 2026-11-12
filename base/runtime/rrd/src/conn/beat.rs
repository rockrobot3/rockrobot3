//! 心跳面（自 `conn.rs` 抽出，批B12 件五）：心跳计数归并 → D-6 三态回报 →
//! Promoted 时的 running 广播与依赖恢复传播。
//!
//! 判据原样搬移：计数先于应答落账（对端读 counters 不得读到上一拍）、
//! Rejected 回 `TimedOut`、只在 `Promoted` 上广播（`Kept` 重发会把 state
//! 转移流变成快照流）。锁序 gate→shared 与全仓既有方向一致。

use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use super::*;

/// 心跳：计数归并先行（对端读 counters 不得读到上一拍）→ D-6 三态回报 →
/// Promoted 时广播 running 并做依赖恢复传播。
pub(super) fn rr3_beat_handle(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    id: u64,
    params: &Value,
) -> Result<Rr3Flow, String> {
    rr3_beat_counters(ctx, me, params);
    let beat = rr3_lock(&ctx.shared).rr3_reg_beat(me.task, ctx.rr3_ctx_now_ms());
    match beat {
        Rr3Beat::Rejected => {
            rr3_reply_err(
                out,
                me,
                id,
                Rr3StatusCode::TimedOut,
                "该 task 已过期或不在册，需重新注册",
            );
        }
        other => rr3_beat_ok(out, ctx, me, id, other),
    }
    Ok(Rr3Flow::Keep)
}

/// 心跳被接受的回执与续命后动作。
fn rr3_beat_ok(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    id: u64,
    beat: Rr3Beat,
) {
    rr3_reply_ok(out, me, id, &json!({"beat": "accepted"}));
    // 只在 Promoted 上广播：Kept 若也发，每次心跳都会重发一条 state
    // （spec §5.0 的 state 是转移流，不是快照流）
    if beat == Rr3Beat::Promoted {
        rr3_beat_promote(ctx, me);
    }
}

/// 心跳可选 counters 字段归并 + R26 合并写（变化 → 闸门 → 快照 → 落盘）。
/// 计数是运维账，不跟着判活走（Rejected 也照归并），但必须先于应答落账。
fn rr3_beat_counters(ctx: &Rr3ConnCtx, me: &Rr3Peer, params: &Value) {
    let Some(counters) = rr3_rpc_counters(params) else {
        return;
    };
    let changed = rr3_lock(&ctx.shared).rr3_reg_ctr_merge(&me.name, &counters);
    if !rr3_ctr_ready(changed, ctx) {
        return;
    }
    // 整段持闸：on_change→快照→store→on_flushed 必须在同一临界区。锁序
    // gate→shared 与全仓既有方向一致（merge 那手的 shared 锁已放）。
    let mut gate = rr3_lock(&ctx.counters_gate);
    if !gate.rr3_on_change(ctx.rr3_ctx_now_ms()) {
        return;
    }
    let snap = rr3_lock(&ctx.shared).rr3_reg_counters();
    let Some(path) = ctx.counters_path.as_deref() else {
        return;
    };
    rr3_ctr_flush(&mut gate, path, &snap, ctx);
}

/// 落盘结果回执：成功开下一拍窗口，失败留痕等自然重试。
fn rr3_ctr_flush(
    gate: &mut crate::counters::Rr3CountersGate,
    path: &std::path::Path,
    snap: &std::collections::BTreeMap<String, u64>,
    ctx: &Rr3ConnCtx,
) {
    match crate::counters::rr3_counters_store(path, snap) {
        Ok(()) => gate.rr3_on_flushed(ctx.rr3_ctx_now_ms()),
        Err(e) => rr3_log(&format!("counters 落盘失败（下一拍窗口到自然重试）: {e}")),
    }
}

/// 是否满足落盘前提：确有变化且有落盘路径。
fn rr3_ctr_ready(changed: bool, ctx: &Rr3ConnCtx) -> bool {
    if !changed || ctx.counters_path.is_none() {
        return false;
    }
    true
}

/// `registered → running` 的唯一升级时刻：广播 running + 依赖恢复传播。
/// 排除项 `RR3_TASK_RRD` = 不排除——发起者自己也要收到关于自己的升级。
fn rr3_beat_promote(ctx: &Rr3ConnCtx, me: &Rr3Peer) {
    let frame = rr3_state_frame(&me.name, Rr3Phase::Running.as_str(), me.task);
    let n = rr3_lock(&ctx.shared).rr3_reg_fanout(&frame, RR3_TASK_RRD);
    rr3_log(&format!("{} → running（广播到 {n} 个连接）", me.name));
    // ADR-0009 裁定 3 恢复半边：提供者回到 Running → 依赖者逐个重算，全部
    // required 就位才回 Running。谁记的账谁传播。
    let recovered = rr3_lock(&ctx.shared).rr3_reg_undegrade(&me.name);
    for (dep, task) in recovered {
        let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
            &rr3_state_frame(&dep, Rr3Phase::Running.as_str(), task),
            RR3_TASK_RRD,
        );
        rr3_log(&format!(
            "{dep} 依赖恢复 → running（提供者 {} 回到 Running）广播到 {n} 个连接",
            me.name
        ));
    }
}
