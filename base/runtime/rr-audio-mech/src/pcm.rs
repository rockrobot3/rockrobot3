// SPDX-License-Identifier: Apache-2.0
//! 播放侧 PCM 机制核（W4-2）：WAV 容器抽取、单/双声道 → I2S 立体声、淡入淡出。
//!
//! 云 TTS 回包可能是裸 PCM，也可能是 WAV（契约只允许这两种）；板侧不解码
//! MP3/AAC。本件只做**字节级容器与样本整形**，不碰 I2S/网络/线程：
//! - [`rr3_wav_pcm`]：识别 RIFF/WAVE 并抽出 `data` 块；非 WAV 回 `Ok(None)`，
//!   由调用方按裸 PCM 处理。块长越界按"截到文件尾"收（编码器写 0/0xFFFFFFFF
//!   是常态），但不接受缺 `fmt `/`data` 或非 PCM 编码。
//! - [`rr3_pcm_stereo`]：单声道复制到双声道、双声道直通；首尾各
//!   `fade_frames` 帧线性淡化到 0，消上/下电爆音（I2S 从静默切到满幅的阶跃）。
//! - [`rr3_pcm_ms`]：播放时长换算（验收判据"时长误差 ≤5%"的分母）。
//!
//! 整数纪律同 VAD/DoA：样本整形全程整数 Q15，浮点仅在测试里做辅助断言。

use alloc::vec::Vec;

/// WAV 容器内的 PCM 视图（`data` 已按文件尾截断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3WavPcm<'a> {
    pub sample_rate: u32,
    pub channels: u16,
    pub data: &'a [u8],
}

/// WAV 容器头信息（流式收包用：只给偏移与声明长度，不要求 `data` 已到齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    /// `data` 载荷起点（相对文件开头）。
    pub data_off: usize,
    /// `data` 声明长度（原始值，未按缓冲截断；流式场景据此定总长）。
    pub data_size: usize,
    /// `data` 当前可用长度（按缓冲截断）。
    pub data_len: usize,
}

/// 扫描 WAV 容器头；非 RIFF/WAVE 回 `Ok(None)`，损坏/非 PCM 回错误。
///
/// 与 [`rr3_wav_pcm`] 共用同一份块扫描：`data` 尚为空（只到了头）也返回信息，
/// 由流式调用方按声明长度继续收。
pub fn rr3_wav_info(bytes: &[u8]) -> Result<Option<Rr3WavInfo>, &'static str> {
    if !rr3_is_wav(bytes) {
        return Ok(None);
    }
    let scan = rr3_wav_scan(bytes)?;
    Ok(Some(Rr3WavInfo {
        sample_rate: scan.rate,
        channels: scan.channels,
        data_off: scan.data_off,
        data_size: scan.data_size,
        data_len: scan.data_len,
    }))
}

/// 识别并抽取 WAV 的 `data` 块；非 RIFF/WAVE 回 `Ok(None)`（调用方按裸 PCM 处理）。
///
/// 接受 `audio_format = 1`（PCM）且 16 bit；其余编码（浮点/ADPCM/压缩）显式拒，
/// 不把噪声送功放。`data` 声明长度超出文件尾时截断（常见编码器 bug），声明为 0
/// 视为空体拒绝。
pub fn rr3_wav_pcm(bytes: &[u8]) -> Result<Option<Rr3WavPcm<'_>>, &'static str> {
    let Some(info) = rr3_wav_info(bytes)? else {
        return Ok(None);
    };
    if info.data_len == 0 {
        return Err("WAV data 为空");
    }
    let end = info.data_off + info.data_len;
    Ok(Some(Rr3WavPcm {
        sample_rate: info.sample_rate,
        channels: info.channels,
        data: &bytes[info.data_off..end],
    }))
}

/// RIFF/WAVE 头识别（只看 12 字节容器头，不进入块扫描）。
fn rr3_is_wav(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'A',
            b'V',
            b'E',
            ..
        ]
    )
}

/// 块扫描结果：`fmt ` 先于 `data`；长度已按当前缓冲截断。
struct Rr3WavScan {
    rate: u32,
    channels: u16,
    data_off: usize,
    data_size: usize,
    data_len: usize,
}

/// 块扫描：`fmt ` 先于 `data`，返回格式与 `data` 偏移/可用长度。
fn rr3_wav_scan(bytes: &[u8]) -> Result<Rr3WavScan, &'static str> {
    let mut off = 12usize;
    let mut fmt: Option<(u16, u32)> = None; // (channels, rate)
    while off + 8 <= bytes.len() {
        let (id, size, body) = rr3_chunk_at(bytes, off);
        let end = body.saturating_add(size).min(bytes.len());
        if id == b"fmt " {
            let (channels, rate, _bits, _format) = rr3_fmt_parse(&bytes[body..end])?;
            fmt = Some((channels, rate));
        } else if id == b"data" {
            return rr3_wav_data(fmt, bytes, body, size);
        }
        // 块按偶数字节对齐（RIFF 规范），越界即停（残块不猜）。
        off = body.saturating_add(size).saturating_add(size & 1);
    }
    Err("WAV 缺 data 块")
}

