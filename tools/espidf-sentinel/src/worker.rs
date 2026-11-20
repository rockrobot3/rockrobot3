//! 哨兵线程循环面：心跳拍点、收帧分类、入站点名应答。
//!
//! crate root 只做启动装配（UART 半体拆分、共享态、注册、线程拉起）；本文件承接
//! 两线程循环与入站面的全部逻辑，机制真值仍在 `rr-core::runtime`：
//! - [`heartbeat_loop`]：机制核 due 判定 → 发拍 → sent 记账（相对节拍）；
//! - [`reader_loop`]：收帧 → 机制核分类（Response→pending 路由；Request→round-trip
//!   应答；其余→inbox 计账）。帧语义全部来自 rr-ipc blanket conn，本文件零帧解析
//!   逻辑（头/payload 拆解走 rr-ipc，分类走 rr-core）。
//!
//! 隐私边界：本模块是 crate root 的子孙模块，`Shared` 的私有字段/方法在
//! Rust 可见性下可直接取用；对外只暴露两个线程入口。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use rr_core::control::parity::{rr3_parity_run, RR3_PARITY_VERSION};
use rr_core::runtime::{rr3_dispatch_class, Rr3Dispatch, Rr3GuardEvent};
use rr_core::{rr3_frame_compose, RR3_TASK_RRD};
use rr_ipc::transport::Rr3TransportConn;
use rr_ipc::{opcode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3FrameBuf, Rr3PayloadType};

use super::{ram_point, Shared, UartRxHalf, RPC_TIMEOUT};

/// 心跳线程：sleep 等拍点 → due 判定（机制核）→ 发拍 → sent 记账（相对节拍）。
pub(super) fn heartbeat_loop(sh: Arc<Shared>) {
    let mut seq: u64 = 0;
    loop {
        let Some(wait_ms) = sh
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_beat_wait_ms(sh.now_ms())
        else {
            return; // Down 即停拍——"不装活着"同一裁定
        };
        thread::sleep(Duration::from_millis(wait_ms));
        if sh.stop.load(Ordering::Relaxed) {
            return;
        }
        let now_ms = sh.now_ms();
        let due = sh
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_heartbeat_due(now_ms);
        if !due {
            continue; // 提前醒，回睡
        }
        match sh.call("rr3.heartbeat", json!({}), RPC_TIMEOUT) {
            Ok(v) if v.get("error").is_none() => {
                sh.rt
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .rr3_heartbeat_sent(now_ms);
                seq += 1;
                sh.guard_line(Rr3GuardEvent::Feed { seq });
            }
            other => sh.rr3_mark_restart(&format!("心跳失败: {other:?}")),
        }
    }
}

/// reader 线程：收帧 → 机制核分类（Response→pending 路由；Request→round-trip 应答；
/// 其余→inbox 计账）。
pub(super) fn reader_loop(mut rx: UartRxHalf, sh: Arc<Shared>) {
    loop {
        let Some((ty, frame, body)) = next_frame(&mut rx, &sh) else {
            continue;
        };
        match rr3_dispatch_class(ty) {
            Rr3Dispatch::Pending => deliver_pending(&body, &sh),
            Rr3Dispatch::Inbox => route_inbox(ty, &frame, &body, &sh),
        }
    }
}

/// 收一帧并做前缀校验：坏帧只打站并回 `None`（下一轮重收）；链路层错误走
/// `rr3_mark_restart`（重启不返回）。
fn next_frame(rx: &mut UartRxHalf, sh: &Shared) -> Option<(Rr3PayloadType, Rr3Frame, Vec<u8>)> {
    // rx 半体 → std adapter → blanket conn：rr3_recv_buf 带全量拒收路径
    // （半帧/坏 magic/长度不符），错误面与宿主三实现同源
    let buf = match rx.rr3_recv_buf() {
        Ok(b) => b,
        Err(e) => sh.rr3_mark_restart(&format!("收帧失败: {e:?}")),
    };
    let frame = match Rr3Frame::try_from(buf) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[sentinel] 帧还原失败: {e:?}");
            return None;
        }
    };
    match rr3_body_split(&frame.payload) {
        Ok((ty, body)) => Some((ty, frame, body)),
        Err(e) => {
            eprintln!("[sentinel] payload 前缀非法: {e:?}");
            None
        }
    }
}

