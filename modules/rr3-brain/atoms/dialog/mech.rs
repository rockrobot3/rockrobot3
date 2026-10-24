//! dialog 原子件机制主体（五件套③，批N32 件四 Rust 拆分定形：utterance 边界
//! （onset 起轮 / offset 判停 / 时长上限判停）。纯逻辑不碰总线不碰网络不碰
//! JSON——机制核（ADR-0017 五层表机制层；件面零 std/serde 直引，⑧-b 依赖闭包
//! 纪律，alloc/契约层编解码同 rr-ipc 先例），事件体字段适配上移组合件
//! [`crate::dispatch`]（0.0.5 批I S-7 同款），编排消费面亦在该组合件。
//!
//! 状态机只管"一轮 utterance 的边界"：
//! - **onset** 边沿起一轮，此后到达的 pcm 帧持续积累（s16le 小端字节）；
//! - **offset** 边沿判停（sense 的 VAD 已按 silence_tail_ms 判静默——brain 不重复
//!   一套静默计时，两本真值是错误形状）；
//! - **时长上限** max_utterance_ms 到点同样判停（继承 legacy asr_begin(max_seconds)
//!   的精神：连续语音无 offset 时也要有界）；
//! - 上限之内的 pcm 帧在无 utterance 时丢弃（不开轮不积累）。
//!
//! 判停后交出的 PCM = **onset 前的预滚**（至多 [`RR3_PREROLL_MS`]）+ onset 起到
//! 判停帧为止的全部字节。预滚的必要性：sense 的 onset 事件要凑满 `min_speech_ms`
//! （config 200ms）连续有声才发出——这段「判据语音」本体在 onset 事件**之前**就
//! 已流过总线；无预滚则每轮削头 200ms+，短句（~0.5s 的「你好」级）所剩过短，云端
//! ASR 对过短音频回空文本（2026-10-21 真板 E2E 实测「识别为空文本」的健康流成因）。
//!
//! 时钟纪律（0.0.5 批I A-2 同款）：状态机不持时钟，`now_ms` 由调用方注入
//! （Dispatcher 的单调时基 [`rr_bus_api::Rr3TimeBase`]）——合成时钟逐拍注入，
//! 单测不真等墙钟。

extern crate alloc;

use alloc::collections::VecDeque;

use rr_ipc::rr3_base64_decode;

/// 预滚窗长（机制量）：≥ sense 侧 min_speech_ms（200）+ 事件 transit 余量；
/// 以帧自身 ms 字段计长，空帧（饥饿拍）不入环不计时。
pub(crate) const RR3_PREROLL_MS: i64 = 400;

/// 一轮对话的载体（对话历史登记项）。登记面 write-only：brain 现阶段只记不读
/// （Python Rr3DialogTurn 同形状——rr3_record 是方案 §4 的计数锚点），字段读取
/// 面（面板脑面回看等）属后续批。
#[allow(dead_code)]
pub struct Rr3DialogTurn {
    pub user_text: String,
    pub reply_text: String,
    pub trace_id: u64,
}

/// utterance 边界状态机（纯逻辑，不碰网络不碰总线）。
///
/// `rr3_feed_pcm` 吃组合件适配后的 pcm 载荷（b64 串＋毫秒长，sense 事件体
/// pcm_b64/ms 字段形状）；`rr3_feed_vad` 吃边沿名；判停时 `rr3_take_utterance`
/// 一次性交出累积的 PCM 字节并复位。
pub struct Rr3Dialog {
    /// 话轮封顶（ms）——ops/set dialog 组就地改参（live 生效不落盘，不丢开轮状态）
    pub max_utterance_ms: i64,
    pub history: Vec<Rr3DialogTurn>,
    active: bool,
    buf: Vec<u8>,
    started_ms: u64,
    /// 预滚环：未开轮期间滚动保留最近 RR3_PREROLL_MS 的 pcm，onset 时拼回采集头。
    /// 开轮期间的帧只进 buf（采集连续，预滚无职责）；判停复位后预滚照滚——下一轮
    /// onset 拿到的永远是「此刻之前」的窗（Python deque 同形状）。
    preroll: VecDeque<(Vec<u8>, i64)>,
    preroll_ms: i64,
}

impl Rr3Dialog {
    /// 装配构造（Dispatcher::new 装配段与测试夹具共用）。
    pub fn rr3_new(max_utterance_ms: i64) -> Self {
        Self {
            max_utterance_ms,
            history: Vec::new(),
            active: false,
            buf: Vec::new(),
            started_ms: 0,
            preroll: VecDeque::new(),
            preroll_ms: 0,
        }
    }

