// SPDX-License-Identifier: Apache-2.0
//! sim 音频面：波形取值、确定性生成、TTS 占位块与接收端尾窗裁剪。

use crate::sim::source::RR3_SIM_NOISE_AMP;
use crate::sim::*;
use crate::{Rr3AudioCfg, Rr3AudioSink, Rr3AudioSource, Rr3HalError, RR3_AUDIO_CFG_16K_MONO16};

/// 占位 TTS 波形（I-10 迁入）：与独立双精度公式 12000·sin(2π·440·i/16000)
/// 的全程偏差 ≤100（幅度 0.8%，听感等价预算）；逐块推进（20ms 块）验证
/// 相位跨块连续
#[test]
fn tts_block_sine_budg() {
    let mut buf = [0i16; 320];
    for chunk in (0..16_000).step_by(320) {
        rr3_sim_tts_block(&mut buf, chunk);
        for (k, &v) in buf.iter().enumerate() {
            let i = (chunk + k) as f64;
            let want =
                (12_000.0f64 * (2.0 * std::f64::consts::PI * 440.0 * i / 16_000.0).sin()) as i64;
            assert!(
                (v as i64 - want).abs() <= 100,
                "sample #{}: {v} vs {want}",
                chunk + k
            );
        }
    }
}

fn source(wave: Rr3SimWave) -> Rr3SimAudioSource {
    Rr3SimAudioSource::rr3_sim_source_new(wave)
}

#[test]
fn wave_rej_garbage() {
    for (s, want) in [
        ("sine", Rr3SimWave::Sine),
        ("SINE", Rr3SimWave::Sine),
        ("noise", Rr3SimWave::Noise),
        ("silence", Rr3SimWave::Silence),
        ("burst", Rr3SimWave::Burst),
    ] {
        assert_eq!(Rr3SimWave::rr3_sim_wave_parse(s), Some(want), "{s}");
        assert_eq!(want.rr3_sim_wave_as_str(), s.to_ascii_lowercase());
    }
    for bad in ["", "square", "sine ", "0"] {
        assert_eq!(Rr3SimWave::rr3_sim_wave_parse(bad), None, "{bad}");
    }
}

/// golden 向量：正弦前 8 点逐点相等（16k 采样率下 440Hz 的确定性开头）。
/// 这批字面量同时钉住频率与幅度——改波形参数，这里当场红。
#[test]
fn sine_wave_prefix() {
    let mut s = source(Rr3SimWave::Sine);
    s.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut buf = [0i16; 8];
    let n = s.read(&mut buf).unwrap();
    assert_eq!(n, 8);
    // golden 前缀：12000·sin(2π·440·i/16000) 的 i64 截断值（独立双精度实现算出，
    // 不是被测代码自己的输出回抄）——改频率/幅度/取整方式，这里当场红。
    let golden: [i16; 8] = [0, 2063, 4064, 5945, 7649, 9124, 10328, 11225];
    assert_eq!(&buf[..], &golden, "正弦开头必须与 golden 逐点一致");
}

/// 相位跨 read 连续：两次 read 拼起来等于一次读双份
#[test]
fn sine_phase_reads() {
    let mut a = source(Rr3SimWave::Sine);
    a.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut b1 = [0i16; 160];
    let n1 = a.read(&mut b1).unwrap();
    let mut b2 = [0i16; 160];
    let n2 = a.read(&mut b2).unwrap();

    let mut b = source(Rr3SimWave::Sine);
    b.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut whole = [0i16; 320];
    let nw = b.read(&mut whole).unwrap();
    let mut joined = Vec::new();
    joined.extend_from_slice(&b1[..n1]);
    joined.extend_from_slice(&b2[..n2]);
    assert_eq!(joined, &whole[..nw]);
}

