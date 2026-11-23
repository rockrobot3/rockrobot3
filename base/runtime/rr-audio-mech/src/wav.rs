// SPDX-License-Identifier: Apache-2.0
//! WAV 容器头（PCM s16le 单声道），字节序与字段值逐字对位 legacy `asr.c`
//! 的 `build_wav_header`。自 `rr3-brain` asr 原子件抽出（W1-3），供云 ASR
//! 上传体与板端录音共用。

/// 44B WAV 头（PCM s16le 单声道）。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 逐字段对位 legacy asr.c build_wav_header（字节序/档位）。
    #[test]
    fn wav_header_matches_legacy_asr_c() {
        let hdr = rr3_wav_header(16000, 3200);
        assert_eq!(&hdr[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(hdr[4..8].try_into().unwrap()), 36 + 3200);
        assert_eq!(&hdr[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(hdr[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(hdr[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(hdr[24..28].try_into().unwrap()), 16000);
        assert_eq!(u32::from_le_bytes(hdr[28..32].try_into().unwrap()), 32000);
        assert_eq!(u16::from_le_bytes(hdr[32..34].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(hdr[34..36].try_into().unwrap()), 16);
        assert_eq!(&hdr[36..40], b"data");
        assert_eq!(u32::from_le_bytes(hdr[40..44].try_into().unwrap()), 3200);
    }

    /// 零长度数据：RIFF 长度只含头，data 长度 0。
    #[test]
    fn wav_header_handles_empty_data() {
        let hdr = rr3_wav_header(8000, 0);
        assert_eq!(u32::from_le_bytes(hdr[4..8].try_into().unwrap()), 36);
        assert_eq!(u32::from_le_bytes(hdr[40..44].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(hdr[24..28].try_into().unwrap()), 8000);
    }
}
