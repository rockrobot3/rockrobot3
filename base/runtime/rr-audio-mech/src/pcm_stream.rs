// SPDX-License-Identifier: Apache-2.0
//! 播放侧 PCM 流式整形（W7-b2）：源字节分块喂入 → I2S 立体声块输出，首块淡入、
//! 末块淡出按**全局帧号**计算。与 [`crate::rr3_pcm_stereo`] 的整段整形逐位同源，
//! 区别只在本件跨块保持帧边界与淡化相位——HTTP 4KB 收包不必先聚合成整段 Vec。
//!
//! 契约：源格式只接受 s16le 单/双声道（[`Rr3PcmStream::rr3_new`] 门）；字节数必须
//! 整帧，EOF 时残留半帧显式报错，不静默丢弃。总源字节数已知时（HTTP Content-Length）
//! 尾 `fade_frames` 帧淡出；未知时不猜总长，只做淡入（调用方须点名该降级）。
//!
//! 整数纪律同 pcm.rs：Q15 增益、`>>15` 饱和，无浮点。

use alloc::vec::Vec;

use crate::pcm::{rr3_apply, rr3_fade_gain};

/// 跨块 PCM 整形器（单线程独占；不并发）。
pub struct Rr3PcmStream {
    channels: u16,
    fade_frames: usize,
    /// 总帧数（来自 Content-Length 的源字节数；未知 = None）。
    total_frames: Option<u64>,
    /// 已消费的源帧数。
    frames: u64,
    /// 已消费的源字节数（含残留半帧）。
    src_bytes: usize,
    /// 不足一帧的残留字节（帧 ≤ 4 B）。
    carry: [u8; 4],
    carry_len: usize,
}

