//! capture 原子件 · 机制核（批N23 件三，MC-7 缺② 收编原子模块形态）。
//!
//! **格位对齐申报**：本件服务 `audio.capture` 格位（esp32-mini 装配单格主 =
//! rr3-capture 宿主件，板侧经桥执行——宿主申报面在 modules/rr3-capture/atoms/
//! capture，板侧执行面**不冒领格主**、不立 atom.toml：`provides` 恰一格与板无关
//! 断言是格主面法则，见批N23 计划册 §2「三不三立」）。
//!
//! **机制与策略分离**（文件级，收编落点）：本文件收拢音频采集上行面的**纯算术**
//! ——32bit 帧解析（大端线序 → 24bit 符号幅度）、增益饱和钳位、分片变换（逐片
//! DC 抑制 → 24bit 截 16bit → s16le 双通道交错）与有界背压入队（满丢最旧）；
//! 零 I2S、零时钟、零锁。策略量（增益标定值 [`crate::board::MIC_GAIN`]、采样率
//! [`crate::board::MIC_SR_HZ`]、分片帧数/队列深度 `AUDIO_*`）由调用方**按参
//! 传入**；I2S 采集线程与 Event 帧组帧（b64+JSON+帧头，依赖时钟与 topics 生成
//! 物）留 [`crate::mic`]——策略→机制单向依赖。

use std::collections::VecDeque;

/// 32bit 标准帧 → 双通道 24bit 符号幅度（大端线序、有效位左对齐：右移 8 位得
/// 符号扩展幅度——INMP441 24bit MSB first，mic.rs 头注 API 对表同源）。
pub(crate) fn frame32_to_pair(frame: &[u8]) -> (i32, i32) {
    (
        i32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) >> 8,
        i32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]) >> 8,
    )
}

/// 增益 + 饱和钳位：输入 16bit 域采样值，×gain 后钳回 i16。**钳位不翻卷**——
/// 翻卷产生假静音假爆音，比削顶伤；增益为真板标定策略量（board.rs），按参传入。
pub(crate) fn gain_i16(v: i64, gain: i64) -> i16 {
    let g = v * gain;
    if g > i16::MAX as i64 {
        i16::MAX
    } else if g < i16::MIN as i64 {
        i16::MIN
    } else {
        g as i16
    }
}

/// 一分片 → s16le PCM 字节流（**纯变换**）：逐通道减**本分片** DC（INMP441 近零
/// 偏置，分片内均值即干净基线，无跨片状态）→ 24bit 截 16bit（>>8 保高位满幅）→
/// 增益钳位 → s16le 双通道交错。零 I2S/零时钟/零锁（返回值即产物；b64+JSON 组帧
/// 面在 mic.rs）。空分片与残半帧（奇数采样）回空——组帧面 frames==0 拒收同口径。
pub(crate) fn chunk_to_pcm(chunk: &[i32], gain: i64) -> Vec<u8> {
    let frames = chunk.len() / 2;
    let mut l_dc = 0i64;
    let mut r_dc = 0i64;
    for f in 0..frames {
        l_dc += chunk[f * 2] as i64;
        r_dc += chunk[f * 2 + 1] as i64;
    }
    if frames > 0 {
        l_dc /= frames as i64;
        r_dc /= frames as i64;
    }
    let mut pcm = Vec::with_capacity(frames * 4);
    for f in 0..frames {
        let l = gain_i16((chunk[f * 2] as i64 - l_dc) >> 8, gain);
        let r = gain_i16((chunk[f * 2 + 1] as i64 - r_dc) >> 8, gain);
        pcm.extend_from_slice(&l.to_le_bytes());
        pcm.extend_from_slice(&r.to_le_bytes());
    }
    pcm
}

/// 有界队列背压入队（**满丢最旧**）：队列深度达 cap 即弹出最旧腾位；返回
/// `true` = 发生丢弃（调用方计 `audio_dropped`——背压观测前哨，与 recorder 端
/// seq 断档互为对账）。纯数据面操作不碰锁（锁面在调用方，mic.rs 取号/入队
/// 两小段纪律不变）。
pub(crate) fn push_bounded(q: &mut VecDeque<Vec<u8>>, item: Vec<u8>, cap: usize) -> bool {
    let dropped = q.len() >= cap;
    if dropped {
        q.pop_front();
    }
    q.push_back(item);
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_i16_clamps_at_saturation() {
        assert_eq!(gain_i16(100, 8), 800);
        assert_eq!(gain_i16(0, 8), 0);
        assert_eq!(gain_i16(4095, 8), 32760, "顶格不饱和");
        assert_eq!(gain_i16(4096, 8), i16::MAX, "正饱和钳位不翻卷");
        assert_eq!(gain_i16(-5000, 8), i16::MIN, "负饱和钳位不翻卷");
    }

    #[test]
    fn frame32_to_pair_sign_extends_24bit() {
        // L = 0x00123456（24bit 值 0x123456）>>8 = 0x1234 = 4660；
        // R = 0xFFEDCBA9（24bit 值 0xFFEDCB 负值）>>8 = 符号扩展 0xFFFFEDCB = -4661
        let (l, r) = frame32_to_pair(&[0x00, 0x12, 0x34, 0x56, 0xFF, 0xED, 0xCB, 0xA9]);
        assert_eq!(l, 4660);
        assert_eq!(r, -4661);
    }

    #[test]
    fn chunk_to_pcm_strips_dc_and_interleaves_s16le() {
        // 2 帧：L=[256, 512] R=[128, 384] → L_dc=384、R_dc=256
        // 差值 >>8：L=(-128>>8, 128>>8)=(-1, 0)；R=(-1, 0)
        let chunk = [256i32, 128, 512, 384];
        // gain=1：[-1, -1, 0, 0] 的 s16le 交错字节流
        assert_eq!(
            chunk_to_pcm(&chunk, 1),
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00]
        );
        // gain=8：-1×8=-8 → 0xFFF8；0×8=0
        assert_eq!(
            chunk_to_pcm(&chunk, 8),
            vec![0xF8, 0xFF, 0xF8, 0xFF, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn chunk_to_pcm_empty_and_half_frame_are_empty() {
        assert!(chunk_to_pcm(&[], 8).is_empty());
        // 残半帧（单采样）按整帧取（len/2=0）→ 空，与组帧面 frames==0 拒收同口径
        assert!(chunk_to_pcm(&[42], 8).is_empty());
    }

    #[test]
    fn push_bounded_drops_oldest_when_full() {
        let mut q = VecDeque::new();
        assert!(!push_bounded(&mut q, vec![1], 2));
        assert!(!push_bounded(&mut q, vec![2], 2));
        assert!(push_bounded(&mut q, vec![3], 2), "满时入队 = 丢最旧");
        assert_eq!(
            q.into_iter().collect::<Vec<_>>(),
            vec![vec![2], vec![3]],
            "最旧 [1] 被丢弃，新片入队尾"
        );
    }
}
