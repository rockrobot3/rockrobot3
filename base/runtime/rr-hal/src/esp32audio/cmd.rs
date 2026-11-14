//! 命令旁路：有界命令队列与产/耗两端。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rr_periph_proto::cmd::Rr3PeriphCmd;

/// 命令旁路队列深度（策略量）：反射环 onset 驱动天然低频（VAD min_speech +
/// silence_tail 下限节流），2 深吸收偶发连发；满丢最旧保最新意图（板侧
/// audio_q 同款背压口径）。
const RR3_AUDIO_CMD_CAP: usize = 2;

/// 命令旁路盒（Mutex 内纯数据）：有界、满丢最旧 + 溢丢计数（板侧 audio_q
/// 同款背压口径）。产面归构造方（rr3-sense 反射环），耗面归读线程独占。
struct Rr3AudioCmdShared {
    deque: Mutex<VecDeque<Rr3PeriphCmd>>,
    cap: usize,
    dropped: AtomicU64,
}

/// 命令旁路产面（Arc 克隆廉价；push 永不阻塞）。
#[derive(Clone)]
pub struct Rr3AudioCmdProducer {
    shared: Arc<Rr3AudioCmdShared>,
}

impl Rr3AudioCmdProducer {
    /// 推一条命令（满 = 丢最旧 + 计数，永不阻塞——反射环节拍不容背压等待）。
    pub fn rr3_cmd_push(&self, cmd: Rr3PeriphCmd) {
        let mut q = self.shared.deque.lock().unwrap_or_else(|p| p.into_inner());
        if q.len() >= self.shared.cap {
            q.pop_front();
            self.shared.dropped.fetch_add(1, Ordering::Release);
        }
        q.push_back(cmd);
    }

    /// 溢丢计数（记账对账面；>0 = 反射环产速超过排水能力，点名可见）。
    pub fn rr3_cmd_dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Acquire)
    }
}

/// 命令旁路耗面（读线程独占——单写者纪律：conn 的全部写收拢读线程）。
#[derive(Clone)]
pub(super) struct Rr3AudioCmdConsumer {
    shared: Arc<Rr3AudioCmdShared>,
}

impl Rr3AudioCmdConsumer {
    /// 弹一条（空回 None）——逐条弹出制：写失败可回插队首不丢命令。
    pub(super) fn rr3_cmd_pop(&self) -> Option<Rr3PeriphCmd> {
        let mut q = self.shared.deque.lock().unwrap_or_else(|p| p.into_inner());
        q.pop_front()
    }

    /// 回插队首（io 失败保命：命令不丢，重连后补写；不计溢丢账）。
    pub(super) fn rr3_cmd_push_front(&self, cmd: Rr3PeriphCmd) {
        let mut q = self.shared.deque.lock().unwrap_or_else(|p| p.into_inner());
        q.push_front(cmd);
    }
}

/// 产/耗面对（cap 见 [`RR3_AUDIO_CMD_CAP`]）。
pub(super) fn rr3_audio_cmd_pair() -> (Rr3AudioCmdProducer, Rr3AudioCmdConsumer) {
    let shared = Arc::new(Rr3AudioCmdShared {
        deque: Mutex::new(VecDeque::with_capacity(RR3_AUDIO_CMD_CAP)),
        cap: RR3_AUDIO_CMD_CAP,
        dropped: AtomicU64::new(0),
    });
    (
        Rr3AudioCmdProducer {
            shared: Arc::clone(&shared),
        },
        Rr3AudioCmdConsumer { shared },
    )
}