impl Rr3PcmStream {
    /// `total_src_bytes`：源载荷总字节（不含 WAV 头）；`None` = 长度未知（只淡入）。
    pub fn rr3_new(
        channels: u16,
        total_src_bytes: Option<usize>,
        fade_frames: usize,
    ) -> Result<Self, &'static str> {
        if !matches!(channels, 1 | 2) {
            return Err("PCM 声道数不是 1/2");
        }
        let frame_bytes = channels as usize * 2;
        let total_frames = total_src_bytes.map(|n| (n / frame_bytes) as u64);
        Ok(Self {
            channels,
            fade_frames,
            total_frames,
            frames: 0,
            src_bytes: 0,
            carry: [0u8; 4],
            carry_len: 0,
        })
    }

    /// 喂一块源字节：完整帧立即整形进 `out`（不清空，调用方自行管理），残帧留到
    /// 下一块；返回本次消费的源字节数（恒等于 `src.len()`）。
    pub fn rr3_feed(&mut self, src: &[u8], out: &mut Vec<u8>) -> Result<usize, &'static str> {
        let fb = self.channels as usize * 2;
        let mut i = 0usize;
        while self.carry_len > 0 && self.carry_len < fb && i < src.len() {
            self.carry[self.carry_len] = src[i];
            self.carry_len += 1;
            i += 1;
        }
        if self.carry_len == fb {
            let frame = self.carry;
            self.emit(&frame[..fb], out);
            self.carry_len = 0;
        }
        let frames = (src.len() - i) / fb;
        for f in 0..frames {
            let o = i + f * fb;
            self.emit(&src[o..o + fb], out);
        }
        let tail = &src[i + frames * fb..];
        self.carry[..tail.len()].copy_from_slice(tail);
        self.carry_len = tail.len();
        self.src_bytes += src.len();
        Ok(src.len())
    }

    /// EOF 收尾：残留半帧必须为 0（整帧契约），否则显式报错。
    pub fn rr3_finish(&self) -> Result<(), &'static str> {
        if self.carry_len == 0 {
            Ok(())
        } else {
            Err("PCM 字节数不是整帧（EOF 残留半帧）")
        }
    }

    /// 已消费的源字节数（ASR/TTS 时长与完整性对账用）。
    pub fn rr3_src_bytes(&self) -> usize {
        self.src_bytes
    }

    /// 已整形的源帧数。
    #[cfg(test)]
    pub fn rr3_frames(&self) -> u64 {
        self.frames
    }

    /// 单帧整形：全局帧号决定淡入/淡出增益，单声道复制、双声道直通。
    fn emit(&mut self, frame: &[u8], out: &mut Vec<u8>) {
        let fb = self.channels as usize * 2;
        let i = self.frames as usize;
        let gain = match self.total_frames {
            Some(total) => {
                let total = total.max(1) as usize;
                rr3_fade_gain(i.min(total - 1), total, self.fade_frames, self.fade_frames)
            }
            // 总长未知：`fade_out=0` 时末项恒满幅，`frames` 取 i+1 无副作用。
            None => rr3_fade_gain(i, i + 1, self.fade_frames, 0),
        };
        let l = rr3_apply(i16::from_le_bytes([frame[0], frame[1]]), gain);
        let r = rr3_apply(i16::from_le_bytes([frame[fb - 2], frame[fb - 1]]), gain);
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
        self.frames += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rr3_pcm_stereo;

    fn bytes(samples: &[i16]) -> Vec<u8> {
        let mut out = Vec::new();
        for s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    #[test]
    fn whole_buf_shaping() {
        let src = bytes(&[1000, -1000, 2000, -2000, 3000, -3000]);
        let mut want = Vec::new();
        rr3_pcm_stereo(&src, 1, 2, 2, &mut want).unwrap();
        let mut st = Rr3PcmStream::rr3_new(1, Some(src.len()), 2).unwrap();
        let mut got = Vec::new();
        st.rr3_feed(&src, &mut got).unwrap();
        st.rr3_finish().unwrap();
        assert_eq!(got, want);
        assert_eq!(st.rr3_frames(), 6);
    }

    #[test]
    fn bytewise_fade_phase() {
        let src = bytes(&[1000, -1000, 2000, -2000, 3000, -3000]);
        let mut want = Vec::new();
        rr3_pcm_stereo(&src, 1, 2, 2, &mut want).unwrap();
        let mut st = Rr3PcmStream::rr3_new(1, Some(src.len()), 2).unwrap();
        let mut got = Vec::new();
        for b in &src {
            st.rr3_feed(core::slice::from_ref(b), &mut got).unwrap();
        }
        st.rr3_finish().unwrap();
        assert_eq!(got, want, "逐字节喂入必须与整段逐位一致");
    }

    #[test]
    fn stereo_odd_tail_rej() {
        let src = bytes(&[1, 2, 3, 4]);
        let mut st = Rr3PcmStream::rr3_new(2, Some(src.len()), 0).unwrap();
        let mut got = Vec::new();
        st.rr3_feed(&src, &mut got).unwrap();
        assert_eq!(got, src);
        st.rr3_finish().unwrap();

        let mut st = Rr3PcmStream::rr3_new(2, None, 0).unwrap();
        let mut out = Vec::new();
        st.rr3_feed(&[0u8; 3], &mut out).unwrap();
        assert!(st.rr3_finish().unwrap_err().contains("整帧"));
        assert!(Rr3PcmStream::rr3_new(3, None, 0).is_err());
    }

    #[test]
    fn unknown_fade_out() {
        let src = bytes(&[1000, 1000]);
        let mut st = Rr3PcmStream::rr3_new(1, None, 1).unwrap();
        let mut got = Vec::new();
        st.rr3_feed(&src, &mut got).unwrap();
        let first = i16::from_le_bytes([got[0], got[1]]);
        let last = i16::from_le_bytes([got[got.len() - 2], got[got.len() - 1]]);
        assert_eq!(first, 0, "首帧淡入为 0");
        assert_eq!(last, 1000, "长度未知不做淡出");
    }
}
