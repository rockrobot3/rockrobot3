// SPDX-License-Identifier: Apache-2.0
//! voice 分段落盘/入队：起音开段、内存/spool 累积、收段交 dialog。

use rr_audio_mech::rr3_pcm_best_channel;

use crate::state::{lock_bus, rr3_expr_set};

use super::*;

impl VoiceRun {
    /// 起音：DoA 转向/聆听表情 → 按承载方式开段（内存补预滚 / spool 逐块落盘）。
    ///
    /// `oled_hold`（标定保持）：只判向与出表情，不落段——避免云端往返/回授打断
    /// 标定节奏，也避免无谓的 flash 写入。
    pub(super) fn onset(&mut self, bus: &Arc<Mutex<Bus>>) {
        self.recording = true;
        self.doa_blocks = 0;
        self.track.rr3_reset();
        let steered = self.steer(bus, "onset");
        rr3_expr_set(bus, if steered { "turn" } else { "listen" });
        let pre_ms = self.pre.len() * 1000 / (MIC_SR_HZ as usize * 2);
        let doa = if steered { "steer" } else { "center/none" };
        let hold = lock_bus(bus).st.oled_hold;
        if hold {
            self.recording = false;
            println!("[mic] VAD onset（预滚 {pre_ms}ms）doa={doa} hold=只判向不落段");
            return;
        }
        match self.mode {
            Rr3VoiceMode::Mem => self.open_mem(pre_ms, doa),
            Rr3VoiceMode::Flash => self.open_flash(pre_ms, doa),
            Rr3VoiceMode::Off => self.recording = false,
        }
    }

    /// 内存路径开段：预滚整窗补进段头（唤醒前缀容错）。
    fn open_mem(&mut self, pre_ms: usize, doa: &str) {
        self.utter.clear();
        self.utter.extend_from_slice(&self.pre);
        println!("[mic] VAD onset（预滚 {pre_ms}ms）doa={doa} path=mem");
    }

    /// spool 路径开段：定声道 + 预滚逐块落盘。
    fn open_flash(&mut self, pre_ms: usize, doa: &str) {
        self.flash_open();
        println!(
            "[mic] VAD onset（预滚 {pre_ms}ms）doa={doa} path=spool sel={} seg={}",
            self.sel, self.seg
        );
    }

    /// spool 开段：起音窗定声道（能量高者）→ 预滚逐块落盘；失败即弃段。
    fn flash_open(&mut self) {
        self.sel = rr3_chan_pick(&self.pre, 2);
        self.sent = 0;
        self.seg = crate::spool::rr3_spool_begin();
        if !self.seg {
            self.recording = false;
            self.dropped = self.dropped.saturating_add(1);
            println!(
                "[mic] spool 忙/未装配：弃段（累计 {}；上一段待上传或写线程退出）",
                self.dropped
            );
            return;
        }
        // 预滚窗临时移出：迭代期间还要可变借用 self 写 spool（Vec move 零拷贝），
        // 写完原样放回，下一段起音的预测窗从当前滚动位置继续。
        let pre = std::mem::take(&mut self.pre);
        for chunk in pre.chunks_exact(BLOCK_SAMPLES) {
            let mut mono = [0i16; BLK_MONO];
            rr3_chan_take(chunk, 2, self.sel, &mut mono);
            if !self.flash_push(&mono) {
                break;
            }
        }
        self.pre = pre;
    }

    /// spool 写一块（非阻塞）；失败即弃段并回报调用方停止累积。
    fn flash_push(&mut self, mono: &[i16; BLK_MONO]) -> bool {
        if !self.seg {
            return false;
        }
        if crate::spool::rr3_spool_block(mono) {
            self.sent += mono.len();
            return true;
        }
        self.seg = false;
        self.recording = false;
        self.dropped = self.dropped.saturating_add(1);
        println!("[mic] spool 写失败/队列满：弃段（累计 {}）", self.dropped);
        crate::spool::rr3_spool_abort();
        false
    }

    /// 录音累积 + 超长强制收段；spool 路径逐块落盘，不驻留整段。
    pub(super) fn append(&mut self, bus: &Arc<Mutex<Bus>>, mono: &[i16; BLK_MONO], block: &[i16]) {
        if self.mode == Rr3VoiceMode::Flash {
            if !self.seg {
                self.recording = false;
                return;
            }
            if self.sent + mono.len() > MAX_SEG_MONO {
                self.finish(bus, "max-utterance");
                return;
            }
            self.flash_push(mono);
            return;
        }
        let room = MAX_UTTER_SAMPLES.saturating_sub(self.utter.len());
        if block.len() > room {
            self.utter.extend_from_slice(&block[..room]);
            self.finish(bus, "max-utterance");
            return;
        }
        self.utter.extend_from_slice(block);
        if self.utter.len() >= MAX_UTTER_SAMPLES {
            self.finish(bus, "max-utterance");
        }
    }

    /// 收段：按承载方式交 dialog（内存）或提交 spool（写线程入云队列）。
    pub(super) fn finish(&mut self, bus: &Arc<Mutex<Bus>>, why: &str) {
        self.track.rr3_reset();
        self.last_bin = None;
        self.doa_blocks = 0;
        if self.mode == Rr3VoiceMode::Flash {
            return self.finish_flash(bus, why);
        }
        let mono = rr3_pcm_best_channel(&self.utter, 2);
        let ms = mono.len() * 1000 / MIC_SR_HZ as usize;
        self.recording = false;
        self.utter.clear();
        self.vad = Rr3VadState::rr3_vad_new(self.vad_cfg, FRAME_MS);
        if mono.is_empty() {
            return;
        }
        match crate::cloud::rr3_turn(mono) {
            Ok(()) => println!("[mic] 语音段入队 {ms}ms（{why}）"),
            Err(e) => {
                self.dropped = self.dropped.saturating_add(1);
                if self.dropped == 1 || self.dropped % 20 == 0 {
                    println!(
                        "[mic] 语音段入队失败 {ms}ms（{why}）: {e}（累计 {}）",
                        self.dropped
                    );
                }
            }
        }
        rr3_expr_set(bus, "think");
    }

    /// spool 收段：提交记录（写线程校 CRC 后入云队列），本线程立即可录下一段。
    fn finish_flash(&mut self, bus: &Arc<Mutex<Bus>>, why: &str) {
        let ms = self.sent * 1000 / MIC_SR_HZ as usize;
        let seg = self.seg;
        self.seg = false;
        self.sent = 0;
        self.recording = false;
        self.vad = Rr3VadState::rr3_vad_new(self.vad_cfg, FRAME_MS);
        if seg {
            if crate::spool::rr3_spool_finish() {
                println!("[mic] 语音段 {ms}ms 已提交 spool（{why}）");
            } else {
                self.dropped = self.dropped.saturating_add(1);
                println!(
                    "[mic] spool 提交投递失败 {ms}ms（{why}；累计 {}）",
                    self.dropped
                );
                crate::spool::rr3_spool_abort();
            }
        }
        rr3_expr_set(bus, "think");
    }
}