/// 应答帧：按 JSON-RPC id 路由到等待者（std 驱动器 reader 同构）。
fn deliver_pending(body: &[u8], sh: &Shared) {
    let v: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[sentinel] 应答体非 JSON: {e}");
            return;
        }
    };
    let id = v.get("id").and_then(Value::as_u64).map(|i| i as u32);
    let waiter = id.and_then(|i| {
        sh.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&i)
    });
    match waiter {
        Some(w) => {
            let _ = w.send(v);
        }
        None => eprintln!("[sentinel] 无等待者的应答帧 id={id:?}"),
    }
}

/// 入站面分流：Request 走 round-trip 应答，其余（Event/Stream/Notification）inbox
/// 计账（哨兵无订阅主题，广播 state 帧是唯一合法来客——按类型点到名即可）。
fn route_inbox(ty: Rr3PayloadType, frame: &Rr3Frame, body: &[u8], sh: &Shared) {
    if ty == Rr3PayloadType::Request {
        handle_request(frame, body, sh);
    } else {
        eprintln!("[sentinel] inbox: {ty:?} {}B", body.len());
    }
}

/// diag/parity 点名（批G ⑤ 列板上腿）：跑真实原语回 JSON，**不回显期望向量**。
/// 定长数组 serde_json 有 ≤32 元定长数组/元组的 Serialize impl，json! 直接展开；
/// 机制核本身零 serde，序列化只落在本 crate（依赖闭包 L1 档位同 rr-bus）。
fn parity_result_json() -> Value {
    let o = rr3_parity_run();
    json!({
        "v": RR3_PARITY_VERSION,
        "rate": o.rate,
        "low": o.low,
        "sat": o.sat,
        "plan": o.plan,
        "wsat": o.wsat,
        "rhz": o.rhz,
        "clamp": o.clamp,
    })
}

/// 点名 Request 的 round-trip 应答（层③ 判据第三腿）：Response + id 回显 +
/// result 原样回带 params；method == `rr3-sentinel/diag/parity` 时回安全核
/// 逐拍向量（批G ⑤）。首次应答触发堆采样③。
/// 请求解析（自 [`handle_request`] 抽出）：非 JSON / 缺 id 当场点名，None = 不应答。
fn rr3_req_of(body: &[u8]) -> Option<(Value, u64)> {
    let v: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[sentinel] 请求体非 JSON: {e}");
            return None;
        }
    };
    let Some(id) = v.get("id").and_then(Value::as_u64) else {
        eprintln!("[sentinel] 请求缺 id，不应答（不猜）");
        return None;
    };
    Some((v, id))
}

fn handle_request(frame: &Rr3Frame, body: &[u8], sh: &Shared) {
    let Some((v, id)) = rr3_req_of(body) else {
        return;
    };
    // 分发判定（形状实锤 conn.rs rr3_route_call L779/L816：rrd 转发是"原始 body
    // 原样"——模块收到的 method 仍是 "rr3.call"，点名主题在 params.topic，不存在
    // "主题提为 method" 的改写。按 method==主题名 分发会在真实链路漏接）。
    let is_parity = v.get("method").and_then(Value::as_str) == Some("rr3.call")
        && v.pointer("/params/topic").and_then(Value::as_str) == Some("rr3-sentinel/diag/parity");
    let result = if is_parity {
        parity_result_json()
    } else {
        json!({"echo": v.get("params").cloned().unwrap_or(Value::Null)})
    };
    let reply = json!({"jsonrpc": "2.0", "id": id, "result": result});
    // 回路凭据与投递方向（M3 应答线形状，E6 彩排实锤修正）：trace_id 原样带回
    // Request 头上的调用方 task——rrd 的 rr3_route_response 按它改道并**自组**下行帧
    // （src=被调方 task、dst=调用方 task），0 无从投递会被拒（Invalid）；上行帧的
    // dst 一律为 rrd（conn.rs 上行门：dst≠1 直接按崩溃收尾）——dst=调用方 task 的
    // 写法过不了这道门（D1 未暴露：harness 扮 rrd 发点名 src=1，reply dst 恰好同值）。
    let out = rr3_frame_compose(
        opcode::SEND,
        0,
        frame.header.trace_id,
        sh.now_ns(),
        sh.task(),
        RR3_TASK_RRD,
        rr3_body_encode(Rr3PayloadType::Response, reply.to_string().as_bytes()),
    );
    let sent = sh
        .tx
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .rr3_send_buf(Rr3FrameBuf::from(&out));
    if let Err(e) = sent {
        eprintln!("[sentinel] round-trip 应答写出失败: {e:?}");
        return;
    }
    let n = sh.round_trips.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 {
        println!("[sentinel] round-trip 完成（id={id}）");
        // 堆采样③：round-trip 完成后（runbook §9.3 ③ 第三点）
        ram_point("3-roundtrip");
    }
}