/// 单块头：`(id, 声明长度, 载荷起点)`。
fn rr3_chunk_at(bytes: &[u8], off: usize) -> (&[u8], usize, usize) {
    let id = &bytes[off..off + 4];
    let size = u32::from_le_bytes([
        bytes[off + 4],
        bytes[off + 5],
        bytes[off + 6],
        bytes[off + 7],
    ]) as usize;
    (id, size, off + 8)
}

/// `data` 块收口：必须已有 `fmt `（格式门在 [`rr3_fmt_parse`] 已过）；长度按
/// 当前缓冲截断，空体由调用方按语义决定（流式头扫描允许空，抽 PCM 不允许）。
fn rr3_wav_data(
    fmt: Option<(u16, u32)>,
    bytes: &[u8],
    body: usize,
    size: usize,
) -> Result<Rr3WavScan, &'static str> {
    let Some((channels, rate)) = fmt else {
        return Err("WAV data 先于 fmt");
    };
    let data_len = bytes.len().saturating_sub(body).min(size);
    Ok(Rr3WavScan {
        rate,
        channels,
        data_off: body,
        data_size: size,
        data_len,
    })
}

/// `fmt ` 块前 16 字节：编码 / 声道 / 采样率 / 位宽，并当场过播放格式门
/// （只接受 16 bit PCM、单/双声道——I2S 槽位固定 16 bit）。
fn rr3_fmt_parse(b: &[u8]) -> Result<(u16, u32, u16, u16), &'static str> {
    if b.len() < 16 {
        return Err("WAV fmt 块截断");
    }
    let format = u16::from_le_bytes([b[0], b[1]]);
    let channels = u16::from_le_bytes([b[2], b[3]]);
    let rate = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    let bits = u16::from_le_bytes([b[14], b[15]]);
    if format != 1 {
        return Err("WAV 非 PCM 编码（不解码压缩格式）");
    }
    if bits != 16 {
        return Err("WAV 位宽非 16 bit");
    }
    if !matches!(channels, 1 | 2) {
        return Err("WAV 声道数不是 1/2");
    }
    Ok((channels, rate, bits, format))
}

/// PCM → I2S 立体声 s16le（`out` 先清空）：单声道复制两声道、双声道直通；
/// 首 `fade_in` 帧与尾 `fade_out` 帧线性淡到 0（防爆音）。分块播放时由调用方
/// 只在首块给 `fade_in`、末块给 `fade_out`。`channels ∈ {1, 2}`，字节数须整帧。
pub fn rr3_pcm_stereo(
    src: &[u8],
    channels: u16,
    fade_in: usize,
    fade_out: usize,
    out: &mut Vec<u8>,
) -> Result<(), &'static str> {
    if !matches!(channels, 1 | 2) {
        return Err("PCM 声道数不是 1/2");
    }
    let frame_bytes = channels as usize * 2;
    if src.len() % frame_bytes != 0 {
        return Err("PCM 字节数不是整帧");
    }
    let frames = src.len() / frame_bytes;
    let fade_in = fade_in.min(frames);
    let fade_out = fade_out.min(frames);
    out.clear();
    out.reserve(frames * 4);
    for i in 0..frames {
        let gain = rr3_fade_gain(i, frames, fade_in, fade_out);
        let o = i * frame_bytes;
        // 右声道偏移用 `frame_bytes - 2` 统一：单声道时即左声道样本（复制），
        // 双声道时即 R 样本——省掉逐帧 `if channels == 2`。
        let l = rr3_apply(i16::from_le_bytes([src[o], src[o + 1]]), gain);
        let r = rr3_apply(
            i16::from_le_bytes([src[o + frame_bytes - 2], src[o + frame_bytes - 1]]),
            gain,
        );
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }
    Ok(())
}

/// 第 `i` 帧的 Q15 增益（满幅 = 32768，保证小样本经 `>>15` 不丢位）：首
/// `fade_in` 帧从 0 升到满、尾 `fade_out` 帧降回 0，其余满幅。
pub(crate) fn rr3_fade_gain(i: usize, frames: usize, fade_in: usize, fade_out: usize) -> i32 {
    // fade=0 时 `min(0)=0` + 布尔项补满幅——用算术代替两个条件分支。
    let up = (i.min(fade_in) as i64 * 32_768 / fade_in.max(1) as i64) as i32
        + (fade_in == 0) as i32 * 32_768;
    let tail = frames - 1 - i;
    let down = (tail.min(fade_out) as i64 * 32_768 / fade_out.max(1) as i64) as i32
        + (fade_out == 0) as i32 * 32_768;
    up.min(down)
}

