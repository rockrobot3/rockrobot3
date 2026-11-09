//! 业务发布面（自 `conn.rs` 抽出，批B12 件四）：上行 `Event` 的解析 → 三层归属门
//! → 排除发布者的广播 → 低电兜底。
//!
//! 判据原样搬移：归属检查仍在 `RrdRegistry::rr3_reg_pub_check` 单点，本面只负责
//! "拒的话怎么回、过的话怎么转"；三种归属拒法计数并回错误帧但不断连，体的形状
//! 错误回帧不断连且不计数（与 spec §6 的计数条款同口径）。
//!
//! 低电兜底（0.0.2 方案 D6/Q4）挂在同一个转发点上：每一帧 Event 都从这里过，
//! 谁发、值多少都已知，省掉第二条订阅链。

use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use super::*;

/// 低电兜底的触发主题（D3 append 的契约主题；owner=rr3-power，rrd 只订阅不发布）
const RR3_BATTERY_LOW_TOPIC: &str = "rr3-power/battery/low";
/// 低电兜底的解除主题（批N5 件二笔3 append 的契约主题；owner=rr3-power，
/// rrd 只订阅不发布）——两个沿一个对，置旗/清旗各听一个主题。
const RR3_BATTERY_OK_TOPIC: &str = "rr3-power/battery/ok";

/// 业务发布：三层归属检查在 Task 5 的关联函数里，这里只负责"拒的话怎么回、过的话怎么转"。
/// 三种**归属**拒法（冒充 / 越权前缀 / 未在册）都回错误帧并计数，但**不断连**——发布越权是数据面
/// 问题，不该带走控制面（与 `src_task` 伪造、payload 类型不支持同一口径：回帧 + `Rr3Flow::Keep`）。
/// 体的形状错误（非 JSON、缺/坏 `topic`）同理的是**回帧 + 不断连 + 不记崩溃**那一半：那是客户端的
/// 一次笔误，不是协议失步，按崩溃收尾会白记一次 `rr3_reg_death` 并可能触发重启。
/// 它**不计数**——`rr3_reg_note_denied` 只在上面那三种归属拒法上调用，口径与 spec §6 的计数条款
/// （只绑"被拒的越权发布"）一致；把形状错也算进 denied 会让这个数变成"客户端错误总数"。
///
/// `sender_name` 取 `me.name`——它是**这条连接注册时**由 rrd 收下并分配的在册身份
/// （`Rr3Peer` 随握手成功才存在），不是从帧体或注册报文里现读的字。`registry.rs` 那句
/// "唯一防线建在调用侧"的前提到这里是成立的：帧上的 `src_task` 已在上面被强制等于
/// `me.task`，而 `me.name` 与 `me.task` 出自同一次 `rr3_reg_add`。
pub(super) fn rr3_publish(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    let (parsed, topic) = match rr3_pub_parse(out, me, body) {
        Ok(v) => v,
        Err(flow) => return Ok(flow),
    };
    if let Some(flow) = rr3_pub_gate(out, ctx, me, body, &topic) {
        return Ok(flow);
    }
    rr3_pub_forward(ctx, me, body, &topic);
    rr3_pub_batt(ctx, &topic, &parsed);
    Ok(Rr3Flow::Keep)
}

/// Event 体的解析与 topic 提取。两条形状错（非 JSON / 缺坏 topic）都回 Invalid 帧
/// 并保持连接：`Err` 携带已应答的 `Keep`，调用方不再重复回帧。
fn rr3_pub_parse(
    out: &Arc<Mutex<UnixStream>>,
    me: &Rr3Peer,
    body: &[u8],
) -> Result<(Value, String), Rr3Flow> {
    let parsed = match serde_json::from_slice::<Value>(body) {
        Ok(v) => v,
        Err(e) => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(body),
                Rr3StatusCode::Invalid,
                &format!("Event 体必须是 JSON 对象: {e}"),
            );
            return Err(Rr3Flow::Keep);
        }
    };
    let topic = match rr3_rpc_topic_param(&parsed) {
        Ok(t) => t,
        Err((_, msg)) => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(body),
                Rr3StatusCode::Invalid,
                &msg,
            );
            return Err(Rr3Flow::Keep);
        }
    };
    Ok((parsed, topic))
}

/// 归属门（三层归属检查的应答侧）：拒则计数 + 回错误帧 + `Some(Keep)`；
/// 放行返回 `None`。三种归属拒法都不断连（与 src_task 伪造同一口径）。
fn rr3_pub_gate(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    body: &[u8],
    topic: &str,
) -> Option<Rr3Flow> {
    let deny = {
        let check = RrdRegistry::rr3_reg_pub_check(&me.name, topic, &ctx.topics);
        if let Err(e) = check {
            rr3_lock(&ctx.shared).rr3_reg_note_denied();
            Some(e)
        } else {
            None
        }
    };
    let e = deny?;
    rr3_reply_err(
        out,
        me,
        rr3_best_effort_id(body),
        rr3_rpc_deny_status(&e),
        &e.to_string(),
    );
    Some(Rr3Flow::Keep)
}