    /// 吃一帧 pcm（b64 串＋毫秒长）。开轮中解码积累进 utterance；未开轮进预滚环
    /// （超窗丢最旧）。空帧（饥饿拍）两边都不收。坏 base64 上抛 Err——Python
    /// binascii.Error 裸奔主循环同结局（传输性失败，进程死交 supervisor），按
    /// 同一档承载。
    pub fn rr3_feed_pcm(&mut self, pcm_b64: &str, ms: i64) -> Result<(), String> {
        let pcm = rr3_base64_decode(pcm_b64).map_err(|e| format!("pcm_b64 解码失败: {e:?}"))?;
        if pcm.is_empty() {
            return Ok(());
        }
        if !self.active {
            self.preroll.push_back((pcm, ms));
            self.preroll_ms += ms;
            while self.preroll_ms > RR3_PREROLL_MS && self.preroll.len() > 1 {
                let (_, old_ms) = self.preroll.pop_front().expect("len>1 保证非空");
                self.preroll_ms -= old_ms;
            }
            return Ok(());
        }
        self.buf.extend_from_slice(&pcm);
        Ok(())
    }

    /// 吃一帧 vad 边沿（组合件适配后的 edge 字段）。返回 true = 本边沿触发判停。
    pub fn rr3_feed_vad(&mut self, edge: &str, now_ms: u64) -> bool {
        match edge {
            "onset" => {
                self.active = true;
                self.buf = self
                    .preroll
                    .iter()
                    .flat_map(|(p, _)| p.iter().copied())
                    .collect();
                self.started_ms = now_ms;
                false
            }
            "offset" if self.active => true,
            _ => false,
        }
    }

    /// 开轮中且时长上限到点（主循环每圈问一次）。
    pub fn rr3_utterance_expired(&self, now_ms: u64) -> bool {
        self.active
            && now_ms.saturating_sub(self.started_ms) as i64 >= self.max_utterance_ms
    }

    /// 交出本轮 PCM 并复位。未开轮时交空（调用方应先看判停条件）。
    pub fn rr3_take_utterance(&mut self) -> Vec<u8> {
        let pcm = core::mem::take(&mut self.buf);
        self.active = false;
        pcm
    }