/// Q15 增益应用（右移 15，饱和到 i16 域）。
pub(crate) fn rr3_apply(s: i16, gain: i32) -> i16 {
    let v = (s as i32 * gain) >> 15;
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// PCM 播放时长（ms）：`channels=0` 或 `rate=0` 回 0，不猜（布尔项归零，无分支）。
pub fn rr3_pcm_ms(bytes: usize, channels: u16, rate: u32) -> u64 {
    let frames = bytes / (channels as usize * 2).max(1);
    let ok = (channels > 0 && rate > 0) as u64;
    frames as u64 * 1000 / rate.max(1) as u64 * ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成一个最小 WAV（fmt 16B + data）。
    fn wav(rate: u32, channels: u16, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn wav_info_accepts_header_before_data() {
        let b = wav(16_000, 1, &[1, 0, 2, 0]);
        let info = rr3_wav_info(&b[..44]).unwrap().unwrap();
        assert_eq!(info.sample_rate, 16_000);
        assert_eq!(info.channels, 1);
        assert_eq!(info.data_off, 44);
        assert_eq!(info.data_size, 4, "声明长度不按缓冲截断");
        assert_eq!(info.data_len, 0, "只到 data 头时载荷为空");
        assert_eq!(rr3_wav_info(b"\x01\x02\x03").unwrap(), None);
        assert_eq!(rr3_wav_pcm(&b[..44]).unwrap_err(), "WAV data 为空");
    }

    #[test]
    fn wav_data_and_format_are_extracted() {
        let pcm = [0x01, 0x00, 0x02, 0x00];
        let b = wav(16_000, 1, &pcm);
        let got = rr3_wav_pcm(&b).unwrap().unwrap();
        assert_eq!(got.sample_rate, 16_000);
        assert_eq!(got.channels, 1);
        assert_eq!(got.data, &pcm);
    }

    #[test]
    fn non_wav_is_reported_as_raw() {
        assert_eq!(rr3_wav_pcm(b"\x01\x02\x03\x04"), Ok(None));
        assert_eq!(rr3_wav_pcm(b""), Ok(None));
    }

    #[test]
    fn malformed_wav_is_rejected() {
        let mut no_data = wav(16_000, 1, &[0, 0]);
        no_data.truncate(40); // 砍掉 data 头
        assert_eq!(rr3_wav_pcm(&no_data).unwrap_err(), "WAV 缺 data 块");

        let mut bad_fmt = wav(16_000, 1, &[0, 0]);
        bad_fmt[20] = 3; // audio_format = IEEE float
        assert!(rr3_wav_pcm(&bad_fmt).unwrap_err().contains("非 PCM"));

        let mut bad_ch = wav(16_000, 6, &[0, 0, 0, 0]);
        bad_ch[22] = 6;
        assert!(rr3_wav_pcm(&bad_ch).unwrap_err().contains("声道"));
    }

    #[test]
    fn data_size_beyond_file_is_clamped() {
        let mut b = wav(16_000, 1, &[1, 0, 2, 0]);
        let data_size_off = b.len() - 8;
        b[data_size_off..data_size_off + 4].copy_from_slice(&9999u32.to_le_bytes());
        let got = rr3_wav_pcm(&b).unwrap().unwrap();
        assert_eq!(got.data.len(), 4, "声明长度越过文件尾时截到文件尾");
    }

    #[test]
    fn mono_is_duplicated_with_fade_endpoints_zero() {
        let src: [i16; 4] = [1000, -1000, 2000, -2000];
        let mut bytes = Vec::new();
        for s in src {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let mut out = Vec::new();
        rr3_pcm_stereo(&bytes, 1, 1, 1, &mut out).unwrap();
        assert_eq!(out.len(), 16);
        let got: Vec<i16> = out
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(got[0], 0, "首帧淡入到 0");
        assert_eq!(got[1], 0, "双声道同值");
        assert_eq!(got[2], -1000);
        assert_eq!(got[3], -1000);
        assert_eq!(got[4], 2000);
        assert_eq!(got[5], 2000);
        assert_eq!(got[6], 0, "末帧淡出到 0");
        assert_eq!(got[7], 0);
    }

    #[test]
    fn stereo_passthrough_keeps_samples() {
        let mut bytes = Vec::new();
        for s in [1i16, 2, 3, 4] {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let mut out = Vec::new();
        rr3_pcm_stereo(&bytes, 2, 0, 0, &mut out).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn odd_length_and_bad_channels_are_rejected() {
        let mut out = Vec::new();
        assert!(rr3_pcm_stereo(&[0, 1, 2], 1, 0, 0, &mut out).is_err());
        assert!(rr3_pcm_stereo(&[0, 1], 3, 0, 0, &mut out).is_err());
        assert!(rr3_pcm_stereo(&[0, 1, 2, 3], 2, 0, 0, &mut out).is_ok());
    }

    #[test]
    fn duration_uses_frames_and_rate() {
        assert_eq!(rr3_pcm_ms(32_000, 1, 16_000), 1000);
        assert_eq!(rr3_pcm_ms(32_000, 2, 16_000), 500);
        assert_eq!(rr3_pcm_ms(32_000, 0, 16_000), 0);
        assert_eq!(rr3_pcm_ms(32_000, 1, 0), 0);
    }
}