/// 转发：保留体，头改写成"发布者 task + 广播目的"，类型仍是 Event。
/// 这里第三处自己构造帧（另两处是 `rr3_out_frame` 的应答）：转发帧的 src 是**发布者**
/// 而不是 rrd，套不进那个 src 恒为 RR3_TASK_RRD 的写法。
/// `payload_len` 必须从**编出来的字节串**取，不能从 `body` 取——少了那 2 字节前缀
/// 就是 Task 1 帧层的 `LengthMismatch`，收侧当场把这条链打死。
fn rr3_pub_forward(ctx: &Rr3ConnCtx, me: &Rr3Peer, body: &[u8], topic: &str) {
    let payload = rr3_body_encode(Rr3PayloadType::Event, body);
    let frame = Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: me.task,
            dst_task: rr_bus::RR3_BROADCAST_DST_TASK,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    };
    // 排除项是发布者自己：模块不该收到自己发出的广播（`rr3_reg_fanout` 只按 task 排除，
    // 不看帧的 dst，见该函数文档）。
    // 留痕只在接收连接数变化（含首次）时落——音频 PCM 每帧一条会把日志环冲垮。
    let (n, changed) = {
        let mut g = rr3_lock(&ctx.shared);
        let n = g.rr3_reg_fanout(&frame, me.task);
        // 影子统计（广播面 ADR 前哨）：只按 manifest subscribes 记账"本应过滤"的
        // 目标数，不改上面的全量投递——计数面 = rr3.ctl.counters 的 fanout_shadow*
        g.rr3_reg_shadow(topic, me.task);
        let changed = g.rr3_reg_fan_changed(topic, n);
        (n, changed)
    };
    if changed {
        rr3_log(&format!("转发 {topic} 到 {n} 个连接"));
    }
}

/// 低电兜底（0.0.2 方案 D6/Q4 裁定，architecture §7.3 横切路径）：rrd 是总线本体，
/// "订阅 battery/low|ok"就落在本转发点——每一帧 Event 都从这里过，谁发、值多少都已知。
/// 四件动作面：置全局低电旗（上升沿恰一次）→ supervisor 的 spawn 漏斗拒新 spawn →
/// 点名广播 rr3-runtime/system/power → 日志环（本函数的 rr3_log 即入口）。
/// 解除路径（批N5 件二笔3，§17 运维留痕）：收到 battery/ok（恢复沿）清旗——
/// 下降沿恰一次点名广播 reason=battery-ok，spawn 漏斗自此放行新 spawn；重复
/// battery/ok（旗本就没置）不重复广播。battery/low|ok 本身照常已转发，兜底是
/// 额外动作，不截断事件流。
fn rr3_pub_batt(ctx: &Rr3ConnCtx, topic: &str, parsed: &Value) {
    if topic == RR3_BATTERY_LOW_TOPIC {
        let rising = rr3_lock(&ctx.shared).rr3_reg_lp_raise();
        if rising {
            let percent = parsed.get("percent").and_then(Value::as_u64).unwrap_or(0);
            let voltage_mv = parsed
                .get("voltage_mv")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
                &crate::daemon::rr3_power_frame("battery-low", percent, voltage_mv),
                rr_bus::RR3_TASK_RRD,
            );
            rr3_log(&format!(
                "低电兜底触发：battery/low percent={percent} voltage_mv={voltage_mv} → 置全局低电旗、停止新 spawn，广播 rr3-runtime/system/power 到 {n} 个连接"
            ));
        } else {
            rr3_log("低电兜底已生效：重复 battery/low 不重复广播（同一次低电段只点名一次）");
        }
    } else if topic == RR3_BATTERY_OK_TOPIC {
        let falling = rr3_lock(&ctx.shared).rr3_reg_lp_lower();
        if falling {
            let percent = parsed.get("percent").and_then(Value::as_u64).unwrap_or(0);
            let voltage_mv = parsed
                .get("voltage_mv")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
                &crate::daemon::rr3_power_frame("battery-ok", percent, voltage_mv),
                rr_bus::RR3_TASK_RRD,
            );
            rr3_log(&format!(
                "低电兜底解除：battery/ok percent={percent} voltage_mv={voltage_mv} → 清全局低电旗、恢复新 spawn，广播 rr3-runtime/system/power(reason=battery-ok) 到 {n} 个连接"
            ));
        } else {
            rr3_log("低电兜底本未触发：重复 battery/ok 不重复广播（旗未置位无从解除）");
        }
    }
}
