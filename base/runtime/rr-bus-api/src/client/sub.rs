//! 收帧、reader 线程与派发（应答按 id 路由、其余进 inbox）。

use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use rr_core::runtime::{rr3_dispatch_class, Rr3Dispatch};
use rr_ipc::{
    rr3_body_split, rr3_frame_read_cap, Rr3Frame, Rr3PayloadType, RR3_FRAMING_PLEN_CAP_DEFAULT,
};
use serde_json::Value;

use super::conn::{Rr3BusClient, Rr3Inner, RR3_COUNTER_INBOX_BACKLOG_DROP, RR3_INBOX_CAP};

impl Rr3BusClient {
    /// 总线还活着吗（reader 线程在 = 活；reader 断开 = 死）。轮询型主循环的死亡探针：
    /// [`Rr3BusClient::rr3_poll_wait`] 在断开时**立即**返回空 `Vec`，与"暂时没帧"不可
    /// 区分——不探测的话，总线一死主循环就满转成僵尸。每圈探一次，假了就走
    /// "模块不装活着"的退出语义（与本文件头的心跳失败同一裁定）。
    pub fn rr3_connected(&self) -> bool {
        !self.inner.stop.load(Ordering::Relaxed)
    }

    pub fn rr3_name(&self) -> &str {
        &self.inner.name
    }

    /// 非阻塞收帧：取走当前积压的全部非应答帧（Event / Notification / 点名 Request）。
    /// 模块主循环的标准姿势：`poll` → 逐帧分派 → `sleep`。
    pub fn rr3_poll(&self) -> Vec<Rr3Frame> {
        let rx = self.inner.inbox.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            out.push(frame);
        }
        rr3_inbox_consume(&self.inner, out.len());
        out
    }

    /// 带等待的收帧：先等第一帧（至多 `timeout`），再非阻塞排空余量。
    /// reader 断开（总线没了）时返回空 `Vec`，模块靠 `rr3_call` 的错误发现死活。
    pub fn rr3_poll_wait(&self, timeout: Duration) -> Vec<Rr3Frame> {
        let rx = self.inner.inbox.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = Vec::new();
        match rx.recv_timeout(timeout) {
            Ok(frame) => out.push(frame),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return out,
        }
        while let Ok(frame) = rx.try_recv() {
            out.push(frame);
        }
        rr3_inbox_consume(&self.inner, out.len());
        out
    }
}

/// 消费侧水位回退：取走多少帧就减多少（reader 入箱前先加，故不会欠账）。
fn rr3_inbox_consume(inner: &Arc<Rr3Inner>, n: usize) {
    if n > 0 {
        inner.inbox_len.fetch_sub(n, Ordering::Relaxed);
    }
}

/// reader 主循环：Response 按 id 投邮箱，其余进 inbox；连接死亡时唤醒所有等待者。
pub(super) fn rr3_reader_loop(
    sock: &mut UnixStream,
    inner: Arc<Rr3Inner>,
    inbox_tx: mpsc::Sender<Rr3Frame>,
) {
    // EOF / 超时 / 坏帧：统一按断开处理。策略帽 1MB（0.0.8b 审计 B-7）：
    // 坏 plen 在分配前拒收，撕裂流/对端 bug 不能按 64MB 协议上界撑爆本进程。
    while let Ok(frame) = rr3_frame_read_cap(&mut *sock, RR3_FRAMING_PLEN_CAP_DEFAULT) {
        if inner.stop.load(Ordering::Relaxed) {
            break;
        }
        if !rr3_reader_frame(frame, &inner, &inbox_tx) {
            break;
        }
    }
    rr3_reader_teardown(&inner);
}

/// 单帧裁决：`true` = 继续读，`false` = 断开（坏帧/机身失配/inbox 已关）。
fn rr3_reader_frame(
    frame: Rr3Frame,
    inner: &Arc<Rr3Inner>,
    inbox_tx: &mpsc::Sender<Rr3Frame>,
) -> bool {
    let (ty, body) = match rr3_body_split(&frame.payload) {
        Ok(v) => v,
        Err(_) => return false,
    };
    match rr3_dispatch_class(ty) {
        // 按 JSON-RPC id 挑自己的应答（序不可依赖的同一条事实，读侧的答案）
        Rr3Dispatch::Pending => rr3_pending_reply(inner, &body),
        Rr3Dispatch::Inbox => rr3_inbox_push(frame, ty, inner, inbox_tx),
    }
}

/// 非应答帧入箱：满箱时 Event/Stream **只丢帧并计数**（总线对事件本来就是尽力
/// 投递，rrd 的 fanout 满队列同策略），Request/Notification 一律入箱——控制面
/// 不丢。reader 永不停在 send 上：长动作（ASR 超时重试、整段 TTS 播放）不再
/// 被放大成"心跳应答读不到 → 模块自杀"。`false` = inbox 已关（客户端已死）。
fn rr3_inbox_push(
    frame: Rr3Frame,
    ty: Rr3PayloadType,
    inner: &Arc<Rr3Inner>,
    inbox_tx: &mpsc::Sender<Rr3Frame>,
) -> bool {
    let droppable = matches!(ty, Rr3PayloadType::Event | Rr3PayloadType::Stream);
    if droppable && inner.inbox_len.load(Ordering::Relaxed) >= RR3_INBOX_CAP {
        inner.rr3_counter_add(RR3_COUNTER_INBOX_BACKLOG_DROP, 1);
        return true;
    }
    // 先记水位再入箱：消费侧收到帧时一定已计过账，fetch_sub 不会欠账。
    inner.inbox_len.fetch_add(1, Ordering::Relaxed);
    if inbox_tx.send(frame).is_err() {
        inner.inbox_len.fetch_sub(1, Ordering::Relaxed);
        return false;
    }
    true
}

/// 应答投邮箱：按 id 取等待者，命中则投 result / 拒因；无主应答静默丢弃
/// （超时已被调用方记账）。`false` = 应答体坏了，读侧按断开处理。
fn rr3_pending_reply(inner: &Arc<Rr3Inner>, body: &[u8]) -> bool {
    let v: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
    let sender = inner
        .pending
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&id);
    if let Some(tx) = sender {
        let payload = match v.get("result") {
            Some(r) => Ok(r.clone()),
            None => Err(format!(
                "RPC 被拒: {}",
                v.get("error").cloned().unwrap_or(Value::Null)
            )),
        };
        let _ = tx.send(payload);
    }
    true
}

/// 收尾：唤醒所有还在等应答的调用方，让它们拿到确定性错误而不是吃满超时。
fn rr3_reader_teardown(inner: &Arc<Rr3Inner>) {
    let mut pending = inner.pending.lock().unwrap_or_else(|p| p.into_inner());
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err("总线已断开".into()));
    }
    inner.stop.store(true, Ordering::Relaxed);
}
