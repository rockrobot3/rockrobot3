//! capture 原子件 · 机制核（批N23 件三，MC-7 缺② 收编原子模块形态）。
//!
//! **格位对齐申报**：本件服务 `audio.capture` 格位（esp32-mini 装配单格主 =
//! rr3-capture 宿主件，板侧经桥执行——宿主申报面在 modules/rr3-capture/atoms/
//! capture，板侧执行面**不冒领格主**、不立 atom.toml：`provides` 恰一格与板无关
//! 断言是格主面法则，见批N23 计划册 §2「三不三立」）。
//!
//! **机制与策略分离**（文件级，收编落点）：本文件收拢音频采集上行面的**纯算术**
//! ——32bit 帧解析（大端线序 → 24bit 符号幅度）、增益饱和钳位、分片变换（跨片
//! DC 阻断 → 24bit 截 16bit → s16le 双通道交错）与有界背压入队（满丢最旧）；
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

/// 跨分片连续的 DC 阻断器（一阶 IIR 高通，逐通道独立状态）。
///
/// 为什么不再"逐分片减均值"（2026-10-31 S3 实机订正）：128ms 分片各自减均值，
/// 分片边界处基线一跳一个台阶，频谱在 7.8Hz 的偶次谐波（31/47/62/78Hz）上堆出
/// 远高于语音的"拉链噪声"——总线 mono RMS 被抬到 450–600，VAD 700 阈值永不触发，
/// ASR 也只剩噪声。连续状态的一阶高通没有分片边界，DC 照去，台阶消失。
/// 系数 α=0.995 @16k ≈ 12.7Hz 截止：只压直流与亚语音低频，不动 80Hz 以上语音基频。
/// 首帧按 x 置位（y=0），避免开机瞬间一个大直流跳变进缓冲。
#[derive(Debug)]
pub(crate) struct Rr3DcBlocker {
    px: [f32; 2],
    py: [f32; 2],
    primed: [bool; 2],
}

impl Rr3DcBlocker {
    pub(crate) fn new() -> Self {
        Self {
            px: [0.0; 2],
            py: [0.0; 2],
            primed: [false; 2],
        }
    }

    /// 清状态（音频关断时调用：再开时按首帧重新起算，不把旧的 py 带进新会话）
    pub(crate) fn reset(&mut self) {
        self.px = [0.0; 2];
        self.py = [0.0; 2];
        self.primed = [false; 2];
    }

    fn step(&mut self, ch: usize, x: f32) -> f32 {
        const ALPHA: f32 = 0.995;
        if !self.primed[ch] {
            self.px[ch] = x;
            self.py[ch] = 0.0;
            self.primed[ch] = true;
            return 0.0;
        }
        let y = x - self.px[ch] + ALPHA * self.py[ch];
        self.px[ch] = x;
        self.py[ch] = y;
        y
    }
}