    /// 历史登记（完成一轮才记账——计数点 = rr3_record 成功，方案 §4 归属表）。
    pub fn rr3_record(&mut self, turn: Rr3DialogTurn) {
        self.history.push(turn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_ipc::rr3_base64_encode;

    /// pcm 载荷夹具（b64 串——组合件适配后的喂入口形状）
    fn feed_pcm(d: &mut Rr3Dialog, payload: &[u8], ms: i64) {
        d.rr3_feed_pcm(&rr3_base64_encode(payload), ms).unwrap();
    }

    /// 未开轮的 pcm 不进 utterance（take 交空——上限之内不开轮不积累）
    #[test]
    fn pcm_dropped_when_not_active() {
        let mut d = Rr3Dialog::rr3_new(8000);
        feed_pcm(&mut d, b"\x01\x02", 32);
        assert_eq!(d.rr3_take_utterance(), b"");
    }

    /// onset 起轮 → 积累 → offset 判停 → take 交出 → 复位；复位后未开轮的
    /// pcm 丢弃，take 再交空
    #[test]
    fn onset_starts_and_offset_stops() {
        let mut d = Rr3Dialog::rr3_new(8000);
        assert!(!d.rr3_feed_vad("onset", 100));
        assert!(d.active);
        feed_pcm(&mut d, b"\xaa\xbb", 32);
        assert!(d.rr3_feed_vad("offset", 200));
        let pcm = d.rr3_take_utterance();
        assert_eq!(pcm, b"\xaa\xbb");
        assert!(!d.active);
        feed_pcm(&mut d, b"\xcc\xdd", 32);
        assert_eq!(d.rr3_take_utterance(), b"");
    }

    /// offset 先于 onset：不判停不开轮
    #[test]
    fn offset_before_onset_does_not_stop() {
        let mut d = Rr3Dialog::rr3_new(8000);
        assert!(!d.rr3_feed_vad("offset", 0));
        assert!(!d.active);
    }

    /// 未判停的第二次 onset：重新起轮（buf 清零重来）
    #[test]
    fn onset_resets_previous_buffer() {
        let mut d = Rr3Dialog::rr3_new(8000);
        d.rr3_feed_vad("onset", 0);
        feed_pcm(&mut d, b"\x01\x02", 32);
        d.rr3_feed_vad("onset", 100);
        feed_pcm(&mut d, b"\x03\x04", 32);
        d.rr3_feed_vad("offset", 200);
        assert_eq!(d.rr3_take_utterance(), b"\x03\x04");
    }

    /// 不认的边沿静默忽略（Python unknown edge 同形状）
    #[test]
    fn unknown_edge_ignored() {
        let mut d = Rr3Dialog::rr3_new(8000);
        assert!(!d.rr3_feed_vad("whatever", 0));
        assert!(!d.active);
    }

    /// onset 前的判据语音（min_speech 段）经预滚环拼回采集头，短句不削头
    #[test]
    fn preroll_prepended_at_onset() {
        let mut d = Rr3Dialog::rr3_new(8000);
        feed_pcm(&mut d, b"\x01\x02", 200); // onset 前流过的帧（判据段）
        feed_pcm(&mut d, b"\x03\x04", 200);
        d.rr3_feed_vad("onset", 0);
        feed_pcm(&mut d, b"\x05\x06", 32); // onset 后第一帧
        d.rr3_feed_vad("offset", 100);
        assert_eq!(d.rr3_take_utterance(), b"\x01\x02\x03\x04\x05\x06");
    }

    /// 预滚只留最近 RR3_PREROLL_MS，更早的帧丢最旧（窗有界，utterance 不虚胖）。
    /// 30 × 32ms = 960ms ≫ 400ms 窗；稳态每推一帧弹至 ≤400ms，环内恒保窗长内
    /// 最完整的整帧组（384ms = 12 帧）
    #[test]
    fn preroll_capped_to_window() {
        let mut d = Rr3Dialog::rr3_new(8000);
        let n_frames = 30;
        for i in 0..n_frames {
            feed_pcm(&mut d, &[i as u8 + 1], 32);
        }
        d.rr3_feed_vad("onset", 0);
        d.rr3_feed_vad("offset", 0);
        let pcm = d.rr3_take_utterance();
        let n_kept = (RR3_PREROLL_MS / 32) as usize;
        let want: Vec<u8> = ((n_frames - n_kept + 1) as u8..=(n_frames as u8)).collect();
        assert_eq!(pcm, want);
    }

    /// 饥饿空帧（b64 解出空载荷）不入预滚不计时——窗长不被空拍虚占
    #[test]
    fn empty_frames_not_in_preroll() {
        let mut d = Rr3Dialog::rr3_new(8000);
        feed_pcm(&mut d, b"\x01\x02", 200);
        d.rr3_feed_pcm(&rr3_base64_encode(b""), 5000).unwrap(); // 空帧带大 ms：不得挤掉真帧
        d.rr3_feed_vad("onset", 0);
        d.rr3_feed_vad("offset", 0);
        assert_eq!(d.rr3_take_utterance(), b"\x01\x02");
    }

    /// 坏 base64 上抛 Err（Python binascii.Error 裸奔主循环同档：传输性失败）
    #[test]
    fn bad_base64_rejected() {
        let mut d = Rr3Dialog::rr3_new(8000);
        assert!(d.rr3_feed_pcm("!!!not-base64!!!", 32).is_err());
    }

    /// 历史登记保留轮次（user_text/trace_id 原样）
    #[test]
    fn record_keeps_turns() {
        let mut d = Rr3Dialog::rr3_new(8000);
        d.rr3_record(Rr3DialogTurn {
            user_text: "你好".into(),
            reply_text: "你好呀".into(),
            trace_id: 1,
        });
        assert_eq!(d.history.len(), 1);
        assert_eq!(d.history[0].user_text, "你好");
        assert_eq!(d.history[0].trace_id, 1);
    }

    // ---------- 件级两腿对拍（⑧-a，§4.0.3 判据②） ----------

    /// ⑧-a 腿一：边界序列确定性（同一脚本喂入两遍 → 逐字节同一 utterance）
    #[test]
    fn parity_leg1_dialog_boundary_train_is_deterministic() {
        let run = || {
            let mut d = Rr3Dialog::rr3_new(8000);
            feed_pcm(&mut d, b"\x11\x22", 200); // 判据段（预滚）
            d.rr3_feed_vad("onset", 0);
            feed_pcm(&mut d, b"\x33\x44", 32);
            feed_pcm(&mut d, b"\x55\x66", 32);
            assert!(d.rr3_feed_vad("offset", 100));
            d.rr3_take_utterance()
        };
        assert_eq!(run(), run());
        assert_eq!(run(), b"\x11\x22\x33\x44\x55\x66");
    }

    /// ⑧-a 腿二：时长上限判停有界（legacy asr_begin(max_seconds) 精神——连续
    /// 语音无 offset 也要有界；0 上限 onset 即刻到点）
    #[test]
    fn parity_leg2_dialog_duration_cap_bounded() {
        let mut d = Rr3Dialog::rr3_new(0);
        d.rr3_feed_vad("onset", 50);
        feed_pcm(&mut d, b"\x01\x02", 32);
        assert!(d.rr3_utterance_expired(50));
        assert_eq!(d.rr3_take_utterance(), b"\x01\x02");
        assert!(!d.rr3_utterance_expired(50));
    }
}
