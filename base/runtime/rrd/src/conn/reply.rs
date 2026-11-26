// SPDX-License-Identifier: Apache-2.0
//! 写出口与应答帧面：帧级原子写、rrd→模块应答/拒绝构造。
//!
//! 写失败点名的分类与计数在父模块；本模块只负责组帧与写序。

use super::*;
use std::io::IoSlice;

/// 唯一的写出口：锁覆盖整帧 → 与并发 writer 的字节永不交错。
/// 写一帧到 `out`（持锁写，帧级原子）。返回 false = 写失败（含写超时）：
/// writer 线程据此收摊退出（对端不收 = 这条连接写向已死），应答路径忽略返回值
/// ——失败已点名进日志，连接生死由 reader 侧的 EOF/心跳回收裁定。
pub(super) fn rr3_write(out: &Arc<Mutex<UnixStream>>, frame: &Rr3Frame) -> bool {
    // 结果先落到局部变量：`if let Err(..) = rr3_frame_write(&mut *rr3_lock(..))`
    // 的临时 guard 会活到整个 if-let 结束，点名日志就成了**持出向锁写磁盘**
    // （续轮十九：锁内零文件 I/O）。先结束语句放锁，再落日志。
    //
    // 这里自己走 `头编码 + 分段 write_all + flush`（与 `rr3_frame_write` 同序、同字节）而不是
    // 直接调它：那条路径把 io::Error 折成字符串，写超时（EAGAIN）与真错分不开——
    // "对端不收"必须能单独点名与计数（续轮二十五）。
    let (res, timed_out) = {
        let mut guard = rr3_lock(out);
        rr3_write_bytes(&mut guard, frame)
    };
    let Err(e) = res else {
        return true;
    };
    // 超时与普通写错分流：超时是"对端不收"，普通错是"通道坏了"——运维
    // 处置不同（前者查慢消费者，后者查进程/句柄），日志与计数都要分开。
    if timed_out {
        RR3_WRITE_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
        rr3_log(&format!(
            "写帧超时 dst={}：对端 {}s 不收，writer 收摊；连接交 reader 回收（{e}）",
            frame.header.dst_task,
            RR3_FAN_WRITE_TIMEOUT.as_secs()
        ));
    } else {
        rr3_log(&format!("写帧失败 dst={}: {e}", frame.header.dst_task));
    }
    false
}

/// 持锁写一帧（头 128B + payload 一次 `writev` + flush），返回 `(结果, 是否写超时)`。
/// 只做字节面，不落日志/计数——分类与点名留给唯一调用方 `rr3_write`。
///
/// **零拷贝、单次提交**：`rr3_frame_to_bytes` 会为每个收件连接各拼一份头+payload
/// （音频片约 11KB × 连接数），而广播帧在 rrd 内是共享体（`Rr3FanFrame`）——
/// 一层深拷已在扇出处消掉，写侧不该再把整帧复制一遍。这里用两段 iovec 一次
/// `write_vectored` 提交，既不复制，也不把单帧的写预算拆成两次系统调用
/// （`RR3_FAN_WRITE_TIMEOUT` 的口径仍是"一帧 5s"）。部分写只在本函数的锁内
/// 续投，别的帧插不进来；读侧本就按长度字段组帧，流上分段是正常形态。
fn rr3_write_bytes(guard: &mut UnixStream, frame: &Rr3Frame) -> (Result<(), String>, bool) {
    if frame.header.payload_len as usize != frame.payload.len() {
        let e = rr_ipc::Rr3FrameError::LengthMismatch {
            declares: frame.header.payload_len,
            actual: frame.payload.len(),
        };
        return (Err(format!("{e:?}")), false);
    }
    let hdr = rr_ipc::rr3_header_encode(&frame.header, &frame.payload);
    match rr3_write_both(guard, &hdr, &frame.payload).and_then(|_| guard.flush()) {
        Ok(()) => (Ok(()), false),
        Err(e) => (
            Err(e.to_string()),
            matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        ),
    }
}

/// 两段缓冲写到底：优先一次 `write_vectored`，部分写只推进已发片数。
/// `Ok(0)` 视同 `write_all` 的口径（`WriteZero`），不静默当成功。
fn rr3_write_both(guard: &mut UnixStream, hdr: &[u8; 128], payload: &[u8]) -> std::io::Result<()> {
    let mut bufs = [IoSlice::new(hdr), IoSlice::new(payload)];
    let mut rest: &mut [IoSlice<'_>] = &mut bufs;
    while rest.iter().any(|b| !b.is_empty()) {
        match guard.write_vectored(rest) {
            Ok(0) => return Err(std::io::Error::from(ErrorKind::WriteZero)),
            Ok(n) => IoSlice::advance_slices(&mut rest, n),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// 组一帧 rrd → 模块（ty=Response/Notification，dst=对端 task）
pub(super) fn rr3_out_frame(ty: Rr3PayloadType, dst_task: u64, body: &[u8]) -> Rr3Frame {
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
pub(super) fn rr3_reply_ok(out: &Arc<Mutex<UnixStream>>, me: &Rr3Peer, id: u64, result: &Value) {
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            me.task,
            &rr3_rpc_build_result(id, result),
        ),
    );
}

pub(super) fn rr3_reply_err(
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
pub(super) fn rr3_best_effort_id(body: &[u8]) -> u64 {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("id").and_then(Value::as_u64))
        .unwrap_or(0)
}

/// 入站帧处理结论
pub(super) enum Rr3Flow {
    /// 继续读下一帧
    Keep,
    /// 干净收尾（EOF / rr3.unregister）：不做崩溃记账
    QuitClean,
}

/// 注册期拒绝的统一出口：按对端自报的 src_task 回错误帧，再把原因交回上层断开连接
pub(super) fn rr3_refuse(
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