/// 一分片 → s16le PCM 字节流（**纯变换 + 跨片 DC 状态**）：逐通道过
/// [`Rr3DcBlocker`]（DC 照去，无分片边界台阶）→ 24bit 截 16bit（>>8 保高位满幅）
/// → 增益钳位 → s16le 双通道交错。零 I2S/零时钟/零锁（返回值即产物；b64+JSON 组帧
/// 面在 mic.rs）。空分片与残半帧（奇数采样）回空——组帧面 frames==0 拒收同口径。
pub(crate) fn chunk_to_pcm(chunk: &[i32], gain: i64, dc: &mut Rr3DcBlocker) -> Vec<u8> {
    let frames = chunk.len() / 2;
    let mut pcm = Vec::with_capacity(frames * 4);
    for f in 0..frames {
        let l = gain_i16((dc.step(0, chunk[f * 2] as f32) as i64) >> 8, gain);
        let r = gain_i16((dc.step(1, chunk[f * 2 + 1] as f32) as i64) >> 8, gain);
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
    fn chunk_to_pcm_interleaves_s16le_in_pairs() {
        // 首帧只做状态置位（y=0）；随后 x 恒定 → y 恒 0，输出全 0（DC 被阻断）
        let chunk = [1_000_000i32; 8];
        let mut dc = Rr3DcBlocker::new();
        assert_eq!(
            chunk_to_pcm(&chunk, 1, &mut dc),
            vec![0u8; 16],
            "恒定直流 + 首帧置位 → 16bit 域全零"
        );
    }

    /// 关键回归：恒定直流跨 128ms 分片边界不得出现台阶（旧"逐片减均值"首片
    /// 与次片都会归零、但片间基线跳变会造出 31/47/62/78Hz 拉链噪声；连续
    /// 阻断器的第二片必须同样收敛到 0）。
    #[test]
    fn dc_blocker_is_continuous_across_chunk_boundaries() {
        let mut dc = Rr3DcBlocker::new();
        let chunk = [1_000_000i32; 4000]; // 2000 帧 ×2ch = 两片 1000 帧
        let a = chunk_to_pcm(&chunk[..2000], 1, &mut dc);
        let b = chunk_to_pcm(&chunk[2000..], 1, &mut dc);
        for pair in b.chunks(2) {
            let l = i16::from_le_bytes([pair[0], pair[1]]);
            let r = i16::from_le_bytes([pair[2], pair[3]]);
            assert!(
                l.abs() <= 2 && r.abs() <= 2,
                "跨片后的恒定直流必须已收敛，实际 {l}/{r}"
            );
        }
        assert!(!a.is_empty(), "首片仍是有效帧（首帧置位、其余归零）");
    }

    /// 分片切法不改变结果：同一序列一次喂与分两次喂输出逐字节相同（状态跨片）
    #[test]
    fn dc_blocker_chunking_does_not_change_output() {
        let samples: Vec<i32> = (0..4000).map(|i| 500_000 + (i % 32) * 1000).collect();
        let mut whole = Rr3DcBlocker::new();
        let a = chunk_to_pcm(&samples, 1, &mut whole);
        let mut split = Rr3DcBlocker::new();
        let b1 = chunk_to_pcm(&samples[..2000], 1, &mut split);
        let b2 = chunk_to_pcm(&samples[2000..], 1, &mut split);
        assert_eq!(a, [b1, b2].concat(), "分片边界不得改变输出");
    }

    /// 空分片不推进状态；reset 回到首帧置位语义
    #[test]
    fn dc_blocker_empty_and_reset_keep_state_discipline() {
        let samples: Vec<i32> = (0..800).map(|i| i as i32 * 7 - 3000).collect();
        let mut dc = Rr3DcBlocker::new();
        let first = chunk_to_pcm(&samples[..400], 1, &mut dc);
        assert!(chunk_to_pcm(&[], 1, &mut dc).is_empty());
        let second = chunk_to_pcm(&samples[400..], 1, &mut dc);
        let mut fresh = Rr3DcBlocker::new();
        let all = chunk_to_pcm(&samples, 1, &mut fresh);
        assert_eq!([first, second].concat(), all, "空片不得推进状态");
        dc.reset();
        let mut after_reset = Rr3DcBlocker::new();
        let again = chunk_to_pcm(&samples, 1, &mut dc);
        assert_eq!(again, chunk_to_pcm(&samples, 1, &mut after_reset), "reset 等价于新阻断器");
    }

    #[test]
    fn chunk_to_pcm_empty_and_half_frame_are_empty() {
        let mut dc = Rr3DcBlocker::new();
        assert!(chunk_to_pcm(&[], 8, &mut dc).is_empty());
        // 残半帧（单采样）按整帧取（len/2=0）→ 空，与组帧面 frames==0 拒收同口径
        assert!(chunk_to_pcm(&[42], 8, &mut dc).is_empty());
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
