//! 多通道 PCM 下混机制（W1-2 自 doa 拆出，W7-b3b 增零分配接口）：选能量最高的
//! 通道作为单声道，**不取平均**。2026-10-31 实机取证：双麦 150 mm 间距，原始
//! L/R 零延迟相关仅 +0.2，平均下混 RMS 比单声道低 2–3 dB 且带梳状滤波；单取
//! 能量高的通道识别明显更好。DoA 仍吃原始双通道（本件不参与判向）。
//!
//! 两种用法共用同一份能量遍历与抽取：
//! - [`rr3_pcm_best_channel`]：一次性整段（返回 `Vec`，内存路径收段时用）；
//! - [`rr3_chan_pick`] + [`rr3_chan_take`]：流式逐块（零分配，spool 路径用；
//!   起音定声道后段内不再跳变）。
//!
//! 并列能量取最小通道号；尾部不足一帧的余数丢弃；越界/空输入不 panic。

use alloc::vec;
use alloc::vec::Vec;

/// 多通道 PCM → 单声道：选能量最高的通道。`channels <= 1` 时原样克隆；
/// 尾部不足一帧的余数丢弃。
pub fn rr3_pcm_best_channel(samples: &[i16], channels: u8) -> Vec<i16> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let frames = samples.len() / channels as usize;
    if frames == 0 {
        return Vec::new();
    }
    let mut out = vec![0i16; frames];
    let sel = rr3_chan_pick(samples, channels);
    rr3_chan_take(samples, channels, sel, &mut out);
    out
}

/// 能量最高的通道号（并列取最小通道号）；**零分配**，供流式路径逐块定声道。
/// `channels == 0` 按单声道处理；空输入回 0。
pub fn rr3_chan_pick(samples: &[i16], channels: u8) -> usize {
    let ch = (channels as usize).max(1);
    let frames = samples.len() / ch;
    let mut best = 0usize;
    let mut best_e = 0i64;
    for c in 0..ch {
        let e = rr3_chan_energy(samples, ch, frames, c);
        if e > best_e {
            best = c;
            best_e = e;
        }
    }
    best
}

/// 单通道整数能量（`sum_sq`，免开方；i64 覆盖 8 s × 16 kHz 满幅）。
fn rr3_chan_energy(samples: &[i16], ch: usize, frames: usize, c: usize) -> i64 {
    let mut sum = 0i64;
    for i in 0..frames {
        let v = samples[i * ch + c] as i64;
        sum += v * v;
    }
    sum
}

/// 按选中通道抽单声道写入 `out`，返回写入样本数（≤ 帧数，尾部余数丢弃）。
/// **零分配**：`out` 短于帧数时截断，通道号越界回 0——都不 panic。
pub fn rr3_chan_take(samples: &[i16], channels: u8, sel: usize, out: &mut [i16]) -> usize {
    let ch = (channels as usize).max(1);
    if sel >= ch {
        return 0;
    }
    let frames = (samples.len() / ch).min(out.len());
    for (i, o) in out.iter_mut().take(frames).enumerate() {
        *o = samples[i * ch + sel];
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// 多通道单声道化：选能量最高通道、单声道原样、并列取首通道、空输入安全。
    #[test]
    fn best_channel_picks_louder_and_mono_passthrough() {
        assert_eq!(rr3_pcm_best_channel(&[1, -2, 3], 1), vec![1, -2, 3]);
        assert_eq!(rr3_pcm_best_channel(&[1, -2, 3], 0), vec![1, -2, 3]);
        assert_eq!(
            rr3_pcm_best_channel(&[1000, 100, 2000, 200, -500, -50], 2),
            vec![1000, 2000, -500]
        );
        assert_eq!(
            rr3_pcm_best_channel(&[100, 900, 50, 1500, -30, 700], 2),
            vec![900, 1500, 700]
        );
        assert_eq!(rr3_pcm_best_channel(&[0, 0, 0, 0], 2), vec![0, 0]);
        assert_eq!(
            rr3_pcm_best_channel(&[0, 0, 0, 4, 8, 8, 8, 8], 4),
            vec![4, 8]
        );
        assert!(rr3_pcm_best_channel(&[], 2).is_empty());
    }

    /// 零分配定声道：能量高者胜、并列取最小通道、单声道/空输入回 0。
    #[test]
    fn chan_pick_matches_energy_order() {
        assert_eq!(rr3_chan_pick(&[1000, 100, 2000, 200], 2), 0);
        assert_eq!(rr3_chan_pick(&[100, 900, 50, 1500], 2), 1);
        assert_eq!(rr3_chan_pick(&[0, 0, 0, 0], 2), 0, "并列取首通道");
        assert_eq!(rr3_chan_pick(&[7, -7, 7], 1), 0);
        assert_eq!(rr3_chan_pick(&[7, -7, 7], 0), 0);
        assert_eq!(rr3_chan_pick(&[], 2), 0);
    }

    /// 零分配抽通道：写入选中通道、`out` 短则截断、越界不 panic。
    #[test]
    fn chan_take_writes_selected_and_truncates() {
        let s = [1000, 100, 2000, 200, -500, -50];
        let mut full = [0i16; 3];
        assert_eq!(rr3_chan_take(&s, 2, 1, &mut full), 3);
        assert_eq!(full, [100, 200, -50]);
        let mut one = [0i16; 1];
        assert_eq!(rr3_chan_take(&s, 2, 0, &mut one), 1);
        assert_eq!(one, [1000]);
        let mut none = [0i16; 2];
        assert_eq!(rr3_chan_take(&s, 2, 2, &mut none), 0, "通道号越界回 0");
        assert_eq!(rr3_chan_take(&[], 2, 0, &mut none), 0);
        let mut mono = [0i16; 3];
        assert_eq!(rr3_chan_take(&[1, -2, 3, 4], 1, 0, &mut mono), 3);
        assert_eq!(mono, [1, -2, 3]);
    }

    /// 流式两函数与分配版逐位同源：pick+take 必须复现 best_channel。
    #[test]
    fn pick_take_reproduce_best_channel() {
        for (samples, ch) in [
            (vec![1000i16, 100, 2000, 200, -500, -50], 2u8),
            (vec![100, 900, 50, 1500, -30, 700], 2),
            (vec![0, 0, 0, 4, 8, 8, 8, 8], 4),
            (vec![1, -2, 3], 1),
        ] {
            let sel = rr3_chan_pick(&samples, ch);
            let mut out = vec![0i16; samples.len() / ch.max(1) as usize];
            let n = rr3_chan_take(&samples, ch, sel, &mut out);
            out.truncate(n);
            assert_eq!(out, rr3_pcm_best_channel(&samples, ch), "ch={ch}");
        }
    }
}
