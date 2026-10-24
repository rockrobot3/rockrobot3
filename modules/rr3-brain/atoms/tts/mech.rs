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

/// WAV 容器里 `data` 块的载荷（宿主 `say` 兜底用）：跳过 RIFF 头与各子块，
/// 只取 `data` 块本体；无 `data` 块/长度越界返回 None（不猜不补）。
pub fn rr3_wav_data_chunk(wav: &[u8]) -> Option<&[u8]> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let len = u32::from_le_bytes(wav[i + 4..i + 8].try_into().ok()?) as usize;
        let body = i + 8;
        if id == b"data" {
            let end = body.checked_add(len)?;
            return wav.get(body..end);
        }
        // 子块按声明长度跳过（奇数长度补 1 字节对齐）
        i = body.checked_add(len + (len & 1))?;
    }
    None
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

    /// WAV 抽 data 块：fmt 块在前、data 块在后（say 的实际形状）；非 RIFF/缺
    /// data/长度越界一律 None
    #[test]
    fn wav_data_chunk_extracts_payload_and_rejects_malformed() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 6).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[0u8; 16]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&6u32.to_le_bytes());
        wav.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(rr3_wav_data_chunk(&wav), Some(&[1, 2, 3, 4, 5, 6][..]));
        // 非 RIFF
        assert_eq!(rr3_wav_data_chunk(b"RIFXxxxxxxxx"), None);
        // 缺 data 块
        let mut no_data = wav[..12].to_vec();
        no_data.extend_from_slice(b"fmt ");
        no_data.extend_from_slice(&16u32.to_le_bytes());
        no_data.extend_from_slice(&[0u8; 16]);
        assert_eq!(rr3_wav_data_chunk(&no_data), None);
        // data 声明长度越过缓冲
        let mut short = wav.clone();
        let n = short.len();
        short[n - 10..n - 6].copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(rr3_wav_data_chunk(&short), None);
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
