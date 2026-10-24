//! tts 原子件机制主体（五件套③，批N32 件四 Rust 拆分定形）：合成侧**纯机制**
//! ——voice 命名（legacy tts_config.h 女声 bella）与 mock 正弦 PCM（确定性
//! 440Hz，时长随文本 80ms/字符；无 IO 无环境读取无 JSON，⑧-b 依赖闭包纪律，
//! core 浮点同 rr-mech 先例）。HTTP 传输在组合件级 [`crate::cloud`]；mock 档
//! 是测试注入点不是产品路径（ADR-0016 裁定 2.1）。M4 的 PCM 是验证性消费
//! （非空 + 时长换算入日志）；真播放随 M5 板上 ALSA sink（ADR-0014 裁定 3）。

/// TTS 女声 bella（男声改 alex——tts_config.h）
pub const RR3_TTS_VOICE_SUFFIX: &str = ":bella";

/// mock TTS 正弦时长：每字符 80ms
const RR3_SINE_MS_PER_CHAR: u64 = 80;

/// voice 命名：模型名 + 女声后缀（legacy tts_config.h 同形状）。
pub fn rr3_tts_voice(model: &str) -> String {
    format!("{model}{RR3_TTS_VOICE_SUFFIX}")
}

/// mock TTS 的确定性 PCM：440Hz 正弦，时长随文本长度（s16le；振幅 12000）。
pub fn rr3_sine_pcm(text: &str, sample_rate: u32) -> Vec<u8> {
    rr3_sine_pcm_ms(text, sample_rate, RR3_SINE_MS_PER_CHAR)
}

/// [`rr3_sine_pcm`] 的参数化底座（ms_per_char 可调——真云冒烟夹具喂 125ms）。
pub fn rr3_sine_pcm_ms(text: &str, sample_rate: u32, ms_per_char: u64) -> Vec<u8> {
    let n = text.trim().chars().count().max(1) as u64;
    let total = (sample_rate as u64 * n * ms_per_char / 1000) as usize;
    let mut pcm = Vec::with_capacity(total * 2);
    for i in 0..total {
        let v = (12000.0
            * (core::f64::consts::TAU * 440.0 * i as f64 / sample_rate as f64).sin())
            as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    pcm
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- 件级两腿对拍（⑧-a，§4.0.3 判据②） ----------

    /// ⑧-a 腿一：mock 正弦确定性（同输入逐字节同 PCM；偶数字节长、非空、
    /// 振幅不越 12000，时长随字符 80ms/char）
    #[test]
    fn parity_leg1_tts_sine_train_is_deterministic() {
        let a = rr3_sine_pcm("你好", 16000);
        let b = rr3_sine_pcm("你好", 16000);
        assert_eq!(a, b);
        assert_eq!(a.len() % 2, 0);
        assert!(!a.is_empty());
        for chunk in a.chunks(2) {
            let v = i16::from_le_bytes(chunk.try_into().unwrap());
            assert!(v.unsigned_abs() <= 12000);
        }
        // 时长换算：2 字符 × 80ms × 16kHz × 2B
        assert_eq!(a.len(), 2 * 80 * 16 * 2);
    }

    /// ⑧-a 腿二：voice 命名对位 legacy tts_config.h（模型名 + 女声 bella）
    #[test]
    fn parity_leg2_tts_voice_matches_legacy_tts_config_h() {
        assert_eq!(
            rr3_tts_voice("FunAudioLLM/CosyVoice2-0.5B"),
            "FunAudioLLM/CosyVoice2-0.5B:bella"
        );
    }
}
