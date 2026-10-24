//! asr 原子件机制主体（五件套③，批N32 件四 Rust 拆分定形）：识别侧**纯机制**
//! ——44B WAV 头与 multipart 请求体装配（字节级，无 IO 无环境读取无 JSON，
//! ⑧-b 依赖闭包纪律）。云端档位/密钥/HTTP 传输/boundary 随机源在组合件级
//! [`crate::cloud`]（策略与 IO 不进机制核；boundary 由组合件供给——随机性是
//! 策略供给，机制只做装配）。语义对位 legacy asr.c（build_wav_header）与
//! rr3_brain/cloud/asr.py（上传体形状）。

/// 44B WAV 头（PCM s16le 单声道），字节序对齐 legacy asr.c 的 build_wav_header。
pub fn rr3_wav_header(sample_rate: u32, data_len: u32) -> [u8; 44] {
    let mut b = [0u8; 44];
    b[0..4].copy_from_slice(b"RIFF");
    b[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    b[8..16].copy_from_slice(b"WAVEfmt ");
    b[16..20].copy_from_slice(&16u32.to_le_bytes());
    b[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    b[22..24].copy_from_slice(&1u16.to_le_bytes()); // 单声道
    b[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    b[28..32].copy_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    b[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    b[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
    b[36..40].copy_from_slice(b"data");
    b[40..44].copy_from_slice(&data_len.to_le_bytes());
    b
}

/// multipart 请求体装配：model 字段 + file 字段（filename="a.wav"，WAV 头仓内
/// 自建）+ PCM + 收尾 boundary——对位 asr.py 上传体形状（整段 WAV 一次上传，
/// ADR-0014 裁定 2；流式是 legacy 内存约束的产物，本机不复刻）。
pub fn rr3_asr_multipart_body(model: &str, pcm: &[u8], sample_rate: u32, boundary: &str) -> Vec<u8> {
    let header = rr3_wav_header(sample_rate, pcm.len() as u32);
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&header);
    body.extend_from_slice(pcm);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- 件级两腿对拍（⑧-a，§4.0.3 判据②） ----------

    /// ⑧-a 腿一：multipart 装配确定性（同输入逐字节同体，段标记齐备）
    #[test]
    fn parity_leg1_asr_multipart_body_is_deterministic() {
        let a = rr3_asr_multipart_body("m", b"\x01\x02", 16000, "bd1");
        let b = rr3_asr_multipart_body("m", b"\x01\x02", 16000, "bd1");
        assert_eq!(a, b);
        assert!(a.windows(12).any(|w| w == b"name=\"model\""));
        assert!(a.windows(2).any(|w| w == b"m\r"), "model 值在体里");
        assert!(a.windows(4).any(|w| w == b"RIFF"), "WAV 头仓内自建");
        assert!(a.windows(16).any(|w| w == b"filename=\"a.wav\""));
        assert!(a.ends_with(b"\r\n--bd1--\r\n"), "收尾 boundary");
    }

    /// ⑧-a 腿二：WAV 头逐字段对位 legacy asr.c build_wav_header（字节序/档位）
    #[test]
    fn parity_leg2_asr_wav_header_matches_legacy_asr_c() {
        let hdr = rr3_wav_header(16000, 3200);
        assert_eq!(&hdr[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(hdr[4..8].try_into().unwrap()), 36 + 3200);
        assert_eq!(&hdr[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(hdr[20..22].try_into().unwrap()), 1); // PCM
        assert_eq!(u16::from_le_bytes(hdr[22..24].try_into().unwrap()), 1); // mono
        assert_eq!(u32::from_le_bytes(hdr[24..28].try_into().unwrap()), 16000);
        assert_eq!(u32::from_le_bytes(hdr[28..32].try_into().unwrap()), 32000); // byte rate
        assert_eq!(u16::from_le_bytes(hdr[32..34].try_into().unwrap()), 2); // block align
        assert_eq!(u16::from_le_bytes(hdr[34..36].try_into().unwrap()), 16); // bits
        assert_eq!(&hdr[36..40], b"data");
        assert_eq!(u32::from_le_bytes(hdr[40..44].try_into().unwrap()), 3200);
    }
}