/// burst：偶秒正弦、奇秒静音；2s 周期恰为 440Hz 整周期数，第 3 秒与第 1 秒逐点相等
#[test]
fn burst_sine_stays_det() {
    let mut s = source(Rr3SimWave::Burst);
    s.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut pcm = [0i16; 3 * 16000];
    let n = s.read(&mut pcm).unwrap();
    assert_eq!(n, 3 * 16000);
    // 第 1 秒有声（远超 VAD 起音阈值），第 2 秒全零，第 3 秒与第 1 秒逐点相等
    assert!(
        pcm[..16000].iter().any(|&v| v.abs() > 1500),
        "第 1 秒必过起音阈值"
    );
    assert!(
        pcm[16000..32000].iter().all(|&v| v == 0),
        "第 2 秒必须全静音"
    );
    assert_eq!(&pcm[32000..], &pcm[..16000], "第 3 秒复现第 1 秒");
    // 门控与正弦同源：第 1 秒头部与 Sine 波形的同序号采样逐点相等
    let mut sine = source(Rr3SimWave::Sine);
    sine.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut head = [0i16; 16000];
    sine.read(&mut head).unwrap();
    assert_eq!(&pcm[..16000], &head[..], "burst 有声段 = Sine 同序号采样");
}

/// 噪声确定性：同波两源读出逐点相等，且幅度不越 ±8000
#[test]
fn noise_det_bounded() {
    let mut a = source(Rr3SimWave::Noise);
    a.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut ba = [0i16; 512];
    a.read(&mut ba).unwrap();
    let mut b = source(Rr3SimWave::Noise);
    b.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut bb = [0i16; 512];
    b.read(&mut bb).unwrap();
    assert_eq!(ba, bb);
    assert!(ba.iter().all(|&v| v.abs() <= RR3_SIM_NOISE_AMP));
    // 不是正弦：存在相邻样本跳变超过 2000（440Hz 正弦 16k 下的最大步进约 1900）
    assert!(ba
        .windows(2)
        .any(|w| (w[0] as i32 - w[1] as i32).abs() > 2000));
}

#[test]
fn silence_open_bad() {
    let mut s = source(Rr3SimWave::Silence);
    let mut probe = [0i16; 10];
    let e = s.read(&mut probe).unwrap_err();
    assert!(matches!(e, Rr3HalError::InvalidParam(_)));
    s.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut pcm = [0i16; 64];
    s.read(&mut pcm).unwrap();
    assert!(pcm.iter().all(|&v| v == 0));
}

#[test]
fn open_rej_non_16bit() {
    for bad in [
        Rr3AudioCfg {
            sample_rate_hz: 0,
            channels: 1,
            bit_depth: 16,
        },
        Rr3AudioCfg {
            sample_rate_hz: 16000,
            channels: 0,
            bit_depth: 16,
        },
        Rr3AudioCfg {
            sample_rate_hz: 16000,
            channels: 1,
            bit_depth: 24,
        },
    ] {
        let mut s = source(Rr3SimWave::Sine);
        let e = s.open(bad).unwrap_err();
        assert!(matches!(e, Rr3HalError::InvalidParam(_)), "{bad:?}");
    }
}

#[test]
fn sink_tail_window() {
    let mut sink = Rr3SimAudioSink::rr3_sim_sink_new(100);
    let e = sink.write(&[1]).unwrap_err();
    assert!(
        matches!(e, Rr3HalError::InvalidParam(_)),
        "write 前必须 open"
    );
    sink.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    for chunk in 0..7u8 {
        sink.write(&[chunk as i16; 40]).unwrap();
    }
    let played = sink.rr3_sim_sink_played();
    assert_eq!(played.len(), 100, "尾窗封顶");
    // 7×40=280 采样进 cap=100 的尾窗：最旧的 180 个被挤出，
    // 窗内 = chunk4 的后 20 个 + chunk5 整块 + chunk6 整块
    assert_eq!(&played[..20], &[4i16; 20], "最旧部分被挤出");
    assert_eq!(&played[20..60], &[5i16; 40]);
    assert_eq!(&played[60..], &[6i16; 40], "最新一块保留");
    assert_eq!(sink.rr3_sim_sink_count(), 100);
}
